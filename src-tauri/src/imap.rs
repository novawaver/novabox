//! Mail services that Novabox reaches over IMAP and SMTP with an email address and a
//! password. Private Email (privateemail.com) is the one set up so far.
//!
//! The rest of Novabox thinks in Gmail's terms: a message carries labels and belongs to a
//! conversation. This module maps IMAP onto that. A message's folder and flags become its
//! labels (INBOX, SENT, DRAFT, TRASH, SPAM, UNREAD, STARRED, or one label per other folder),
//! and its conversation is worked out from its Message-ID, In-Reply-To and References headers.
//!
//! A message's id is `<folder key>-<UIDVALIDITY>-<UID>`, so a message that moves to another
//! folder gets a new id. The folder sync drops the old copy and stores the new one; the
//! conversation id stays the same, since it comes from the headers.

use std::collections::{HashMap, HashSet};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD};
use base64::Engine;
use imap::types::{Flag, NameAttribute};
use mail_parser::{Address as ParsedAddress, HeaderValue, MessageParser, MimeHeaders, PartType};
use serde_json::json;
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, Manager};

use crate::commands::{with_db, Change};
use crate::db;
use crate::files;
use crate::model::{Account, Address, AttachmentMeta, Label, StoredMessage, SyncProgress, ThreadTarget};
use crate::secrets;
use crate::AppState;

type ImapSession = imap::Session<native_tls::TlsStream<TcpStream>>;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const IO_TIMEOUT: Duration = Duration::from_secs(90);
/// Messages downloaded by one sync. The first sync of a new account takes a little more.
const SYNC_BATCH: usize = 200;
const FIRST_SYNC_BATCH: usize = 300;
/// One download command asks for at most this many messages, or this many bytes.
const FETCH_COUNT: usize = 20;
const FETCH_BYTES: u64 = 8 * 1024 * 1024;
const MAX_BODY: usize = 1_500_000;
/// Attachments up to this size are kept on this PC as the message downloads.
const CACHED_FILE: usize = 1_000_000;
/// Inline images up to this size are placed straight into the message.
const INLINE_IMAGE: usize = 300_000;

pub struct Server {
    /// Stored as the account's `provider`.
    pub provider: &'static str,
    pub name: &'static str,
    id_prefix: &'static str,
    imap_host: &'static str,
    imap_port: u16,
    smtp_host: &'static str,
    /// SMTP over TLS from the first byte (not STARTTLS).
    smtp_port: u16,
}

const SERVERS: [Server; 1] = [Server {
    provider: "privateemail",
    name: "Private Email",
    id_prefix: "pe",
    imap_host: "mail.privateemail.com",
    imap_port: 993,
    smtp_host: "mail.privateemail.com",
    smtp_port: 465,
}];

pub fn server(provider: &str) -> Option<&'static Server> {
    SERVERS.iter().find(|server| server.provider == provider)
}

/// Whether accounts of this provider are handled here, instead of through the Gmail API.
pub fn handles(provider: &str) -> bool {
    server(provider).is_some()
}

/// Whether this account is handled here. An account that is not on this PC is not.
pub fn uses(app: &AppHandle, account_id: &str) -> bool {
    with_db(app, |conn| db::account_provider(conn, account_id))
        .map(|provider| handles(&provider))
        .unwrap_or(false)
}

/// The same address gets the same id on every PC, so a backup restores onto the same account.
pub fn account_id(server: &Server, email: &str) -> String {
    format!("{}-{}", server.id_prefix, &hex(&Sha256::digest(email.trim().to_lowercase().as_bytes()))[..20])
}

#[derive(Default)]
pub struct State {
    /// One signed-in connection per account, kept between syncs.
    sessions: Mutex<HashMap<String, Conn>>,
    /// What each account's inbox looked like when it was last synced.
    inbox_marks: Mutex<HashMap<String, Mark>>,
}

struct Conn {
    session: ImapSession,
    can_move: bool,
    uid_plus: bool,
}

/// A cheap fingerprint of the inbox. While it stays the same, no mail arrived, left or was read.
#[derive(Clone, PartialEq)]
struct Mark {
    validity: u32,
    uid_next: u32,
    exists: u32,
    unseen: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Role {
    Inbox,
    Sent,
    Drafts,
    Trash,
    Spam,
    Archive,
    Other,
}

#[derive(Clone, Debug)]
struct Folder {
    /// The name as the server spells it (modified UTF-7), used in commands.
    name: String,
    role: Role,
    /// Short and stable; starts the id of every message in the folder.
    key: String,
    /// The name as shown to people.
    title: String,
}

impl Folder {
    fn new(name: &str, role: Role, delimiter: Option<&str>) -> Folder {
        let mut title = decode_utf7(name);
        if let Some(delimiter) = delimiter.filter(|item| !item.is_empty()) {
            title = title.replace(delimiter, "/");
        }
        Folder {
            name: name.to_string(),
            role,
            key: hex(&Sha256::digest(name.as_bytes()))[..12].to_string(),
            title,
        }
    }

    /// The label a message has for being in this folder. Archived mail has none.
    fn label(&self) -> Option<String> {
        match self.role {
            Role::Inbox => Some("INBOX".into()),
            Role::Sent => Some("SENT".into()),
            Role::Drafts => Some("DRAFT".into()),
            Role::Trash => Some("TRASH".into()),
            Role::Spam => Some("SPAM".into()),
            Role::Archive => None,
            Role::Other => Some(format!("F-{}", self.key)),
        }
    }

    fn labels(&self, seen: bool, flagged: bool) -> Vec<String> {
        let mut labels: Vec<String> = self.label().into_iter().collect();
        if !seen {
            labels.push("UNREAD".into());
        }
        if flagged {
            labels.push("STARRED".into());
        }
        labels
    }
}

// ---------------------------------------------------------------------------------------
// Connections
// ---------------------------------------------------------------------------------------

fn connect(server: &Server, email: &str, password: &str) -> Result<Conn, String> {
    let unreachable = |detail: String| format!("Could not reach {}: {detail}", server.name);
    let addresses = (server.imap_host, server.imap_port)
        .to_socket_addrs()
        .map_err(|e| unreachable(e.to_string()))?;
    let mut tcp = None;
    let mut last = "the server has no address".to_string();
    for address in addresses {
        match TcpStream::connect_timeout(&address, CONNECT_TIMEOUT) {
            Ok(stream) => {
                tcp = Some(stream);
                break;
            }
            Err(err) => last = err.to_string(),
        }
    }
    let tcp = tcp.ok_or_else(|| unreachable(last))?;
    tcp.set_read_timeout(Some(IO_TIMEOUT)).ok();
    tcp.set_write_timeout(Some(IO_TIMEOUT)).ok();
    let tls = native_tls::TlsConnector::new().map_err(|e| unreachable(e.to_string()))?;
    let stream = tls
        .connect(server.imap_host, tcp)
        .map_err(|e| unreachable(e.to_string()))?;
    let mut client = imap::Client::new(stream);
    client.read_greeting().map_err(fail)?;
    let mut session = client.login(email, password).map_err(|(err, _)| match err {
        imap::Error::No(_) | imap::Error::Bad(_) => format!(
            "{} did not accept that email address and password. Check them and try again.",
            server.name
        ),
        other => fail(other),
    })?;
    let (can_move, uid_plus) = match session.capabilities() {
        Ok(capabilities) => (capabilities.has_str("MOVE"), capabilities.has_str("UIDPLUS")),
        Err(err) => return Err(fail(err)),
    };
    Ok(Conn { session, can_move, uid_plus })
}

fn open(account: &Account) -> Result<Conn, String> {
    let server = server(&account.provider).ok_or("This account's mail service is not supported.")?;
    let password = secrets::load(&account.id)?;
    connect(server, &account.email, &password)
}

fn fail(err: imap::Error) -> String {
    match err {
        imap::Error::Io(_) | imap::Error::ConnectionLost | imap::Error::Tls(_) | imap::Error::TlsHandshake(_) => {
            format!("Could not reach the mail server: {err}")
        }
        imap::Error::No(text) | imap::Error::Bad(text) => {
            format!("The mail server refused that: {}", text.trim())
        }
        imap::Error::Parse(_) => "The mail server sent a reply Novabox could not read.".into(),
        other => other.to_string(),
    }
}

/// Whether an error means the connection is gone, as opposed to the server saying no.
fn lost(err: &str) -> bool {
    err.starts_with("Could not reach")
}

/// Runs IMAP work for an account off the async runtime, on the account's kept connection
/// or a new one. A kept connection the server has since dropped is replaced and the work
/// run once more, so `work` may run twice.
async fn run<T: Send + 'static>(
    app: &AppHandle,
    account_id: &str,
    work: impl Fn(&AppHandle, &Account, &mut Conn) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let app = app.clone();
    let account_id = account_id.to_string();
    tauri::async_runtime::spawn_blocking(move || {
        let account = with_db(&app, |conn| db::get_account(conn, &account_id))?;
        let kept = app
            .state::<AppState>()
            .imap
            .sessions
            .lock()
            .map_err(|e| e.to_string())?
            .remove(&account_id);
        let (mut conn, reused) = match kept {
            Some(conn) => (conn, true),
            None => (open(&account)?, false),
        };
        let mut result = work(&app, &account, &mut conn);
        if reused && matches!(&result, Err(err) if lost(err)) {
            conn = open(&account)?;
            result = work(&app, &account, &mut conn);
        }
        // After an error the connection may be mid-reply, so only a clean one is kept.
        if result.is_ok() {
            keep(&app, &account_id, conn);
        }
        result
    })
    .await
    .map_err(|e| e.to_string())?
}

fn keep(app: &AppHandle, account_id: &str, conn: Conn) {
    // Nothing reads these notices; left alone they would pile up for as long as the app runs.
    while conn.session.unsolicited_responses.try_recv().is_ok() {}
    if let Ok(mut sessions) = app.state::<AppState>().imap.sessions.lock() {
        sessions.insert(account_id.to_string(), conn);
    }
}

/// Drops what is remembered about an account that is being removed.
pub fn forget(app: &AppHandle, account_id: &str) {
    let state = app.state::<AppState>();
    if let Ok(mut sessions) = state.imap.sessions.lock() {
        sessions.remove(account_id);
    }
    if let Ok(mut marks) = state.imap.inbox_marks.lock() {
        marks.remove(account_id);
    };
}

/// Checks an email address and password by signing in, and returns the account's id.
/// The connection is kept for the first sync.
pub async fn sign_in(app: &AppHandle, provider: &str, email: &str, password: &str) -> Result<String, String> {
    let server = server(provider).ok_or("That mail service is not supported.")?;
    let id = account_id(server, email);
    let (app, email, password, kept_id) = (app.clone(), email.to_string(), password.to_string(), id.clone());
    tauri::async_runtime::spawn_blocking(move || {
        let conn = connect(server, &email, &password)?;
        keep(&app, &kept_id, conn);
        Ok::<(), String>(())
    })
    .await
    .map_err(|e| e.to_string())??;
    Ok(id)
}

// ---------------------------------------------------------------------------------------
// Folders
// ---------------------------------------------------------------------------------------

struct Listed {
    name: String,
    attributes: Vec<String>,
    delimiter: Option<String>,
}

fn list_folders(conn: &mut Conn) -> Result<Vec<Folder>, String> {
    let names = conn.session.list(Some(""), Some("*")).map_err(fail)?;
    let listed = names
        .iter()
        .filter(|name| !name.attributes().contains(&NameAttribute::NoSelect))
        .map(|name| Listed {
            name: name.name().to_string(),
            attributes: name
                .attributes()
                .iter()
                .filter_map(|attribute| match attribute {
                    NameAttribute::Custom(text) => Some(text.to_string()),
                    _ => None,
                })
                .collect(),
            delimiter: name.delimiter().map(|item| item.to_string()),
        })
        .collect();
    Ok(assign_roles(listed))
}

/// Decides what each folder is for: from the server's own markers (RFC 6154) where it sets
/// them, otherwise from the usual names. Returned in the order they are synced, inbox first.
fn assign_roles(listed: Vec<Listed>) -> Vec<Folder> {
    let marked = |item: &Listed| -> Option<Role> {
        item.attributes.iter().find_map(|attribute| match attribute.to_ascii_lowercase().as_str() {
            "\\sent" => Some(Role::Sent),
            "\\drafts" => Some(Role::Drafts),
            "\\trash" => Some(Role::Trash),
            "\\junk" => Some(Role::Spam),
            "\\archive" => Some(Role::Archive),
            _ => None,
        })
    };
    // Views over mail that also sits in a real folder; syncing them would store it twice.
    let is_view = |item: &Listed| {
        item.attributes.iter().any(|attribute| {
            matches!(attribute.to_ascii_lowercase().as_str(), "\\all" | "\\flagged" | "\\important" | "\\noselect" | "\\nonexistent")
        })
    };
    let named = |item: &Listed| -> Option<Role> {
        let leaf = match item.delimiter.as_deref().filter(|delimiter| !delimiter.is_empty()) {
            Some(delimiter) => item.name.rsplit(delimiter).next().unwrap_or(&item.name),
            None => &item.name,
        };
        match leaf.to_ascii_lowercase().as_str() {
            "sent" | "sent items" | "sent messages" | "sent mail" => Some(Role::Sent),
            "drafts" | "draft" => Some(Role::Drafts),
            "trash" | "deleted items" | "deleted messages" | "bin" => Some(Role::Trash),
            "spam" | "junk" | "junk e-mail" | "junk email" | "bulk mail" => Some(Role::Spam),
            "archive" | "archives" => Some(Role::Archive),
            _ => None,
        }
    };
    let listed: Vec<Listed> = listed.into_iter().filter(|item| !is_view(item)).collect();
    let mut roles: Vec<Option<Role>> = listed
        .iter()
        .map(|item| {
            if item.name.eq_ignore_ascii_case("INBOX") {
                Some(Role::Inbox)
            } else {
                marked(item)
            }
        })
        .collect();
    // A role goes to one folder only: the first the server marks, else the first with the name.
    let mut taken: HashSet<Role> = HashSet::new();
    for role in roles.iter_mut() {
        if let Some(found) = *role {
            if !taken.insert(found) {
                *role = None;
            }
        }
    }
    for (index, item) in listed.iter().enumerate() {
        if roles[index].is_some() {
            continue;
        }
        if let Some(found) = named(item) {
            if taken.insert(found) {
                roles[index] = Some(found);
            }
        }
    }
    let mut folders: Vec<Folder> = listed
        .iter()
        .zip(roles)
        .map(|(item, role)| match role {
            // Servers accept any capitalisation of the inbox's name; ids are made from this one.
            Some(Role::Inbox) => Folder::new("INBOX", Role::Inbox, None),
            role => Folder::new(&item.name, role.unwrap_or(Role::Other), item.delimiter.as_deref()),
        })
        .collect();
    let order = |role: Role| match role {
        Role::Inbox => 0,
        Role::Sent => 1,
        Role::Drafts => 2,
        Role::Archive => 3,
        Role::Other => 4,
        Role::Spam => 5,
        Role::Trash => 6,
    };
    folders.sort_by(|a, b| order(a.role).cmp(&order(b.role)).then(a.title.cmp(&b.title)));
    folders
}

/// The folder with a role, made on the server if the account has none yet.
fn ensure_folder(conn: &mut Conn, folders: &[Folder], role: Role) -> Result<Folder, String> {
    if let Some(folder) = folders.iter().find(|folder| folder.role == role) {
        return Ok(folder.clone());
    }
    let name = match role {
        Role::Sent => "Sent",
        Role::Drafts => "Drafts",
        Role::Trash => "Trash",
        Role::Spam => "Spam",
        Role::Archive => "Archive",
        Role::Inbox | Role::Other => return Err("That folder is missing from the mailbox.".into()),
    };
    conn.session.create(name).map_err(fail)?;
    let _ = conn.session.subscribe(name);
    Ok(Folder::new(name, role, None))
}

/// Modified UTF-7 (RFC 3501 section 5.1.3), which IMAP uses for folder names outside ASCII.
fn decode_utf7(name: &str) -> String {
    let mut out = String::new();
    let mut rest = name;
    while let Some(start) = rest.find('&') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let Some(end) = after.find('-') else {
            out.push_str(&rest[start..]);
            return out;
        };
        let code = &after[..end];
        if code.is_empty() {
            out.push('&');
        } else {
            match STANDARD_NO_PAD.decode(code.replace(',', "/")) {
                Ok(bytes) => {
                    let units: Vec<u16> = bytes
                        .chunks_exact(2)
                        .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
                        .collect();
                    out.push_str(&String::from_utf16_lossy(&units));
                }
                Err(_) => out.push_str(&rest[start..start + end + 2]),
            }
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}

// ---------------------------------------------------------------------------------------
// Message ids and conversations
// ---------------------------------------------------------------------------------------

fn message_id(folder: &Folder, validity: u32, uid: u32) -> String {
    format!("{}-{validity}-{uid}", folder.key)
}

/// Splits a message id into its folder key, UIDVALIDITY and UID.
fn parse_id(id: &str) -> Option<(&str, u32, u32)> {
    let mut parts = id.split('-');
    let key = parts.next()?;
    let validity = parts.next()?.parse().ok()?;
    let uid = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((key, validity, uid))
}

/// The conversation a message belongs to. A message joins the conversation of any stored
/// message it refers to (or of its own other copy, after a move). Otherwise the conversation
/// is named after the first message of the chain, so its replies land in it whichever of
/// them downloads first.
fn thread_for(
    conn: &rusqlite::Connection,
    account_id: &str,
    headers: &Headers,
    fallback: &str,
) -> Result<String, String> {
    let known = headers
        .references
        .iter()
        .chain(headers.in_reply_to.iter())
        .chain(std::iter::once(&headers.message_id));
    for header in known {
        if let Some((_, thread_id)) = db::message_by_header(conn, account_id, header)? {
            return Ok(thread_id);
        }
    }
    let root = headers
        .references
        .first()
        .or(headers.in_reply_to.first())
        .or(Some(&headers.message_id))
        .filter(|header| !header.is_empty());
    Ok(match root {
        Some(root) => format!("t{}", &hex(&Sha256::digest(root.as_bytes()))[..20]),
        None => format!("t{fallback}"),
    })
}

// ---------------------------------------------------------------------------------------
// Reading messages
// ---------------------------------------------------------------------------------------

/// The headers that tie messages into conversations, each id in angle brackets.
#[derive(Default)]
struct Headers {
    message_id: String,
    in_reply_to: Vec<String>,
    references: Vec<String>,
}

struct Parsed {
    /// Its `thread_id` is left empty for the caller to fill in.
    message: StoredMessage,
    headers: Headers,
    files: Vec<(String, Vec<u8>)>,
}

fn id_list(value: &HeaderValue) -> Vec<String> {
    value
        .as_text_list()
        .unwrap_or_default()
        .into_iter()
        .map(|id| id.trim().trim_matches(['<', '>']).to_string())
        .filter(|id| !id.is_empty())
        .map(|id| format!("<{id}>"))
        .collect()
}

fn addresses(list: Option<&ParsedAddress>) -> Vec<Address> {
    list.map(|list| {
        list.iter()
            .filter_map(|item| {
                let email = item.address()?.trim().to_string();
                if email.is_empty() {
                    return None;
                }
                Some(Address {
                    name: item.name().unwrap_or("").replace('\0', ""),
                    email,
                })
            })
            .collect()
    })
    .unwrap_or_default()
}

fn cut(text: &mut String, limit: usize) {
    if text.len() > limit {
        let mut end = limit;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
    }
}

fn headers_of(raw: &[u8]) -> Headers {
    MessageParser::default()
        .parse_headers(raw)
        .map(|message| Headers {
            message_id: message.message_id().map(|id| format!("<{}>", id.trim())).unwrap_or_default(),
            in_reply_to: id_list(message.in_reply_to()),
            references: id_list(message.references()),
        })
        .unwrap_or_default()
}

fn parse_message(account_id: &str, id: &str, raw: &[u8], labels: Vec<String>, received: i64, draft: bool) -> Result<Parsed, String> {
    let message = MessageParser::default()
        .parse(raw)
        .ok_or("Novabox could not read that message.")?;
    let headers = Headers {
        message_id: message.message_id().map(|id| format!("<{}>", id.trim())).unwrap_or_default(),
        in_reply_to: id_list(message.in_reply_to()),
        references: id_list(message.references()),
    };
    let mut html = String::new();
    for index in &message.html_body {
        if let Some(PartType::Html(part)) = message.parts.get(*index).map(|part| &part.body) {
            html.push_str(part);
        }
    }
    let mut text = String::new();
    for index in &message.text_body {
        if let Some(PartType::Text(part)) = message.parts.get(*index).map(|part| &part.body) {
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(part);
        }
    }
    if text.is_empty() && !html.is_empty() {
        // Search and the list preview read the plain text.
        text = message.body_text(0).map(|body| body.into_owned()).unwrap_or_default();
    }
    let mut attachments = Vec::new();
    let mut files = Vec::new();
    for (index, part) in message.attachments().enumerate() {
        let bytes = part.contents();
        let mime_type = part
            .content_type()
            .map(|kind| match kind.subtype() {
                Some(subtype) => format!("{}/{subtype}", kind.ctype()),
                None => kind.ctype().to_string(),
            })
            .unwrap_or_else(|| if part.is_message() { "message/rfc822".into() } else { "application/octet-stream".into() })
            .to_ascii_lowercase();
        // A picture the message shows in its own text goes into that text, not the file list.
        let content_id = part.content_id().map(|id| id.trim().trim_matches(['<', '>']).to_string());
        if let Some(content_id) = content_id.filter(|id| !id.is_empty()) {
            let link = format!("cid:{content_id}");
            if mime_type.starts_with("image/") && bytes.len() <= INLINE_IMAGE && html.contains(&link) {
                html = html.replace(&link, &format!("data:{mime_type};base64,{}", STANDARD.encode(bytes)));
                continue;
            }
        }
        let attachment_id = format!("att-{index}");
        if !bytes.is_empty() && bytes.len() <= CACHED_FILE {
            files.push((attachment_id.clone(), bytes.to_vec()));
        }
        attachments.push(AttachmentMeta {
            id: attachment_id,
            filename: part
                .attachment_name()
                .map(|name| name.replace(['\0', '\r', '\n'], ""))
                .filter(|name| !name.trim().is_empty())
                .unwrap_or_else(|| format!("attachment-{}", index + 1)),
            mime_type,
            size: bytes.len() as i64,
        });
    }
    html = html.replace('\0', "");
    text = text.replace('\0', "");
    cut(&mut html, MAX_BODY + INLINE_IMAGE * 4);
    cut(&mut text, MAX_BODY);
    let snippet: String = text.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(200).collect();
    let sender = message.from().and_then(|from| from.first());
    let date = if received > 0 {
        received
    } else {
        message.date().map(|date| date.to_timestamp() * 1000).unwrap_or(0)
    };
    Ok(Parsed {
        message: StoredMessage {
            account_id: account_id.to_string(),
            id: id.to_string(),
            thread_id: String::new(),
            from_name: sender.and_then(|from| from.name()).unwrap_or("").replace('\0', ""),
            from_email: sender.and_then(|from| from.address()).unwrap_or("").trim().to_string(),
            to: addresses(message.to()),
            cc: addresses(message.cc()),
            bcc: addresses(message.bcc()),
            subject: message.subject().unwrap_or("").replace('\0', ""),
            snippet,
            body_html: html,
            body_text: text,
            date,
            label_ids: labels,
            attachments,
            message_id_header: headers.message_id.clone(),
            references_header: headers.references.join(" "),
            draft_id: draft.then(|| id.to_string()),
            size_estimate: raw.len() as i64,
        },
        headers,
        files,
    })
}

// ---------------------------------------------------------------------------------------
// Sync
// ---------------------------------------------------------------------------------------

/// What one message on the server looks like without downloading it.
struct Remote {
    seen: bool,
    flagged: bool,
    size: u32,
}

struct Downloaded {
    uid: u32,
    id: String,
    unread: bool,
    /// The same message was already stored under another id: it moved here, it is not new.
    moved: bool,
}

#[derive(Default)]
struct FolderSync {
    changed: bool,
    /// Unread mail that arrived in the inbox since the last sync.
    fresh: Vec<String>,
    /// Messages on the server that this sync did not get to.
    left: usize,
}

fn flags_of(row: &imap::types::Fetch) -> (bool, bool, bool) {
    let flags = row.flags();
    (
        flags.contains(&Flag::Seen),
        flags.contains(&Flag::Flagged),
        flags.contains(&Flag::Deleted),
    )
}

fn uid_set(uids: &[u32]) -> String {
    uids.iter().map(|uid| uid.to_string()).collect::<Vec<_>>().join(",")
}

fn progress(app: &AppHandle, account: &Account, phase: &str) {
    let fetched = with_db(app, |conn| db::message_count(conn, &account.id)).unwrap_or(0) as u32;
    let _ = app.emit(
        "sync://progress",
        SyncProgress {
            email: account.email.clone(),
            phase: phase.to_string(),
            fetched,
        },
    );
}

/// Downloads and stores messages of the folder that is currently open.
fn download(
    app: &AppHandle,
    account: &Account,
    conn: &mut Conn,
    folder: &Folder,
    validity: u32,
    uids: &[u32],
    sizes: &HashMap<u32, Remote>,
    phase: &str,
) -> Result<Vec<Downloaded>, String> {
    let data_dir = app.state::<AppState>().data_dir.clone();
    let mut done = Vec::new();
    let mut last_refresh = Instant::now();
    let mut start = 0;
    while start < uids.len() {
        let mut end = start;
        let mut bytes = 0u64;
        while end < uids.len() && end - start < FETCH_COUNT {
            bytes += sizes.get(&uids[end]).map_or(0, |remote| remote.size as u64);
            end += 1;
            if bytes >= FETCH_BYTES {
                break;
            }
        }
        let rows = conn
            .session
            .uid_fetch(uid_set(&uids[start..end]), "(UID FLAGS INTERNALDATE BODY.PEEK[])")
            .map_err(fail)?;
        for row in rows.iter() {
            let (Some(uid), Some(raw)) = (row.uid, row.body()) else {
                continue;
            };
            let (seen, flagged, deleted) = flags_of(row);
            if deleted {
                continue;
            }
            let id = message_id(folder, validity, uid);
            let received = row.internal_date().map(|date| date.timestamp_millis()).unwrap_or(0);
            let parsed = match parse_message(&account.id, &id, raw, folder.labels(seen, flagged), received, folder.role == Role::Drafts) {
                Ok(parsed) => parsed,
                Err(err) => {
                    eprintln!("Novabox message {id}: {err}");
                    continue;
                }
            };
            let mut message = parsed.message;
            let moved = with_db(app, |db| {
                let moved = db::message_by_header(db, &account.id, &parsed.headers.message_id)?.is_some();
                message.thread_id = thread_for(db, &account.id, &parsed.headers, &id)?;
                db::upsert_message(db, &message)?;
                Ok(moved)
            })?;
            for (attachment_id, bytes) in parsed.files {
                if let Err(err) = files::write_cached(&data_dir, &account.id, &id, &attachment_id, &bytes) {
                    eprintln!("Novabox attachment cache: {err}");
                }
            }
            done.push(Downloaded { uid, id, unread: !seen, moved });
        }
        start = end;
        progress(app, account, phase);
        // Every refresh makes the window re-query the mailbox, so refresh at most every few seconds.
        if last_refresh.elapsed() >= Duration::from_secs(3) {
            let _ = app.emit("mail://updated", json!({}));
            last_refresh = Instant::now();
        }
    }
    Ok(done)
}

/// Brings what is stored of one folder in line with the server: drops what is gone, updates
/// read and starred state, and downloads up to `budget` new messages, newest first.
fn sync_folder(
    app: &AppHandle,
    account: &Account,
    conn: &mut Conn,
    folder: &Folder,
    budget: &mut usize,
    phase: &str,
) -> Result<FolderSync, String> {
    let mailbox = conn.session.examine(&folder.name).map_err(fail)?;
    let validity = mailbox.uid_validity.unwrap_or(0);
    let mut remote: HashMap<u32, Remote> = HashMap::new();
    if mailbox.exists > 0 {
        let rows = conn.session.uid_fetch("1:*", "(UID FLAGS RFC822.SIZE)").map_err(fail)?;
        for row in rows.iter() {
            let Some(uid) = row.uid else { continue };
            let (seen, flagged, deleted) = flags_of(row);
            // Marked for deletion by another mail app: as good as gone.
            if !deleted {
                remote.insert(uid, Remote { seen, flagged, size: row.size.unwrap_or(0) });
            }
        }
    }
    let stored = with_db(app, |db| db::messages_with_prefix(db, &account.id, &format!("{}-", folder.key)))?;
    let data_dir = app.state::<AppState>().data_dir.clone();
    let mut done = FolderSync::default();
    let mut have: HashSet<u32> = HashSet::new();
    let mut newest_stored = 0u32;
    for (id, labels) in stored {
        let current = parse_id(&id)
            .filter(|(_, stored_validity, _)| *stored_validity == validity)
            .and_then(|(_, _, uid)| remote.get(&uid).map(|message| (uid, message)));
        match current {
            Some((uid, message)) => {
                have.insert(uid);
                newest_stored = newest_stored.max(uid);
                let wanted = folder.labels(message.seen, message.flagged);
                let same = labels.len() == wanted.len() && wanted.iter().all(|label| labels.contains(label));
                if !same {
                    with_db(app, |db| db::set_labels(db, &account.id, &id, &wanted))?;
                    done.changed = true;
                }
            }
            None => {
                with_db(app, |db| db::delete_message(db, &account.id, &id))?;
                files::remove_message_files(&data_dir, &account.id, &id);
                done.changed = true;
            }
        }
    }
    let mut missing: Vec<u32> = remote.keys().copied().filter(|uid| !have.contains(uid)).collect();
    missing.sort_unstable_by(|a, b| b.cmp(a));
    let take = missing.len().min(*budget);
    done.left = missing.len() - take;
    if take > 0 {
        progress(app, account, phase);
        let fetched = download(app, account, conn, folder, validity, &missing[..take], &remote, phase)?;
        *budget -= take;
        done.changed |= !fetched.is_empty();
        if folder.role == Role::Inbox && account.initial_done {
            done.fresh = fetched
                .into_iter()
                .filter(|message| message.unread && !message.moved && message.uid > newest_stored)
                .map(|message| message.id)
                .collect();
        }
    }
    Ok(done)
}

fn inbox_mark(conn: &mut Conn) -> Result<Mark, String> {
    let mailbox = conn.session.examine("INBOX").map_err(fail)?;
    let unseen = conn.session.uid_search("UNSEEN").map_err(fail)?.len();
    Ok(Mark {
        validity: mailbox.uid_validity.unwrap_or(0),
        uid_next: mailbox.uid_next.unwrap_or(0),
        exists: mailbox.exists,
        unseen,
    })
}

fn remember_mark(app: &AppHandle, account_id: &str, mark: Mark) {
    if let Ok(mut marks) = app.state::<AppState>().imap.inbox_marks.lock() {
        marks.insert(account_id.to_string(), mark);
    }
}

/// The full sync: folders, then every folder's mail. A big mailbox downloads over several syncs.
pub async fn sync(app: &AppHandle, account_id: &str, eager: bool) -> Result<(), String> {
    let budget = if eager { FIRST_SYNC_BATCH } else { SYNC_BATCH };
    run(app, account_id, move |app, account, conn| sync_mailbox(app, account, conn, budget)).await
}

fn sync_mailbox(app: &AppHandle, account: &Account, conn: &mut Conn, mut budget: usize) -> Result<(), String> {
    with_db(app, db::ensure_header_index)?;
    let folders = list_folders(conn)?;
    let labels: Vec<Label> = folders
        .iter()
        .filter(|folder| folder.role == Role::Other)
        .map(|folder| Label {
            account_id: account.id.clone(),
            id: folder.label().unwrap_or_default(),
            name: folder.title.clone(),
            kind: "user".into(),
            color: None,
            unread: 0,
        })
        .collect();
    with_db(app, |db| db::replace_labels(db, &account.id, &labels))?;
    let phase = if account.initial_done { "Downloading older mail" } else { "Downloading recent mail" };
    let mut left = 0;
    let mut fresh = Vec::new();
    for folder in &folders {
        let mark = if folder.role == Role::Inbox { Some(inbox_mark(conn)?) } else { None };
        match sync_folder(app, account, conn, folder, &mut budget, phase) {
            Ok(done) => {
                left += done.left;
                fresh.extend(done.fresh);
                if let Some(mark) = mark {
                    remember_mark(app, &account.id, mark);
                }
            }
            Err(err) if lost(&err) || folder.role == Role::Inbox => return Err(err),
            Err(err) => {
                // One unreadable folder should not stop the others; it is tried again next sync.
                eprintln!("Novabox folder {}: {err}", folder.title);
                left += 1;
            }
        }
    }
    if !fresh.is_empty() {
        crate::sync::notify_new_mail(app, account, &fresh);
    }
    with_db(app, |db| db::set_sync_state(db, &account.id, None, None, None, left == 0, true))
}

/// The cheap new-mail check: looks at the inbox's fingerprint, and syncs the inbox only when
/// it has changed. Returns whether anything stored changed.
pub async fn check(app: &AppHandle, account_id: &str) -> Result<bool, String> {
    run(app, account_id, |app, account, conn| {
        let mark = inbox_mark(conn)?;
        let known = app
            .state::<AppState>()
            .imap
            .inbox_marks
            .lock()
            .ok()
            .and_then(|marks| marks.get(&account.id).cloned());
        if known.as_ref() == Some(&mark) {
            return Ok(false);
        }
        let inbox = Folder::new("INBOX", Role::Inbox, None);
        let mut budget = SYNC_BATCH;
        let done = sync_folder(app, account, conn, &inbox, &mut budget, "Updating inbox")?;
        remember_mark(app, &account.id, mark);
        if !done.fresh.is_empty() {
            crate::sync::notify_new_mail(app, account, &done.fresh);
        }
        Ok(done.changed)
    })
    .await
}

// ---------------------------------------------------------------------------------------
// Changing mail
// ---------------------------------------------------------------------------------------

/// Where a change sends a conversation's messages. IMAP keeps a message in one folder, so
/// what Gmail does by adding and removing labels is done here by moving it.
#[derive(Debug, PartialEq)]
enum Move {
    Trash,
    /// Out of Trash, back to where each message belongs.
    Restore,
    Spam,
    Inbox { from_spam: bool, from_trash: bool },
    Archive,
    /// Into the folder behind a label.
    Label(String),
    /// Out of the folder behind a label, back to the inbox. A folder is the only place its
    /// mail is, so taking the label off has to put the mail somewhere it will be seen.
    Unlabel(String),
}

fn planned_move(change: &Change) -> Option<Move> {
    let (add, remove) = match change {
        Change::Trash => return Some(Move::Trash),
        Change::Untrash => return Some(Move::Restore),
        Change::Labels { add, remove } => (add, remove),
    };
    let has = |list: &[String], id: &str| list.iter().any(|label| label == id);
    let folder_label = |list: &[String]| list.iter().find(|label| label.starts_with("F-")).cloned();
    if has(add, "SPAM") {
        Some(Move::Spam)
    } else if has(add, "INBOX") {
        Some(Move::Inbox { from_spam: has(remove, "SPAM"), from_trash: has(remove, "TRASH") })
    } else if let Some(label) = folder_label(add) {
        Some(Move::Label(label))
    } else if has(remove, "INBOX") {
        Some(Move::Archive)
    } else {
        folder_label(remove).map(Move::Unlabel)
    }
}

/// Whether a move takes messages out of this folder.
fn leaves(moving: &Move, folder: &Folder) -> bool {
    let loose = matches!(folder.role, Role::Inbox | Role::Archive | Role::Other);
    match moving {
        Move::Trash => folder.role != Role::Trash,
        Move::Restore => folder.role == Role::Trash,
        Move::Spam => loose,
        Move::Inbox { from_spam, from_trash } => {
            folder.role == Role::Archive || (*from_spam && folder.role == Role::Spam) || (*from_trash && folder.role == Role::Trash)
        }
        Move::Archive => folder.role == Role::Inbox,
        Move::Label(label) => loose && folder.label().as_deref() != Some(label.as_str()),
        Move::Unlabel(label) => folder.label().as_deref() == Some(label.as_str()),
    }
}

/// The flag commands a change needs, for read and starred state.
fn flag_commands(change: &Change) -> Vec<&'static str> {
    let Change::Labels { add, remove } = change else {
        return Vec::new();
    };
    let has = |list: &[String], id: &str| list.iter().any(|label| label == id);
    let mut commands = Vec::new();
    if has(remove, "UNREAD") {
        commands.push("+FLAGS.SILENT (\\Seen)");
    }
    if has(add, "UNREAD") {
        commands.push("-FLAGS.SILENT (\\Seen)");
    }
    if has(add, "STARRED") {
        commands.push("+FLAGS.SILENT (\\Flagged)");
    }
    if has(remove, "STARRED") {
        commands.push("-FLAGS.SILENT (\\Flagged)");
    }
    commands
}

fn move_messages(conn: &mut Conn, uids: &[u32], to: &Folder) -> Result<(), String> {
    let set = uid_set(uids);
    if conn.can_move {
        return conn.session.uid_mv(&set, &to.name).map_err(fail);
    }
    conn.session.uid_copy(&set, &to.name).map_err(fail)?;
    expunge(conn, &set)
}

/// Deletes messages of the open folder for good.
fn expunge(conn: &mut Conn, set: &str) -> Result<(), String> {
    conn.session.uid_store(set, "+FLAGS.SILENT (\\Deleted)").map_err(fail)?;
    if conn.uid_plus {
        conn.session.uid_expunge(set).map_err(fail)?;
    } else {
        conn.session.expunge().map_err(fail)?;
    }
    Ok(())
}

/// Applies a change to a conversation on the server.
pub async fn change(app: &AppHandle, target: &ThreadTarget, change: &Change) -> Result<(), String> {
    let (thread_id, change) = (target.thread_id.clone(), change.clone());
    run(app, &target.account_id, move |app, account, conn| {
        change_thread(app, account, conn, &thread_id, &change)
    })
    .await
}

fn change_thread(app: &AppHandle, account: &Account, conn: &mut Conn, thread_id: &str, change: &Change) -> Result<(), String> {
    let messages = with_db(app, |db| db::thread_senders(db, &account.id, thread_id))?;
    if messages.is_empty() {
        return Ok(());
    }
    let mut folders = list_folders(conn)?;
    let moving = planned_move(change);
    let commands = flag_commands(change);
    let mine = |email: &str| {
        email.eq_ignore_ascii_case(&account.email) || account.send_as.iter().any(|alias| alias.email.eq_ignore_ascii_case(email))
    };
    // Per folder: the UIDVALIDITY its ids were made under, every message, and the ones that move.
    struct Batch {
        folder: Folder,
        validity: u32,
        all: Vec<u32>,
        moves: Vec<(Role, Option<String>, u32)>,
    }
    let mut batches: Vec<Batch> = Vec::new();
    for (id, from_email) in &messages {
        let Some((key, validity, uid)) = parse_id(id) else { continue };
        let Some(folder) = folders.iter().find(|folder| folder.key == key) else { continue };
        let index = match batches.iter().position(|batch| batch.folder.key == key && batch.validity == validity) {
            Some(index) => index,
            None => {
                batches.push(Batch { folder: folder.clone(), validity, all: Vec::new(), moves: Vec::new() });
                batches.len() - 1
            }
        };
        batches[index].all.push(uid);
        let Some(moving) = moving.as_ref().filter(|moving| leaves(moving, folder)) else { continue };
        let to = match moving {
            Move::Trash => (Role::Trash, None),
            Move::Spam => (Role::Spam, None),
            Move::Inbox { .. } | Move::Unlabel(_) => (Role::Inbox, None),
            Move::Archive => (Role::Archive, None),
            Move::Label(label) => (Role::Other, Some(label.clone())),
            // Mail this account sent goes back to Sent; the rest goes back to the inbox.
            Move::Restore if mine(from_email) => (Role::Sent, None),
            Move::Restore => (Role::Inbox, None),
        };
        batches[index].moves.push((to.0, to.1, uid));
    }
    let mut touched: Vec<Folder> = Vec::new();
    for batch in &batches {
        let mailbox = conn.session.select(&batch.folder.name).map_err(fail)?;
        if mailbox.uid_validity.unwrap_or(0) != batch.validity {
            // The folder was rebuilt on the server; the next sync downloads it afresh.
            continue;
        }
        for command in &commands {
            conn.session.uid_store(uid_set(&batch.all), command).map_err(fail)?;
        }
        let mut by_target: Vec<((Role, Option<String>), Vec<u32>)> = Vec::new();
        for (role, label, uid) in &batch.moves {
            let key = (*role, label.clone());
            match by_target.iter_mut().find(|(target, _)| *target == key) {
                Some((_, uids)) => uids.push(*uid),
                None => by_target.push((key, vec![*uid])),
            }
        }
        for ((role, label), uids) in by_target {
            let to = match label {
                Some(label) => folders
                    .iter()
                    .find(|folder| folder.label().as_deref() == Some(label.as_str()))
                    .cloned()
                    .ok_or("That folder is no longer in the mailbox.")?,
                None => {
                    let folder = ensure_folder(conn, &folders, role)?;
                    if !folders.iter().any(|known| known.key == folder.key) {
                        folders.push(folder.clone());
                        // Making a folder closes nothing, but be sure the source is still open.
                        conn.session.select(&batch.folder.name).map_err(fail)?;
                    }
                    folder
                }
            };
            move_messages(conn, &uids, &to)?;
            for folder in [&to, &batch.folder] {
                if !touched.iter().any(|known| known.key == folder.key) {
                    touched.push(folder.clone());
                }
            }
        }
    }
    // A moved message has a new id. Store the new copies before dropping the old ones, so the
    // conversation never disappears from the list in between.
    touched.sort_by_key(|folder| batches.iter().any(|batch| batch.folder.key == folder.key));
    for folder in &touched {
        let mut budget = SYNC_BATCH;
        if let Err(err) = sync_folder(app, account, conn, folder, &mut budget, "Updating inbox") {
            eprintln!("Novabox folder {}: {err}", folder.title);
            if lost(&err) {
                // The change itself went through. The next sync tidies up on a new connection.
                break;
            }
        }
    }
    Ok(())
}

/// Deletes one message from its folder for good. Used for drafts that were sent or replaced.
fn remove_message(conn: &mut Conn, folders: &[Folder], id: &str) -> Result<(), String> {
    let Some((key, validity, uid)) = parse_id(id) else { return Ok(()) };
    let Some(folder) = folders.iter().find(|folder| folder.key == key) else { return Ok(()) };
    let mailbox = conn.session.select(&folder.name).map_err(fail)?;
    if mailbox.uid_validity.unwrap_or(0) != validity {
        return Ok(());
    }
    expunge(conn, &uid.to_string())
}

/// Deletes messages for good. Only ones in the Trash folder go; any other id is left alone.
pub async fn delete_forever(app: &AppHandle, account_id: &str, ids: Vec<String>) -> Result<(), String> {
    run(app, account_id, move |_, _, conn| {
        let folders = list_folders(conn)?;
        let Some(trash) = folders.iter().find(|folder| folder.role == Role::Trash) else {
            return Ok(());
        };
        let mailbox = conn.session.select(&trash.name).map_err(fail)?;
        let validity = mailbox.uid_validity.unwrap_or(0);
        let uids: Vec<u32> = ids
            .iter()
            .filter_map(|id| parse_id(id))
            .filter(|(key, stored, _)| *key == trash.key && *stored == validity)
            .map(|(_, _, uid)| uid)
            .collect();
        if uids.is_empty() {
            return Ok(());
        }
        expunge(conn, &uid_set(&uids))
    })
    .await
}

/// Deletes everything in the Trash folder for good.
pub async fn empty_trash(app: &AppHandle, account_id: &str) -> Result<(), String> {
    run(app, account_id, |_, _, conn| {
        let folders = list_folders(conn)?;
        let Some(trash) = folders.iter().find(|folder| folder.role == Role::Trash) else {
            return Ok(());
        };
        let mailbox = conn.session.select(&trash.name).map_err(fail)?;
        if mailbox.exists == 0 {
            return Ok(());
        }
        conn.session.store("1:*", "+FLAGS.SILENT (\\Deleted)").map_err(fail)?;
        conn.session.expunge().map_err(fail)?;
        Ok(())
    })
    .await
}

// ---------------------------------------------------------------------------------------
// Sending and drafts
// ---------------------------------------------------------------------------------------

#[derive(Default)]
pub struct Filed {
    pub message_id: String,
    pub thread_id: String,
}

/// A mailbox name the append command can carry as it is.
fn appendable(folder: &Folder) -> Result<&str, String> {
    if folder.name.contains(['"', '\\', '\r', '\n']) {
        return Err(format!("Novabox can't save to the folder {}.", folder.title));
    }
    Ok(&folder.name)
}

/// The message without its Bcc header, as it goes out. The saved copy keeps the header.
fn without_bcc(raw: &[u8]) -> Vec<u8> {
    let split = raw.windows(4).position(|window| window == b"\r\n\r\n").map_or(raw.len(), |index| index + 2);
    let (head, body) = raw.split_at(split);
    let mut out = Vec::with_capacity(raw.len());
    let mut dropping = false;
    for line in head.split_inclusive(|byte| *byte == b'\n') {
        let continued = line.first().is_some_and(|byte| *byte == b' ' || *byte == b'\t');
        if !continued {
            dropping = line.len() >= 4 && line[..4].eq_ignore_ascii_case(b"bcc:");
        }
        if !dropping {
            out.extend_from_slice(line);
        }
    }
    out.extend_from_slice(body);
    out
}

fn smtp_send(server: &Server, email: &str, password: &str, from: &str, recipients: &[String], raw: &[u8]) -> Result<(), String> {
    use lettre::address::Envelope;
    use lettre::transport::smtp::authentication::Credentials;
    use lettre::transport::smtp::extension::ClientId;
    use lettre::{SmtpTransport, Transport};

    let parse = |address: &str| {
        address
            .parse::<lettre::Address>()
            .map_err(|_| format!("Invalid email address: {address}"))
    };
    let to = recipients.iter().map(|address| parse(address)).collect::<Result<Vec<_>, _>>()?;
    let envelope = Envelope::new(Some(parse(from)?), to).map_err(|e| e.to_string())?;
    let refused = |err: &dyn std::error::Error| {
        let mut detail = err.to_string();
        let mut source = err.source();
        while let Some(inner) = source {
            detail = format!("{detail}: {inner}");
            source = inner.source();
        }
        format!("Could not send through {}. {detail}", server.name)
    };
    let mailer = SmtpTransport::relay(server.smtp_host)
        .map_err(|e| refused(&e))?
        .port(server.smtp_port)
        .hello_name(ClientId::Domain("novabox.local".into()))
        .credentials(Credentials::new(email.to_string(), password.to_string()))
        .timeout(Some(IO_TIMEOUT))
        .build();
    mailer.send_raw(&envelope, &without_bcc(raw)).map_err(|e| refused(&e))?;
    Ok(())
}

/// Sends a message, then files a copy in Sent and removes the draft it came from. Once the
/// message has gone out this reports success even if the filing failed, so that nobody
/// sends it a second time; the ids are then empty.
pub async fn send(
    app: &AppHandle,
    account_id: &str,
    raw: Vec<u8>,
    from: String,
    recipients: Vec<String>,
    draft_id: Option<String>,
) -> Result<Filed, String> {
    let account = with_db(app, |conn| db::get_account(conn, account_id))?;
    let server = server(&account.provider).ok_or("This account's mail service is not supported.")?;
    let raw = std::sync::Arc::new(raw);
    let outgoing = raw.clone();
    let (email, id) = (account.email.clone(), account.id.clone());
    tauri::async_runtime::spawn_blocking(move || {
        let password = secrets::load(&id)?;
        smtp_send(server, &email, &password, &from, &recipients, &outgoing)
    })
    .await
    .map_err(|e| e.to_string())??;
    let filed = run(app, account_id, move |app, account, conn| {
        let folders = list_folders(conn)?;
        let sent = ensure_folder(conn, &folders, Role::Sent)?;
        conn.session
            .append_with_flags(appendable(&sent)?, raw.as_slice(), &[Flag::Seen])
            .map_err(fail)?;
        if let Some(draft_id) = &draft_id {
            if let Err(err) = remove_message(conn, &folders, draft_id) {
                eprintln!("Novabox draft cleanup: {err}");
            }
        }
        let mut budget = FETCH_COUNT;
        sync_folder(app, account, conn, &sent, &mut budget, "Updating inbox")?;
        let header = headers_of(&raw).message_id;
        let stored = with_db(app, |db| db::message_by_header(db, &account.id, &header))?;
        Ok(stored
            .map(|(message_id, thread_id)| Filed { message_id, thread_id })
            .unwrap_or_default())
    })
    .await;
    Ok(filed.unwrap_or_else(|err| {
        eprintln!("Novabox sent copy: {err}");
        Filed::default()
    }))
}

/// Saves a draft to the Drafts folder, replacing `previous`. A draft's id is its message id.
pub async fn save_draft(
    app: &AppHandle,
    account_id: &str,
    raw: Vec<u8>,
    previous: Option<String>,
    thread_id: Option<String>,
) -> Result<Filed, String> {
    run(app, account_id, move |app, account, conn| {
        let folders = list_folders(conn)?;
        let drafts = ensure_folder(conn, &folders, Role::Drafts)?;
        let headers = headers_of(&raw);
        if headers.message_id.is_empty() {
            return Err("Novabox could not prepare that draft.".into());
        }
        // Saving twice would leave two copies, and this may run again on a new connection.
        let search = format!("HEADER Message-ID {}", quoted(&headers.message_id));
        let mut mailbox = conn.session.select(&drafts.name).map_err(fail)?;
        let mut found = conn.session.uid_search(&search).map_err(fail)?;
        if found.is_empty() {
            conn.session
                .append_with_flags(appendable(&drafts)?, raw.as_slice(), &[Flag::Draft, Flag::Seen])
                .map_err(fail)?;
            mailbox = conn.session.select(&drafts.name).map_err(fail)?;
            found = conn.session.uid_search(&search).map_err(fail)?;
        }
        let uid = found.into_iter().max().ok_or("The mail server did not keep the draft.")?;
        let id = message_id(&drafts, mailbox.uid_validity.unwrap_or(0), uid);
        if let Some(previous) = previous.as_deref().filter(|previous| *previous != id) {
            if let Err(err) = remove_message(conn, &folders, previous) {
                eprintln!("Novabox draft cleanup: {err}");
            }
        }
        let thread_id = match thread_id.clone().filter(|thread_id| !thread_id.is_empty()) {
            Some(thread_id) => thread_id,
            None => with_db(app, |db| thread_for(db, &account.id, &headers, &id))?,
        };
        Ok(Filed { message_id: id, thread_id })
    })
    .await
}

pub async fn delete_draft(app: &AppHandle, account_id: &str, draft_id: &str) -> Result<(), String> {
    let draft_id = draft_id.to_string();
    run(app, account_id, move |_, _, conn| {
        let folders = list_folders(conn)?;
        remove_message(conn, &folders, &draft_id)
    })
    .await
}

// ---------------------------------------------------------------------------------------
// Search and attachments
// ---------------------------------------------------------------------------------------

fn quoted(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Searches the mailbox on the server, stores what it finds, and returns the message ids.
/// This reaches mail that has not downloaded yet; stored mail is searched on this PC.
pub async fn search(app: &AppHandle, account_id: &str, query: &str) -> Result<Vec<String>, String> {
    // A search string outside ASCII needs a different command form; search what is stored instead.
    if !query.is_ascii() || query.chars().any(|ch| ch.is_ascii_control()) {
        return Ok(Vec::new());
    }
    let criteria = format!("TEXT {}", quoted(query));
    run(app, account_id, move |app, account, conn| {
        let mut ids = Vec::new();
        for folder in list_folders(conn)? {
            if !matches!(folder.role, Role::Inbox | Role::Archive | Role::Sent | Role::Other) {
                continue;
            }
            let mailbox = conn.session.examine(&folder.name).map_err(fail)?;
            if mailbox.exists == 0 {
                continue;
            }
            let validity = mailbox.uid_validity.unwrap_or(0);
            let mut uids: Vec<u32> = conn.session.uid_search(&criteria).map_err(fail)?.into_iter().collect();
            uids.sort_unstable_by(|a, b| b.cmp(a));
            uids.truncate(FETCH_COUNT);
            let missing = with_db(app, |db| {
                let mut missing = Vec::new();
                for uid in &uids {
                    if !db::has_message(db, &account.id, &message_id(&folder, validity, *uid))? {
                        missing.push(*uid);
                    }
                }
                Ok(missing)
            })?;
            if !missing.is_empty() {
                download(app, account, conn, &folder, validity, &missing, &HashMap::new(), "Searching")?;
            }
            ids.extend(uids.iter().map(|uid| message_id(&folder, validity, *uid)));
        }
        Ok(ids)
    })
    .await
}

/// Downloads one attachment. The whole message is read again and the file taken from it.
pub async fn attachment(app: &AppHandle, account_id: &str, message_id: &str, attachment_id: &str) -> Result<Vec<u8>, String> {
    let (message_id, attachment_id) = (message_id.to_string(), attachment_id.to_string());
    run(app, account_id, move |_, _, conn| {
        let gone = || "That message has moved or been deleted. Sync and open it again.".to_string();
        let (key, validity, uid) = parse_id(&message_id).ok_or_else(gone)?;
        let index: usize = attachment_id
            .strip_prefix("att-")
            .and_then(|index| index.parse().ok())
            .ok_or("That attachment is not saved on this PC.")?;
        let folders = list_folders(conn)?;
        let folder = folders.iter().find(|folder| folder.key == key).ok_or_else(gone)?;
        let mailbox = conn.session.examine(&folder.name).map_err(fail)?;
        if mailbox.uid_validity.unwrap_or(0) != validity {
            return Err(gone());
        }
        let rows = conn.session.uid_fetch(uid.to_string(), "(UID BODY.PEEK[])").map_err(fail)?;
        let raw = rows.iter().find_map(|row| row.body()).ok_or_else(gone)?;
        let message = MessageParser::default()
            .parse(raw)
            .ok_or("Novabox could not read that message.")?;
        let part = message.attachment(index).ok_or("That attachment is no longer in the message.")?;
        Ok(part.contents().to_vec())
    })
    .await
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listed(name: &str, attributes: &[&str]) -> Listed {
        Listed {
            name: name.into(),
            attributes: attributes.iter().map(|item| item.to_string()).collect(),
            delimiter: Some("/".into()),
        }
    }

    fn labels(add: &[&str], remove: &[&str]) -> Change {
        Change::Labels {
            add: add.iter().map(|item| item.to_string()).collect(),
            remove: remove.iter().map(|item| item.to_string()).collect(),
        }
    }

    #[test]
    fn folders_get_roles_from_markers_then_names() {
        let folders = assign_roles(vec![
            listed("Trash", &[]),
            listed("Clients/Acme", &["\\HasNoChildren"]),
            listed("Sent Messages", &["\\Sent"]),
            listed("Sent", &[]),
            listed("Junk", &[]),
            listed("INBOX", &[]),
            listed("Everything", &["\\All"]),
            listed("Drafts", &["\\Drafts"]),
        ]);
        let role = |name: &str| folders.iter().find(|folder| folder.name == name).map(|folder| folder.role);
        assert_eq!(folders[0].role, Role::Inbox);
        assert_eq!(role("Sent Messages"), Some(Role::Sent));
        // The server marked another folder as Sent, so this one is an ordinary folder.
        assert_eq!(role("Sent"), Some(Role::Other));
        assert_eq!(role("Junk"), Some(Role::Spam));
        assert_eq!(role("Trash"), Some(Role::Trash));
        assert_eq!(role("Drafts"), Some(Role::Drafts));
        assert_eq!(role("Clients/Acme"), Some(Role::Other));
        assert_eq!(role("Everything"), None);
        assert_eq!(folders.last().map(|folder| folder.role), Some(Role::Trash));
    }

    #[test]
    fn folder_labels_follow_flags() {
        let inbox = Folder::new("INBOX", Role::Inbox, None);
        assert_eq!(inbox.labels(false, true), vec!["INBOX", "UNREAD", "STARRED"]);
        assert!(Folder::new("Archive", Role::Archive, None).labels(true, false).is_empty());
        let other = Folder::new("Clients", Role::Other, None);
        let label = other.label().unwrap();
        assert!(crate::util::validate_label(&label).is_ok());
        assert_eq!(other.labels(true, false), vec![label]);
    }

    #[test]
    fn message_ids_round_trip() {
        let folder = Folder::new("INBOX", Role::Inbox, None);
        let id = message_id(&folder, 1_700_000_000, 42);
        assert_eq!(parse_id(&id), Some((folder.key.as_str(), 1_700_000_000, 42)));
        assert_eq!(parse_id("19f2a8c01b3d5e77"), None);
        assert_eq!(crate::util::safe_id(&id), id);
    }

    #[test]
    fn decodes_folder_names() {
        assert_eq!(decode_utf7("Entw&APw-rfe"), "Entwürfe");
        assert_eq!(decode_utf7("Tom &- Jerry"), "Tom & Jerry");
        assert_eq!(decode_utf7("&ZeVnLIqe-"), "日本語");
        assert_eq!(decode_utf7("Plain"), "Plain");
        assert_eq!(decode_utf7("R&D"), "R&D");
    }

    #[test]
    fn changes_become_moves() {
        let inbox = Folder::new("INBOX", Role::Inbox, None);
        let sent = Folder::new("Sent", Role::Sent, None);
        let trash = Folder::new("Trash", Role::Trash, None);
        let spam = Folder::new("Spam", Role::Spam, None);
        let archive = Folder::new("Archive", Role::Archive, None);
        let clients = Folder::new("Clients", Role::Other, None);
        let clients_label = clients.label().unwrap();

        let archiving = planned_move(&labels(&[], &["INBOX"])).unwrap();
        assert_eq!(archiving, Move::Archive);
        assert!(leaves(&archiving, &inbox) && !leaves(&archiving, &sent) && !leaves(&archiving, &clients));

        let trashing = planned_move(&Change::Trash).unwrap();
        assert!(leaves(&trashing, &inbox) && leaves(&trashing, &sent) && !leaves(&trashing, &trash));

        let restoring = planned_move(&Change::Untrash).unwrap();
        assert!(leaves(&restoring, &trash) && !leaves(&restoring, &inbox));

        let not_spam = planned_move(&labels(&["INBOX"], &["SPAM"])).unwrap();
        assert!(leaves(&not_spam, &spam) && leaves(&not_spam, &archive) && !leaves(&not_spam, &trash));

        let filing = planned_move(&labels(&[&clients_label], &[])).unwrap();
        assert!(leaves(&filing, &inbox) && !leaves(&filing, &clients) && !leaves(&filing, &sent));

        // Read and starred state are flags; nothing moves.
        assert_eq!(planned_move(&labels(&[], &["UNREAD"])), None);
        assert_eq!(flag_commands(&labels(&["STARRED"], &["UNREAD"])), vec!["+FLAGS.SILENT (\\Seen)", "+FLAGS.SILENT (\\Flagged)"]);
    }

    #[test]
    fn bcc_is_left_out_of_what_is_sent() {
        let raw = b"From: a@b.co\r\nBcc: hidden@b.co,\r\n other@b.co\r\nSubject: Hi\r\n\r\nBcc: stays in the body\r\n";
        let sent = String::from_utf8(without_bcc(raw)).unwrap();
        assert_eq!(sent, "From: a@b.co\r\nSubject: Hi\r\n\r\nBcc: stays in the body\r\n");
    }

    #[test]
    fn reads_a_message_into_a_conversation() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        db::migrate(&conn).unwrap();
        let first = b"From: Ada Lovelace <ada@harbor.test>\r\nTo: maya@northmail.test\r\nSubject: Proof\r\nMessage-ID: <one@harbor.test>\r\nContent-Type: text/html; charset=utf-8\r\n\r\n<p>Hello <b>there</b></p>\r\n";
        let reply = b"From: maya@northmail.test\r\nTo: ada@harbor.test\r\nSubject: Re: Proof\r\nMessage-ID: <two@northmail.test>\r\nIn-Reply-To: <one@harbor.test>\r\nReferences: <one@harbor.test>\r\n\r\nLooks right.\r\n";
        // The reply downloads first, as the newest mail does.
        let mut later = parse_message("acct", "k-1-2", reply, vec!["SENT".into()], 2_000, false).unwrap();
        later.message.thread_id = thread_for(&conn, "acct", &later.headers, "k-1-2").unwrap();
        db::upsert_message(&conn, &later.message).unwrap();
        let mut earlier = parse_message("acct", "k-1-1", first, vec!["INBOX".into(), "UNREAD".into()], 1_000, false).unwrap();
        earlier.message.thread_id = thread_for(&conn, "acct", &earlier.headers, "k-1-1").unwrap();
        assert_eq!(earlier.message.thread_id, later.message.thread_id);
        assert_eq!(earlier.message.from_name, "Ada Lovelace");
        assert_eq!(earlier.message.from_email, "ada@harbor.test");
        assert_eq!(earlier.message.message_id_header, "<one@harbor.test>");
        assert!(earlier.message.body_html.contains("<b>there</b>"));
        assert!(earlier.message.body_text.contains("Hello"));
        assert_eq!(later.message.references_header, "<one@harbor.test>");
        assert_eq!(later.message.date, 2_000);
        // The same message seen again in another folder keeps its conversation.
        db::upsert_message(&conn, &earlier.message).unwrap();
        let moved = parse_message("acct", "z-9-9", first, vec![], 1_000, false).unwrap();
        assert_eq!(thread_for(&conn, "acct", &moved.headers, "z-9-9").unwrap(), earlier.message.thread_id);
    }

    #[test]
    fn lists_attachments_and_inlines_small_pictures() {
        let raw = concat!(
            "From: a@b.co\r\nSubject: Files\r\nMIME-Version: 1.0\r\n",
            "Content-Type: multipart/mixed; boundary=\"x\"\r\n\r\n",
            "--x\r\nContent-Type: text/html\r\n\r\n<p>Logo <img src=\"cid:logo@b.co\"></p>\r\n",
            "--x\r\nContent-Type: image/png\r\nContent-ID: <logo@b.co>\r\nContent-Transfer-Encoding: base64\r\n\r\niVBORw0KGgo=\r\n",
            "--x\r\nContent-Type: application/pdf; name=\"plan.pdf\"\r\nContent-Disposition: attachment; filename=\"plan.pdf\"\r\nContent-Transfer-Encoding: base64\r\n\r\nJVBERi0xLjQ=\r\n",
            "--x--\r\n"
        );
        let parsed = parse_message("acct", "k-1-3", raw.as_bytes(), vec![], 0, true).unwrap();
        assert!(parsed.message.body_html.contains("src=\"data:image/png;base64,iVBORw0KGgo=\""));
        assert_eq!(parsed.message.attachments.len(), 1);
        assert_eq!(parsed.message.attachments[0].filename, "plan.pdf");
        assert_eq!(parsed.message.attachments[0].id, "att-1");
        assert_eq!(parsed.files[0].1, b"%PDF-1.4");
        assert_eq!(parsed.message.draft_id.as_deref(), Some("k-1-3"));
    }
}
