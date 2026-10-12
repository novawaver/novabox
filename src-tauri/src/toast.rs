//! Desktop notifications under Novabox's own name and icon.
//!
//! The notification plugin falls back to PowerShell's identity whenever the exe runs from
//! `target\debug` or `target\release`, so Windows labels the toast "Windows PowerShell".
//! Registering our AppUserModelID under HKCU lets an unpackaged build show toasts as Novabox.

use std::path::Path;

use tauri::AppHandle;

#[cfg(windows)]
pub fn register(identifier: &str, data_dir: &Path) {
    let icon = data_dir.join("notify-icon.png");
    if let Err(err) = std::fs::write(&icon, include_bytes!("../icons/128x128.png")) {
        eprintln!("Novabox toast icon: {err}");
    }
    let path = format!(r"Software\Classes\AppUserModelId\{identifier}");
    let result = windows_registry::CURRENT_USER.create(&path).and_then(|key| {
        key.set_string("DisplayName", "Novabox")?;
        key.set_string("IconUri", icon.to_string_lossy())
    });
    if let Err(err) = result {
        eprintln!("Novabox toast registration: {err}");
    }
}

#[cfg(not(windows))]
pub fn register(_identifier: &str, _data_dir: &Path) {}

#[tauri::command]
pub fn notify_show(app: AppHandle, title: String, body: String) -> Result<(), String> {
    #[cfg(windows)]
    {
        let identifier = app.config().identifier.clone();
        // Showing a toast is a blocking WinRT call; keep it off the command thread.
        std::thread::spawn(move || {
            if let Err(err) = tauri_winrt_notification::Toast::new(&identifier).title(&title).text1(&body).show() {
                eprintln!("Novabox toast: {err}");
            }
        });
        Ok(())
    }
    #[cfg(not(windows))]
    {
        use tauri_plugin_notification::NotificationExt;
        app.notification().builder().title(title).body(body).show().map_err(|err| err.to_string())
    }
}
