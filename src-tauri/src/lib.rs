// Public so the Phase 1 embedding surface is reachable from the crate root;
// nothing calls it yet, that lands with the search wiring in Phase 2.
pub mod ai;
mod commands;
mod db;
mod models;
mod tray;

use std::path::PathBuf;

use commands::{
    create_resource, create_script, create_task, create_workspace, delete_env_var, delete_resource,
    delete_script, delete_task, delete_workspace, execute_script, export_all_data, get_ai_status,
    get_env_vars, get_resources, get_scripts, get_settings, get_tasks,
    get_workspace_time, get_workspaces, import_data, launch_resource, log_timer_session,
    notify_timer_complete, open_log_folder, reindex_all, reset_workspace_time, search_all,
    set_env_var, toggle_autostart, update_resource, update_script, update_setting,
    update_task_status, update_workspace,
};
use db::Db;
use tauri::{AppHandle, Manager};
use tauri_plugin_autostart::MacosLauncher;
use tauri_plugin_log::{RotationStrategy, Target, TargetKind};
use tray::{hide_spotlight, show_main};

/// One-time migration: the app used the `com.orion.cockpit` identifier
/// before Milestone 7. Copy the SQLite database into the new identifier's
/// data directory so existing users keep their workspaces.
fn migrate_legacy_database(app: &AppHandle) {
    use std::fs;

    let Ok(new_dir) = app.path().app_data_dir() else {
        return;
    };
    if new_dir.join("data.db").exists() {
        return;
    }

    let Some(legacy_dir) = std::env::var("APPDATA")
        .ok()
        .map(|appdata| PathBuf::from(appdata).join("com.orion.cockpit"))
    else {
        return;
    };

    if !legacy_dir.join("data.db").exists() {
        return;
    }

    if fs::create_dir_all(&new_dir).is_err() {
        return;
    }

    for suffix in ["data.db", "data.db-wal", "data.db-shm"] {
        let from = legacy_dir.join(suffix);
        let to = new_dir.join(suffix);
        if from.exists() && fs::copy(&from, &to).is_ok() {
            log::info!("Migrated {} from legacy data directory", suffix);
        }
    }
}

pub fn run() {
    tauri::Builder::default()
        // MUST be the first registered plugin: a second launch is dropped
        // immediately and the primary instance surfaces its main window.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            log::info!("Second instance launched — focusing primary instead");
            if let Some(main) = app.get_webview_window("main") {
                let _ = main.unminimize();
                let _ = main.show();
                let _ = main.set_focus();
            }
        }))
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Info)
                .max_file_size(2_000_000)
                .rotation_strategy(RotationStrategy::KeepSome(3))
                .targets([
                    Target::new(TargetKind::Stdout),
                    Target::new(TargetKind::LogDir {
                        file_name: Some("orion".into()),
                    }),
                ])
                .build(),
        )
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tray::init_global_shortcut())
        .setup(|app| {
            migrate_legacy_database(app.handle());
            let database = Db::initialize(app.handle())?;
            app.manage(database);
            tray::setup_tray(app.handle())?;
            log::info!("Orion started (v{})", app.package_info().version);
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the main window hides it to the tray instead of exiting.
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    api.prevent_close();
                    let _ = window.hide();
                    log::info!("Main window hidden to tray");
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_workspaces,
            create_workspace,
            update_workspace,
            delete_workspace,
            get_resources,
            create_resource,
            update_resource,
            delete_resource,
            launch_resource,
            get_scripts,
            create_script,
            update_script,
            delete_script,
            execute_script,
            log_timer_session,
            get_workspace_time,
            reset_workspace_time,
            notify_timer_complete,
            search_all,
            show_main,
            hide_spotlight,
            get_settings,
            update_setting,
            export_all_data,
            import_data,
            get_tasks,
            create_task,
            update_task_status,
            delete_task,
            get_env_vars,
            set_env_var,
            delete_env_var,
            toggle_autostart,
            open_log_folder,
            get_ai_status,
            reindex_all
        ])
        .run(tauri::generate_context!())
        .expect("error while running orion");
}
