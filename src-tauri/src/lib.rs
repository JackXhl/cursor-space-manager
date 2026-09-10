pub mod commands;
pub mod core;
pub mod error;
pub mod platforms;
pub mod settings;
pub mod util;

use crate::commands::AppState;
use crate::core::journal::Journal;
use crate::settings::SettingsStore;

use std::sync::Arc;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            // State lives in this tool's own app-data directory, never inside a
            // Cursor directory that might itself be migrated mid-write.
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;

            let journal = Arc::new(Journal::open(&data_dir.join("journal.sqlite"))?);
            let (store, load) = SettingsStore::new(data_dir.join("settings.json"));
            let settings = Arc::new(store);

            app.manage(AppState::new(journal, settings, load.message));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_startup_state,
            commands::get_build_info,
            commands::get_settings,
            commands::save_settings,
            commands::reset_settings,
            commands::reload_settings,
            commands::start_scan,
            commands::cancel_scan,
            commands::get_latest_scan,
            commands::list_volumes,
            commands::probe_target_directory,
            commands::check_quiescence,
            commands::create_plan,
            commands::get_active_plan,
            commands::start_migration,
            commands::cancel_migration,
            commands::cleanup_backups,
            commands::undo_migration,
            commands::get_history,
            commands::get_operation,
            commands::get_recovery_reports,
            commands::diagnose_operation,
            commands::export_diagnostics,
            commands::check_for_updates,
            commands::install_update,
        ])
        .run(tauri::generate_context!())
        .expect("error while running cursor-space-manager");
}
