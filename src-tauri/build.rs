// Declaring the app's commands here makes Tauri generate one permission per
// command; the webview can only call commands granted in `capabilities/`.
const COMMANDS: &[&str] = &[
    "get_summary",
    "get_interval_totals",
    "get_status",
    "set_paused",
    "start_focus",
    "stop_focus",
    "get_settings",
    "save_settings",
    "clear_titles",
    "list_categories",
    "set_app_category",
    "export_data",
    "delete_all_data",
    "get_autostart",
    "set_autostart",
];

fn main() {
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(COMMANDS)),
    )
    .expect("failed to run tauri-build");
}
