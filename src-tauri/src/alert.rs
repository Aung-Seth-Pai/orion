//! The timer alert window.
//!
//! Windows toasts are not a dependable way to tell someone their pomodoro
//! finished. They can be delivered and still never appear as a banner — routed
//! straight to the notification centre by Focus Assist, by a per-app setting,
//! or by policy — and the app has no way to know that happened. The symptom is
//! exactly what it looks like from the outside: a faint sound and no pop-up.
//!
//! So the visible notification is Orion's own: a small frameless always-on-top
//! window it owns outright, which cannot be suppressed by notification
//! settings. The native toast is still attempted alongside it, because when it
//! does work it is the better citizen — it persists in the notification centre.

use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

/// What the alert window renders. Kept in Rust rather than passed only as an
/// event payload so the webview can ask for it on mount: an event emitted
/// before the window's listener is attached would otherwise be lost, leaving a
/// visible but blank alert.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlertPayload {
    pub title: String,
    pub body: String,
    /// Distinguishes consecutive alerts with identical text, so the frontend
    /// can restart its dismiss timer rather than treat the second as a repeat
    /// render of the first.
    pub nonce: u64,
}

/// The alert currently being shown, if any.
#[derive(Default)]
pub struct PendingAlert(pub Mutex<Option<AlertPayload>>);

const ALERT_WINDOW: &str = "alert";
const MAX_ALERT_CHARS: usize = 200;

/// Parks the alert in the bottom-right of the work area, where Windows puts its
/// own toasts, instead of wherever the window happened to be created.
///
/// Positioned on every show rather than once, because the monitor it should
/// appear on can change between alerts and the work area moves with the
/// taskbar. Physical pixels throughout, which is what both `work_area` and
/// `outer_size` report.
fn park_bottom_right(window: &tauri::WebviewWindow) {
    /// Gap from the screen edges, in logical pixels, matching the inset Windows
    /// leaves around its own notifications.
    const MARGIN: f64 = 12.0;

    let Ok(Some(monitor)) = window.current_monitor().or_else(|_| window.primary_monitor()) else {
        return;
    };
    let Ok(size) = window.outer_size() else {
        return;
    };

    let work = monitor.work_area();
    let margin = (MARGIN * monitor.scale_factor()) as i32;

    let x = work.position.x + work.size.width as i32 - size.width as i32 - margin;
    let y = work.position.y + work.size.height as i32 - size.height as i32 - margin;

    if let Err(e) = window.set_position(tauri::PhysicalPosition::new(x, y)) {
        log::warn!("Could not position the alert window: {e}");
    }
}

/// Shows the alert window with `title`/`body`.
///
/// Unlike the toast path this reports failure, because it is the notification
/// the user is relying on — if this cannot be shown they should be told rather
/// than left wondering why the timer went quiet.
#[tauri::command]
pub async fn show_timer_alert(app: AppHandle, title: String, body: String) -> Result<(), String> {
    if title.chars().count() > MAX_ALERT_CHARS || body.chars().count() > MAX_ALERT_CHARS {
        return Err("Alert text is too long".into());
    }

    let payload = AlertPayload {
        title,
        body,
        // Monotonic within a run, which is all the frontend needs to tell two
        // alerts apart.
        nonce: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0),
    };

    // Stored before the window is shown, so a webview that mounts and asks for
    // the payload immediately always finds it.
    if let Some(state) = app.try_state::<PendingAlert>() {
        if let Ok(mut slot) = state.0.lock() {
            *slot = Some(payload.clone());
        }
    }

    let window = app
        .get_webview_window(ALERT_WINDOW)
        .ok_or_else(|| "alert window is missing".to_string())?;

    // Emitted as well as stored: a window that is already open and showing a
    // previous alert needs to swap its contents, not re-mount.
    let _ = window.emit("orion-timer-alert", &payload);

    // Before showing, so it never appears in one place and jumps to another.
    park_bottom_right(&window);

    window
        .show()
        .map_err(|e| format!("failed to show the alert window: {e}"))?;
    // Deliberately not set_focus: stealing focus would interrupt whatever the
    // user is typing, which is the opposite of helpful. alwaysOnTop is enough
    // to make it visible.
    let _ = window.set_always_on_top(true);

    log::info!("Timer alert shown");
    Ok(())
}

/// Hides the alert window. Called when the alert auto-dismisses or is clicked.
#[tauri::command]
pub async fn hide_timer_alert(app: AppHandle) -> Result<(), String> {
    if let Some(state) = app.try_state::<PendingAlert>() {
        if let Ok(mut slot) = state.0.lock() {
            *slot = None;
        }
    }
    if let Some(window) = app.get_webview_window(ALERT_WINDOW) {
        let _ = window.hide();
    }
    Ok(())
}

/// The alert the window should currently be displaying, if any. Called by the
/// alert webview on mount to close the gap between `show` and its listener
/// being ready.
#[tauri::command]
pub async fn get_pending_alert(app: AppHandle) -> Result<Option<AlertPayload>, String> {
    let Some(state) = app.try_state::<PendingAlert>() else {
        return Ok(None);
    };
    let slot = state
        .0
        .lock()
        .map_err(|_| "pending alert mutex poisoned".to_string())?;
    Ok(slot.clone())
}
