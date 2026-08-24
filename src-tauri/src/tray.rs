use tauri::menu::{Menu, MenuItem};
use tauri::tray::{TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::ShortcutState;

pub const SPOTLIGHT_SHORTCUT: &str = "CmdOrCtrl+Shift+O";

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

pub fn init_global_shortcut() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    tauri_plugin_global_shortcut::Builder::new()
        .with_shortcuts([SPOTLIGHT_SHORTCUT])
        .expect("spotlight shortcut definition must be valid")
        .with_handler(|app, _shortcut, event| {
            if event.state == ShortcutState::Pressed {
                toggle_spotlight(app);
            }
        })
        .build()
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
