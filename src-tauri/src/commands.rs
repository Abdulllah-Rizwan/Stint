//! IPC commands callable from the UI. Every argument is validated here:
//! the webview is treated as untrusted input.

use std::path::PathBuf;

use serde::Serialize;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_autostart::ManagerExt;

use crate::categories::{self, Category};
use crate::db::{self, ManualSession};
use crate::settings::{clean_text, Settings, MAX_APP_NAME_LEN};
use crate::summary::{self, Categorizer, IntervalTotal, Summary};
use crate::tracker::{unix_now, Current};
use crate::{export, lock, AppState};

type CmdResult<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// Upper bound on any queried range, to keep queries cheap.
const MAX_RANGE_SECS: i64 = 400 * 86_400;

fn check_range(from: i64, to: i64) -> CmdResult<()> {
    if to <= from || to - from > MAX_RANGE_SECS || from < 0 {
        return Err("Invalid time range".into());
    }
    Ok(())
}

fn build_summary(state: &AppState, from: i64, to: i64) -> CmdResult<Summary> {
    state.tracker.flush(&state.db);
    let settings = state.tracker.settings();
    let conn = lock(&state.db);
    let rows = db::activity_in_range(&conn, from, to).map_err(err)?;
    let manual = db::focus_in_range(&conn, from, to).map_err(err)?;
    let overrides = db::category_overrides(&conn).map_err(err)?;
    Ok(summary::compute(
        &rows,
        manual,
        &Categorizer::new(&overrides),
        &settings,
        from,
        to,
    ))
}

#[tauri::command]
pub fn get_summary(state: State<'_, AppState>, from: i64, to: i64) -> CmdResult<Summary> {
    check_range(from, to)?;
    build_summary(&state, from, to)
}

/// Totals for consecutive intervals given by `boundaries` (e.g. local midnights),
/// so the UI controls time zones and the backend stays time-zone agnostic.
#[tauri::command]
pub fn get_interval_totals(
    state: State<'_, AppState>,
    boundaries: Vec<i64>,
) -> CmdResult<Vec<IntervalTotal>> {
    if boundaries.len() < 2 || boundaries.len() > 400 {
        return Err("Invalid boundaries".into());
    }
    if boundaries.windows(2).any(|w| w[1] <= w[0]) {
        return Err("Boundaries must be increasing".into());
    }
    let (first, last) = (boundaries[0], boundaries[boundaries.len() - 1]);
    check_range(first, last)?;

    state.tracker.flush(&state.db);
    let settings = state.tracker.settings();
    let conn = lock(&state.db);
    let rows = db::activity_in_range(&conn, first, last).map_err(err)?;
    let overrides = db::category_overrides(&conn).map_err(err)?;
    drop(conn);
    let cats = Categorizer::new(&overrides);
    Ok(boundaries
        .windows(2)
        .map(|w| {
            let s = summary::compute(&rows, vec![], &cats, &settings, w[0], w[1]);
            IntervalTotal {
                from: w[0],
                to: w[1],
                total_secs: s.total_secs,
                productive_secs: s.productive_secs,
                focus_secs: s.focus_secs,
            }
        })
        .collect())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    paused: bool,
    current: Option<Current>,
    active_focus: Option<ManualSession>,
    now: i64,
}

#[tauri::command]
pub fn get_status(state: State<'_, AppState>) -> CmdResult<Status> {
    let active_focus = db::active_focus(&lock(&state.db)).map_err(err)?;
    Ok(Status {
        paused: state
            .tracker
            .paused
            .load(std::sync::atomic::Ordering::SeqCst),
        current: state.tracker.current(),
        active_focus,
        now: unix_now(),
    })
}

#[tauri::command]
pub fn set_paused(app: AppHandle, paused: bool) -> CmdResult<()> {
    crate::apply_pause(&app, paused);
    Ok(())
}

#[tauri::command]
pub fn start_focus(
    state: State<'_, AppState>,
    label: String,
    target_minutes: u32,
) -> CmdResult<ManualSession> {
    if !(1..=480).contains(&target_minutes) {
        return Err("Target must be between 1 and 480 minutes".into());
    }
    let label = clean_text(&label, 120);
    db::start_focus(&lock(&state.db), unix_now(), target_minutes, &label).map_err(err)
}

#[tauri::command]
pub fn stop_focus(state: State<'_, AppState>) -> CmdResult<()> {
    db::stop_focus(&lock(&state.db), unix_now()).map_err(err)
}

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Settings {
    state.tracker.settings()
}

#[tauri::command]
pub fn save_settings(state: State<'_, AppState>, settings: Settings) -> CmdResult<Settings> {
    let settings = settings.normalized();
    settings.validate()?;
    let was_capturing = state.tracker.settings().capture_titles;
    {
        let conn = lock(&state.db);
        db::save_settings(&conn, &settings).map_err(err)?;
        db::apply_retention(&conn, settings.retention_days, unix_now()).map_err(err)?;
    }
    *state
        .tracker
        .settings
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = settings.clone();
    if was_capturing && !settings.capture_titles {
        state.tracker.drop_titles(&state.db);
    }
    Ok(settings)
}

/// Permanently erase all stored window titles.
#[tauri::command]
pub fn clear_titles(state: State<'_, AppState>) -> CmdResult<()> {
    state.tracker.close(&state.db);
    let conn = lock(&state.db);
    db::clear_titles(&conn).map_err(err)?;
    conn.execute_batch("VACUUM;").map_err(err)
}

#[tauri::command]
pub fn list_categories() -> Vec<Category> {
    categories::CATEGORIES.to_vec()
}

#[tauri::command]
pub fn set_app_category(
    state: State<'_, AppState>,
    app: String,
    category: String,
) -> CmdResult<()> {
    let app = clean_text(&app, MAX_APP_NAME_LEN);
    if app.is_empty() {
        return Err("App name is empty".into());
    }
    if !categories::is_valid(&category) {
        return Err("Unknown category".into());
    }
    db::set_category(&lock(&state.db), &app, &category).map_err(err)
}

/// Writes an export file to the user's Downloads folder and returns its path.
#[tauri::command]
pub fn export_data(
    app: AppHandle,
    state: State<'_, AppState>,
    format: String,
) -> CmdResult<String> {
    let (ext, render): (&str, fn(&[db::ActivityRow], &_) -> String) = match format.as_str() {
        "csv" => ("csv", export::to_csv),
        "json" => ("json", export::to_json),
        _ => return Err("Unknown format".into()),
    };
    state.tracker.flush(&state.db);
    let (rows, overrides) = {
        let conn = lock(&state.db);
        (
            db::activity_in_range(&conn, i64::MIN, i64::MAX).map_err(err)?,
            db::category_overrides(&conn).map_err(err)?,
        )
    };
    let body = render(&rows, &overrides);

    let dir: PathBuf = app
        .path()
        .download_dir()
        .or_else(|_| app.path().app_data_dir())
        .map_err(err)?;
    std::fs::create_dir_all(&dir).map_err(err)?;
    let path = dir.join(format!("stint-export-{}.{ext}", unix_now()));
    crate::write_private_file(&path, body.as_bytes()).map_err(err)?;
    Ok(path.display().to_string())
}

#[tauri::command]
pub fn delete_all_data(state: State<'_, AppState>) -> CmdResult<()> {
    state.tracker.close(&state.db);
    db::delete_all(&lock(&state.db)).map_err(err)
}

#[tauri::command]
pub fn get_autostart(app: AppHandle) -> bool {
    app.autolaunch().is_enabled().unwrap_or(false)
}

#[tauri::command]
pub fn set_autostart(app: AppHandle, enabled: bool) -> CmdResult<()> {
    let al = app.autolaunch();
    if enabled { al.enable() } else { al.disable() }.map_err(err)
}
