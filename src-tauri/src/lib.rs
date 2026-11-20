mod categories;
mod commands;
mod db;
mod export;
mod settings;
mod summary;
mod tracker;

use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, RunEvent, WindowEvent, Wry};

pub struct AppState {
    pub db: Arc<Mutex<rusqlite::Connection>>,
    pub tracker: Arc<tracker::Shared>,
    pause_item: Mutex<Option<MenuItem<Wry>>>,
}

/// Lock a mutex, recovering from poisoning (a panicked tick must not brick the app).
pub fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Owner-only permissions on Unix (Windows: per-user AppData ACLs already apply).
fn restrict(path: &Path, mode: u32) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode));
    }
    #[cfg(not(unix))]
    let _ = (path, mode);
}

pub fn write_private_file(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    use std::io::Write;
    opts.open(path)?.write_all(bytes)
}

fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

pub(crate) fn apply_pause(app: &AppHandle, paused: bool) {
    let state = app.state::<AppState>();
    state.tracker.set_paused(&state.db, paused);
    let item = lock(&state.pause_item);
    if let Some(item) = item.as_ref() {
        let _ = item.set_text(if paused {
            "Resume tracking"
        } else {
            "Pause tracking"
        });
    }
}

fn build_tray(app: &tauri::App) -> tauri::Result<MenuItem<Wry>> {
    let open = MenuItem::with_id(app, "open", "Open Stint", true, None::<&str>)?;
    let pause = MenuItem::with_id(app, "pause", "Pause tracking", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Stint", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &pause, &sep, &quit])?;

    let mut builder = TrayIconBuilder::with_id("main")
        .menu(&menu)
        .tooltip("Stint")
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => show_main(app),
            "pause" => {
                let paused = app
                    .state::<AppState>()
                    .tracker
                    .paused
                    .load(std::sync::atomic::Ordering::SeqCst);
                apply_pause(app, !paused);
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(pause)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // Must be first: a second launch just focuses the running instance,
        // so two trackers never write to the same database.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_main(app)
        }))
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--minimized"]),
        ))
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            restrict(&dir, 0o700);
            let db_path = dir.join("stint.sqlite3");
            let conn = db::open(&db_path)?;
            restrict(&db_path, 0o600);

            let settings = db::load_settings(&conn)?;
            db::apply_retention(&conn, settings.retention_days, tracker::unix_now())?;

            let db = Arc::new(Mutex::new(conn));
            let shared = Arc::new(tracker::Shared::new(settings));
            tracker::spawn(db.clone(), shared.clone());
            app.manage(AppState {
                db,
                tracker: shared,
                pause_item: Mutex::new(None),
            });

            let pause = build_tray(app)?;
            *lock(&app.state::<AppState>().pause_item) = Some(pause);

            if !std::env::args().any(|a| a == "--minimized") {
                show_main(app.handle());
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the window keeps tracking in the tray; quit from the tray menu.
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_summary,
            commands::get_interval_totals,
            commands::get_status,
            commands::set_paused,
            commands::start_focus,
            commands::stop_focus,
            commands::get_settings,
            commands::save_settings,
            commands::clear_titles,
            commands::list_categories,
            commands::set_app_category,
            commands::export_data,
            commands::delete_all_data,
            commands::get_autostart,
            commands::set_autostart,
        ])
        .build(tauri::generate_context!())
        .expect("error while building Stint")
        .run(|app, event| match event {
            RunEvent::Exit => {
                let state = app.state::<AppState>();
                state.tracker.close(&state.db);
            }
            #[cfg(target_os = "macos")]
            RunEvent::Reopen { .. } => show_main(app),
            _ => {}
        });
}
