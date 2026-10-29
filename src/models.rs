use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AccountKind {
    Local,
    Imap,
    Graph,
}

impl AccountKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            AccountKind::Local => "local",
            AccountKind::Imap => "imap",
            AccountKind::Graph => "graph",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "imap" => AccountKind::Imap,
            "graph" => AccountKind::Graph,
            _ => AccountKind::Local,
        }
    }
}

/// Account as exposed to the UI (never contains secrets).
#[derive(Debug, Clone, Serialize)]
pub struct Account {
    pub id: i64,
    pub kind: AccountKind,
    pub name: String,
    pub email: String,
    pub last_sync: Option<String>,
    pub last_error: Option<String>,
}

/// Connection settings for an IMAP/SMTP account. Stored as JSON in `accounts.config`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImapConfig {
    pub imap_host: String,
    pub imap_port: u16,
    pub smtp_host: String,
    pub smtp_port: u16,
    pub username: String,
    pub password: String,
}

/// OAuth tokens for a Microsoft Graph account. Stored as JSON in `accounts.config`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphConfig {
    pub access_token: String,
    pub refresh_token: String,
    /// Unix timestamp (seconds) when `access_token` expires.
    pub expires_at: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Folder {
    pub id: i64,
    pub account_id: i64,
    pub remote_id: String,
    pub name: String,
    /// inbox | sent | drafts | trash | junk | archive | other
    pub role: String,
    pub unread: i64,
    pub total: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct MessageSummary {
    pub id: i64,
    pub account_id: i64,
    pub folder_id: i64,
    pub subject: String,
    pub from_name: String,
    pub from_addr: String,
    pub date: String,
    pub preview: String,
    pub is_read: bool,
    pub is_flagged: bool,
    pub is_pinned: bool,
    pub has_attachments: bool,
    pub importance: String,
    pub categories: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Message {
    #[serde(flatten)]
    pub summary: MessageSummary,
    pub to: String,
    pub cc: String,
    pub body_html: String,
    pub body_text: String,
}

/// A message as fetched from a provider, before it is stored.
#[derive(Debug, Clone, Default)]
pub struct NewMessage {
    pub remote_id: String,
    pub subject: String,
    pub from_name: String,
    pub from_addr: String,
    pub to: String,
    pub cc: String,
    pub date: String,
    pub preview: String,
    pub body_html: String,
    pub body_text: String,
    pub is_read: bool,
    pub is_flagged: bool,
    pub has_attachments: bool,
    pub importance: String,
    pub attachments: Vec<NewAttachment>,
}

/// File attached to a message (incoming or outgoing).
#[derive(Debug, Clone, Default)]
pub struct NewAttachment {
    pub name: String,
    pub mime: String,
    pub data: Vec<u8>,
}

/// Stored attachment without its content.
#[derive(Debug, Clone)]
pub struct Attachment {
    pub id: i64,
    pub message_id: i64,
    pub name: String,
    pub mime: String,
    pub size: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Event {
    pub id: i64,
    pub account_id: i64,
    pub remote_id: String,
    pub title: String,
    /// RFC 3339 (UTC)
    pub start: String,
    /// RFC 3339 (UTC)
    pub end: String,
    pub all_day: bool,
    pub location: String,
    pub body: String,
    pub attendees: String,
    /// Outlook "show as": free | tentative | busy | oof
    pub show_as: String,
    pub category: String,
    /// Minutes before start, -1 = none
    pub reminder: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Contact {
    pub id: i64,
    pub account_id: i64,
    pub remote_id: String,
    pub display_name: String,
    pub email: String,
    pub phone: String,
    pub company: String,
    pub job_title: String,
    pub address: String,
    pub notes: String,
    pub favorite: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Task {
    pub id: i64,
    pub account_id: i64,
    pub remote_id: String,
    /// Task list name (e.g. "Aufgaben" or a Microsoft To Do list)
    pub list_name: String,
    /// Remote list id (Graph To Do list id); empty for local/IMAP
    pub list_remote_id: String,
    pub title: String,
    /// YYYY-MM-DD or empty
    pub due: String,
    pub completed: bool,
    pub important: bool,
    pub my_day: bool,
    pub notes: String,
}
