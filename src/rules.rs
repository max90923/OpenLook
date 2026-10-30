//! Client-side mail rules ("Regeln"), applied to new messages in the inbox
//! after every send/receive – like Outlook's client-only rules.

use anyhow::Result;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::{
    db,
    models::*,
    providers::{self, MessagePatch},
    state::AppState,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Field {
    #[default]
    From,
    Subject,
    To,
    Body,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    #[default]
    Move,
    MarkRead,
    Flag,
    Category,
    Delete,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(default)]
pub struct Rule {
    pub name: String,
    pub enabled: bool,
    /// None = all accounts.
    pub account_id: Option<i64>,
    pub field: Field,
    pub contains: String,
    pub action: Action,
    pub folder_id: Option<i64>,
    pub category: String,
}

impl Field {
    pub fn label(self) -> &'static str {
        match self {
            Field::From => "Absender enthält",
            Field::Subject => "Betreff enthält",
            Field::To => "Empfänger enthält",
            Field::Body => "Text enthält",
        }
    }
}

impl Action {
    pub fn label(self) -> &'static str {
        match self {
            Action::Move => "in Ordner verschieben",
            Action::MarkRead => "als gelesen markieren",
            Action::Flag => "zur Nachverfolgung kennzeichnen",
            Action::Category => "Kategorie zuweisen",
            Action::Delete => "löschen",
        }
    }
}

const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS rule_log (
    message_id INTEGER PRIMARY KEY REFERENCES messages(id) ON DELETE CASCADE
)";

pub fn load(c: &Connection) -> Vec<Rule> {
    db::get_settings(c)
        .ok()
        .and_then(|s| s.get("rules").cloned())
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default()
}

pub fn save(c: &Connection, rules: &[Rule]) -> Result<()> {
    db::set_setting(c, "rules", &serde_json::to_value(rules)?)
}

/// Conversations the user chose to ignore ("Ignorieren").
pub fn ignored(c: &Connection) -> Vec<String> {
    db::get_settings(c)
        .ok()
        .and_then(|s| s.get("ignored_conversations").cloned())
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default()
}

pub fn ignore(c: &Connection, subject: &str) -> Result<()> {
    let mut list = ignored(c);
    let key = crate::conversation::key(subject);
    if !list.contains(&key) {
        list.push(key);
    }
    db::set_setting(c, "ignored_conversations", &serde_json::to_value(list)?)
}

pub fn matches(rule: &Rule, m: &Message) -> bool {
    let needle = rule.contains.trim().to_lowercase();
    if !rule.enabled
        || needle.is_empty()
        || rule.account_id.is_some_and(|a| a != m.summary.account_id)
    {
        return false;
    }
    let hay = match rule.field {
        Field::From => format!("{} {}", m.summary.from_name, m.summary.from_addr),
        Field::Subject => m.summary.subject.clone(),
        Field::To => format!("{} {}", m.to, m.cc),
        Field::Body => m.body_text.clone(),
    };
    hay.to_lowercase().contains(&needle)
}

/// Apply the rules to inbox messages of an account. With `only_new`,
/// messages handled before are skipped. Returns the number of actions.
pub async fn apply(state: &AppState, account_id: i64, only_new: bool) -> Result<usize> {
    let (rules, ignored, messages) = {
        let c = state.db();
        c.execute(SCHEMA, [])?;
        let rules = load(&c);
        let ignored = ignored(&c);
        let Some(inbox) = db::folder_by_role(&c, account_id, "inbox")? else {
            return Ok(0);
        };
        let list = db::list_messages(
            &c,
            &db::MessageQuery {
                folder_id: Some(inbox.id),
                limit: 100_000,
                ..Default::default()
            },
        )?;
        let mut messages = vec![];
        for m in list {
            let seen: bool = c.query_row(
                "SELECT EXISTS(SELECT 1 FROM rule_log WHERE message_id = ?)",
                [m.id],
                |r| r.get(0),
            )?;
            if !(only_new && seen) {
                if let Some(full) = db::get_message(&c, m.id)? {
                    messages.push(full);
                }
            }
        }
        (rules, ignored, messages)
    };
    let mut done = 0;
    for m in messages {
        let id = m.summary.id;
        if ignored.contains(&crate::conversation::key(&m.summary.subject)) {
            providers::delete_message(state, id).await?;
            done += 1;
            continue;
        }
        // Outlook stops at the first matching rule.
        if let Some(rule) = rules.iter().find(|r| matches(r, &m)) {
            match rule.action {
                Action::Move => {
                    if let Some(f) = rule.folder_id {
                        providers::move_message(state, id, f).await?;
                    }
                }
                Action::MarkRead => {
                    providers::patch_message(
                        state,
                        id,
                        &MessagePatch {
                            is_read: Some(true),
                            ..Default::default()
                        },
                    )
                    .await?
                }
                Action::Flag => {
                    providers::patch_message(
                        state,
                        id,
                        &MessagePatch {
                            is_flagged: Some(true),
                            ..Default::default()
                        },
                    )
                    .await?
                }
                Action::Category => {
                    let mut cats = m.summary.categories.clone();
                    if !rule.category.is_empty() && !cats.contains(&rule.category) {
                        cats.push(rule.category.clone());
                    }
                    providers::patch_message(
                        state,
                        id,
                        &MessagePatch {
                            categories: Some(cats),
                            ..Default::default()
                        },
                    )
                    .await?;
                }
                Action::Delete => providers::delete_message(state, id).await?,
            }
            done += 1;
        }
        // The message may be gone (deleted) – ignore the foreign key then.
        let _ = state.db().execute(
            "INSERT OR IGNORE INTO rule_log (message_id) VALUES (?)",
            params![id],
        );
    }
    Ok(done)
}

/// Remember all current inbox messages as handled (used on an account's
/// first sync, so rules only touch mail that arrives afterwards).
pub fn mark_all_handled(state: &AppState, account_id: i64) -> Result<()> {
    let c = state.db();
    c.execute(SCHEMA, [])?;
    if let Some(inbox) = db::folder_by_role(&c, account_id, "inbox")? {
        c.execute("INSERT OR IGNORE INTO rule_log (message_id) SELECT id FROM messages WHERE folder_id = ?", [inbox.id])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(from: &str, subject: &str) -> Message {
        Message {
            summary: MessageSummary {
                id: 1,
                account_id: 1,
                folder_id: 1,
                subject: subject.into(),
                from_name: from.into(),
                from_addr: String::new(),
                date: String::new(),
                preview: String::new(),
                is_read: false,
                is_flagged: false,
                is_pinned: false,
                has_attachments: false,
                importance: "normal".into(),
                categories: vec![],
            },
            to: String::new(),
            cc: String::new(),
            body_html: String::new(),
            body_text: String::new(),
        }
    }

    #[test]
    fn matching() {
        let r = Rule {
            enabled: true,
            field: Field::Subject,
            contains: "rechnung".into(),
            ..Default::default()
        };
        assert!(matches(&r, &msg("x", "Ihre Rechnung 42")));
        assert!(!matches(&r, &msg("x", "Hallo")));
        assert!(!matches(
            &Rule {
                enabled: false,
                ..r.clone()
            },
            &msg("x", "Rechnung")
        ));
        assert!(!matches(
            &Rule {
                account_id: Some(2),
                ..r
            },
            &msg("x", "Rechnung")
        ));
    }

    #[tokio::test]
    async fn ignored_conversations_go_to_trash() {
        let state = AppState::in_memory();
        crate::seed::ensure_local_account(&state.db()).unwrap();
        let acc = db::list_accounts(&state.db()).unwrap()[0].id;
        ignore(&state.db(), "AW: Re: Urlaubsplanung").unwrap();
        assert_eq!(apply(&state, acc, true).await.unwrap(), 1);
        let trash = db::folder_by_role(&state.db(), acc, "trash")
            .unwrap()
            .unwrap();
        assert_eq!(trash.total, 1);
    }

    #[tokio::test]
    async fn applies_move_rule_once() {
        let state = AppState::in_memory();
        crate::seed::ensure_local_account(&state.db()).unwrap();
        let acc = db::list_accounts(&state.db()).unwrap()[0].id;
        let archive = db::folder_by_role(&state.db(), acc, "archive")
            .unwrap()
            .unwrap();
        save(
            &state.db(),
            &[Rule {
                name: "Rechnungen".into(),
                enabled: true,
                field: Field::Subject,
                contains: "Rechnung".into(),
                action: Action::Move,
                folder_id: Some(archive.id),
                ..Default::default()
            }],
        )
        .unwrap();
        assert_eq!(apply(&state, acc, true).await.unwrap(), 1);
        assert_eq!(
            db::get_folder(&state.db(), archive.id)
                .unwrap()
                .unwrap()
                .total,
            1
        );
        assert_eq!(apply(&state, acc, true).await.unwrap(), 0);
    }
}
