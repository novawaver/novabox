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

/// Labels compared against Gmail's own lists, to repair what a missed change left behind.
const CHECKED_LABELS: [&str; 3] = ["INBOX", "UNREAD", "STARRED"];
/// How often an account's labels are compared. Starting Novabox and Sync now compare at once.
const LABEL_CHECK_EVERY: Duration = Duration::from_secs(30 * 60);

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
        if label_check_due(app, account_id) {
            match check_labels(app, &account).await {
                Ok(()) => label_check_done(app, account_id),
                Err(err) => eprintln!("Novabox label check: {err}"),
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
    // The caller's copy can be older than the last sync, so read the saved position again.
    let account = with_db(app, |conn| db::get_account(conn, &account.id))?;
    let Some(start) = account.history_id.clone() else {
        return Ok(false);
    };
    let mut changed = false;
    let mut fresh = Vec::new();
    let result = follow_history(app, &account, &start, &mut changed, &mut fresh).await;
    if !fresh.is_empty() && account.initial_done {
        notify_new_mail(app, &account, &fresh);
    }
    match result {
        Ok(()) => Ok(changed),
        Err(err) => {
            // Part of the history was applied before the failure; show that much.
            if changed {
                let _ = app.emit("mail://updated", json!({}));
                crate::refresh_badge(app);
            }
            Err(err)
        }
    }
}

/// Walks Gmail's history from `start`, applying it a page at a time. Each page's position
/// is saved once the page is applied, so a long catch-up that is cut short carries on from
/// there, and a page that could not be applied in full is read again by the next sync.
async fn follow_history(
    app: &AppHandle,
    account: &Account,
    start: &str,
    changed: &mut bool,
    fresh: &mut Vec<String>,
) -> Result<(), String> {
    let mut page: Option<String> = None;
    loop {
        let value = match gmail::history(app, &account.id, start, page.as_deref()).await {
            Ok(value) => value,
            Err(err) if err.starts_with("gmail_404") => {
                // Gmail no longer keeps history this far back, so start again from recent mail.
                let mut reset = account.clone();
                reset.history_id = None;
                reset.initial_done = false;
                reset.recent_page_token = None;
                save_account(app, &reset)?;
                return Ok(());
            }
            Err(err) => return Err(err),
        };
        let records = entries(&value, "history");
        let next = value
            .get("nextPageToken")
            .and_then(|item| item.as_str())
            .map(|token| token.to_string());
        let applied = apply_history(app, account, records).await?;
        *changed |= applied.changed;
        fresh.extend(applied.fresh);
        if let Some(err) = applied.failure {
            return Err(err);
        }
        if let Some(position) = position_after(&value, records, next.is_none()) {
            if position != start {
                with_db(app, |conn| db::set_history_id(conn, &account.id, &position))?;
            }
        }
        match next {
            Some(next) => page = Some(next),
            None => return Ok(()),
        }
    }
}

/// What applying one page of history did.
struct Applied {
    changed: bool,
    /// Messages that arrived and were not on this PC before.
    fresh: Vec<String>,
    /// Why a message could not be downloaded, when one could not.
    failure: Option<String>,
}

async fn apply_history(app: &AppHandle, account: &Account, records: &[Value]) -> Result<Applied, String> {
    let mut added: Vec<String> = Vec::new();
    let mut deleted: HashSet<String> = HashSet::new();
    let mut label_changes: Vec<db::LabelChange> = Vec::new();
    for record in records {
        for entry in entries(record, "messagesAdded") {
            if let Some(id) = entry_message_id(entry) {
                if !added.contains(&id) {
                    added.push(id);
                }
            }
        }
        for (key, add) in [("labelsAdded", true), ("labelsRemoved", false)] {
            for entry in entries(record, key) {
                let Some(message_id) = entry_message_id(entry) else {
                    continue;
                };
                let labels = entries(entry, "labelIds")
                    .iter()
                    .filter_map(|label| label.as_str().map(|label| label.to_string()))
                    .collect();
                label_changes.push(db::LabelChange { message_id, add, labels });
            }
        }
        for entry in entries(record, "messagesDeleted") {
            if let Some(id) = entry_message_id(entry) {
                deleted.insert(id);
            }
        }
    }
    let data_dir = app.state::<AppState>().data_dir.clone();
    for id in &deleted {
        with_db(app, |conn| db::delete_message(conn, &account.id, id))?;
        files::remove_message_files(&data_dir, &account.id, id);
    }
    // Label changes are applied to the stored messages directly. Downloading every changed
    // message again would make a bulk delete or archive in Gmail cost one request each.
    label_changes.retain(|change| !deleted.contains(&change.message_id));
    let (relabeled, unknown) = with_db(app, |conn| db::apply_label_changes(conn, &account.id, &label_changes))?;
    let mut fetch: Vec<String> = Vec::new();
    let mut fresh = Vec::new();
    for id in added {
        if deleted.contains(&id) {
            continue;
        }
        if !with_db(app, |conn| db::has_message(conn, &account.id, &id))? {
            fresh.push(id.clone());
        }
        fetch.push(id);
    }
    // A message whose labels changed but that is not stored yet is downloaded as it is now.
    for id in unknown {
        if !fetch.contains(&id) {
            fetch.push(id);
        }
    }
    let mut failure = None;
    if !fetch.is_empty() {
        emit(app, &account.email, "Updating inbox", 0);
        let done = fetch_each(app, &account.id, &account.email, "Updating inbox", 0, &fetch).await;
        for (id, err) in done.failed {
            if err.starts_with("gmail_404") {
                // Deleted in Gmail since this page of history was written.
                with_db(app, |conn| db::delete_message(conn, &account.id, &id))?;
                files::remove_message_files(&data_dir, &account.id, &id);
                fresh.retain(|item| item != &id);
            } else if failure.is_none() {
                failure = Some(err);
            }
        }
    }
    Ok(Applied {
        changed: !deleted.is_empty() || relabeled > 0 || !fetch.is_empty(),
        fresh,
        failure,
    })
}

/// Where the next history call should start once a page is applied: Gmail's current
/// position after the last page, otherwise the newest record of the page.
fn position_after(page: &Value, records: &[Value], last: bool) -> Option<String> {
    let current = if last { page.get("historyId").and_then(history_number) } else { None };
    current
        .or_else(|| records.iter().filter_map(|record| record.get("id").and_then(history_number)).max())
        .map(|id| id.to_string())
}

fn history_number(value: &Value) -> Option<u64> {
    match value {
        Value::String(text) => text.parse().ok(),
        Value::Number(number) => number.as_u64(),
        _ => None,
    }
}

fn entries<'a>(value: &'a Value, key: &str) -> &'a [Value] {
    value
        .get(key)
        .and_then(|item| item.as_array())
        .map(|items| items.as_slice())
        .unwrap_or(&[])
}

fn entry_message_id(entry: &Value) -> Option<String> {
    entry.get("message")?.get("id")?.as_str().map(|id| id.to_string())
}

fn notify_new_mail(app: &AppHandle, account: &Account, fresh: &[String]) {
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

/// Whether it is time to compare this account's labels with Gmail's lists.
fn label_check_due(app: &AppHandle, account_id: &str) -> bool {
    let state = app.state::<AppState>();
    let Ok(checks) = state.label_checks.lock() else {
        return false;
    };
    let due = checks
        .get(account_id)
        .map_or(true, |checked| checked.elapsed() >= LABEL_CHECK_EVERY);
    due
}

fn label_check_done(app: &AppHandle, account_id: &str) {
    if let Ok(mut checks) = app.state::<AppState>().label_checks.lock() {
        checks.insert(account_id.to_string(), Instant::now());
    }
}

/// Makes the next full sync of every account compare its labels, however recently it did.
pub fn request_label_check(app: &AppHandle) {
    if let Ok(mut checks) = app.state::<AppState>().label_checks.lock() {
        checks.clear();
    }
}

async fn check_labels(app: &AppHandle, account: &Account) -> Result<(), String> {
    for label in CHECKED_LABELS {
        check_label(app, account, label).await?;
    }
    Ok(())
}

/// Compares the messages stored under a label with the ids Gmail lists for it, and reads
/// again the labels of every message the two disagree on. History only reports changes
/// made after its saved position, so this is what repairs a change an earlier sync missed.
async fn check_label(app: &AppHandle, account: &Account, label: &str) -> Result<(), String> {
    let mut listed: HashSet<String> = HashSet::new();
    let mut token: Option<String> = None;
    loop {
        let (ids, next) = gmail::list_label_ids_wide(app, &account.id, label, token.as_deref()).await?;
        listed.extend(ids);
        match next {
            Some(next) => token = Some(next),
            None => break,
        }
    }
    // `doubtful` are stored here with labels that disagree with Gmail's list; `absent` are
    // on Gmail's list and not stored here at all.
    let (doubtful, absent) = with_db(app, |conn| {
        let stored = db::ids_with_label(conn, &account.id, label)?;
        let mut doubtful: Vec<String> = stored.difference(&listed).cloned().collect();
        let mut absent = Vec::new();
        for id in listed.difference(&stored) {
            if db::has_message(conn, &account.id, id)? {
                doubtful.push(id.clone());
            } else {
                absent.push(id.clone());
            }
        }
        Ok((doubtful, absent))
    })?;
    if !doubtful.is_empty() {
        emit(app, &account.email, "Checking labels", 0);
    }
    let data_dir = app.state::<AppState>().data_dir.clone();
    for chunk in doubtful.chunks(4) {
        let lookups = chunk
            .iter()
            .map(|id| async move { (id, gmail::message_labels(app, &account.id, id).await) });
        for (id, result) in join_all(lookups).await {
            match result {
                Ok(labels) => with_db(app, |conn| db::set_labels(conn, &account.id, id, &labels))?,
                Err(err) if err.starts_with("gmail_404") => {
                    with_db(app, |conn| db::delete_message(conn, &account.id, id))?;
                    files::remove_message_files(&data_dir, &account.id, id);
                }
                Err(err) => return Err(err),
            }
        }
    }
    // Until the older mail is all downloaded, a listed message that is not stored is
    // simply one the download has not reached yet.
    if account.backfill_done && !absent.is_empty() {
        emit(app, &account.email, "Updating inbox", 0);
        fetch_store(app, &account.id, &account.email, "Updating inbox", 0, &absent).await?;
    }
    Ok(())
}

fn lookup_or_empty(conn: &rusqlite::Connection, account_id: &str, message_id: &str) -> Result<String, String> {
    db::threads_for_messages(conn, &[(account_id.to_string(), message_id.to_string())])?
        .into_iter()
        .next()
        .map(|thread| thread.thread_id)
        .ok_or_else(|| "missing".to_string())
}

/// Downloads and stores messages. Fails only when none could be stored.
async fn fetch_store(
    app: &AppHandle,
    account_id: &str,
    email: &str,
    phase: &str,
    base: u32,
    ids: &[String],
) -> Result<(), String> {
    let done = fetch_each(app, account_id, email, phase, base, ids).await;
    if done.stored == 0 {
        if let Some((_, err)) = done.failed.into_iter().next() {
            return Err(err);
        }
    }
    Ok(())
}

/// What a batch of downloads did: how many messages were stored, and which failed and why.
struct Fetched {
    stored: u32,
    failed: Vec<(String, String)>,
}

async fn fetch_each(
    app: &AppHandle,
    account_id: &str,
    email: &str,
    phase: &str,
    base: u32,
    ids: &[String],
) -> Fetched {
    let mut stored = 0u32;
    let mut failed = Vec::new();
    // Every refresh makes the window re-query the mailbox, so refresh at most every few seconds.
    let mut last_refresh = Instant::now();
    let mut stale = false;
    for chunk in ids.chunks(4) {
        let futures = chunk.iter().cloned().map(|id| {
            let app = app.clone();
            let account_id = account_id.to_string();
            async move {
                let result = gmail::get_message(&app, &account_id, &id).await;
                (id, result)
            }
        });
        for (id, result) in join_all(futures).await {
            match result {
                Ok(value) => {
                    if let Err(err) = store_message(app, account_id, &value) {
                        eprintln!("Novabox store: {err}");
                        failed.push((id, err));
                    } else {
                        stored += 1;
                    }
                }
                Err(err) => {
                    eprintln!("Novabox fetch: {err}");
                    failed.push((id, err));
                }
            }
        }
        emit(app, email, phase, base + stored);
        stale |= stored > 0;
        if stale && last_refresh.elapsed() >= Duration::from_secs(3) {
            let _ = app.emit("mail://updated", json!({}));
            last_refresh = Instant::now();
            stale = false;
        }
    }
    if stale {
        let _ = app.emit("mail://updated", json!({}));
    }
    Fetched { stored, failed }
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
