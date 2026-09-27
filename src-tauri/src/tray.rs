use tauri::menu::{Menu, MenuItem};
use tauri::tray::{TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

/// Used until the user chooses otherwise, and the fallback if a stored
/// accelerator turns out to be unregistrable at startup.
pub const DEFAULT_SPOTLIGHT_SHORTCUT: &str = "CmdOrCtrl+Shift+O";

/// `app_settings` key holding the user's chosen accelerator.
pub const SPOTLIGHT_SHORTCUT_KEY: &str = "spotlight_shortcut";

fn focus_main(app: &AppHandle) {
    if let Some(main) = app.get_webview_window("main") {
        let _ = main.unminimize();
        let _ = main.show();
        let _ = main.set_focus();
    }
}

fn toggle_main(app: &AppHandle) {
    if let Some(main) = app.get_webview_window("main") {
        if main.is_visible().unwrap_or(false) {
            let _ = main.hide();
        } else {
            focus_main(app);
        }
    }
}

pub fn toggle_spotlight(app: &AppHandle) {
    let Some(spotlight) = app.get_webview_window("spotlight") else {
        return;
    };

    if spotlight.is_visible().unwrap_or(false) {
        let _ = spotlight.hide();
    } else {
        let _ = spotlight.center();
        let _ = spotlight.show();
        let _ = spotlight.set_focus();
    }
}

pub fn setup_tray(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let open = MenuItem::with_id(app, "open", "Open Orion", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &quit])?;

    TrayIconBuilder::with_id("orion-tray")
        .icon(
            app.default_window_icon()
                .expect("embedded default icon must exist")
                .clone(),
        )
        .tooltip("Orion")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => focus_main(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if matches!(event, TrayIconEvent::DoubleClick { .. }) {
                toggle_main(tray.app_handle());
            }
        })
        .build(app)?;

    Ok(())
}

/// The plugin is built with no shortcuts registered.
///
/// Registration happens at runtime in [`apply_spotlight_shortcut`] instead,
/// because `with_shortcuts` is fixed at build time and the accelerator has to be
/// changeable from Settings. The handler ignores which shortcut fired: only one
/// is ever registered, so whatever comes through is the spotlight key.
pub fn init_global_shortcut() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    tauri_plugin_global_shortcut::Builder::new()
        .with_handler(|app, _shortcut, event| {
            if event.state == ShortcutState::Pressed {
                toggle_spotlight(app);
            }
        })
        .build()
}

/// Makes `accelerator` the spotlight shortcut, replacing any current one.
///
/// Only ever one shortcut is registered, so clearing first is safe and avoids
/// leaving the old key live if the user switches away from it. The accelerator
/// is parsed before anything is unregistered, so a malformed string cannot leave
/// the app with no shortcut at all.
pub fn apply_spotlight_shortcut(app: &AppHandle, accelerator: &str) -> Result<(), String> {
    use std::str::FromStr;
    use tauri_plugin_global_shortcut::Shortcut;

    let parsed = Shortcut::from_str(accelerator)
        .map_err(|e| format!("'{accelerator}' is not a valid shortcut: {e}"))?;

    let shortcuts = app.global_shortcut();
    let _ = shortcuts.unregister_all();

    shortcuts.register(parsed).map_err(|e| {
        // Almost always means another application already owns the combination.
        format!("Could not register '{accelerator}': {e}. Another app may already use it.")
    })?;

    log::info!("Spotlight shortcut registered as '{accelerator}'");
    Ok(())
}

#[tauri::command]
pub async fn show_main(app: AppHandle) -> Result<(), String> {
    focus_main(&app);
    Ok(())
}

#[tauri::command]
pub async fn hide_spotlight(app: AppHandle) -> Result<(), String> {
    if let Some(spotlight) = app.get_webview_window("spotlight") {
        let _ = spotlight.hide();
    }
    Ok(())
}
