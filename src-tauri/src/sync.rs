use std::collections::HashSet;
use std::time::{Duration, Instant};

use futures_util::future::join_all;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager};

use crate::db;
use crate::files;
use crate::gmail;
use crate::model::{Account, SyncProgress};
use crate::parse;
use crate::AppState;

pub async fn sync_all(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    if state.sync_all_running.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return Ok(());
    }
    let result = async {
        let ids = {
            let db = state.db.lock().map_err(|e| e.to_string())?;
            db::list_accounts(&db)?
                .into_iter()
                .map(|account| account.id)
                .collect::<Vec<_>>()
        };
        // Accounts sync side by side so one account's backfill never holds up another's new mail.
        join_all(ids.iter().map(|id| sync_account(app, id, false))).await;
        Ok(())
    }
    .await;
    app.state::<AppState>()
        .sync_all_running
        .store(false, std::sync::atomic::Ordering::SeqCst);
    result
}

/// Cheap new-mail check: only the Gmail history call, with no labels, backfill or spam/trash.
/// Runs often; accounts already mid-sync are skipped since that sync reads history itself.
pub async fn check_new_mail(app: &AppHandle) {
    let accounts = {
        let state = app.state::<AppState>();
        let Ok(db) = state.db.lock() else { return };
        db::list_accounts(&db).unwrap_or_default()
    };
    let checks = accounts
        .into_iter()
        .filter(|account| account.initial_done)
        .map(|account| async move {
            if !begin(app, &account.id).unwrap_or(false) {
                return None;
            }
            let started = Instant::now();
            let result = history_sync(app, &account).await;
            let elapsed = started.elapsed().as_millis() as u64;
            end(app, &account.id);
            if let Ok(true) = result {
                let _ = app.emit("mail://updated", json!({}));
                let _ = app.emit(
                    "sync://done",
                    json!({ "accountId": account.id, "ok": true, "message": "", "offline": false }),
                );
                crate::refresh_badge(app);
            }
            // With no changes the check was a single history call, so its time is the round trip to Gmail.
            Some(result.map(|changed| (!changed).then_some(elapsed)))
        });
    let probes: Vec<_> = join_all(checks).await.into_iter().flatten().collect();
    if probes.is_empty() {
        return;
    }
    let reached: Vec<Option<u64>> = probes.iter().filter_map(|probe| probe.as_ref().ok().copied()).collect();
    let status = if !reached.is_empty() {
        let latency = reached.into_iter().flatten().max();
        json!({ "state": "online", "latencyMs": latency, "message": "" })
    } else {
        let err = probes.iter().find_map(|probe| probe.as_ref().err()).cloned().unwrap_or_default();
        let state = if is_offline(&err) { "offline" } else { "error" };
        json!({ "state": state, "latencyMs": null, "message": err })
    };
    let _ = app.emit("net://status", status);
}

pub async fn sync_account(app: &AppHandle, account_id: &str, eager: bool) -> Result<(), String> {
    if !begin(app, account_id)? {
        return Ok(());
    }
    let result = sync_inner(app, account_id, eager).await;
    end(app, account_id);
    match &result {
        Ok(()) => {
            let _ = app.emit("mail://updated", json!({}));
            let _ = app.emit(
                "sync://done",
                json!({ "accountId": account_id, "ok": true, "message": "", "offline": false }),
            );
            crate::refresh_badge(app);
        }
        Err(err) => {
            let _ = app.emit(
                "sync://done",
                json!({ "accountId": account_id, "ok": false, "message": err, "offline": is_offline(err) }),
            );
        }
    }
    result
}

async fn sync_inner(app: &AppHandle, account_id: &str, eager: bool) -> Result<(), String> {
    let mut account = with_db(app, |conn| db::get_account(conn, account_id))?;
    emit(app, &account.email, "Checking for new mail", 0);
    // New mail first, so its notification does not wait on the slower steps below.
    if account.initial_done {
        history_sync(app, &account).await?;
        account = with_db(app, |conn| db::get_account(conn, account_id))?;
    }
    if let Ok(labels) = gmail::labels(app, account_id).await {
        with_db(app, |conn| db::replace_labels(conn, account_id, &labels))?;
    }
    if let Ok(send_as) = gmail::send_as(app, account_id).await {
        with_db(app, |conn| db::update_send_as(conn, account_id, &send_as))?;
    }
    if !account.initial_done {
        sync_recent(app, &mut account, if eager { 4 } else { 1 }).await?;
    } else {
        backfill_page(app, &mut account).await?;
    }
    if account.initial_done {
        for label in ["SPAM", "TRASH"] {
            if let Err(err) = sync_hidden_label(app, &account, label).await {
                eprintln!("Novabox {label} sync: {err}");
            }
        }
    }
    if let Ok(drafts) = gmail::list_drafts(app, account_id).await {
        with_db(app, |conn| {
            for (draft_id, message_id) in &drafts {
                db::set_draft_id(conn, account_id, message_id, draft_id)?;
            }
            Ok(())
        })?;
    }
    Ok(())
}

async fn sync_recent(app: &AppHandle, account: &mut Account, max_pages: usize) -> Result<(), String> {
    let stored = with_db(app, |conn| db::message_count(conn, &account.id))?;
    if stored == 0 && account.recent_page_token.is_some() {
        account.recent_page_token = None;
        account.initial_done = false;
        save_account(app, account)?;
    }
    if account.history_id.is_none() {
        let profile = gmail::profile(app, &account.id).await?;
        if profile.history_id.is_empty() {
            return Err("Gmail did not return a history id.".into());
        }
        account.history_id = Some(profile.history_id);
        save_account(app, account)?;
    }
    let mut token = account.recent_page_token.clone();
    for _ in 0..max_pages {
        // Progress counts every message stored so far, so the setup screen shows a running total.
        let stored = with_db(app, |conn| db::message_count(conn, &account.id))? as u32;
        emit(app, &account.email, "Downloading recent mail", stored);
        let (ids, next) = gmail::list_message_ids(app, &account.id, "newer_than:90d", token.as_deref()).await?;
        fetch_store(app, &account.id, &account.email, "Downloading recent mail", stored, &ids).await?;
        if let Some(next) = next {
            account.recent_page_token = Some(next.clone());
            token = Some(next);
            save_account(app, account)?;
        } else {
            account.recent_page_token = None;
            account.initial_done = true;
            save_account(app, account)?;
            history_sync(app, account).await?;
            *account = with_db(app, |conn| db::get_account(conn, &account.id))?;
            backfill_page(app, account).await?;
            break;
        }
    }
    Ok(())
}

async fn backfill_page(app: &AppHandle, account: &mut Account) -> Result<(), String> {
    if account.backfill_done {
        return Ok(());
    }
    emit(app, &account.email, "Downloading older mail", 0);
    let (ids, next) = gmail::list_message_ids(app, &account.id, "older_than:90d", account.backfill_token.as_deref()).await?;
    fetch_store(app, &account.id, &account.email, "Downloading older mail", 0, &ids).await?;
    match next {
        Some(next) => account.backfill_token = Some(next),
        None => {
            account.backfill_token = None;
            account.backfill_done = true;
        }
    }
    save_account(app, account)
}

/// Spam and Trash never come back from the normal list calls, so pull them separately.
/// Pages run newest first; stop at the first page that holds nothing new.
async fn sync_hidden_label(app: &AppHandle, account: &Account, label: &str) -> Result<(), String> {
    let phase = if label == "SPAM" { "Downloading spam" } else { "Downloading trash" };
    let mut token: Option<String> = None;
    for _ in 0..20 {
        let (ids, next) = gmail::list_label_ids(app, &account.id, label, token.as_deref()).await?;
        let mut missing = Vec::new();
        for id in ids {
            if !with_db(app, |conn| db::has_message(conn, &account.id, &id))? {
                missing.push(id);
            }
        }
        if missing.is_empty() {
            break;
        }
        emit(app, &account.email, phase, 0);
        fetch_store(app, &account.id, &account.email, phase, 0, &missing).await?;
        match next {
            Some(next) => token = Some(next),
            None => break,
        }
    }
    Ok(())
}

/// Returns whether anything in the mailbox changed.
async fn history_sync(app: &AppHandle, account: &Account) -> Result<bool, String> {
    let Some(start) = account.history_id.clone() else {
        return Ok(false);
    };
    let mut page: Option<String> = None;
    let mut latest = start.clone();
    let mut to_fetch = HashSet::new();
    let mut to_delete = HashSet::new();
    loop {
        let value = match gmail::history(app, &account.id, &start, page.as_deref()).await {
            Ok(value) => value,
            Err(err) if err.starts_with("gmail_404") => {
                let mut reset = account.clone();
                reset.history_id = None;
                reset.initial_done = false;
                reset.recent_page_token = None;
                save_account(app, &reset)?;
                return Ok(false);
            }
            Err(err) => return Err(err),
        };
        if let Some(id) = value.get("historyId").and_then(|item| item.as_str()) {
            latest = id.to_string();
        }
        if let Some(records) = value.get("history").and_then(|item| item.as_array()) {
            for record in records {
                collect_ids(record, "messagesDeleted", &mut to_delete);
                collect_ids(record, "messagesAdded", &mut to_fetch);
                collect_ids(record, "labelsAdded", &mut to_fetch);
                collect_ids(record, "labelsRemoved", &mut to_fetch);
            }
        }
        if let Some(next) = value.get("nextPageToken").and_then(|item| item.as_str()) {
            page = Some(next.to_string());
            continue;
        }
        break;
    }
    let data_dir = app.state::<AppState>().data_dir.clone();
    for id in &to_delete {
        to_fetch.remove(id);
        with_db(app, |conn| db::delete_message(conn, &account.id, id))?;
        files::remove_message_files(&data_dir, &account.id, id);
    }
    let fetch_list: Vec<String> = to_fetch.into_iter().take(200).collect();
    let mut fresh = Vec::new();
    for id in &fetch_list {
        if !with_db(app, |conn| db::has_message(conn, &account.id, id))? {
            fresh.push(id.clone());
        }
    }
    if !fetch_list.is_empty() {
        emit(app, &account.email, "Updating inbox", 0);
        fetch_store(app, &account.id, &account.email, "Updating inbox", 0, &fetch_list).await?;
    }
    let changed = !to_delete.is_empty() || !fetch_list.is_empty();
    let mut updated = account.clone();
    updated.history_id = Some(latest);
    save_account(app, &updated)?;
    if !fresh.is_empty() && account.initial_done {
        let thread = with_db(app, |conn| {
            let thread_id = lookup_or_empty(conn, &account.id, &fresh[0])?;
            db::get_thread(conn, &account.id, &thread_id)
        })
        .ok();
        let subject = thread.as_ref().map(|thread| thread.subject.clone()).unwrap_or_default();
        let sender = thread
            .as_ref()
            .and_then(|thread| thread.messages.iter().find(|message| message.id == fresh[0]))
            .map(|message| {
                if message.from_name.is_empty() {
                    message.from_email.clone()
                } else {
                    message.from_name.clone()
                }
            })
            .unwrap_or_default();
        let single = fresh.len() == 1;
        let body = if single && !subject.is_empty() {
            subject
        } else {
            format!("{} new messages", fresh.len())
        };
        let _ = app.emit(
            "mail://notify",
            json!({
                "kind": "mail",
                "title": format!("New mail — {}", account.email),
                "body": body,
                "accountId": account.id,
                "accountColor": account.color,
                "threadId": if single { thread.as_ref().map(|thread| thread.thread_id.clone()) } else { None },
                "sender": if single && !sender.is_empty() { Some(sender) } else { None },
            }),
        );
    }
    Ok(changed)
}

fn lookup_or_empty(conn: &rusqlite::Connection, account_id: &str, message_id: &str) -> Result<String, String> {
    db::threads_for_messages(conn, &[(account_id.to_string(), message_id.to_string())])?
        .into_iter()
        .next()
        .map(|thread| thread.thread_id)
        .ok_or_else(|| "missing".to_string())
}

fn collect_ids(record: &Value, key: &str, into: &mut HashSet<String>) {
    if let Some(items) = record.get(key).and_then(|value| value.as_array()) {
        for item in items {
            if let Some(id) = item
                .get("message")
                .and_then(|message| message.get("id"))
                .and_then(|id| id.as_str())
            {
                into.insert(id.to_string());
            }
        }
    }
}

async fn fetch_store(
    app: &AppHandle,
    account_id: &str,
    email: &str,
    phase: &str,
    base: u32,
    ids: &[String],
) -> Result<(), String> {
    let mut fetched = 0u32;
    let mut first_error: Option<String> = None;
    // Every refresh makes the window re-query the mailbox, so refresh at most every few seconds.
    let mut last_refresh = Instant::now();
    let mut stale = false;
    for chunk in ids.chunks(4) {
        let futures = chunk.iter().cloned().map(|id| {
            let app = app.clone();
            let account_id = account_id.to_string();
            async move { gmail::get_message(&app, &account_id, &id).await }
        });
        for result in join_all(futures).await {
            match result {
                Ok(value) => {
                    if let Err(err) = store_message(app, account_id, &value) {
                        eprintln!("Novabox store: {err}");
                        if first_error.is_none() {
                            first_error = Some(err);
                        }
                    } else {
                        fetched += 1;
                    }
                }
                Err(err) => {
                    eprintln!("Novabox fetch: {err}");
                    if first_error.is_none() {
                        first_error = Some(err);
                    }
                }
            }
        }
        emit(app, email, phase, base + fetched);
        stale |= fetched > 0;
        if stale && last_refresh.elapsed() >= Duration::from_secs(3) {
            let _ = app.emit("mail://updated", json!({}));
            last_refresh = Instant::now();
            stale = false;
        }
    }
    if stale {
        let _ = app.emit("mail://updated", json!({}));
    }
    if fetched == 0 {
        if let Some(err) = first_error {
            return Err(err);
        }
    }
    Ok(())
}

fn store_message(app: &AppHandle, account_id: &str, value: &Value) -> Result<(), String> {
    let parsed = parse::parse_message(account_id, value)?;
    let data_dir = app.state::<AppState>().data_dir.clone();
    with_db(app, |conn| db::upsert_message(conn, &parsed.message))?;
    for file in parsed.files {
        if let Err(err) = files::write_cached(
            &data_dir,
            account_id,
            &parsed.message.id,
            &file.attachment_id,
            &file.bytes,
        ) {
            eprintln!("Novabox attachment cache: {err}");
        }
    }
    Ok(())
}

fn save_account(app: &AppHandle, account: &Account) -> Result<(), String> {
    with_db(app, |conn| {
        db::set_sync_state(
            conn,
            &account.id,
            account.history_id.as_deref(),
            account.recent_page_token.as_deref(),
            account.backfill_token.as_deref(),
            account.backfill_done,
            account.initial_done,
        )
    })
}

fn emit(app: &AppHandle, email: &str, phase: &str, fetched: u32) {
    let _ = app.emit(
        "sync://progress",
        SyncProgress {
            email: email.to_string(),
            phase: phase.to_string(),
            fetched,
        },
    );
}

fn begin(app: &AppHandle, account_id: &str) -> Result<bool, String> {
    let state = app.state::<AppState>();
    let mut accounts = state.syncing_accounts.lock().map_err(|e| e.to_string())?;
    Ok(accounts.insert(account_id.to_string()))
}

fn end(app: &AppHandle, account_id: &str) {
    if let Ok(mut accounts) = app.state::<AppState>().syncing_accounts.lock() {
        accounts.remove(account_id);
    }
}

fn with_db<T>(app: &AppHandle, op: impl FnOnce(&rusqlite::Connection) -> Result<T, String>) -> Result<T, String> {
    let state = app.state::<AppState>();
    let db = state.db.lock().map_err(|e| e.to_string())?;
    op(&db)
}

fn is_offline(err: &str) -> bool {
    let text = err.to_lowercase();
    text.contains("could not reach")
        || text.contains("dns")
        || text.contains("timed out")
        || text.contains("connection")
        || text.contains("network")
}
