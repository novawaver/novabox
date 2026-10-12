//! Novabox's own notification pop-ups, in the style of Telegram Desktop: a borderless,
//! always-on-top window in a screen corner that never takes focus when it appears.
//!
//! The notices live here, not in the window, so the window can be closed when empty and
//! recreated later. Every change bumps a version; the window ignores anything older than
//! what it already shows, because an `invoke` reply and an event can arrive in either order.

use std::sync::Mutex;

use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindowBuilder};

pub const LABEL: &str = "popup";
const WIDTH: f64 = 380.0;
const MARGIN: f64 = 8.0;
const ESTIMATE: f64 = 120.0;

#[derive(Default)]
pub struct PopupState(Mutex<Inner>);

#[derive(Default)]
struct Inner {
    notices: Vec<Value>,
    corner: String,
    version: u64,
    /// A build is in flight; the new window reads the list itself once it loads.
    creating: bool,
    /// We asked the window to close. `close()` only queues the request, so the old window
    /// can still be registered for a moment; pushes during that gap wait for `on_destroyed`.
    closing: bool,
}

#[derive(Clone, Serialize)]
pub struct Snapshot {
    version: u64,
    corner: String,
    notices: Vec<Value>,
}

enum Next {
    Emit(Snapshot),
    Create,
    Close,
    Wait,
}

fn snapshot(inner: &Inner) -> Snapshot {
    Snapshot {
        version: inner.version,
        corner: if inner.corner.is_empty() { "bottomRight".into() } else { inner.corner.clone() },
        notices: inner.notices.clone(),
    }
}

/// Applies a change to the list, then shows, updates or closes the window to match.
fn update(app: &AppHandle, change: impl FnOnce(&mut Inner)) {
    let exists = app.get_webview_window(LABEL).is_some();
    let next = {
        let state = app.state::<PopupState>();
        let Ok(mut inner) = state.0.lock() else {
            return;
        };
        change(&mut inner);
        inner.version += 1;
        if inner.closing || inner.creating {
            Next::Wait
        } else if inner.notices.is_empty() {
            if exists {
                inner.closing = true;
                Next::Close
            } else {
                Next::Wait
            }
        } else if exists {
            Next::Emit(snapshot(&inner))
        } else {
            inner.creating = true;
            Next::Create
        }
    };
    match next {
        Next::Emit(snapshot) => {
            let _ = app.emit_to(LABEL, "popup://changed", snapshot);
        }
        Next::Close => {
            if let Some(window) = app.get_webview_window(LABEL) {
                let _ = window.close();
            }
        }
        Next::Create => spawn_window(app),
        Next::Wait => {}
    }
}

/// Builds the window off the main thread (a build on it deadlocks on Windows).
fn spawn_window(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let built = build_window(&app);
        let retry = {
            let state = app.state::<PopupState>();
        let Ok(mut inner) = state.0.lock() else {
                return;
            };
            inner.creating = false;
            if let Err(err) = &built {
                eprintln!("Novabox pop-up: {err}");
            }
            // Anything pushed while building is picked up by the window's own popup_list.
            // If everything was dismissed meanwhile, or the build failed, settle it now.
            built.is_ok() && inner.notices.is_empty()
        };
        if retry {
            update(&app, |_| {});
        }
    });
}

fn build_window(app: &AppHandle) -> Result<(), String> {
    // Created visible and unfocused in one step: tao only honours "don't activate" while the
    // window is being created, so a later show() would steal focus from whatever you're doing.
    let mut builder = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("index.html".into()))
        .title("Novabox notifications")
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .focused(false)
        .visible(true)
        .inner_size(WIDTH, ESTIMATE);
    if let Some((position, _, scale)) = geometry(app, ESTIMATE) {
        builder = builder.position(position.x as f64 / scale, position.y as f64 / scale);
    }
    builder.build().map(|_| ()).map_err(|e| e.to_string())
}

/// Called from the window-event hook once the pop-up window is gone.
pub fn on_destroyed(app: &AppHandle) {
    let ours = {
        let state = app.state::<PopupState>();
        let Ok(mut inner) = state.0.lock() else {
            return;
        };
        let ours = inner.closing;
        inner.closing = false;
        ours
    };
    if ours {
        // Something may have arrived while it was closing; reopen for it.
        update(app, |_| {});
    } else {
        // Closed from outside (Alt+F4 and the like): treat that as dismissing everything.
        update(app, |inner| inner.notices.clear());
    }
}

/// Where the pop-up goes for a given content height: the chosen corner of the primary
/// monitor's work area (the screen minus the taskbar). Returns physical pixels and the scale.
fn geometry(app: &AppHandle, height: f64) -> Option<(PhysicalPosition<i32>, PhysicalSize<u32>, f64)> {
    let monitor = app.primary_monitor().ok()??;
    let scale = monitor.scale_factor();
    let area = monitor.work_area();
    let width = (WIDTH * scale).round() as i32;
    let height = (height.max(40.0) * scale).round().min(area.size.height as f64) as i32;
    let margin = (MARGIN * scale).round() as i32;
    let corner = app.state::<PopupState>().0.lock().map(|inner| inner.corner.clone()).unwrap_or_default();
    let left = corner.ends_with("Left");
    let top = corner.starts_with("top");
    let x = if left { area.position.x + margin } else { area.position.x + area.size.width as i32 - width - margin };
    let y = if top { area.position.y + margin } else { area.position.y + area.size.height as i32 - height - margin };
    Some((PhysicalPosition::new(x, y), PhysicalSize::new(width as u32, height as u32), scale))
}

#[tauri::command]
pub async fn popup_push(app: AppHandle, notice: Value, corner: String, max: usize) -> Result<(), String> {
    update(&app, |inner| {
        inner.corner = corner;
        inner.notices.push(notice);
        let keep = max.clamp(1, 5);
        if inner.notices.len() > keep {
            let extra = inner.notices.len() - keep;
            inner.notices.drain(0..extra);
        }
    });
    Ok(())
}

#[tauri::command]
pub fn popup_list(app: AppHandle) -> Snapshot {
    app.state::<PopupState>()
        .0
        .lock()
        .map(|inner| snapshot(&inner))
        .unwrap_or(Snapshot { version: 0, corner: "bottomRight".into(), notices: Vec::new() })
}

/// Called by the pop-up after it renders, with its content height in CSS pixels.
#[tauri::command]
pub fn popup_layout(app: AppHandle, height: f64) -> Result<(), String> {
    let (Some(window), Some((position, size, _))) = (app.get_webview_window(LABEL), geometry(&app, height)) else {
        return Ok(());
    };
    window.set_size(size).map_err(|e| e.to_string())?;
    window.set_position(position).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn popup_dismiss(app: AppHandle, id: String) {
    update(&app, |inner| {
        inner.notices.retain(|notice| notice.get("id").and_then(|v| v.as_str()) != Some(id.as_str()));
    });
}

/// Brings Novabox forward and asks the main window to open the notice's conversation.
#[tauri::command]
pub fn popup_open(app: AppHandle, notice: Value) {
    if let Some(id) = notice.get("id").and_then(|v| v.as_str()) {
        popup_dismiss(app.clone(), id.to_string());
    }
    if let Some(main) = app.get_webview_window("main") {
        let _ = main.unminimize();
        let _ = main.show();
        let _ = main.set_focus();
    }
    let _ = app.emit_to("main", "popup://open", notice);
}
