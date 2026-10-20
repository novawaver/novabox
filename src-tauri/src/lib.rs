use std::collections::{HashMap, HashSet};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Manager, WindowEvent};

mod auth;
mod backup;
mod commands;
mod db;
mod files;
mod gmail;
mod later;
mod mime_build;
mod model;
mod outbox;
mod parse;
mod popup;
mod secrets;
mod settings;
mod sync;
mod toast;
mod util;

pub(crate) struct AppState {
    pub db: Mutex<rusqlite::Connection>,
    pub http: reqwest::Client,
    pub tokens: Mutex<HashMap<String, auth::CachedToken>>,
    pub data_dir: std::path::PathBuf,
    pub syncing_accounts: Mutex<HashSet<String>>,
    pub sync_all_running: AtomicBool,
    pub close_to_tray: AtomicBool,
    pub due_running: AtomicBool,
    pub sign_in_cancel: Arc<AtomicBool>,
}

pub fn refresh_badge(app: &tauri::AppHandle) {
    let unread = app
        .state::<AppState>()
        .db
        .lock()
        .ok()
        .and_then(|db| db::total_unread(&db).ok())
        .unwrap_or(0);
    if let Some(window) = app.get_webview_window("main") {
        let title = if unread > 0 {
            format!("Novabox ({unread})")
        } else {
            "Novabox".into()
        };
        let _ = window.set_title(&title);
    }
    if let Some(tray) = app.tray_by_id("novabox") {
        let tip = if unread > 0 {
            format!("Novabox — {unread} unread")
        } else {
            "Novabox".into()
        };
        let _ = tray.set_tooltip(Some(tip));
    }
}

fn reveal(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

fn setup_tray(app: &tauri::App) -> bool {
    let Some(icon) = app.default_window_icon().cloned() else {
        eprintln!("Novabox: no window icon, so the tray icon is disabled.");
        return false;
    };
    let show = match MenuItem::with_id(app, "show", "Open Novabox", true, None::<&str>) {
        Ok(item) => item,
        Err(err) => {
            eprintln!("Novabox tray: {err}");
            return false;
        }
    };
    let sync_item = match MenuItem::with_id(app, "sync", "Sync now", true, None::<&str>) {
        Ok(item) => item,
        Err(err) => {
            eprintln!("Novabox tray: {err}");
            return false;
        }
    };
    let quit = match MenuItem::with_id(app, "quit", "Quit Novabox", true, None::<&str>) {
        Ok(item) => item,
        Err(err) => {
            eprintln!("Novabox tray: {err}");
            return false;
        }
    };
    let menu = match Menu::with_items(app, &[&show, &sync_item, &quit]) {
        Ok(menu) => menu,
        Err(err) => {
            eprintln!("Novabox tray: {err}");
            return false;
        }
    };
    let built = TrayIconBuilder::with_id("novabox")
        .tooltip("Novabox")
        .icon(icon)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => reveal(app),
            "sync" => {
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    let _ = sync::sync_all(&app).await;
                });
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
                reveal(tray.app_handle());
            }
        })
        .build(app);
    match built {
        Ok(_) => true,
        Err(err) => {
            eprintln!("Novabox tray: {err}");
            false
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .manage(popup::PopupState::default())
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            toast::register(&app.config().identifier, &dir);
            let db = db::open(&dir.join("novabox.db"))?;
            let http = reqwest::Client::builder()
                .user_agent("Novabox/0.1")
                .timeout(Duration::from_secs(120))
                // Google's front end rejects HTTP/2 posts that omit Content-Length.
                .http1_only()
                .build()
                .map_err(|err| std::io::Error::other(err.to_string()))?;
            app.manage(AppState {
                db: Mutex::new(db),
                http,
                tokens: Mutex::new(HashMap::new()),
                data_dir: dir,
                syncing_accounts: Mutex::new(HashSet::new()),
                sync_all_running: AtomicBool::new(false),
                close_to_tray: AtomicBool::new(false),
                due_running: AtomicBool::new(false),
                sign_in_cancel: Arc::new(AtomicBool::new(false)),
            });
            let outbox = outbox::start(app.handle());
            app.manage(outbox);
            if setup_tray(app) {
                app.state::<AppState>()
                    .close_to_tray
                    .store(true, std::sync::atomic::Ordering::Relaxed);
            }
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                let startup = handle.clone();
                tauri::async_runtime::spawn(async move {
                    let _ = later::release_due(&startup).await;
                    let _ = sync::sync_all(&startup).await;
                });
                std::thread::sleep(Duration::from_secs(6));
                // Every 10s: a cheap history check for new mail. Every 2 min: the full sync
                // (labels, older mail, spam/trash, drafts).
                let mut ticks = 0u32;
                loop {
                    ticks += 1;
                    let handle = handle.clone();
                    let full_sync_due = ticks % 12 == 0;
                    tauri::async_runtime::spawn(async move {
                        let _ = later::release_due(&handle).await;
                        if full_sync_due {
                            let _ = sync::sync_all(&handle).await;
                        } else {
                            sync::check_new_mail(&handle).await;
                        }
                    });
                    std::thread::sleep(Duration::from_secs(10));
                }
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() == popup::LABEL {
                // The pop-up really closes; it is not sent to the tray.
                if let WindowEvent::Destroyed = event {
                    popup::on_destroyed(window.app_handle());
                }
                return;
            }
            if let WindowEvent::CloseRequested { api, .. } = event {
                let hide = window
                    .try_state::<AppState>()
                    .map(|state| state.close_to_tray.load(std::sync::atomic::Ordering::Relaxed))
                    .unwrap_or(false);
                if hide {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::settings_get,
            commands::settings_save,
            commands::mailbox_query,
            commands::thread_get,
            commands::sign_in,
            commands::sign_in_cancel,
            commands::account_remove,
            commands::account_set_signature,
            commands::threads_modify,
            commands::mail_send,
            commands::draft_save,
            commands::draft_delete,
            commands::remote_search,
            commands::sync_now,
            commands::check_connection,
            commands::attachment_get,
            commands::attachment_save,
            commands::threads_snooze,
            commands::threads_unsnooze,
            commands::mail_schedule,
            commands::schedule_cancel,
            commands::schedule_send_now,
            commands::templates_list,
            commands::template_save,
            commands::template_delete,
            toast::notify_show,
            backup::backup_export,
            backup::backup_import,
            popup::popup_push,
            popup::popup_list,
            popup::popup_layout,
            popup::popup_dismiss,
            popup::popup_open
        ])
        .run(tauri::generate_context!())
        .expect("error while running Novabox");
}
