use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendAs {
    pub email: String,
    pub display_name: String,
    pub is_default: bool,
    pub is_primary: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: String,
    pub email: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub picture: Option<String>,
    pub color: String,
    pub unread: i64,
    pub signature: String,
    pub send_as: Vec<SendAs>,
    #[serde(default, skip_serializing)]
    pub history_id: Option<String>,
    #[serde(default, skip_serializing)]
    pub recent_page_token: Option<String>,
    #[serde(default, skip_serializing)]
    pub backfill_token: Option<String>,
    #[serde(default, skip_serializing)]
    pub backfill_done: bool,
    #[serde(default, skip_serializing)]
    pub initial_done: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Label {
    pub account_id: String,
    pub id: String,
    pub name: String,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    pub unread: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Address {
    pub name: String,
    pub email: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Template {
    pub id: String,
    pub name: String,
    pub subject: String,
    pub html: String,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentMeta {
    pub id: String,
    pub filename: String,
    pub mime_type: String,
    pub size: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadSummary {
    pub account_id: String,
    pub account_email: String,
    pub account_color: String,
    pub thread_id: String,
    pub latest_message_id: String,
    pub from_name: String,
    pub from_email: String,
    pub subject: String,
    pub snippet: String,
    pub date: i64,
    pub unread: bool,
    pub starred: bool,
    pub has_attachments: bool,
    pub message_count: i64,
    pub label_ids: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snooze_until: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scheduled_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body_html: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview_to: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageDetail {
    pub account_id: String,
    pub id: String,
    pub thread_id: String,
    pub from_name: String,
    pub from_email: String,
    pub to: Vec<Address>,
    pub cc: Vec<Address>,
    pub bcc: Vec<Address>,
    pub subject: String,
    pub snippet: String,
    pub body_html: String,
    pub body_text: String,
    pub date: i64,
    pub unread: bool,
    pub starred: bool,
    pub label_ids: Vec<String>,
    pub attachments: Vec<AttachmentMeta>,
    pub message_id_header: String,
    pub references_header: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub draft_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadDetail {
    pub account_id: String,
    pub account_email: String,
    pub account_color: String,
    pub thread_id: String,
    pub subject: String,
    pub messages: Vec<MessageDetail>,
}

/// Which Gmail inbox tabs are turned on. A category whose tab is off stays in Primary.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InboxTabs {
    pub promotions: bool,
    pub social: bool,
    pub updates: bool,
}

impl Default for InboxTabs {
    fn default() -> Self {
        Self {
            promotions: true,
            social: true,
            updates: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub theme: String,
    pub density: String,
    pub reading_pane: String,
    pub start_with_windows: bool,
    pub image_allowlist: Vec<String>,
    pub inbox_tabs: InboxTabs,
    pub notify_in_app: bool,
    pub notify_desktop: bool,
    pub notify_desktop_background_only: bool,
    /// "windows" for native toasts, "novabox" for Novabox's own corner pop-ups.
    pub notify_style: String,
    pub popup_corner: String,
    pub popup_max: u32,
    pub popup_preview: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: "system".into(),
            density: "default".into(),
            reading_pane: "none".into(),
            start_with_windows: false,
            image_allowlist: Vec::new(),
            inbox_tabs: InboxTabs::default(),
            notify_in_app: true,
            notify_desktop: true,
            notify_desktop_background_only: true,
            notify_style: "windows".into(),
            popup_corner: "bottomRight".into(),
            popup_max: 3,
            popup_preview: true,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderCounts {
    pub inbox: i64,
    pub inbox_unread: i64,
    pub starred: i64,
    pub drafts: i64,
    pub primary_unread: i64,
    pub promotions_unread: i64,
    pub social_unread: i64,
    pub updates_unread: i64,
    pub spam_unread: i64,
    pub snoozed: i64,
    pub scheduled: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Mailbox {
    pub accounts: Vec<Account>,
    pub labels: Vec<Label>,
    pub threads: Vec<ThreadSummary>,
    pub has_more: bool,
    pub total: usize,
    pub counts: FolderCounts,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadQuery {
    #[serde(default)]
    pub account_id: Option<String>,
    pub folder: String,
    #[serde(default)]
    pub label_id: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub search: Option<String>,
    pub limit: i64,
    pub offset: i64,
    /// Missing on older clients. Defaults match Gmail: Promotions and Social on, Updates off.
    #[serde(default)]
    pub inbox_tabs: InboxTabs,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadTarget {
    pub account_id: String,
    pub thread_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModifyBatch {
    pub targets: Vec<ThreadTarget>,
    pub action: String,
    #[serde(default)]
    pub label_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutgoingAttachment {
    pub filename: String,
    pub mime_type: String,
    pub data_base64: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComposeRequest {
    pub account_id: String,
    pub from_email: String,
    #[serde(default)]
    pub to: String,
    #[serde(default)]
    pub cc: String,
    #[serde(default)]
    pub bcc: String,
    #[serde(default)]
    pub subject: String,
    #[serde(default)]
    pub html: String,
    #[serde(default)]
    pub attachments: Vec<OutgoingAttachment>,
    #[serde(default)]
    pub thread_id: Option<String>,
    #[serde(default)]
    pub in_reply_to: Option<String>,
    #[serde(default)]
    pub references: Option<String>,
    #[serde(default)]
    pub draft_id: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SendResult {
    pub message_id: String,
    pub thread_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub draft_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignatureUpdate {
    pub account_id: String,
    pub signature: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentRequest {
    pub account_id: String,
    pub message_id: String,
    pub attachment_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveAttachmentRequest {
    pub account_id: String,
    pub message_id: String,
    pub attachment_id: String,
    pub path: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentData {
    pub filename: String,
    pub mime_type: String,
    pub data_base64: String,
}

#[derive(Debug, Clone)]
pub struct StoredMessage {
    pub account_id: String,
    pub id: String,
    pub thread_id: String,
    pub from_name: String,
    pub from_email: String,
    pub to: Vec<Address>,
    pub cc: Vec<Address>,
    pub bcc: Vec<Address>,
    pub subject: String,
    pub snippet: String,
    pub body_html: String,
    pub body_text: String,
    pub date: i64,
    pub label_ids: Vec<String>,
    pub attachments: Vec<AttachmentMeta>,
    pub message_id_header: String,
    pub references_header: String,
    pub draft_id: Option<String>,
    pub size_estimate: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncProgress {
    pub email: String,
    pub phase: String,
    pub fetched: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub version: String,
    pub data_dir: String,
    pub redirect_uri: String,
    pub google_ready: bool,
}
