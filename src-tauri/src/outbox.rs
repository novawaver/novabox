//! Mail actions take effect on this PC at once, so the window never waits on the network.
//! Gmail is told afterwards, one change at a time in the order they were made, so an
//! action and its undo can't reach Gmail the wrong way round. A change Gmail refuses is
//! rolled back here and reported with a `mail://action-failed` event.

use serde_json::json;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};

use crate::commands::{local_change, remote_change, with_db, Change};
use crate::db;
use crate::model::ThreadTarget;

pub enum Job {
    /// A label change already applied locally; `before` is each message's labels before it.
    Change {
        target: ThreadTarget,
        change: Change,
        before: Vec<(String, Vec<String>)>,
    },
}

pub struct Outbox(UnboundedSender<Job>);

pub fn start(app: &AppHandle) -> Outbox {
    let (sender, mut jobs) = unbounded_channel::<Job>();
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        while let Some(job) = jobs.recv().await {
            run(&app, job).await;
        }
    });
    Outbox(sender)
}

pub fn push(app: &AppHandle, job: Job) {
    let _ = app.state::<Outbox>().0.send(job);
}

async fn run(app: &AppHandle, job: Job) {
    match job {
        Job::Change { target, change, before } => match remote_change(app, &target, &change).await {
            // Applying it again undoes a sync that stored Gmail's old labels in the meantime.
            Ok(()) => {
                let _ = local_change(app, &target, &change);
            }
            // The conversation is gone from Gmail; the next sync clears it here too.
            Err(err) if err.starts_with("gmail_404") => {}
            Err(err) => {
                let _ = with_db(app, |conn| db::restore_labels(conn, &target.account_id, &before));
                fail(app, &format!("Gmail didn't accept that change, so it was undone. {err}"));
            }
        },
    }
}

fn refreshed(app: &AppHandle) {
    crate::refresh_badge(app);
    let _ = app.emit("mail://updated", json!({}));
}

fn fail(app: &AppHandle, message: &str) {
    refreshed(app);
    let _ = app.emit("mail://action-failed", json!({ "message": message }));
}
