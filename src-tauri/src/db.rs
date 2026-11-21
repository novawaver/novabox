use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::time::Duration;

use rusqlite::{params, Connection};
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::model::{
    Account, AttachmentMeta, ComposeRequest, FolderCounts, InboxTabs, Label, LocalLabel, Mailbox, MessageDetail, SendAs,
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
    created_at INTEGER NOT NULL,
    provider TEXT NOT NULL DEFAULT 'gmail'
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
CREATE INDEX IF NOT EXISTS idx_messages_header ON messages(account_id, message_id_header);
CREATE INDEX IF NOT EXISTS idx_messages_sort ON messages(account_id, thread_id, id, date, has_attachments, label_ids);
CREATE INDEX IF NOT EXISTS idx_messages_unread ON messages(account_id, thread_id, label_ids) WHERE instr(label_ids, '"UNREAD"') > 0 AND instr(label_ids, '"INBOX"') > 0;
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
    // With the write-ahead log, NORMAL cannot corrupt the file; a power cut can only lose the
    // last few writes, and mail downloads again. FULL waits on the disk for every message.
    conn.pragma_update(None, "synchronous", "NORMAL").ok();
    // 64 MB of pages kept in memory, enough to hold the list index of a large mailbox.
    conn.pragma_update(None, "cache_size", -65_536).ok();
    conn.pragma_update(None, "temp_store", "MEMORY").ok();
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
    } else {
        if version < 2 {
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
        }
        if version < 3 {
            // Accounts from before other mail services were supported are all Gmail.
            conn.execute_batch("ALTER TABLE accounts ADD COLUMN provider TEXT NOT NULL DEFAULT 'gmail';")
                .map_err(|e| format!("Could not prepare the local mailbox for other mail services: {e}"))?;
        }
    }
    // A new mailbox has every index from the start. An older one stops at version 3 here and
    // gets its list indexes from `build_list_indexes`, which can take a while.
    let reached = if version < 1 { LIST_VERSION } else { 3 };
    if version < reached {
        conn.pragma_update(None, "user_version", reached).map_err(|e| e.to_string())?;
    }
    // Small tables that any version may be missing; making them costs nothing.
    conn.execute_batch(LOCAL_LABEL_TABLES)
        .map_err(|e| format!("Could not prepare Novabox labels: {e}"))?;
    Ok(())
}

const LOCAL_LABEL_TABLES: &str = "
CREATE TABLE IF NOT EXISTS local_labels (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    color TEXT NOT NULL DEFAULT '',
    created_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS local_label_threads (
    label_id TEXT NOT NULL,
    account_id TEXT NOT NULL,
    thread_id TEXT NOT NULL,
    PRIMARY KEY (label_id, account_id, thread_id)
);";

pub fn local_labels_list(conn: &Connection) -> Result<Vec<LocalLabel>, String> {
    let mut stmt = conn
        .prepare_cached("SELECT id, name, color FROM local_labels ORDER BY name COLLATE NOCASE, id")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok(LocalLabel { id: row.get(0)?, name: row.get(1)?, color: row.get(2)?, total: 0, unread: 0 })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

pub fn local_label_save(conn: &Connection, label: &LocalLabel) -> Result<(), String> {
    conn.execute(
        "INSERT INTO local_labels (id, name, color, created_at) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(id) DO UPDATE SET name = excluded.name, color = excluded.color",
        params![label.id, label.name, label.color, crate::util::now_secs()],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn local_label_delete(conn: &Connection, id: &str) -> Result<(), String> {
    conn.execute("DELETE FROM local_label_threads WHERE label_id = ?1", [id]).map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM local_labels WHERE id = ?1", [id]).map_err(|e| e.to_string())?;
    Ok(())
}

/// Puts a Novabox label on a conversation, or takes it off.
pub fn local_label_assign(conn: &Connection, label_id: &str, account_id: &str, thread_id: &str, on: bool) -> Result<(), String> {
    let sql = if on {
        "INSERT OR IGNORE INTO local_label_threads (label_id, account_id, thread_id) VALUES (?1, ?2, ?3)"
    } else {
        "DELETE FROM local_label_threads WHERE label_id = ?1 AND account_id = ?2 AND thread_id = ?3"
    };
    conn.execute(sql, params![label_id, account_id, thread_id]).map_err(|e| e.to_string())?;
    Ok(())
}

/// Every (label, account, conversation) a Novabox label is on.
fn local_label_threads(conn: &Connection) -> Result<Vec<(String, String, String)>, String> {
    let mut stmt = conn
        .prepare_cached("SELECT label_id, account_id, thread_id FROM local_label_threads")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

/// The messages of a conversation that are in Trash.
pub fn trashed_in_thread(conn: &Connection, account_id: &str, thread_id: &str) -> Result<Vec<String>, String> {
    Ok(thread_labels(conn, account_id, thread_id)?
        .into_iter()
        .filter(|(_, labels)| has_label(labels, "TRASH"))
        .map(|(id, _)| id)
        .collect())
}

/// Removes messages from this PC in one go. Their cached files are the caller's to remove.
pub fn delete_messages(conn: &Connection, account_id: &str, ids: &[String]) -> Result<(), String> {
    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    for id in ids {
        delete_message(&tx, account_id, id)?;
    }
    tx.commit().map_err(|e| e.to_string())
}

const LIST_VERSION: i64 = 5;

/// Whether this mailbox is from before the list indexes and still has to get them.
pub fn needs_list_indexes(conn: &Connection) -> bool {
    conn.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
        .map_or(false, |version| version < LIST_VERSION)
}

/// Gives an older mailbox the indexes the list is read from. A message's row holds its whole
/// body, so reading the list out of the table walks the entire file: seconds for a large
/// mailbox, on every refresh. The two indexes hold only what the list and the unread counts
/// need. Building them reads the table twice, which for a large mailbox takes the better part
/// of a minute, once; the app runs this after its window is up rather than before.
/// Version 4 had a wider list index, which the one here replaces.
pub fn build_list_indexes(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(LIST_INDEXES)
        .and_then(|_| conn.execute_batch("DROP INDEX IF EXISTS idx_messages_list;"))
        .and_then(|_| conn.pragma_update(None, "user_version", LIST_VERSION))
        .map_err(|e| format!("Could not speed up the local mailbox: {e}"))
}

const LIST_INDEXES: &str = r#"
CREATE INDEX IF NOT EXISTS idx_messages_sort ON messages(account_id, thread_id, id, date, has_attachments, label_ids);
CREATE INDEX IF NOT EXISTS idx_messages_unread ON messages(account_id, thread_id, label_ids) WHERE instr(label_ids, '"UNREAD"') > 0 AND instr(label_ids, '"INBOX"') > 0;
"#;

/// The index that finds a message by its Message-ID header, which ties IMAP mail into
/// conversations. Only IMAP accounts need it, and building it reads every stored message,
/// so a mailbox from before those accounts existed gets it when the first one syncs.
pub fn ensure_header_index(conn: &Connection) -> Result<(), String> {
    conn.execute_batch("CREATE INDEX IF NOT EXISTS idx_messages_header ON messages(account_id, message_id_header);")
        .map_err(|e| e.to_string())
}

/// Which mail service an account is with, without counting its unread mail.
pub fn account_provider(conn: &Connection, id: &str) -> Result<String, String> {
    conn.query_row("SELECT provider FROM accounts WHERE id = ?1", [id], |row| row.get(0))
        .map_err(|_| "That account is not on this PC.".to_string())
}

pub fn upsert_account(conn: &Connection, account: &Account) -> Result<(), String> {
    let send_as = to_json(&account.send_as);
    conn.execute(
        "INSERT INTO accounts (id, email, name, picture, color, signature, send_as_json, history_id, recent_page_token, backfill_token, backfill_done, initial_done, created_at, provider)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
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
            account.provider,
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn list_accounts(conn: &Connection) -> Result<Vec<Account>, String> {
    let mut stmt = conn
        .prepare("SELECT id, email, name, picture, color, signature, send_as_json, history_id, recent_page_token, backfill_token, backfill_done, initial_done, provider FROM accounts ORDER BY created_at")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| Ok(read_account(row)?))
        .map_err(|e| e.to_string())?;
    let mut accounts = Vec::new();
    for row in rows {
        accounts.push(row.map_err(|e| e.to_string())?);
    }
    drop(stmt);
    let unread = unread_counts(conn)?;
    for account in &mut accounts {
        account.unread = unread.get(&account.id).copied().unwrap_or(0);
    }
    Ok(accounts)
}

pub fn get_account(conn: &Connection, id: &str) -> Result<Account, String> {
    let mut account = conn
        .query_row(
            "SELECT id, email, name, picture, color, signature, send_as_json, history_id, recent_page_token, backfill_token, backfill_done, initial_done, provider FROM accounts WHERE id = ?1",
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

/// Saves only the history position, leaving the download progress as it is.
pub fn set_history_id(conn: &Connection, account_id: &str, history_id: &str) -> Result<(), String> {
    conn.execute(
        "UPDATE accounts SET history_id = ?1 WHERE id = ?2",
        params![history_id, account_id],
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
    tx.execute("DELETE FROM local_label_threads WHERE account_id = ?1", [account_id]).map_err(|e| e.to_string())?;
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

/// The stored messages whose id starts with `prefix`, each with its labels. An IMAP message's
/// id starts with its folder's key, so this lists what is stored of one folder.
pub fn messages_with_prefix(conn: &Connection, account_id: &str, prefix: &str) -> Result<Vec<(String, Vec<String>)>, String> {
    // A range over the primary key, so one folder is read without walking the whole mailbox.
    let mut stmt = conn
        .prepare("SELECT id, label_ids FROM messages WHERE account_id = ?1 AND id >= ?2 AND id < ?3")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![account_id, prefix, format!("{prefix}\u{10FFFF}")], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(rows.into_iter().map(|(id, json)| (id, json_vec(&json))).collect())
}

/// The id and conversation of a stored message with this Message-ID header.
pub fn message_by_header(conn: &Connection, account_id: &str, header: &str) -> Result<Option<(String, String)>, String> {
    if header.is_empty() {
        return Ok(None);
    }
    match conn.query_row(
        "SELECT id, thread_id FROM messages WHERE account_id = ?1 AND message_id_header = ?2 ORDER BY date DESC LIMIT 1",
        params![account_id, header],
        |row| Ok((row.get(0)?, row.get(1)?)),
    ) {
        Ok(found) => Ok(Some(found)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(err) => Err(err.to_string()),
    }
}

/// Each message of a conversation: its id and who it is from.
pub fn thread_senders(conn: &Connection, account_id: &str, thread_id: &str) -> Result<Vec<(String, String)>, String> {
    let mut stmt = conn
        .prepare("SELECT id, from_email FROM messages WHERE account_id = ?1 AND thread_id = ?2")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![account_id, thread_id], |row| Ok((row.get(0)?, row.get(1)?)))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(rows)
}

/// A stored message's labels, or `None` when the message is not on this PC.
pub fn message_labels(conn: &Connection, account_id: &str, message_id: &str) -> Result<Option<Vec<String>>, String> {
    match conn.query_row(
        "SELECT label_ids FROM messages WHERE account_id = ?1 AND id = ?2",
        params![account_id, message_id],
        |row| row.get::<_, String>(0),
    ) {
        Ok(json) => Ok(Some(json_vec(&json))),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(err) => Err(err.to_string()),
    }
}

pub fn set_labels(conn: &Connection, account_id: &str, message_id: &str, labels: &[String]) -> Result<(), String> {
    conn.execute(
        "UPDATE messages SET label_ids = ?1 WHERE account_id = ?2 AND id = ?3",
        params![to_json(&labels), account_id, message_id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Ids of the stored messages that carry a label.
pub fn ids_with_label(conn: &Connection, account_id: &str, label_id: &str) -> Result<HashSet<String>, String> {
    let mut stmt = conn
        .prepare("SELECT id FROM messages WHERE account_id = ?1 AND instr(label_ids, ?2) > 0")
        .map_err(|e| e.to_string())?;
    let quoted = format!("\"{label_id}\"");
    let rows = stmt
        .query_map(params![account_id, quoted], |row| row.get::<_, String>(0))
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<HashSet<_>, _>>().map_err(|e| e.to_string())
}

/// One entry of Gmail's history: labels added to a message, or removed from it.
pub struct LabelChange {
    pub message_id: String,
    pub add: bool,
    pub labels: Vec<String>,
}

/// Applies Gmail's label changes to the stored messages, in the order given. Returns how
/// many messages ended up with different labels, and the ids that are not on this PC.
pub fn apply_label_changes(
    conn: &Connection,
    account_id: &str,
    changes: &[LabelChange],
) -> Result<(usize, Vec<String>), String> {
    // Per message: its labels as stored and as changed, or `None` when it is not stored.
    let mut states: HashMap<String, Option<(Vec<String>, Vec<String>)>> = HashMap::new();
    let mut missing = Vec::new();
    for change in changes {
        if !states.contains_key(&change.message_id) {
            let stored = message_labels(conn, account_id, &change.message_id)?;
            if stored.is_none() {
                missing.push(change.message_id.clone());
            }
            states.insert(change.message_id.clone(), stored.map(|labels| (labels.clone(), labels)));
        }
        let Some(Some((_, labels))) = states.get_mut(&change.message_id) else {
            continue;
        };
        if change.add {
            for label in &change.labels {
                if !labels.contains(label) {
                    labels.push(label.clone());
                }
            }
        } else {
            labels.retain(|label| !change.labels.contains(label));
        }
    }
    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    let mut updated = 0;
    for (message_id, state) in &states {
        let Some((before, after)) = state else {
            continue;
        };
        if before == after {
            continue;
        }
        tx.execute(
            "UPDATE messages SET label_ids = ?1 WHERE account_id = ?2 AND id = ?3",
            params![to_json(after), account_id, message_id],
        )
        .map_err(|e| e.to_string())?;
        updated += 1;
    }
    tx.commit().map_err(|e| e.to_string())?;
    Ok((updated, missing))
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

/// Each message of a conversation with its labels, so a change can be rolled back.
pub fn thread_labels(conn: &Connection, account_id: &str, thread_id: &str) -> Result<Vec<(String, Vec<String>)>, String> {
    let mut stmt = conn
        .prepare("SELECT id, label_ids FROM messages WHERE account_id = ?1 AND thread_id = ?2")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![account_id, thread_id], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(rows.into_iter().map(|(id, json)| (id, json_vec(&json))).collect())
}

pub fn restore_labels(conn: &Connection, account_id: &str, saved: &[(String, Vec<String>)]) -> Result<(), String> {
    for (id, labels) in saved {
        conn.execute(
            "UPDATE messages SET label_ids = ?1 WHERE account_id = ?2 AND id = ?3",
            params![to_json(labels), account_id, id],
        )
        .map_err(|e| e.to_string())?;
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
    let in_group = |id: &str| {
        query
            .account_ids
            .as_ref()
            .map_or(true, |ids| ids.iter().any(|item| item == id))
    };
    let scope = if account_filter.is_empty() { Scope::Everything } else { Scope::Account(&account_filter) };
    let wanted = if query.folder == "label" { query.label_id.as_deref() } else { None };
    let mut owners = owners(&accounts);
    let loaded = load_conversations(conn, &mut owners, &in_group, scope, wanted)?;
    let conversations = &loaded.conversations;
    let tabs = &query.inbox_tabs;
    // Where each snoozed conversation is in the list, and when it comes back.
    let snoozes = load_snoozes(conn, &account_filter)?;
    let mut snoozed: HashMap<usize, i64> = HashMap::new();
    if !snoozes.is_empty() {
        let waiting: HashMap<(usize, &str), i64> = snoozes
            .iter()
            .filter_map(|((account_id, thread_id), until)| {
                let owner = owners.iter().position(|owner| &owner.id == account_id)?;
                Some(((owner, thread_id.as_str()), *until))
            })
            .collect();
        for (place, conversation) in conversations.iter().enumerate() {
            if let Some(until) = waiting.get(&(conversation.account, conversation.thread_id.as_str())) {
                snoozed.insert(place, *until);
            }
        }
    }
    let mut scheduled = load_scheduled(conn, &account_filter)?;
    scheduled.retain(|item| in_group(&item.account_id));
    let mut counts = count_folders(&loaded, tabs);
    let inbox = View::new("inbox", None, tabs);
    for place in snoozed.keys() {
        let conversation = loaded.messages_of(&conversations[*place]);
        if !inbox.has(conversation) {
            continue;
        }
        counts.inbox -= 1;
        if inbox.has_unread(conversation) {
            counts.inbox_unread -= 1;
        }
        if View::new("inbox", Some("primary"), tabs).has_unread(conversation) {
            counts.primary_unread -= 1;
        }
        if View::new("inbox", Some("promotions"), tabs).has_unread(conversation) {
            counts.promotions_unread -= 1;
        }
        if View::new("inbox", Some("social"), tabs).has_unread(conversation) {
            counts.social_unread -= 1;
        }
        if View::new("inbox", Some("updates"), tabs).has_unread(conversation) {
            counts.updates_unread -= 1;
        }
    }
    counts.snoozed = snoozed.len() as i64;
    counts.scheduled = scheduled.len() as i64;
    let mut labels = load_labels(conn, &account_filter)?;
    labels.retain(|label| in_group(&label.account_id));
    // One pass over the conversations, counting each once per label it has unread mail under.
    let mut label_unread: HashMap<(usize, &str), i64> = HashMap::new();
    for conversation in conversations {
        for label in &conversation.unread_labels {
            *label_unread.entry((conversation.account, label.as_str())).or_default() += 1;
        }
    }
    for label in &mut labels {
        if label.kind != "user" {
            continue;
        }
        label.unread = owners
            .iter()
            .position(|owner| owner.id == label.account_id)
            .and_then(|account| label_unread.get(&(account, label.id.as_str())))
            .copied()
            .unwrap_or(0);
    }
    let mut local_labels = local_labels_list(conn)?;
    let tagged = tagged_places(conn, &local_labels, &owners, &loaded)?;
    for (place, on) in &tagged {
        let live = |message: &&Lean| message.flags & (TRASH | SPAM) == 0;
        let messages = loaded.messages_of(&conversations[*place]);
        if !messages.iter().any(|message| live(&message)) {
            continue;
        }
        let unread = messages.iter().filter(live).any(|message| message.flags & UNREAD != 0);
        for label in on {
            local_labels[*label].total += 1;
            local_labels[*label].unread += unread as i64;
        }
    }
    let local_ids = |place: usize| -> Vec<String> {
        tagged
            .get(&place)
            .map(|on| on.iter().map(|label| local_labels[*label].id.clone()).collect())
            .unwrap_or_default()
    };
    let search = query.search.as_deref().unwrap_or("").trim().to_string();
    let offset = query.offset.max(0) as usize;
    let limit = query.limit.clamp(1, 200) as usize;
    if query.folder == "scheduled" && search.is_empty() {
        let threads = scheduled_threads(&scheduled, &account_pairs(&accounts));
        let total = threads.len();
        let threads = threads.into_iter().skip(offset).take(limit).collect();
        return Ok(Mailbox { accounts, labels, local_labels, threads, has_more: total > offset + limit, total, counts });
    }
    let lines = if query.folder == "local" && search.is_empty() {
        // A Novabox label: its conversations, wherever their mail is, short of Trash and Spam.
        let wanted = query
            .label_id
            .as_deref()
            .and_then(|id| local_labels.iter().position(|label| label.id == id));
        summarize(
            &loaded,
            |place| wanted.is_some_and(|wanted| tagged.get(&place).is_some_and(|on| on.contains(&wanted))),
            |message| message.flags & (TRASH | SPAM) == 0,
        )
    } else if query.folder == "snoozed" && search.is_empty() {
        let mut lines = summarize(&loaded, |place| snoozed.contains_key(&place), |_| true);
        lines.retain(|line| line.latest.flags & (TRASH | SPAM) == 0);
        lines
    } else if search.is_empty() {
        let view = View::new(&query.folder, query.category.as_deref(), tabs);
        let hide_snoozed = query.folder == "inbox";
        summarize(
            &loaded,
            |place| !(hide_snoozed && snoozed.contains_key(&place)),
            |message| view.holds(message.flags),
        )
    } else {
        let matched = matched_rows(conn, &search, &account_filter, query.account_ids.as_deref())?;
        summarize(&loaded, |_| true, |message| matched.contains(&message.rowid))
    };
    let total = lines.len();
    // Only the page on screen is read in full; the rest of the mailbox never leaves the index.
    let page: Vec<&Line> = lines.iter().skip(offset).take(limit).collect();
    let mut threads = hydrate(conn, &owners, conversations, &page, &local_ids)?;
    if query.folder == "snoozed" && search.is_empty() {
        for (thread, line) in threads.iter_mut().zip(&page) {
            thread.snooze_until = snoozed.get(&line.conversation).copied();
        }
    }
    Ok(Mailbox {
        accounts,
        labels,
        local_labels,
        threads,
        has_more: total > offset + limit,
        total,
        counts,
    })
}

/// Which Novabox labels are on which of the loaded conversations: positions in
/// `conversations` to positions in `labels`.
fn tagged_places(conn: &Connection, labels: &[LocalLabel], owners: &[Owner], loaded: &Loaded) -> Result<HashMap<usize, Vec<usize>>, String> {
    let mut tagged: HashMap<usize, Vec<usize>> = HashMap::new();
    let rows = local_label_threads(conn)?;
    if rows.is_empty() {
        return Ok(tagged);
    }
    let mut by_thread: HashMap<(usize, &str), Vec<usize>> = HashMap::new();
    for (label_id, account_id, thread_id) in &rows {
        let label = labels.iter().position(|label| &label.id == label_id);
        let owner = owners.iter().position(|owner| &owner.id == account_id);
        if let (Some(label), Some(owner)) = (label, owner) {
            by_thread.entry((owner, thread_id.as_str())).or_default().push(label);
        }
    }
    for (place, conversation) in loaded.conversations.iter().enumerate() {
        if let Some(on) = by_thread.get(&(conversation.account, conversation.thread_id.as_str())) {
            tagged.insert(place, on.clone());
        }
    }
    Ok(tagged)
}

pub fn threads_for_messages(conn: &Connection, keys: &[(String, String)]) -> Result<Vec<ThreadSummary>, String> {
    let mut owners = owners(&list_accounts(conn)?);
    let mut seen = HashSet::new();
    let mut found = Loaded::default();
    for (account_id, message_id) in keys {
        if let Some(thread_id) = lookup_thread(conn, account_id, message_id)? {
            if seen.insert((account_id.clone(), thread_id.clone())) {
                let loaded = load_conversations(conn, &mut owners, &|_| true, Scope::Thread(account_id, &thread_id), None)?;
                found.append(loaded);
            }
        }
    }
    let lines = summarize(&found, |_| true, |_| true);
    let page: Vec<&Line> = lines.iter().take(200).collect();
    let local_labels = local_labels_list(conn)?;
    let tagged = tagged_places(conn, &local_labels, &owners, &found)?;
    hydrate(conn, &owners, &found.conversations, &page, &|place| {
        tagged
            .get(&place)
            .map(|on| on.iter().map(|label| local_labels[*label].id.clone()).collect())
            .unwrap_or_default()
    })
}

// The unread queries below repeat the exact condition of idx_messages_unread, which is what
// lets SQLite answer them from that small index. Conversation ids are per account, so the
// total is the sum over accounts.
pub fn total_unread(conn: &Connection) -> Result<i64, String> {
    Ok(unread_counts(conn)?.values().sum())
}

fn unread_counts(conn: &Connection) -> Result<HashMap<String, i64>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT account_id, COUNT(DISTINCT thread_id) FROM messages WHERE instr(label_ids, '\"UNREAD\"') > 0 AND instr(label_ids, '\"INBOX\"') > 0 AND instr(label_ids, '\"TRASH\"') = 0 AND instr(label_ids, '\"SPAM\"') = 0 GROUP BY account_id",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)))
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<HashMap<_, _>, _>>().map_err(|e| e.to_string())
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
        "SELECT COUNT(DISTINCT thread_id) FROM messages WHERE instr(label_ids, '\"UNREAD\"') > 0 AND instr(label_ids, '\"INBOX\"') > 0 AND instr(label_ids, '\"TRASH\"') = 0 AND instr(label_ids, '\"SPAM\"') = 0 AND account_id = ?1",
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
        provider: row.get(12)?,
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

// ---------------------------------------------------------------------------------------
// The conversation list
//
// Every refresh of the list sorts the whole mailbox into folders and counts it, so this is
// kept cheap per message: a row id, a date and a few bits, read from idx_messages_sort.
// Names, subjects and previews are read afterwards, for the one page that is shown.
// ---------------------------------------------------------------------------------------

const INBOX: u16 = 1 << 0;
const UNREAD: u16 = 1 << 1;
const STARRED: u16 = 1 << 2;
const SENT: u16 = 1 << 3;
const DRAFT: u16 = 1 << 4;
const TRASH: u16 = 1 << 5;
const SPAM: u16 = 1 << 6;
const PROMOTIONS: u16 = 1 << 7;
const SOCIAL: u16 = 1 << 8;
const UPDATES: u16 = 1 << 9;
const ATTACHMENT: u16 = 1 << 10;
/// Carries the label whose folder is being shown.
const WANTED: u16 = 1 << 11;

#[derive(Clone, Copy)]
struct Lean {
    rowid: i64,
    date: i64,
    flags: u16,
}

struct Conversation {
    /// Position of its owner in the owner list.
    account: usize,
    thread_id: String,
    /// Where its messages start in the loaded list, and how many there are.
    first: usize,
    count: usize,
    /// Labels other than the built-in ones that it has unread mail under.
    unread_labels: Vec<String>,
}

/// Whose mail a conversation is: an account, or the id left on mail whose account is gone.
struct Owner {
    id: String,
    email: String,
    color: String,
}

fn owners(accounts: &[Account]) -> Vec<Owner> {
    accounts
        .iter()
        .map(|account| Owner {
            id: account.id.clone(),
            email: account.email.clone(),
            color: account.color.clone(),
        })
        .collect()
}

/// Conversations and their messages, each conversation's messages side by side in one
/// list. One list rather than one per conversation: most conversations are a single message,
/// and a large mailbox has tens of thousands.
#[derive(Default)]
struct Loaded {
    conversations: Vec<Conversation>,
    messages: Vec<Lean>,
}

impl Loaded {
    fn messages_of(&self, conversation: &Conversation) -> &[Lean] {
        &self.messages[conversation.first..conversation.first + conversation.count]
    }

    fn append(&mut self, other: Loaded) {
        let shift = self.messages.len();
        self.messages.extend(other.messages);
        self.conversations.extend(other.conversations.into_iter().map(|mut conversation| {
            conversation.first += shift;
            conversation
        }));
    }
}

enum Scope<'a> {
    Everything,
    Account(&'a str),
    Thread(&'a str, &'a str),
}

/// The ids in a stored label list. It is a JSON list of plain ids, so every second piece
/// between quotes is one.
fn label_ids_in(text: &str) -> impl Iterator<Item = &str> {
    text.split('"').skip(1).step_by(2)
}

fn label_flag(label: &str) -> u16 {
    match label {
        "INBOX" => INBOX,
        "UNREAD" => UNREAD,
        "STARRED" => STARRED,
        "SENT" => SENT,
        "DRAFT" => DRAFT,
        "TRASH" => TRASH,
        "SPAM" => SPAM,
        "CATEGORY_PROMOTIONS" => PROMOTIONS,
        "CATEGORY_SOCIAL" | "CATEGORY_FORUMS" => SOCIAL,
        "CATEGORY_UPDATES" => UPDATES,
        _ => 0,
    }
}

fn load_conversations(
    conn: &Connection,
    accounts: &mut Vec<Owner>,
    allowed: &dyn Fn(&str) -> bool,
    scope: Scope,
    wanted: Option<&str>,
) -> Result<Loaded, String> {
    const COLUMNS: &str = "SELECT rowid, account_id, thread_id, date, has_attachments, label_ids FROM messages";
    // Separate statements, since one "all or this account" condition would stop SQLite from
    // going straight to the account in the index. The order is the index's own, so it costs
    // nothing, and it brings a conversation's messages out one after another.
    const ORDER: &str = "ORDER BY account_id, thread_id";
    let (sql, args): (String, Vec<&dyn rusqlite::ToSql>) = match &scope {
        Scope::Everything => (format!("{COLUMNS} {ORDER}"), vec![]),
        Scope::Account(account_id) => (format!("{COLUMNS} WHERE account_id = ?1 {ORDER}"), vec![account_id]),
        Scope::Thread(account_id, thread_id) => (
            format!("{COLUMNS} WHERE account_id = ?1 AND thread_id = ?2 {ORDER}"),
            vec![account_id, thread_id],
        ),
    };
    let mut open: Vec<bool> = accounts.iter().map(|account| allowed(&account.id)).collect();
    let mut loaded = Loaded::default();
    let failed = |e: rusqlite::Error| e.to_string();
    let unreadable = |e: rusqlite::types::FromSqlError| e.to_string();
    let mut stmt = conn.prepare_cached(&sql).map_err(failed)?;
    let mut rows = stmt.query(args.as_slice()).map_err(failed)?;
    // An account's rows come one after another, so the last account found is usually right.
    let mut account = 0;
    while let Some(row) = rows.next().map_err(failed)? {
        let account_id = row.get_ref(1).map_err(failed)?.as_str().map_err(unreadable)?;
        if accounts.get(account).map_or(true, |known| known.id != account_id) {
            account = match accounts.iter().position(|known| known.id == account_id) {
                Some(found) => found,
                None => {
                    accounts.push(Owner { id: account_id.to_string(), email: String::new(), color: "#888888".into() });
                    open.push(allowed(account_id));
                    accounts.len() - 1
                }
            };
        }
        if !open[account] {
            continue;
        }
        let thread_id = row.get_ref(2).map_err(failed)?.as_str().map_err(unreadable)?;
        let labels = row.get_ref(5).map_err(failed)?.as_str().map_err(unreadable)?;
        let mut flags = 0;
        for label in label_ids_in(labels) {
            flags |= label_flag(label);
            if wanted == Some(label) {
                flags |= WANTED;
            }
        }
        if row.get::<_, i64>(4).map_err(failed)? != 0 {
            flags |= ATTACHMENT;
        }
        let continues = loaded
            .conversations
            .last()
            .is_some_and(|last| last.account == account && last.thread_id == thread_id);
        if !continues {
            loaded.conversations.push(Conversation {
                account,
                thread_id: thread_id.to_string(),
                first: loaded.messages.len(),
                count: 0,
                unread_labels: Vec::new(),
            });
        }
        loaded.messages.push(Lean {
            rowid: row.get(0).map_err(failed)?,
            date: row.get(3).map_err(failed)?,
            flags,
        });
        let conversation = loaded.conversations.last_mut().expect("just pushed");
        conversation.count += 1;
        if flags & UNREAD != 0 && flags & TRASH == 0 {
            for label in label_ids_in(labels) {
                let built_in = label_flag(label) != 0 || label.starts_with("CATEGORY_");
                if !built_in && !conversation.unread_labels.iter().any(|known| known == label) {
                    conversation.unread_labels.push(label.to_string());
                }
            }
        }
    }
    Ok(loaded)
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    All,
    Inbox,
    Starred,
    Sent,
    Drafts,
    Spam,
    Trash,
    Label,
    Nothing,
}

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    All,
    Primary,
    Promotions,
    Social,
    Updates,
}

/// A folder, as a test of one message's flags.
#[derive(Clone, Copy)]
struct View {
    kind: Kind,
    tab: Tab,
    promotions: bool,
    social: bool,
    updates: bool,
}

impl View {
    /// With no category, the inbox means its Primary tab.
    fn new(folder: &str, category: Option<&str>, tabs: &InboxTabs) -> View {
        View {
            kind: match folder {
                "all" => Kind::All,
                "inbox" => Kind::Inbox,
                "starred" => Kind::Starred,
                "sent" => Kind::Sent,
                "drafts" => Kind::Drafts,
                "spam" => Kind::Spam,
                "trash" => Kind::Trash,
                "label" => Kind::Label,
                _ => Kind::Nothing,
            },
            tab: match category.unwrap_or("primary") {
                "all" => Tab::All,
                "promotions" => Tab::Promotions,
                "social" => Tab::Social,
                "updates" => Tab::Updates,
                _ => Tab::Primary,
            },
            promotions: tabs.promotions,
            social: tabs.social,
            updates: tabs.updates,
        }
    }

    fn holds(&self, flags: u16) -> bool {
        let has = |flag: u16| flags & flag != 0;
        let trash = has(TRASH);
        let spam = has(SPAM);
        match self.kind {
            Kind::All => true,
            Kind::Inbox => {
                if !has(INBOX) || trash || spam {
                    return false;
                }
                // Gmail keeps a category in Primary until that inbox tab is turned on.
                // Updates is off by default, so CATEGORY_UPDATES mail stays in Primary.
                let promotions = self.promotions && has(PROMOTIONS);
                let social = self.social && has(SOCIAL);
                let updates = self.updates && has(UPDATES);
                match self.tab {
                    Tab::All => true,
                    Tab::Promotions => promotions,
                    Tab::Social => social,
                    Tab::Updates => updates,
                    Tab::Primary => !promotions && !social && !updates,
                }
            }
            Kind::Starred => has(STARRED) && !trash,
            Kind::Sent => has(SENT) && !trash,
            Kind::Drafts => has(DRAFT) && !trash,
            Kind::Spam => spam,
            Kind::Trash => trash,
            Kind::Label => has(WANTED) && !trash,
            Kind::Nothing => false,
        }
    }

    fn has(&self, messages: &[Lean]) -> bool {
        messages.iter().any(|message| self.holds(message.flags))
    }

    fn has_unread(&self, messages: &[Lean]) -> bool {
        messages
            .iter()
            .any(|message| message.flags & UNREAD != 0 && self.holds(message.flags))
    }
}

/// One row of the list, before its text is read.
struct Line {
    conversation: usize,
    latest: Lean,
    unread: bool,
    starred: bool,
    attachments: bool,
    count: i64,
}

/// The conversations that `keep` lets through, each summed up over the messages `shows`
/// picks, newest first. A conversation with no such message is left out.
fn summarize(loaded: &Loaded, keep: impl Fn(usize) -> bool, shows: impl Fn(&Lean) -> bool) -> Vec<Line> {
    let conversations = &loaded.conversations;
    let mut lines = Vec::new();
    for (place, conversation) in conversations.iter().enumerate() {
        if !keep(place) {
            continue;
        }
        let mut line: Option<Line> = None;
        for message in loaded.messages_of(conversation).iter().filter(|message| shows(message)) {
            let line = line.get_or_insert(Line {
                conversation: place,
                latest: *message,
                unread: false,
                starred: false,
                attachments: false,
                count: 0,
            });
            if (message.date, message.rowid) > (line.latest.date, line.latest.rowid) {
                line.latest = *message;
            }
            line.unread |= message.flags & UNREAD != 0;
            line.starred |= message.flags & STARRED != 0;
            line.attachments |= message.flags & ATTACHMENT != 0;
            line.count += 1;
        }
        lines.extend(line);
    }
    lines.sort_by(|a, b| {
        b.latest
            .date
            .cmp(&a.latest.date)
            .then_with(|| conversations[b.conversation].thread_id.cmp(&conversations[a.conversation].thread_id))
    });
    lines
}

/// Reads what a row shows (sender, subject, preview) for the lines of one page. In a
/// message's row the labels sit after the body, and reading past a long body is slow, so
/// the labels come from the index instead.
fn hydrate(
    conn: &Connection,
    accounts: &[Owner],
    conversations: &[Conversation],
    lines: &[&Line],
    local_ids: &dyn Fn(usize) -> Vec<String>,
) -> Result<Vec<ThreadSummary>, String> {
    let mut stmt = conn
        .prepare_cached("SELECT id, from_name, from_email, subject, snippet FROM messages WHERE rowid = ?1")
        .map_err(|e| e.to_string())?;
    const LABELS: &str = "WHERE account_id = ?1 AND thread_id = ?2 AND id = ?3";
    // Naming the index keeps SQLite from going through the table. Until an older mailbox
    // has the index, the plain form is the one that works.
    let mut labels = conn
        .prepare_cached(&format!("SELECT label_ids FROM messages INDEXED BY idx_messages_sort {LABELS}"))
        .or_else(|_| conn.prepare_cached(&format!("SELECT label_ids FROM messages {LABELS}")))
        .map_err(|e| e.to_string())?;
    let mut threads = Vec::with_capacity(lines.len());
    for line in lines {
        let conversation = &conversations[line.conversation];
        let account = &accounts[conversation.account];
        let mut thread = stmt
            .query_row([line.latest.rowid], |row| {
                Ok(ThreadSummary {
                    account_id: account.id.clone(),
                    account_email: account.email.clone(),
                    account_color: account.color.clone(),
                    thread_id: conversation.thread_id.clone(),
                    latest_message_id: row.get(0)?,
                    from_name: row.get(1)?,
                    from_email: row.get(2)?,
                    subject: row.get(3)?,
                    snippet: row.get(4)?,
                    date: line.latest.date,
                    unread: line.unread,
                    starred: line.starred,
                    has_attachments: line.attachments,
                    message_count: line.count,
                    label_ids: Vec::new(),
                    snooze_until: None,
                    scheduled_id: None,
                    body_html: None,
                    preview_to: None,
                    local_label_ids: local_ids(line.conversation),
                })
            })
            .map_err(|e| e.to_string())?;
        thread.label_ids = labels
            .query_row(params![account.id, conversation.thread_id, thread.latest_message_id], |row| row.get::<_, String>(0))
            .map(|json| json_vec(&json))
            .map_err(|e| e.to_string())?;
        threads.push(thread);
    }
    Ok(threads)
}

/// The rows of the messages a search finds: by the full-text index, or if that cannot run
/// the search, by looking for the text in senders, subjects and previews.
fn matched_rows(conn: &Connection, search: &str, account_id: &str, group: Option<&[String]>) -> Result<HashSet<i64>, String> {
    let mut rows = HashSet::new();
    match scoped_fts_ids(conn, search, account_id, group)? {
        Some(found) => {
            let mut stmt = conn
                .prepare_cached("SELECT rowid FROM messages WHERE account_id = ?1 AND id = ?2")
                .map_err(|e| e.to_string())?;
            for (account_id, message_id) in found {
                if let Ok(rowid) = stmt.query_row(params![account_id, message_id], |row| row.get(0)) {
                    rows.insert(rowid);
                }
            }
        }
        None => {
            let mut stmt = conn
                .prepare_cached(
                    "SELECT rowid FROM messages WHERE instr(lower(subject), ?1) > 0 OR instr(lower(snippet), ?1) > 0
                     OR instr(lower(from_email), ?1) > 0 OR instr(lower(from_name), ?1) > 0",
                )
                .map_err(|e| e.to_string())?;
            let found = stmt
                .query_map([search.to_lowercase()], |row| row.get(0))
                .map_err(|e| e.to_string())?;
            for rowid in found {
                rows.insert(rowid.map_err(|e| e.to_string())?);
            }
        }
    }
    Ok(rows)
}

fn count_folders(loaded: &Loaded, tabs: &InboxTabs) -> FolderCounts {
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
    let inbox = View::new("inbox", None, tabs);
    let starred = View::new("starred", None, tabs);
    let drafts = View::new("drafts", None, tabs);
    let spam = View::new("spam", None, tabs);
    let promotions = View::new("inbox", Some("promotions"), tabs);
    let social = View::new("inbox", Some("social"), tabs);
    let updates = View::new("inbox", Some("updates"), tabs);
    for conversation in &loaded.conversations {
        let conversation = loaded.messages_of(conversation);
        if inbox.has(conversation) {
            counts.inbox += 1;
            // The inbox with no category is its Primary tab, so the two unread counts agree.
            if inbox.has_unread(conversation) {
                counts.inbox_unread += 1;
                counts.primary_unread += 1;
            }
        }
        if starred.has(conversation) {
            counts.starred += 1;
        }
        if drafts.has(conversation) {
            counts.drafts += 1;
        }
        if promotions.has_unread(conversation) {
            counts.promotions_unread += 1;
        }
        if social.has_unread(conversation) {
            counts.social_unread += 1;
        }
        if updates.has_unread(conversation) {
            counts.updates_unread += 1;
        }
        if spam.has_unread(conversation) {
            counts.spam_unread += 1;
        }
    }
    counts
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

/// A group searches account by account, so one busy account can't use up the result limit.
fn scoped_fts_ids(
    conn: &Connection,
    raw: &str,
    account_id: &str,
    group: Option<&[String]>,
) -> Result<Option<HashSet<(String, String)>>, String> {
    let Some(group) = group else {
        return fts_ids(conn, raw, account_id);
    };
    let mut matched = HashSet::new();
    for id in group {
        match fts_ids(conn, raw, id)? {
            Some(found) => matched.extend(found),
            None => return Ok(None),
        }
    }
    Ok(Some(matched))
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
            local_label_ids: Vec::new(),
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
    fn history_label_changes_apply_in_order() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        upsert_message(&conn, &message("a", "t1", &["INBOX", "UNREAD"], "Lease", "rent is due")).unwrap();
        upsert_message(&conn, &message("b", "t2", &["INBOX"], "Sale", "watermelon festival")).unwrap();
        let change = |id: &str, add: bool, labels: &[&str]| LabelChange {
            message_id: id.into(),
            add,
            labels: labels.iter().map(|label| (*label).to_string()).collect(),
        };
        let (updated, missing) = apply_label_changes(
            &conn,
            "acct",
            &[
                change("a", true, &["TRASH"]),
                change("a", false, &["INBOX", "UNREAD"]),
                change("b", false, &["INBOX"]),
                change("b", true, &["INBOX"]),
                change("gone", true, &["TRASH"]),
            ],
        )
        .unwrap();
        assert_eq!(updated, 1);
        assert_eq!(missing, vec!["gone".to_string()]);
        assert_eq!(message_labels(&conn, "acct", "a").unwrap(), Some(vec!["TRASH".to_string()]));
        assert_eq!(message_labels(&conn, "acct", "b").unwrap(), Some(vec!["INBOX".to_string()]));
        assert!(ids_with_label(&conn, "acct", "INBOX").unwrap().contains("b"));
        assert!(!ids_with_label(&conn, "acct", "INBOX").unwrap().contains("a"));
    }

    #[test]
    fn a_group_shows_only_its_accounts() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        upsert_message(&conn, &message("a", "t1", &["INBOX"], "Lease", "rent is due")).unwrap();
        let other = StoredMessage {
            account_id: "other".into(),
            ..message("b", "t2", &["INBOX"], "Sale", "rent reduced")
        };
        upsert_message(&conn, &other).unwrap();
        let ask = |account_ids: Option<Vec<String>>, search: Option<&str>| {
            query_mailbox(
                &conn,
                &ThreadQuery {
                    account_id: None,
                    account_ids,
                    folder: "inbox".into(),
                    label_id: None,
                    category: Some("all".into()),
                    search: search.map(|text| text.to_string()),
                    limit: 50,
                    offset: 0,
                    inbox_tabs: InboxTabs::default(),
                },
            )
            .unwrap()
        };
        assert_eq!(ask(None, None).threads.len(), 2);
        let group = ask(Some(vec!["other".into()]), None);
        assert_eq!(group.threads.len(), 1);
        assert_eq!(group.threads[0].subject, "Sale");
        assert_eq!(group.counts.inbox, 1);
        let found = ask(Some(vec!["other".into()]), Some("rent"));
        assert_eq!(found.threads.len(), 1);
        assert_eq!(found.threads[0].subject, "Sale");
        assert!(ask(Some(vec![]), None).threads.is_empty());
    }

    #[test]
    fn novabox_labels_and_trash() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        upsert_message(&conn, &message("a", "t1", &["INBOX", "UNREAD"], "Lease", "rent is due")).unwrap();
        upsert_message(&conn, &message("b", "t2", &["INBOX"], "Sale", "rent reduced")).unwrap();
        upsert_message(&conn, &message("c", "t3", &["TRASH", "UNREAD"], "Old", "gone")).unwrap();
        upsert_message(&conn, &message("d", "t3", &["INBOX"], "Old", "kept")).unwrap();
        let label = LocalLabel { id: "L1".into(), name: "Follow up".into(), color: "#2F7D6D".into(), total: 0, unread: 0 };
        local_label_save(&conn, &label).unwrap();
        for thread in ["t1", "t2"] {
            local_label_assign(&conn, "L1", "acct", thread, true).unwrap();
        }
        let ask = |folder: &str, label_id: Option<&str>| {
            query_mailbox(
                &conn,
                &ThreadQuery {
                    account_id: None,
                    account_ids: None,
                    folder: folder.into(),
                    label_id: label_id.map(|id| id.to_string()),
                    category: Some("all".into()),
                    search: None,
                    limit: 50,
                    offset: 0,
                    inbox_tabs: InboxTabs::default(),
                },
            )
            .unwrap()
        };
        let labelled = ask("local", Some("L1"));
        assert_eq!(labelled.threads.len(), 2);
        assert_eq!((labelled.local_labels[0].total, labelled.local_labels[0].unread), (2, 1));
        assert!(labelled.threads.iter().all(|thread| thread.local_label_ids == vec!["L1".to_string()]));
        // The label shows on the rows of other folders too, and comes off again.
        let inbox = ask("inbox", None);
        assert_eq!(inbox.threads.iter().filter(|thread| !thread.local_label_ids.is_empty()).count(), 2);
        local_label_assign(&conn, "L1", "acct", "t2", false).unwrap();
        assert_eq!(ask("local", Some("L1")).threads.len(), 1);
        local_label_delete(&conn, "L1").unwrap();
        assert!(ask("local", Some("L1")).threads.is_empty());
        assert!(ask("inbox", None).local_labels.is_empty());
        // Deleting for good takes only the trashed message of a conversation.
        assert_eq!(trashed_in_thread(&conn, "acct", "t3").unwrap(), vec!["c".to_string()]);
        delete_messages(&conn, "acct", &["c".to_string()]).unwrap();
        assert!(ask("trash", None).threads.is_empty());
        assert_eq!(message_count(&conn, "acct").unwrap(), 3);
    }

    /// Times the list queries against a real mailbox file. Point NOVABOX_BENCH_DB at a copy
    /// (never the live file: this migrates it) and run
    /// `cargo test --lib list_query_speed -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn list_query_speed() {
        let path = std::env::var("NOVABOX_BENCH_DB").expect("set NOVABOX_BENCH_DB to a copy of novabox.db");
        let started = std::time::Instant::now();
        let conn = open(Path::new(&path)).unwrap();
        println!("open and migrate: {} ms", started.elapsed().as_millis());
        if needs_list_indexes(&conn) {
            let started = std::time::Instant::now();
            build_list_indexes(&conn).unwrap();
            println!("build list indexes: {} ms", started.elapsed().as_millis());
        }
        let accounts = list_accounts(&conn).unwrap();
        let largest = accounts
            .iter()
            .max_by_key(|account| message_count(&conn, &account.id).unwrap_or(0))
            .map(|account| account.id.clone());
        let smallest = accounts
            .iter()
            .min_by_key(|account| message_count(&conn, &account.id).unwrap_or(0))
            .map(|account| account.id.clone());
        let ask = |account_id: Option<String>, folder: &str, search: Option<&str>| ThreadQuery {
            account_id,
            account_ids: None,
            folder: folder.into(),
            label_id: None,
            category: Some("primary".into()),
            search: search.map(|text| text.to_string()),
            limit: 50,
            offset: 0,
            inbox_tabs: InboxTabs::default(),
        };
        for (name, query) in [
            ("all inboxes", ask(None, "inbox", None)),
            ("all inboxes again", ask(None, "inbox", None)),
            ("largest account", ask(largest, "inbox", None)),
            ("smallest account", ask(smallest, "inbox", None)),
            ("all sent", ask(None, "sent", None)),
            ("search", ask(None, "inbox", Some("invoice"))),
        ] {
            let started = std::time::Instant::now();
            let found = query_mailbox(&conn, &query).unwrap();
            println!("{name}: {} ms ({} conversations)", started.elapsed().as_millis(), found.total);
        }
        let started = std::time::Instant::now();
        let unread = total_unread(&conn).unwrap();
        println!("unread badge: {} us ({unread} unread)", started.elapsed().as_micros());
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
            provider: "gmail".into(),
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
                account_ids: None,
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
                account_ids: None,
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
                account_ids: None,
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
                account_ids: None,
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
                account_ids: None,
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
                account_ids: None,
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
                account_ids: None,
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
                account_ids: None,
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
