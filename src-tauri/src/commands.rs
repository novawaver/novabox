use std::fs;
use std::sync::atomic::Ordering;
use std::time::{SystemTime, UNIX_EPOCH};

use base64::engine::general_purpose::{STANDARD, URL_SAFE, URL_SAFE_NO_PAD};
use base64::Engine;
use serde_json::json;
use tauri::{AppHandle, Emitter, Manager};

use crate::auth::{self, REDIRECT_URI};
use crate::db;
use crate::files;
use crate::gmail;
use crate::mime_build::{self, OutgoingMail};
use crate::outbox::{self, Job};
use rand::RngCore;

use crate::model::{
    Account, Address, AppInfo, AttachmentData, AttachmentRequest, ComposeRequest, Mailbox,
    ModifyBatch, SaveAttachmentRequest, SendResult, Settings, SignatureUpdate, StoredMessage,
    Template, ThreadDetail, ThreadQuery, ThreadTarget,
};
use crate::parse;
use crate::secrets;
use crate::settings;
use crate::sync;
use crate::util::validate_label;
use crate::AppState;

const COLORS: [&str; 8] = [
    "#C9842A", "#2F7D6D", "#3D5A80", "#8A4E7A", "#C4535A", "#3E6B8A", "#8A6A2F", "#4F6B4A",
];

#[derive(Clone)]
pub(crate) enum Change {
    Labels { add: Vec<String>, remove: Vec<String> },
    Trash,
    Untrash,
}

#[tauri::command]
pub fn app_info(app: AppHandle) -> AppInfo {
    AppInfo {
        version: env!("CARGO_PKG_VERSION").into(),
        data_dir: app.state::<AppState>().data_dir.display().to_string(),
        redirect_uri: REDIRECT_URI.into(),
        google_ready: !auth::CLIENT_ID.is_empty(),
    }
}

// Async so the query runs off the UI thread; sync commands block the window.
#[tauri::command(async)]
pub fn settings_get(app: AppHandle) -> Result<Settings, String> {
    settings::load(&app.state::<AppState>().data_dir)
}

#[tauri::command]
pub fn settings_save(app: AppHandle, settings: Settings) -> Result<(), String> {
    let mut next = settings;
    if next.theme.is_empty() {
        next.theme = "system".into();
    }
    if next.density.is_empty() {
        next.density = "default".into();
    }
    if next.reading_pane.is_empty() {
        next.reading_pane = "none".into();
    }
    settings::save(&app.state::<AppState>().data_dir, &next)
}

#[tauri::command]
pub fn sign_in_cancel(app: AppHandle) {
    app.state::<AppState>()
        .sign_in_cancel
        .store(true, Ordering::SeqCst);
}

#[tauri::command(async)]
pub fn mailbox_query(app: AppHandle, query: ThreadQuery) -> Result<Mailbox, String> {
    with_db(&app, |conn| db::query_mailbox(conn, &query))
}

#[tauri::command(async)]
pub fn thread_get(app: AppHandle, account_id: String, thread_id: String) -> Result<ThreadDetail, String> {
    with_db(&app, |conn| db::get_thread(conn, &account_id, &thread_id))
}

#[tauri::command]
pub async fn sign_in(app: AppHandle) -> Result<Account, String> {
    if auth::CLIENT_ID.is_empty() {
        return Err(auth::MISSING_CLIENT.into());
    }
    let client_id = auth::CLIENT_ID;
    let cancel = app.state::<AppState>().sign_in_cancel.clone();
    cancel.store(false, Ordering::SeqCst);
    let oauth = tauri::async_runtime::spawn_blocking(move || auth::browser_sign_in(client_id, &cancel))
        .await
        .map_err(|e| e.to_string())??;
    let http = app.state::<AppState>().http.clone();
    let session = auth::exchange_code(
        &http,
        auth::CLIENT_ID,
        auth::CLIENT_SECRET,
        &oauth.code,
        &oauth.verifier,
    )
    .await?;
    if session.refresh_token.is_empty() {
        return Err("Google did not return a refresh token. Remove Novabox from your Google Account permissions and sign in again.".into());
    }
    let profile = auth::fetch_profile(&http, &session.access_token).await?;
    let account_id = profile.id.clone();
    let refresh = session.refresh_token.clone();
    tauri::async_runtime::spawn_blocking(move || secrets::save(&account_id, &refresh))
        .await
        .map_err(|e| e.to_string())??;
    auth::store_token(&app, &profile.id, &session);

    register_account(
        &app,
        Account {
            id: profile.id.clone(),
            email: profile.email.clone(),
            name: profile.name.clone(),
            picture: profile.picture.clone(),
            color: String::new(),
            unread: 0,
            signature: String::new(),
            send_as: vec![crate::model::SendAs {
                email: profile.email.clone(),
                display_name: profile.name.clone(),
                is_default: true,
                is_primary: true,
            }],
            history_id: None,
            recent_page_token: None,
            backfill_token: None,
            backfill_done: false,
            initial_done: false,
        },
    )
    .await
}

/// Saves an account whose sign-in is already stored, then fetches its labels and aliases
/// and starts syncing. An account that already exists keeps its colour, signature and
/// sync progress; a new one gets `account.color`, or the next palette colour if that is empty.
pub(crate) async fn register_account(app: &AppHandle, account: Account) -> Result<Account, String> {
    let id = account.id.clone();
    with_db(app, |conn| {
        let existing = db::list_accounts(conn)?;
        let mut account = account;
        if account.color.is_empty() {
            account.color = COLORS[existing.len() % COLORS.len()].to_string();
        }
        db::upsert_account(conn, &account)
    })?;
    if let Ok(labels) = gmail::labels(app, &id).await {
        let _ = with_db(app, |conn| db::replace_labels(conn, &id, &labels));
    }
    if let Ok(send_as) = gmail::send_as(app, &id).await {
        let _ = with_db(app, |conn| db::update_send_as(conn, &id, &send_as));
    }
    let account = with_db(app, |conn| db::get_account(conn, &id))?;
    let app_for_sync = app.clone();
    let sync_id = account.id.clone();
    tauri::async_runtime::spawn(async move {
        let _ = sync::sync_account(&app_for_sync, &sync_id, true).await;
    });
    Ok(account)
}

#[tauri::command]
/// Removes an account from this PC. With `revoke` (the default) Google access is revoked
/// too, which signs it out on every PC that shares the sign-in through a backup.
pub async fn account_remove(app: AppHandle, account_id: String, revoke: Option<bool>) -> Result<(), String> {
    let refresh = secrets::load(&account_id).ok();
    if let (Some(refresh), true) = (refresh, revoke.unwrap_or(true)) {
        let http = app.state::<AppState>().http.clone();
        let _ = auth::revoke(&http, &refresh).await;
    }
    let _ = secrets::delete(&account_id);
    auth::invalidate(&app, &account_id);
    with_db(&app, |conn| db::delete_account(conn, &account_id))?;
    files::remove_account_files(&app.state::<AppState>().data_dir, &account_id);
    crate::refresh_badge(&app);
    let _ = app.emit("mail://updated", json!({}));
    Ok(())
}

#[tauri::command]
pub fn account_set_signature(app: AppHandle, update: SignatureUpdate) -> Result<(), String> {
    with_db(&app, |conn| db::set_signature(conn, &update.account_id, &update.signature))
}

#[tauri::command]
pub async fn threads_modify(app: AppHandle, batch: ModifyBatch) -> Result<(), String> {
    if batch.targets.is_empty() {
        return Ok(());
    }
    let change = action_change(&batch.action, batch.label_id.as_deref())?;
    for target in &batch.targets {
        change_now(&app, target, &change)?;
        if matches!(batch.action.as_str(), "trash" | "spam") {
            let account_id = target.account_id.clone();
            let thread_id = target.thread_id.clone();
            let _ = with_db(&app, |conn| db::snooze_clear(conn, &account_id, &thread_id));
        }
    }
    crate::refresh_badge(&app);
    let _ = app.emit("mail://updated", json!({}));
    Ok(())
}

#[tauri::command]
pub async fn mail_send(app: AppHandle, request: ComposeRequest) -> Result<SendResult, String> {
    let (raw, thread_id) = prepare_raw(&app, &request)?;
    if let Some(draft_id) = request.draft_id.clone() {
        let _ = gmail::delete_draft(&app, &request.account_id, &draft_id).await;
        let _ = with_db(&app, |conn| db::delete_draft(conn, &request.account_id, &draft_id));
    }
    let sent = gmail::send_raw(&app, &request.account_id, &raw, thread_id.as_deref()).await?;
    let message_id = sent
        .get("id")
        .and_then(|value| value.as_str())
        .unwrap_or_default()
        .to_string();
    let thread_id = sent
        .get("threadId")
        .and_then(|value| value.as_str())
        .unwrap_or(thread_id.as_deref().unwrap_or_default())
        .to_string();
    if !message_id.is_empty() {
        if let Ok(value) = gmail::get_message(&app, &request.account_id, &message_id).await {
            if let Ok(parsed) = parse::parse_message(&request.account_id, &value) {
                let _ = with_db(&app, |conn| db::upsert_message(conn, &parsed.message));
            }
        }
    }
    crate::refresh_badge(&app);
    let _ = app.emit("mail://updated", json!({}));
    Ok(SendResult {
        message_id,
        thread_id,
        draft_id: None,
    })
}

#[tauri::command]
pub async fn draft_save(app: AppHandle, request: ComposeRequest) -> Result<SendResult, String> {
    let (raw, thread_id) = prepare_raw(&app, &request)?;
    let response = if let Some(draft_id) = &request.draft_id {
        gmail::update_draft(&app, &request.account_id, draft_id, &raw, thread_id.as_deref()).await?
    } else {
        gmail::create_draft(&app, &request.account_id, &raw, thread_id.as_deref()).await?
    };
    let draft_id = response
        .get("id")
        .and_then(|value| value.as_str())
        .unwrap_or_default()
        .to_string();
    let message = &response["message"];
    let message_id = message
        .get("id")
        .and_then(|value| value.as_str())
        .unwrap_or_default()
        .to_string();
    let thread_id = message
        .get("threadId")
        .and_then(|value| value.as_str())
        .or(thread_id.as_deref())
        .unwrap_or(&message_id)
        .to_string();
    if draft_id.is_empty() || message_id.is_empty() {
        return Err("Gmail did not save the draft.".into());
    }
    let previous = request.draft_id.as_deref().and_then(|id| {
        with_db(&app, |conn| db::draft_message_id(conn, &request.account_id, id)).ok().flatten()
    });
    if let Some(previous) = previous {
        if previous != message_id {
            let _ = with_db(&app, |conn| db::delete_message(conn, &request.account_id, &previous));
        }
    }
    let account = with_db(&app, |conn| db::get_account(conn, &request.account_id))?;
    let plain = html_to_text(&request.html);
    let stored = StoredMessage {
        account_id: request.account_id.clone(),
        id: message_id.clone(),
        thread_id: thread_id.clone(),
        from_name: display_name(&account, &request.from_email),
        from_email: request.from_email.clone(),
        to: address_list(&request.to),
        cc: address_list(&request.cc),
        bcc: address_list(&request.bcc),
        subject: request.subject.clone(),
        snippet: plain.chars().take(160).collect(),
        body_html: request.html.clone(),
        body_text: plain,
        date: now_millis(),
        label_ids: vec!["DRAFT".into()],
        attachments: vec![],
        message_id_header: String::new(),
        references_header: request.references.clone().unwrap_or_default(),
        draft_id: Some(draft_id.clone()),
        size_estimate: request.html.len() as i64,
    };
    with_db(&app, |conn| db::upsert_message(conn, &stored))?;
    let _ = app.emit("mail://updated", json!({}));
    Ok(SendResult {
        message_id,
        thread_id,
        draft_id: Some(draft_id),
    })
}

#[tauri::command]
pub async fn draft_delete(app: AppHandle, account_id: String, draft_id: String) -> Result<(), String> {
    let _ = gmail::delete_draft(&app, &account_id, &draft_id).await;
    with_db(&app, |conn| db::delete_draft(conn, &account_id, &draft_id))?;
    let _ = app.emit("mail://updated", json!({}));
    Ok(())
}

#[tauri::command]
pub async fn remote_search(app: AppHandle, account_id: Option<String>, query: String) -> Result<Vec<crate::model::ThreadSummary>, String> {
    let query = query.trim().to_string();
    if query.is_empty() {
        return Ok(vec![]);
    }
    let accounts = with_db(&app, db::list_accounts)?;
    let targets: Vec<String> = match account_id.filter(|id| !id.is_empty()) {
        Some(id) => vec![id],
        None => accounts.into_iter().map(|account| account.id).collect(),
    };
    let mut keys = Vec::new();
    for id in targets {
        let (ids, _) = gmail::list_message_ids(&app, &id, &query, None).await?;
        let ids: Vec<String> = ids.into_iter().take(20).collect();
        for message_id in &ids {
            if let Ok(value) = gmail::get_message(&app, &id, message_id).await {
                if let Ok(parsed) = parse::parse_message(&id, &value) {
                    let _ = with_db(&app, |conn| db::upsert_message(conn, &parsed.message));
                }
            }
            keys.push((id.clone(), message_id.clone()));
        }
    }
    let threads = with_db(&app, |conn| db::threads_for_messages(conn, &keys))?;
    let _ = app.emit("mail://updated", json!({}));
    Ok(threads)
}

#[tauri::command]
pub async fn sync_now(app: AppHandle) -> Result<(), String> {
    let _ = crate::later::release_due(&app).await;
    sync::request_label_check(&app);
    sync::sync_all(&app).await
}

/// Runs the quick new-mail check now; its result arrives as a `net://status` event.
#[tauri::command]
pub async fn check_connection(app: AppHandle) {
    sync::check_new_mail(&app).await;
}

#[tauri::command]
pub async fn threads_snooze(app: AppHandle, targets: Vec<ThreadTarget>, until_at: i64) -> Result<(), String> {
    if targets.is_empty() {
        return Ok(());
    }
    if until_at <= now_millis() {
        return Err("Choose a time in the future.".into());
    }
    let change = action_change("archive", None)?;
    for target in &targets {
        change_now(&app, target, &change)?;
        let account_id = target.account_id.clone();
        let thread_id = target.thread_id.clone();
        with_db(&app, |conn| db::snooze_set(conn, &account_id, &thread_id, until_at))?;
    }
    crate::refresh_badge(&app);
    let _ = app.emit("mail://updated", json!({}));
    Ok(())
}

#[tauri::command]
pub async fn threads_unsnooze(app: AppHandle, targets: Vec<ThreadTarget>) -> Result<(), String> {
    let change = action_change("unarchive", None)?;
    for target in &targets {
        change_now(&app, target, &change)?;
        let account_id = target.account_id.clone();
        let thread_id = target.thread_id.clone();
        with_db(&app, |conn| db::snooze_clear(conn, &account_id, &thread_id))?;
    }
    crate::refresh_badge(&app);
    let _ = app.emit("mail://updated", json!({}));
    Ok(())
}

/// Waking a snooze on its timer waits on Gmail: done offline, a rollback would leave the
/// conversation archived with its snooze already gone.
pub async fn wake_thread(app: &AppHandle, account_id: &str, thread_id: &str) -> Result<(), String> {
    let target = ThreadTarget {
        account_id: account_id.to_string(),
        thread_id: thread_id.to_string(),
    };
    apply_change(app, &target, &action_change("unarchive", None)?).await?;
    with_db(app, |conn| db::snooze_clear(conn, account_id, thread_id))
}

#[tauri::command]
pub async fn mail_schedule(app: AppHandle, request: ComposeRequest, send_at: i64) -> Result<String, String> {
    if send_at <= now_millis() + 4_000 {
        return Err("Choose a time at least a few seconds from now.".into());
    }
    if request.to.trim().is_empty() {
        return Err("Add a recipient before scheduling.".into());
    }
    let _ = prepare_raw(&app, &request)?;
    let id = local_id();
    let account_id = request.account_id.clone();
    with_db(&app, |conn| db::schedule_insert(conn, &id, &account_id, send_at, &request))?;
    let _ = app.emit("mail://updated", json!({}));
    Ok(id)
}

#[tauri::command]
pub async fn schedule_cancel(app: AppHandle, id: String) -> Result<(), String> {
    with_db(&app, |conn| db::schedule_delete(conn, &id))?;
    let _ = app.emit("mail://updated", json!({}));
    Ok(())
}

#[tauri::command]
pub async fn schedule_send_now(app: AppHandle, id: String) -> Result<SendResult, String> {
    let item = with_db(&app, |conn| db::schedule_get(conn, &id))?;
    let sent = mail_send(app.clone(), item.request).await?;
    let _ = with_db(&app, |conn| db::schedule_delete(conn, &id));
    Ok(sent)
}

#[tauri::command(async)]
pub fn templates_list(app: AppHandle) -> Result<Vec<Template>, String> {
    with_db(&app, db::templates_list)
}

#[tauri::command]
pub fn template_save(app: AppHandle, template: Template) -> Result<Template, String> {
    let name = template.name.trim();
    if name.is_empty() || name.len() > 80 {
        return Err("Name the template in a few words.".into());
    }
    if template.html.len() > 200_000 {
        return Err("That template is too long.".into());
    }
    let saved = Template {
        id: if template.id.trim().is_empty() { local_id() } else { template.id },
        name: name.to_string(),
        subject: template.subject.chars().take(200).collect(),
        html: template.html,
        updated_at: now_millis(),
    };
    let stored = saved.clone();
    with_db(&app, |conn| db::template_save(conn, &stored))?;
    Ok(saved)
}

#[tauri::command]
pub fn template_delete(app: AppHandle, id: String) -> Result<(), String> {
    with_db(&app, |conn| db::template_delete(conn, &id))
}

fn local_id() -> String {
    let mut bytes = [0u8; 9];
    rand::thread_rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

#[tauri::command]
pub async fn attachment_get(app: AppHandle, request: AttachmentRequest) -> Result<AttachmentData, String> {
    let meta = with_db(&app, |conn| {
        db::attachment_meta(conn, &request.account_id, &request.message_id, &request.attachment_id)
    })?;
    let bytes = load_attachment_bytes(&app, &request).await?;
    if bytes.len() > 15 * 1024 * 1024 {
        return Err("This file is too large to preview. Save it instead.".into());
    }
    Ok(AttachmentData {
        filename: meta.filename,
        mime_type: meta.mime_type,
        data_base64: STANDARD.encode(bytes),
    })
}

#[tauri::command]
pub async fn attachment_save(app: AppHandle, request: SaveAttachmentRequest) -> Result<(), String> {
    let bytes = load_attachment_bytes(
        &app,
        &AttachmentRequest {
            account_id: request.account_id,
            message_id: request.message_id,
            attachment_id: request.attachment_id,
        },
    )
    .await?;
    if let Some(parent) = std::path::Path::new(&request.path).parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::write(&request.path, bytes).map_err(|e| format!("Could not save the file: {e}"))
}

async fn apply_change(app: &AppHandle, target: &ThreadTarget, change: &Change) -> Result<(), String> {
    remote_change(app, target, change).await?;
    local_change(app, target, change)
}

/// Applies a change on this PC and queues it for Gmail, which may refuse it and roll it back.
fn change_now(app: &AppHandle, target: &ThreadTarget, change: &Change) -> Result<(), String> {
    let before = with_db(app, |conn| db::thread_labels(conn, &target.account_id, &target.thread_id))?;
    local_change(app, target, change)?;
    outbox::push(
        app,
        Job::Change {
            target: target.clone(),
            change: change.clone(),
            before,
        },
    );
    Ok(())
}

pub(crate) async fn remote_change(app: &AppHandle, target: &ThreadTarget, change: &Change) -> Result<(), String> {
    match change {
        Change::Trash => gmail::trash_thread(app, &target.account_id, &target.thread_id).await,
        Change::Untrash => gmail::untrash_thread(app, &target.account_id, &target.thread_id).await,
        Change::Labels { add, remove } => {
            gmail::modify_thread(app, &target.account_id, &target.thread_id, add, remove).await
        }
    }
}

pub(crate) fn local_change(app: &AppHandle, target: &ThreadTarget, change: &Change) -> Result<(), String> {
    let (add, remove): (Vec<String>, Vec<String>) = match change {
        Change::Trash => (vec!["TRASH".into()], vec!["INBOX".into()]),
        Change::Untrash => (vec!["INBOX".into()], vec!["TRASH".into()]),
        Change::Labels { add, remove } => (add.clone(), remove.clone()),
    };
    with_db(app, |conn| {
        db::apply_labels(conn, &target.account_id, &target.thread_id, &add, &remove)
    })
}

fn action_change(action: &str, label_id: Option<&str>) -> Result<Change, String> {
    let labels = |add: &[&str], remove: &[&str]| Change::Labels {
        add: add.iter().map(|item| (*item).to_string()).collect(),
        remove: remove.iter().map(|item| (*item).to_string()).collect(),
    };
    match action {
        "archive" => Ok(labels(&[], &["INBOX"])),
        "unarchive" => Ok(labels(&["INBOX"], &[])),
        "read" => Ok(labels(&[], &["UNREAD"])),
        "unread" => Ok(labels(&["UNREAD"], &[])),
        "star" => Ok(labels(&["STARRED"], &[])),
        "unstar" => Ok(labels(&[], &["STARRED"])),
        "spam" => Ok(labels(&["SPAM"], &["INBOX"])),
        "unspam" => Ok(labels(&["INBOX"], &["SPAM"])),
        "trash" => Ok(Change::Trash),
        "untrash" => Ok(Change::Untrash),
        "add-label" => {
            let id = label_id.unwrap_or("").to_string();
            validate_label(&id)?;
            Ok(Change::Labels { add: vec![id], remove: vec![] })
        }
        "remove-label" => {
            let id = label_id.unwrap_or("").to_string();
            validate_label(&id)?;
            Ok(Change::Labels { add: vec![], remove: vec![id] })
        }
        _ => Err("Unknown mail action.".into()),
    }
}

fn prepare_raw(app: &AppHandle, request: &ComposeRequest) -> Result<(String, Option<String>), String> {
    let account = with_db(app, |conn| db::get_account(conn, &request.account_id))?;
    let allowed = account.email == request.from_email
        || account.send_as.iter().any(|alias| alias.email == request.from_email);
    if !allowed {
        return Err("Choose a From address that belongs to the selected account.".into());
    }
    let to = mime_build::split_emails(&request.to)?;
    let cc = mime_build::split_emails(&request.cc)?;
    let bcc = mime_build::split_emails(&request.bcc)?;
    let mut bytes = Vec::new();
    for attachment in &request.attachments {
        let decoded = STANDARD
            .decode(attachment.data_base64.trim())
            .or_else(|_| URL_SAFE.decode(attachment.data_base64.trim()))
            .map_err(|_| format!("Could not read the attachment {}", attachment.filename))?;
        bytes.push(decoded);
    }
    let from_name = display_name(&account, &request.from_email);
    let mail = OutgoingMail {
        from_name: &from_name,
        from_email: &request.from_email,
        to: &to,
        cc: &cc,
        bcc: &bcc,
        subject: &request.subject,
        html: &request.html,
        attachments: &request.attachments,
        attachment_bytes: &bytes,
        in_reply_to: request.in_reply_to.as_deref(),
        references: request.references.as_deref(),
    };
    let raw_bytes = mime_build::build_rfc822(&mail)?;
    Ok((
        mime_build::to_gmail_raw(&raw_bytes),
        request.thread_id.clone().filter(|id| !id.is_empty()),
    ))
}

async fn load_attachment_bytes(app: &AppHandle, request: &AttachmentRequest) -> Result<Vec<u8>, String> {
    let data_dir = app.state::<AppState>().data_dir.clone();
    if let Some(bytes) = files::read_cached(
        &data_dir,
        &request.account_id,
        &request.message_id,
        &request.attachment_id,
    ) {
        return Ok(bytes);
    }
    if request.attachment_id.starts_with("inline-") {
        return Err("This inline file was not saved.".into());
    }
    let data = gmail::attachment(
        app,
        &request.account_id,
        &request.message_id,
        &request.attachment_id,
    )
    .await?;
    let bytes = URL_SAFE_NO_PAD
        .decode(data.trim())
        .or_else(|_| URL_SAFE.decode(data.trim()))
        .or_else(|_| STANDARD.decode(data.trim()))
        .map_err(|_| "Gmail returned an attachment Novabox could not read.".to_string())?;
    let _ = files::write_cached(
        &data_dir,
        &request.account_id,
        &request.message_id,
        &request.attachment_id,
        &bytes,
    );
    Ok(bytes)
}

fn display_name(account: &Account, email: &str) -> String {
    account
        .send_as
        .iter()
        .find(|alias| alias.email == email)
        .map(|alias| {
            if alias.display_name.is_empty() {
                account.name.clone()
            } else {
                alias.display_name.clone()
            }
        })
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| account.name.clone())
}

fn address_list(input: &str) -> Vec<Address> {
    mime_build::split_emails(input)
        .unwrap_or_default()
        .into_iter()
        .map(|email| Address { name: String::new(), email })
        .collect()
}

fn html_to_text(html: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                out.push(' ');
            }
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

pub(crate) fn with_db<T>(app: &AppHandle, op: impl FnOnce(&rusqlite::Connection) -> Result<T, String>) -> Result<T, String> {
    let state = app.state::<AppState>();
    let db = state.db.lock().map_err(|e| e.to_string())?;
    op(&db)
}
