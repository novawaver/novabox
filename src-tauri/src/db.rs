use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::time::Duration;

use rusqlite::{params, Connection};
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::model::{
    Account, AttachmentMeta, ComposeRequest, FolderCounts, InboxTabs, Label, Mailbox, MessageDetail, SendAs,
    StoredMessage, Template, ThreadDetail, ThreadQuery, ThreadSummary,
};

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS accounts (
    id TEXT PRIMARY KEY,
    email TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    picture TEXT,
    color TEXT NOT NULL,
    signature TEXT NOT NULL DEFAULT '',
    send_as_json TEXT NOT NULL DEFAULT '[]',
    history_id TEXT,
    recent_page_token TEXT,
    backfill_token TEXT,
    backfill_done INTEGER NOT NULL DEFAULT 0,
    initial_done INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS messages (
    account_id TEXT NOT NULL,
    id TEXT NOT NULL,
    thread_id TEXT NOT NULL,
    from_name TEXT NOT NULL DEFAULT '',
    from_email TEXT NOT NULL DEFAULT '',
    to_json TEXT NOT NULL DEFAULT '[]',
    cc_json TEXT NOT NULL DEFAULT '[]',
    bcc_json TEXT NOT NULL DEFAULT '[]',
    subject TEXT NOT NULL DEFAULT '',
    snippet TEXT NOT NULL DEFAULT '',
    body_html TEXT NOT NULL DEFAULT '',
    body_text TEXT NOT NULL DEFAULT '',
    date INTEGER NOT NULL DEFAULT 0,
    label_ids TEXT NOT NULL DEFAULT '[]',
    has_attachments INTEGER NOT NULL DEFAULT 0,
    attachments_json TEXT NOT NULL DEFAULT '[]',
    message_id_header TEXT NOT NULL DEFAULT '',
    references_header TEXT NOT NULL DEFAULT '',
    draft_id TEXT,
    size_estimate INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (account_id, id)
);
CREATE TABLE IF NOT EXISTS labels (
    account_id TEXT NOT NULL,
    id TEXT NOT NULL,
    name TEXT NOT NULL,
    kind TEXT NOT NULL,
    color TEXT,
    unread INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (account_id, id)
);
CREATE INDEX IF NOT EXISTS idx_messages_thread ON messages(account_id, thread_id);
CREATE INDEX IF NOT EXISTS idx_messages_date ON messages(account_id, date);
CREATE TABLE IF NOT EXISTS snoozes (
    account_id TEXT NOT NULL,
    thread_id TEXT NOT NULL,
    until_at INTEGER NOT NULL,
    PRIMARY KEY (account_id, thread_id)
);
CREATE TABLE IF NOT EXISTS scheduled_mail (
    id TEXT PRIMARY KEY,
    account_id TEXT NOT NULL,
    send_at INTEGER NOT NULL,
    payload_json TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS templates (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    subject TEXT NOT NULL DEFAULT '',
    html TEXT NOT NULL DEFAULT '',
    updated_at INTEGER NOT NULL
);
CREATE VIRTUAL TABLE IF NOT EXISTS messages_fts USING fts5(
    account_id UNINDEXED,
    message_id UNINDEXED,
    subject,
    body_text,
    from_name,
    from_email,
    snippet,
    tokenize = 'porter unicode61'
);
"#;

pub fn open(path: &Path) -> Result<Connection, String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let conn = Connection::open(path).map_err(|e| format!("Could not open the local mailbox: {e}"))?;
    conn.pragma_update(None, "journal_mode", "WAL").ok();
    conn.busy_timeout(Duration::from_secs(5)).map_err(|e| e.to_string())?;
    migrate(&conn)?;
    Ok(conn)
}

pub fn migrate(conn: &Connection) -> Result<(), String> {
    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap_or(0);
    if version < 1 {
        conn.execute_batch(SCHEMA).map_err(|e| format!("Could not prepare the local mailbox: {e}"))?;
        conn.pragma_update(None, "user_version", 1).map_err(|e| e.to_string())?;
    } else if version < 2 {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS snoozes (
                account_id TEXT NOT NULL,
                thread_id TEXT NOT NULL,
                until_at INTEGER NOT NULL,
                PRIMARY KEY (account_id, thread_id)
            );
            CREATE TABLE IF NOT EXISTS scheduled_mail (
                id TEXT PRIMARY KEY,
                account_id TEXT NOT NULL,
                send_at INTEGER NOT NULL,
                payload_json TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS templates (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                subject TEXT NOT NULL DEFAULT '',
                html TEXT NOT NULL DEFAULT '',
                updated_at INTEGER NOT NULL
            );",
        )
        .map_err(|e| format!("Could not prepare snooze and templates: {e}"))?;
        conn.pragma_update(None, "user_version", 2).map_err(|e| e.to_string())?;
    }
    if version < 1 {
        conn.pragma_update(None, "user_version", 2).map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub fn upsert_account(conn: &Connection, account: &Account) -> Result<(), String> {
    let send_as = to_json(&account.send_as);
    conn.execute(
        "INSERT INTO accounts (id, email, name, picture, color, signature, send_as_json, history_id, recent_page_token, backfill_token, backfill_done, initial_done, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
         ON CONFLICT(id) DO UPDATE SET
            email = excluded.email,
            name = excluded.name,
            picture = excluded.picture,
            send_as_json = excluded.send_as_json",
        params![
            account.id,
            account.email,
            account.name,
            account.picture,
            account.color,
            account.signature,
            send_as,
            account.history_id,
            account.recent_page_token,
            account.backfill_token,
            account.backfill_done as i64,
            account.initial_done as i64,
            crate::util::now_secs(),
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn list_accounts(conn: &Connection) -> Result<Vec<Account>, String> {
    let mut stmt = conn
        .prepare("SELECT id, email, name, picture, color, signature, send_as_json, history_id, recent_page_token, backfill_token, backfill_done, initial_done FROM accounts ORDER BY created_at")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| Ok(read_account(row)?))
        .map_err(|e| e.to_string())?;
    let mut accounts = Vec::new();
    for row in rows {
        accounts.push(row.map_err(|e| e.to_string())?);
    }
    drop(stmt);
    for account in &mut accounts {
        account.unread = unread_count(conn, &account.id)?;
    }
    Ok(accounts)
}

pub fn get_account(conn: &Connection, id: &str) -> Result<Account, String> {
    let mut account = conn
        .query_row(
            "SELECT id, email, name, picture, color, signature, send_as_json, history_id, recent_page_token, backfill_token, backfill_done, initial_done FROM accounts WHERE id = ?1",
            [id],
            |row| Ok(read_account(row)?),
        )
        .map_err(|_| "That account is not on this PC.".to_string())?;
    account.unread = unread_count(conn, id)?;
    Ok(account)
}

pub fn set_signature(conn: &Connection, account_id: &str, signature: &str) -> Result<(), String> {
    conn.execute(
        "UPDATE accounts SET signature = ?1 WHERE id = ?2",
        params![signature, account_id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn set_sync_state(
    conn: &Connection,
    account_id: &str,
    history_id: Option<&str>,
    recent_page_token: Option<&str>,
    backfill_token: Option<&str>,
    backfill_done: bool,
    initial_done: bool,
) -> Result<(), String> {
    conn.execute(
        "UPDATE accounts SET history_id = ?1, recent_page_token = ?2, backfill_token = ?3, backfill_done = ?4, initial_done = ?5 WHERE id = ?6",
        params![
            history_id,
            recent_page_token,
            backfill_token,
            backfill_done as i64,
            initial_done as i64,
            account_id
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn update_send_as(conn: &Connection, account_id: &str, send_as: &[SendAs]) -> Result<(), String> {
    conn.execute(
        "UPDATE accounts SET send_as_json = ?1 WHERE id = ?2",
        params![to_json(&send_as), account_id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn replace_labels(conn: &Connection, account_id: &str, labels: &[Label]) -> Result<(), String> {
    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    tx.execute("DELETE FROM labels WHERE account_id = ?1", [account_id])
        .map_err(|e| e.to_string())?;
    for label in labels {
        tx.execute(
            "INSERT INTO labels (account_id, id, name, kind, color, unread) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![label.account_id, label.id, label.name, label.kind, label.color, label.unread],
        )
        .map_err(|e| e.to_string())?;
    }
    tx.commit().map_err(|e| e.to_string())?;
    Ok(())
}

pub fn delete_account(conn: &Connection, account_id: &str) -> Result<(), String> {
    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    tx.execute("DELETE FROM messages_fts WHERE account_id = ?1", [account_id]).map_err(|e| e.to_string())?;
    tx.execute("DELETE FROM messages WHERE account_id = ?1", [account_id]).map_err(|e| e.to_string())?;
    tx.execute("DELETE FROM labels WHERE account_id = ?1", [account_id]).map_err(|e| e.to_string())?;
    tx.execute("DELETE FROM snoozes WHERE account_id = ?1", [account_id]).map_err(|e| e.to_string())?;
    tx.execute("DELETE FROM scheduled_mail WHERE account_id = ?1", [account_id]).map_err(|e| e.to_string())?;
    tx.execute("DELETE FROM accounts WHERE id = ?1", [account_id]).map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(())
}

pub fn upsert_message(conn: &Connection, message: &StoredMessage) -> Result<(), String> {
    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    tx.execute(
        "INSERT INTO messages (
            account_id, id, thread_id, from_name, from_email, to_json, cc_json, bcc_json, subject, snippet,
            body_html, body_text, date, label_ids, has_attachments, attachments_json, message_id_header,
            references_header, draft_id, size_estimate
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20)
        ON CONFLICT(account_id, id) DO UPDATE SET
            thread_id = excluded.thread_id,
            from_name = excluded.from_name,
            from_email = excluded.from_email,
            to_json = excluded.to_json,
            cc_json = excluded.cc_json,
            bcc_json = excluded.bcc_json,
            subject = excluded.subject,
            snippet = excluded.snippet,
            body_html = excluded.body_html,
            body_text = excluded.body_text,
            date = excluded.date,
            label_ids = excluded.label_ids,
            has_attachments = excluded.has_attachments,
            attachments_json = excluded.attachments_json,
            message_id_header = excluded.message_id_header,
            references_header = excluded.references_header,
            draft_id = COALESCE(excluded.draft_id, messages.draft_id),
            size_estimate = excluded.size_estimate",
        params![
            message.account_id,
            message.id,
            message.thread_id,
            message.from_name,
            message.from_email,
            to_json(&message.to),
            to_json(&message.cc),
            to_json(&message.bcc),
            message.subject,
            message.snippet,
            message.body_html,
            message.body_text,
            message.date,
            to_json(&message.label_ids),
            if message.attachments.is_empty() { 0 } else { 1 },
            to_json(&message.attachments),
            message.message_id_header,
            message.references_header,
            message.draft_id,
            message.size_estimate,
        ],
    )
    .map_err(|e| e.to_string())?;
    tx.execute(
        "DELETE FROM messages_fts WHERE account_id = ?1 AND message_id = ?2",
        params![message.account_id, message.id],
    )
    .map_err(|e| e.to_string())?;
    tx.execute(
        "INSERT INTO messages_fts (account_id, message_id, subject, body_text, from_name, from_email, snippet) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            message.account_id,
            message.id,
            message.subject,
            message.body_text,
            message.from_name,
            message.from_email,
            message.snippet
        ],
    )
    .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(())
}

pub fn message_count(conn: &Connection, account_id: &str) -> Result<i64, String> {
    conn.query_row(
        "SELECT COUNT(*) FROM messages WHERE account_id = ?1",
        [account_id],
        |row| row.get(0),
    )
    .map_err(|e| e.to_string())
}

pub fn has_message(conn: &Connection, account_id: &str, message_id: &str) -> Result<bool, String> {
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM messages WHERE account_id = ?1 AND id = ?2",
            params![account_id, message_id],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    Ok(count > 0)
}

pub fn set_draft_id(conn: &Connection, account_id: &str, message_id: &str, draft_id: &str) -> Result<(), String> {
    conn.execute(
        "UPDATE messages SET draft_id = ?1 WHERE account_id = ?2 AND id = ?3",
        params![draft_id, account_id, message_id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn delete_message(conn: &Connection, account_id: &str, message_id: &str) -> Result<(), String> {
    conn.execute(
        "DELETE FROM messages_fts WHERE account_id = ?1 AND message_id = ?2",
        params![account_id, message_id],
    )
    .map_err(|e| e.to_string())?;
    conn.execute(
        "DELETE FROM messages WHERE account_id = ?1 AND id = ?2",
        params![account_id, message_id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn draft_message_id(conn: &Connection, account_id: &str, draft_id: &str) -> Result<Option<String>, String> {
    match conn.query_row(
        "SELECT id FROM messages WHERE account_id = ?1 AND draft_id = ?2",
        params![account_id, draft_id],
        |row| row.get(0),
    ) {
        Ok(id) => Ok(Some(id)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(err) => Err(err.to_string()),
    }
}

pub fn delete_draft(conn: &Connection, account_id: &str, draft_id: &str) -> Result<(), String> {
    let mut stmt = conn
        .prepare("SELECT id FROM messages WHERE account_id = ?1 AND draft_id = ?2")
        .map_err(|e| e.to_string())?;
    let ids: Vec<String> = stmt
        .query_map(params![account_id, draft_id], |row| row.get(0))
        .map_err(|e| e.to_string())?
        .filter_map(|row| row.ok())
        .collect();
    drop(stmt);
    for id in ids {
        delete_message(conn, account_id, &id)?;
    }
    Ok(())
}

pub fn apply_labels(
    conn: &Connection,
    account_id: &str,
    thread_id: &str,
    add: &[String],
    remove: &[String],
) -> Result<(), String> {
    let mut stmt = conn
        .prepare("SELECT id, label_ids FROM messages WHERE account_id = ?1 AND thread_id = ?2")
        .map_err(|e| e.to_string())?;
    let rows: Vec<(String, String)> = stmt
        .query_map(params![account_id, thread_id], |row| Ok((row.get(0)?, row.get(1)?)))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    drop(stmt);
    for (id, json) in rows {
        let mut labels: Vec<String> = json_vec(&json);
        for item in add {
            if !labels.iter().any(|label| label == item) {
                labels.push(item.clone());
            }
        }
        labels.retain(|label| !remove.iter().any(|item| item == label));
        conn.execute(
            "UPDATE messages SET label_ids = ?1 WHERE account_id = ?2 AND id = ?3",
            params![to_json(&labels), account_id, id],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub fn get_thread(conn: &Connection, account_id: &str, thread_id: &str) -> Result<ThreadDetail, String> {
    let account = get_account(conn, account_id)?;
    let mut stmt = conn
        .prepare(
            "SELECT account_id, id, thread_id, from_name, from_email, to_json, cc_json, bcc_json, subject, snippet, body_html, body_text, date, label_ids, attachments_json, message_id_header, references_header, draft_id
             FROM messages WHERE account_id = ?1 AND thread_id = ?2 ORDER BY date ASC, id ASC",
        )
        .map_err(|e| e.to_string())?;
    let messages = stmt
        .query_map(params![account_id, thread_id], |row| Ok(read_message(row)?))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    if messages.is_empty() {
        return Err("That conversation is not saved on this PC.".into());
    }
    let subject = messages
        .iter()
        .find(|message| !message.subject.is_empty())
        .map(|message| message.subject.clone())
        .unwrap_or_default();
    Ok(ThreadDetail {
        account_id: account_id.to_string(),
        account_email: account.email,
        account_color: account.color,
        thread_id: thread_id.to_string(),
        subject,
        messages,
    })
}

#[derive(Debug, Clone)]
pub struct ScheduledMail {
    pub id: String,
    pub account_id: String,
    pub send_at: i64,
    pub request: ComposeRequest,
}

pub fn snooze_set(conn: &Connection, account_id: &str, thread_id: &str, until_at: i64) -> Result<(), String> {
    conn.execute(
        "INSERT INTO snoozes (account_id, thread_id, until_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(account_id, thread_id) DO UPDATE SET until_at = excluded.until_at",
        params![account_id, thread_id, until_at],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn snooze_clear(conn: &Connection, account_id: &str, thread_id: &str) -> Result<(), String> {
    conn.execute(
        "DELETE FROM snoozes WHERE account_id = ?1 AND thread_id = ?2",
        params![account_id, thread_id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn snoozes_due(conn: &Connection, now: i64) -> Result<Vec<(String, String)>, String> {
    let mut stmt = conn
        .prepare("SELECT account_id, thread_id FROM snoozes WHERE until_at <= ?1")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![now], |row| Ok((row.get(0)?, row.get(1)?)))
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

pub fn schedule_insert(conn: &Connection, id: &str, account_id: &str, send_at: i64, request: &ComposeRequest) -> Result<(), String> {
    let payload = serde_json::to_string(request).map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO scheduled_mail (id, account_id, send_at, payload_json) VALUES (?1, ?2, ?3, ?4)",
        params![id, account_id, send_at, payload],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn schedule_delete(conn: &Connection, id: &str) -> Result<(), String> {
    conn.execute("DELETE FROM scheduled_mail WHERE id = ?1", [id]).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn schedule_delay(conn: &Connection, id: &str, send_at: i64) -> Result<(), String> {
    conn.execute("UPDATE scheduled_mail SET send_at = ?1 WHERE id = ?2", params![send_at, id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn schedule_get(conn: &Connection, id: &str) -> Result<ScheduledMail, String> {
    load_scheduled(conn, "")?
        .into_iter()
        .find(|item| item.id == id)
        .ok_or_else(|| "That scheduled message is no longer waiting.".to_string())
}

pub fn scheduled_due(conn: &Connection, now: i64) -> Result<Vec<ScheduledMail>, String> {
    Ok(load_scheduled(conn, "")?.into_iter().filter(|item| item.send_at <= now).collect())
}

pub fn templates_list(conn: &Connection) -> Result<Vec<Template>, String> {
    let mut stmt = conn
        .prepare("SELECT id, name, subject, html, updated_at FROM templates ORDER BY updated_at DESC")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok(Template {
                id: row.get(0)?,
                name: row.get(1)?,
                subject: row.get(2)?,
                html: row.get(3)?,
                updated_at: row.get(4)?,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

pub fn template_save(conn: &Connection, template: &Template) -> Result<(), String> {
    conn.execute(
        "INSERT INTO templates (id, name, subject, html, updated_at) VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(id) DO UPDATE SET name = excluded.name, subject = excluded.subject, html = excluded.html, updated_at = excluded.updated_at",
        params![template.id, template.name, template.subject, template.html, template.updated_at],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn template_delete(conn: &Connection, id: &str) -> Result<(), String> {
    conn.execute("DELETE FROM templates WHERE id = ?1", [id]).map_err(|e| e.to_string())?;
    Ok(())
}

fn load_snoozes(conn: &Connection, account_id: &str) -> Result<HashMap<(String, String), i64>, String> {
    let mut stmt = conn
        .prepare("SELECT account_id, thread_id, until_at FROM snoozes WHERE (?1 = '' OR account_id = ?1)")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![account_id], |row| Ok(((row.get(0)?, row.get(1)?), row.get(2)?)))
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<HashMap<_, _>, _>>().map_err(|e| e.to_string())
}

fn load_scheduled(conn: &Connection, account_id: &str) -> Result<Vec<ScheduledMail>, String> {
    let mut stmt = conn
        .prepare("SELECT id, account_id, send_at, payload_json FROM scheduled_mail WHERE (?1 = '' OR account_id = ?1) ORDER BY send_at ASC")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![account_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, i64>(2)?, row.get::<_, String>(3)?))
        })
        .map_err(|e| e.to_string())?;
    let mut items = Vec::new();
    for row in rows {
        let (id, account_id, send_at, payload) = row.map_err(|e| e.to_string())?;
        if let Ok(request) = serde_json::from_str::<ComposeRequest>(&payload) {
            items.push(ScheduledMail { id, account_id, send_at, request });
        }
    }
    Ok(items)
}

pub fn query_mailbox(conn: &Connection, query: &ThreadQuery) -> Result<Mailbox, String> {
    let accounts = list_accounts(conn)?;
    let account_filter = query.account_id.clone().unwrap_or_default();
    let headers = load_headers(conn, &account_filter)?;
    let account_map = account_pairs(&accounts);
    let groups = group_headers(&headers);
    let snoozes = load_snoozes(conn, &account_filter)?;
    let scheduled = load_scheduled(conn, &account_filter)?;
    let tabs = &query.inbox_tabs;
    let mut counts = counts_from(&groups, tabs);
    for ((account_id, thread_id), messages) in &groups {
        let key = (account_id.clone(), thread_id.clone());
        if !snoozes.contains_key(&key) || !thread_in(messages, "inbox", None, None, tabs) {
            continue;
        }
        counts.inbox -= 1;
        if unread_in(messages, "inbox", None, None, tabs) {
            counts.inbox_unread -= 1;
        }
        if unread_in(messages, "inbox", None, Some("primary"), tabs) {
            counts.primary_unread -= 1;
        }
        if unread_in(messages, "inbox", None, Some("promotions"), tabs) {
            counts.promotions_unread -= 1;
        }
        if unread_in(messages, "inbox", None, Some("social"), tabs) {
            counts.social_unread -= 1;
        }
        if unread_in(messages, "inbox", None, Some("updates"), tabs) {
            counts.updates_unread -= 1;
        }
    }
    counts.snoozed = groups.keys().filter(|key| snoozes.contains_key(*key)).count() as i64;
    counts.scheduled = scheduled.len() as i64;
    let mut labels = load_labels(conn, &account_filter)?;
    for label in &mut labels {
        if label.kind != "user" {
            continue;
        }
        label.unread = groups
            .values()
            .filter(|messages| {
                messages.iter().any(|message| {
                    message.account_id == label.account_id
                        && has_label(&message.label_ids, &label.id)
                        && has_label(&message.label_ids, "UNREAD")
                        && !has_label(&message.label_ids, "TRASH")
                })
            })
            .count() as i64;
    }
    let search = query.search.as_deref().unwrap_or("").trim().to_string();
    let threads = if query.folder == "scheduled" && search.is_empty() {
        scheduled_threads(&scheduled, &account_map)
    } else if query.folder == "snoozed" && search.is_empty() {
        let waiting: HashMap<_, _> = groups.into_iter().filter(|(key, _)| snoozes.contains_key(key)).collect();
        let mut threads = summarize(waiting, &account_map, "all", None, None, tabs);
        for thread in &mut threads {
            thread.snooze_until = snoozes.get(&(thread.account_id.clone(), thread.thread_id.clone())).copied();
        }
        threads.retain(|thread| !has_label(&thread.label_ids, "TRASH") && !has_label(&thread.label_ids, "SPAM"));
        threads
    } else if search.is_empty() {
        let mut threads = summarize(groups, &account_map, &query.folder, query.label_id.as_deref(), query.category.as_deref(), tabs);
        if query.folder == "inbox" {
            threads.retain(|thread| !snoozes.contains_key(&(thread.account_id.clone(), thread.thread_id.clone())));
        }
        threads
    } else {
        let matched = match fts_ids(conn, &search, &account_filter)? {
            Some(ids) => ids,
            None => like_ids(&headers, &search),
        };
        let filtered: Vec<Header> = headers
            .into_iter()
            .filter(|header| matched.contains(&(header.account_id.clone(), header.id.clone())))
            .collect();
        summarize(group_headers(&filtered), &account_map, "all", None, None, tabs)
    };
    let offset = query.offset.max(0) as usize;
    let limit = query.limit.clamp(1, 200) as usize;
    let total = threads.len();
    let has_more = total > offset + limit;
    let threads = threads.into_iter().skip(offset).take(limit).collect();
    Ok(Mailbox {
        accounts,
        labels,
        threads,
        has_more,
        total,
        counts,
    })
}

pub fn threads_for_messages(conn: &Connection, keys: &[(String, String)]) -> Result<Vec<ThreadSummary>, String> {
    let accounts = list_accounts(conn)?;
    let account_map = account_pairs(&accounts);
    let mut seen = HashSet::new();
    let mut headers = Vec::new();
    for (account_id, message_id) in keys {
        if let Some(thread_id) = lookup_thread(conn, account_id, message_id)? {
            if seen.insert((account_id.clone(), thread_id.clone())) {
                headers.extend(load_thread_headers(conn, account_id, &thread_id)?);
            }
        }
    }
    let mut threads = summarize(group_headers(&headers), &account_map, "all", None, None, &InboxTabs::default());
    threads.truncate(200);
    Ok(threads)
}

pub fn total_unread(conn: &Connection) -> Result<i64, String> {
    conn.query_row(
        "SELECT COUNT(DISTINCT thread_id) FROM messages WHERE instr(label_ids, '\"UNREAD\"') > 0 AND instr(label_ids, '\"INBOX\"') > 0 AND instr(label_ids, '\"TRASH\"') = 0 AND instr(label_ids, '\"SPAM\"') = 0",
        [],
        |row| row.get(0),
    )
    .map_err(|e| e.to_string())
}

pub fn attachment_meta(conn: &Connection, account_id: &str, message_id: &str, attachment_id: &str) -> Result<AttachmentMeta, String> {
    let json: String = conn
        .query_row(
            "SELECT attachments_json FROM messages WHERE account_id = ?1 AND id = ?2",
            params![account_id, message_id],
            |row| row.get(0),
        )
        .map_err(|_| "That attachment is not saved on this PC.".to_string())?;
    json_vec::<AttachmentMeta>(&json)
        .into_iter()
        .find(|item| item.id == attachment_id)
        .ok_or_else(|| "That attachment is not saved on this PC.".to_string())
}

fn unread_count(conn: &Connection, account_id: &str) -> Result<i64, String> {
    conn.query_row(
        "SELECT COUNT(DISTINCT thread_id) FROM messages WHERE account_id = ?1 AND instr(label_ids, '\"UNREAD\"') > 0 AND instr(label_ids, '\"INBOX\"') > 0 AND instr(label_ids, '\"TRASH\"') = 0 AND instr(label_ids, '\"SPAM\"') = 0",
        [account_id],
        |row| row.get(0),
    )
    .map_err(|e| e.to_string())
}

fn read_account(row: &rusqlite::Row) -> rusqlite::Result<Account> {
    Ok(Account {
        id: row.get(0)?,
        email: row.get(1)?,
        name: row.get(2)?,
        picture: row.get(3)?,
        color: row.get(4)?,
        unread: 0,
        signature: row.get(5)?,
        send_as: json_vec(&row.get::<_, String>(6)?),
        history_id: row.get(7)?,
        recent_page_token: row.get(8)?,
        backfill_token: row.get(9)?,
        backfill_done: row.get::<_, i64>(10)? != 0,
        initial_done: row.get::<_, i64>(11)? != 0,
    })
}

fn read_message(row: &rusqlite::Row) -> rusqlite::Result<MessageDetail> {
    let label_ids: Vec<String> = json_vec(&row.get::<_, String>(13)?);
    let unread = has_label(&label_ids, "UNREAD");
    let starred = has_label(&label_ids, "STARRED");
    Ok(MessageDetail {
        account_id: row.get(0)?,
        id: row.get(1)?,
        thread_id: row.get(2)?,
        from_name: row.get(3)?,
        from_email: row.get(4)?,
        to: json_vec(&row.get::<_, String>(5)?),
        cc: json_vec(&row.get::<_, String>(6)?),
        bcc: json_vec(&row.get::<_, String>(7)?),
        subject: row.get(8)?,
        snippet: row.get(9)?,
        body_html: row.get(10)?,
        body_text: row.get(11)?,
        date: row.get(12)?,
        unread,
        starred,
        attachments: json_vec(&row.get::<_, String>(14)?),
        message_id_header: row.get(15)?,
        references_header: row.get(16)?,
        draft_id: row.get(17)?,
        label_ids,
    })
}

#[derive(Clone)]
struct Header {
    account_id: String,
    id: String,
    thread_id: String,
    from_name: String,
    from_email: String,
    subject: String,
    snippet: String,
    date: i64,
    label_ids: Vec<String>,
    has_attachments: bool,
}

fn load_headers(conn: &Connection, account_id: &str) -> Result<Vec<Header>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT account_id, id, thread_id, from_name, from_email, subject, snippet, date, label_ids, has_attachments
             FROM messages WHERE (?1 = '' OR account_id = ?1)",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![account_id], |row| {
            let label_ids: Vec<String> = json_vec(&row.get::<_, String>(8)?);
            Ok(Header {
                account_id: row.get(0)?,
                id: row.get(1)?,
                thread_id: row.get(2)?,
                from_name: row.get(3)?,
                from_email: row.get(4)?,
                subject: row.get(5)?,
                snippet: row.get(6)?,
                date: row.get(7)?,
                label_ids,
                has_attachments: row.get::<_, i64>(9)? != 0,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

fn load_thread_headers(conn: &Connection, account_id: &str, thread_id: &str) -> Result<Vec<Header>, String> {
    Ok(load_headers(conn, account_id)?
        .into_iter()
        .filter(|header| header.thread_id == thread_id)
        .collect())
}

fn lookup_thread(conn: &Connection, account_id: &str, message_id: &str) -> Result<Option<String>, String> {
    match conn.query_row(
        "SELECT thread_id FROM messages WHERE account_id = ?1 AND id = ?2",
        params![account_id, message_id],
        |row| row.get(0),
    ) {
        Ok(id) => Ok(Some(id)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(err) => Err(err.to_string()),
    }
}

fn load_labels(conn: &Connection, account_id: &str) -> Result<Vec<Label>, String> {
    let mut stmt = conn
        .prepare("SELECT account_id, id, name, kind, color, unread FROM labels WHERE (?1 = '' OR account_id = ?1) ORDER BY name")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![account_id], |row| {
            Ok(Label {
                account_id: row.get(0)?,
                id: row.get(1)?,
                name: row.get(2)?,
                kind: row.get(3)?,
                color: row.get(4)?,
                unread: row.get(5)?,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

fn fts_ids(conn: &Connection, raw: &str, account_id: &str) -> Result<Option<HashSet<(String, String)>>, String> {
    let query = fts_query(raw);
    if query.is_empty() {
        return Ok(Some(HashSet::new()));
    }
    let mut stmt = match conn.prepare(
        "SELECT account_id, message_id FROM messages_fts WHERE messages_fts MATCH ?1 AND (?2 = '' OR account_id = ?2) LIMIT 300",
    ) {
        Ok(stmt) => stmt,
        Err(_) => return Ok(None),
    };
    let rows = match stmt.query_map(params![query, account_id], |row| Ok((row.get(0)?, row.get(1)?))) {
        Ok(rows) => rows,
        Err(_) => return Ok(None),
    };
    let mut ids = HashSet::new();
    for row in rows {
        match row {
            Ok(pair) => {
                ids.insert(pair);
            }
            Err(_) => return Ok(None),
        }
    }
    Ok(Some(ids))
}

fn fts_query(input: &str) -> String {
    input
        .split_whitespace()
        .filter_map(|word| {
            let clean: String = word
                .chars()
                .filter(|ch| ch.is_alphanumeric() || matches!(*ch, '@' | '.' | '_' | '-'))
                .collect();
            if clean.is_empty() {
                None
            } else {
                Some(format!("\"{clean}\"*"))
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn like_ids(headers: &[Header], needle: &str) -> HashSet<(String, String)> {
    let needle = needle.to_lowercase();
    headers
        .iter()
        .filter(|header| {
            header.subject.to_lowercase().contains(&needle)
                || header.snippet.to_lowercase().contains(&needle)
                || header.from_email.to_lowercase().contains(&needle)
                || header.from_name.to_lowercase().contains(&needle)
        })
        .map(|header| (header.account_id.clone(), header.id.clone()))
        .collect()
}

fn group_headers(headers: &[Header]) -> HashMap<(String, String), Vec<Header>> {
    let mut groups: HashMap<(String, String), Vec<Header>> = HashMap::new();
    for header in headers {
        groups
            .entry((header.account_id.clone(), header.thread_id.clone()))
            .or_default()
            .push(header.clone());
    }
    groups
}

fn account_pairs(accounts: &[Account]) -> HashMap<String, (String, String)> {
    accounts
        .iter()
        .map(|account| (account.id.clone(), (account.email.clone(), account.color.clone())))
        .collect()
}

fn scheduled_threads(items: &[ScheduledMail], accounts: &HashMap<String, (String, String)>) -> Vec<ThreadSummary> {
    let mut threads = Vec::new();
    for item in items {
        let (email, color) = accounts
            .get(&item.account_id)
            .cloned()
            .unwrap_or_else(|| (String::new(), "#888888".into()));
        let plain = html_plain(&item.request.html);
        threads.push(ThreadSummary {
            account_id: item.account_id.clone(),
            account_email: email,
            account_color: color,
            thread_id: format!("sched:{}", item.id),
            latest_message_id: item.id.clone(),
            from_name: item.request.from_email.clone(),
            from_email: item.request.from_email.clone(),
            subject: item.request.subject.clone(),
            snippet: plain.chars().take(160).collect(),
            date: item.send_at,
            unread: false,
            starred: false,
            has_attachments: !item.request.attachments.is_empty(),
            message_count: 1,
            label_ids: Vec::new(),
            snooze_until: None,
            scheduled_id: Some(item.id.clone()),
            body_html: Some(item.request.html.clone()),
            preview_to: Some(item.request.to.clone()),
        });
    }
    threads
}

fn html_plain(html: &str) -> String {
    let mut out = String::new();
    let mut tag = false;
    for ch in html.chars() {
        if ch == '<' {
            tag = true;
            out.push(' ');
            continue;
        }
        if ch == '>' {
            tag = false;
            continue;
        }
        if !tag {
            out.push(ch);
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn summarize(
    groups: HashMap<(String, String), Vec<Header>>,
    accounts: &HashMap<String, (String, String)>,
    folder: &str,
    label_id: Option<&str>,
    category: Option<&str>,
    tabs: &InboxTabs,
) -> Vec<ThreadSummary> {
    let mut threads = Vec::new();
    for ((account_id, thread_id), messages) in groups {
        let matching: Vec<&Header> = messages
            .iter()
            .filter(|message| message_in_folder(&message.label_ids, folder, label_id, category, tabs))
            .collect();
        if matching.is_empty() {
            continue;
        }
        let latest = matching
            .iter()
            .max_by_key(|message| (message.date, message.id.clone()))
            .expect("matching is non-empty");
        let (email, color) = accounts
            .get(&account_id)
            .cloned()
            .unwrap_or_else(|| (String::new(), "#888888".into()));
        threads.push(ThreadSummary {
            account_id,
            account_email: email,
            account_color: color,
            thread_id,
            latest_message_id: latest.id.clone(),
            from_name: latest.from_name.clone(),
            from_email: latest.from_email.clone(),
            subject: latest.subject.clone(),
            snippet: latest.snippet.clone(),
            date: latest.date,
            unread: matching.iter().any(|message| has_label(&message.label_ids, "UNREAD")),
            starred: matching.iter().any(|message| has_label(&message.label_ids, "STARRED")),
            has_attachments: matching.iter().any(|message| message.has_attachments),
            message_count: matching.len() as i64,
            label_ids: latest.label_ids.clone(),
            snooze_until: None,
            scheduled_id: None,
            body_html: None,
            preview_to: None,
        });
    }
    threads.sort_by(|a, b| b.date.cmp(&a.date).then(b.thread_id.cmp(&a.thread_id)));
    threads
}

fn counts_from(groups: &HashMap<(String, String), Vec<Header>>, tabs: &InboxTabs) -> FolderCounts {
    let mut counts = FolderCounts {
        inbox: 0,
        inbox_unread: 0,
        starred: 0,
        drafts: 0,
        primary_unread: 0,
        promotions_unread: 0,
        social_unread: 0,
        updates_unread: 0,
        spam_unread: 0,
        snoozed: 0,
        scheduled: 0,
    };
    for messages in groups.values() {
        if thread_in(messages, "inbox", None, None, tabs) {
            counts.inbox += 1;
            if unread_in(messages, "inbox", None, None, tabs) {
                counts.inbox_unread += 1;
            }
        }
        if thread_in(messages, "starred", None, None, tabs) {
            counts.starred += 1;
        }
        if thread_in(messages, "drafts", None, None, tabs) {
            counts.drafts += 1;
        }
        if unread_in(messages, "inbox", None, Some("primary"), tabs) {
            counts.primary_unread += 1;
        }
        if unread_in(messages, "inbox", None, Some("promotions"), tabs) {
            counts.promotions_unread += 1;
        }
        if unread_in(messages, "inbox", None, Some("social"), tabs) {
            counts.social_unread += 1;
        }
        if unread_in(messages, "inbox", None, Some("updates"), tabs) {
            counts.updates_unread += 1;
        }
        if unread_in(messages, "spam", None, None, tabs) {
            counts.spam_unread += 1;
        }
    }
    counts
}

fn thread_in(messages: &[Header], folder: &str, label_id: Option<&str>, category: Option<&str>, tabs: &InboxTabs) -> bool {
    messages
        .iter()
        .any(|message| message_in_folder(&message.label_ids, folder, label_id, category, tabs))
}

fn unread_in(messages: &[Header], folder: &str, label_id: Option<&str>, category: Option<&str>, tabs: &InboxTabs) -> bool {
    messages.iter().any(|message| {
        has_label(&message.label_ids, "UNREAD") && message_in_folder(&message.label_ids, folder, label_id, category, tabs)
    })
}

fn message_in_folder(labels: &[String], folder: &str, label_id: Option<&str>, category: Option<&str>, tabs: &InboxTabs) -> bool {
    let trash = has_label(labels, "TRASH");
    let spam = has_label(labels, "SPAM");
    match folder {
        "all" => true,
        "inbox" => {
            if !has_label(labels, "INBOX") || trash || spam {
                return false;
            }
            // Gmail keeps a category in Primary until that inbox tab is turned on.
            // Updates is off by default, so CATEGORY_UPDATES mail stays in Primary.
            let promotions = tabs.promotions && has_label(labels, "CATEGORY_PROMOTIONS");
            let social = tabs.social && (has_label(labels, "CATEGORY_SOCIAL") || has_label(labels, "CATEGORY_FORUMS"));
            let updates = tabs.updates && has_label(labels, "CATEGORY_UPDATES");
            match category.unwrap_or("primary") {
                "promotions" => promotions,
                "social" => social,
                "updates" => updates,
                _ => !promotions && !social && !updates,
            }
        }
        "starred" => has_label(labels, "STARRED") && !trash,
        "sent" => has_label(labels, "SENT") && !trash,
        "drafts" => has_label(labels, "DRAFT") && !trash,
        "spam" => spam,
        "trash" => trash,
        "label" => label_id.map(|id| has_label(labels, id)).unwrap_or(false) && !trash,
        _ => false,
    }
}

fn has_label(labels: &[String], id: &str) -> bool {
    labels.iter().any(|label| label == id)
}

fn to_json<T: Serialize>(value: &T) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "[]".into())
}

fn json_vec<T: DeserializeOwned>(text: &str) -> Vec<T> {
    serde_json::from_str(text).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(id: &str, thread: &str, labels: &[&str], subject: &str, body: &str) -> StoredMessage {
        StoredMessage {
            account_id: "acct".into(),
            id: id.into(),
            thread_id: thread.into(),
            from_name: "Ada".into(),
            from_email: "ada@harbor.test".into(),
            to: vec![crate::model::Address { name: "Maya".into(), email: "maya@northmail.test".into() }],
            cc: vec![],
            bcc: vec![],
            subject: subject.into(),
            snippet: body.into(),
            body_html: format!("<p>{body}</p>"),
            body_text: body.into(),
            date: 1_700_000_000_000 + id.as_bytes()[0] as i64,
            label_ids: labels.iter().map(|label| (*label).to_string()).collect(),
            attachments: vec![],
            message_id_header: format!("<{id}@test>"),
            references_header: String::new(),
            draft_id: None,
            size_estimate: 10,
        }
    }

    #[test]
    fn folders_search_and_labels() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        let account = Account {
            id: "acct".into(),
            email: "maya@northmail.test".into(),
            name: "Maya".into(),
            picture: None,
            color: "#C9842A".into(),
            unread: 0,
            signature: String::new(),
            send_as: vec![],
            history_id: None,
            recent_page_token: None,
            backfill_token: None,
            backfill_done: false,
            initial_done: false,
        };
        upsert_account(&conn, &account).unwrap();
        upsert_message(&conn, &message("a", "t1", &["INBOX", "UNREAD"], "Lease", "rent is due")).unwrap();
        upsert_message(
            &conn,
            &message("b", "t2", &["INBOX", "CATEGORY_PROMOTIONS"], "Sale", "watermelon festival"),
        )
        .unwrap();
        upsert_message(&conn, &message("c", "t3", &["TRASH"], "Old", "discard")).unwrap();
        upsert_message(
            &conn,
            &message("d", "t4", &["INBOX", "CATEGORY_UPDATES"], "Receipt", "parcel delivered"),
        )
        .unwrap();
        let inbox = query_mailbox(
            &conn,
            &ThreadQuery {
                account_id: None,
                folder: "inbox".into(),
                label_id: None,
                category: Some("primary".into()),
                search: None,
                limit: 50,
                offset: 0,
                inbox_tabs: InboxTabs::default(),
            },
        )
        .unwrap();
        assert_eq!(inbox.threads.len(), 2);
        assert!(inbox.threads.iter().any(|thread| thread.subject == "Lease"));
        assert!(inbox.threads.iter().any(|thread| thread.subject == "Receipt"));
        assert_eq!(inbox.counts.primary_unread, 1);
        assert_eq!(inbox.counts.promotions_unread, 0);
        let promos = query_mailbox(
            &conn,
            &ThreadQuery {
                account_id: None,
                folder: "inbox".into(),
                label_id: None,
                category: Some("promotions".into()),
                search: None,
                limit: 50,
                offset: 0,
                inbox_tabs: InboxTabs::default(),
            },
        )
        .unwrap();
        assert_eq!(promos.threads.len(), 1);
        let split = query_mailbox(
            &conn,
            &ThreadQuery {
                account_id: None,
                folder: "inbox".into(),
                label_id: None,
                category: Some("primary".into()),
                search: None,
                limit: 50,
                offset: 0,
                inbox_tabs: InboxTabs {
                    updates: true,
                    ..InboxTabs::default()
                },
            },
        )
        .unwrap();
        assert!(split.threads.iter().all(|thread| thread.subject != "Receipt"));
        let updates = query_mailbox(
            &conn,
            &ThreadQuery {
                account_id: None,
                folder: "inbox".into(),
                label_id: None,
                category: Some("updates".into()),
                search: None,
                limit: 50,
                offset: 0,
                inbox_tabs: InboxTabs {
                    updates: true,
                    ..InboxTabs::default()
                },
            },
        )
        .unwrap();
        assert_eq!(updates.threads.len(), 1);
        assert_eq!(updates.threads[0].subject, "Receipt");
        snooze_set(&conn, "acct", "t1", 9_000_000_000_000).unwrap();
        let hidden = query_mailbox(
            &conn,
            &ThreadQuery {
                account_id: None,
                folder: "inbox".into(),
                label_id: None,
                category: Some("primary".into()),
                search: None,
                limit: 50,
                offset: 0,
                inbox_tabs: InboxTabs::default(),
            },
        )
        .unwrap();
        assert!(hidden.threads.iter().all(|thread| thread.thread_id != "t1"));
        assert_eq!(hidden.counts.snoozed, 1);
        let waiting = query_mailbox(
            &conn,
            &ThreadQuery {
                account_id: None,
                folder: "snoozed".into(),
                label_id: None,
                category: None,
                search: None,
                limit: 50,
                offset: 0,
                inbox_tabs: InboxTabs::default(),
            },
        )
        .unwrap();
        assert_eq!(waiting.threads.len(), 1);
        assert_eq!(waiting.threads[0].subject, "Lease");
        let request = ComposeRequest {
            account_id: "acct".into(),
            from_email: "maya@northmail.test".into(),
            to: "rae@lindenhomes.test".into(),
            cc: String::new(),
            bcc: String::new(),
            subject: "Hold until morning".into(),
            html: "<p>Sending later</p>".into(),
            attachments: vec![],
            thread_id: None,
            in_reply_to: None,
            references: None,
            draft_id: None,
        };
        schedule_insert(&conn, "sched-1", "acct", 9_000_000_000_000, &request).unwrap();
        let later = query_mailbox(
            &conn,
            &ThreadQuery {
                account_id: None,
                folder: "scheduled".into(),
                label_id: None,
                category: None,
                search: None,
                limit: 50,
                offset: 0,
                inbox_tabs: InboxTabs::default(),
            },
        )
        .unwrap();
        assert_eq!(later.counts.scheduled, 1);
        assert_eq!(later.threads[0].subject, "Hold until morning");
        template_save(
            &conn,
            &Template {
                id: "tpl-1".into(),
                name: "Thanks".into(),
                subject: "Thank you".into(),
                html: "<p>Thanks</p>".into(),
                updated_at: 10,
            },
        )
        .unwrap();
        assert_eq!(templates_list(&conn).unwrap()[0].name, "Thanks");
        let found = query_mailbox(
            &conn,
            &ThreadQuery {
                account_id: None,
                folder: "inbox".into(),
                label_id: None,
                category: None,
                search: Some("watermelon".into()),
                limit: 50,
                offset: 0,
                inbox_tabs: InboxTabs::default(),
            },
        )
        .unwrap();
        assert_eq!(found.threads.len(), 1);
        assert_eq!(found.threads[0].subject, "Sale");
        apply_labels(&conn, "acct", "t1", &[], &["UNREAD".into()]).unwrap();
        assert_eq!(total_unread(&conn).unwrap(), 0);
    }
}
