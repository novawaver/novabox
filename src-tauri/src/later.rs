use std::sync::atomic::Ordering;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::json;
use tauri::{AppHandle, Emitter, Manager};

use crate::commands::{self, wake_thread};
use crate::db;
use crate::AppState;

pub async fn release_due(app: &AppHandle) -> Result<(), String> {
    let running = &app.state::<AppState>().due_running;
    if running.swap(true, Ordering::AcqRel) {
        return Ok(());
    }
    let result = release_inner(app).await;
    running.store(false, Ordering::Release);
    result
}

async fn release_inner(app: &AppHandle) -> Result<(), String> {
    let now = now_millis();
    let due = with_db(app, |conn| db::snoozes_due(conn, now))?;
    let mut changed = false;
    for (account_id, thread_id) in due {
        match wake_thread(app, &account_id, &thread_id).await {
            Ok(()) => {
                changed = true;
                let _ = app.emit(
                    "mail://notify",
                    json!({
                        "kind": "snooze",
                        "title": "Novabox",
                        "body": "A snoozed conversation is back in the inbox.",
                        "accountId": account_id,
                        "threadId": thread_id,
                    }),
                );
            }
            Err(err) => eprintln!("Novabox snooze: {err}"),
        }
    }
    let waiting = with_db(app, |conn| db::scheduled_due(conn, now))?;
    for item in waiting {
        let subject = item.request.subject.clone();
        match commands::mail_send(app.clone(), item.request).await {
            Ok(_) => {
                let _ = with_db(app, |conn| db::schedule_delete(conn, &item.id));
                let _ = app.emit(
                    "mail://notify",
                    json!({ "kind": "sent", "title": "Novabox", "body": format!("Sent {subject}") }),
                );
                changed = true;
            }
            Err(err) => {
                let _ = with_db(app, |conn| db::schedule_delay(conn, &item.id, now + 120_000));
                let _ = app.emit(
                    "mail://notify",
                    json!({ "kind": "error", "title": "Novabox", "body": format!("Could not send {subject}. {err}") }),
                );
            }
        }
    }
    if changed {
        crate::refresh_badge(app);
        let _ = app.emit("mail://updated", json!({}));
    }
    Ok(())
}

fn with_db<T>(app: &AppHandle, op: impl FnOnce(&rusqlite::Connection) -> Result<T, String>) -> Result<T, String> {
    let state = app.state::<AppState>();
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    op(&conn)
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}
