//! Provider dispatch: every mutation is applied to the remote account first
//! (IMAP/SMTP or Microsoft Graph) and then mirrored into the local cache.
//! The built-in local account only touches the cache.

pub mod graph;
pub mod imap;

use anyhow::{anyhow, bail, Result};

use crate::{db, mail, models::*, state::AppState};

#[derive(Debug, Clone, Default)]
pub struct OutgoingMail {
    pub account_id: i64,
    pub to: String,
    pub cc: String,
    pub bcc: String,
    pub subject: String,
    pub body_html: String,
    /// low | normal | high
    pub importance: String,
    pub attachments: Vec<NewAttachment>,
}

impl OutgoingMail {
    pub fn to_lettre(&self, from: &str) -> Result<lettre::Message> {
        use lettre::message::{header, Attachment, Mailbox, MultiPart, SinglePart};
        let mut b = lettre::Message::builder()
            .from(from.parse::<Mailbox>()?)
            .subject(&self.subject);
        for a in split_addresses(&self.to) {
            b = b.to(a.parse()?);
        }
        for a in split_addresses(&self.cc) {
            b = b.cc(a.parse()?);
        }
        for a in split_addresses(&self.bcc) {
            b = b.bcc(a.parse()?);
        }
        if self.importance == "high" || self.importance == "low" {
            b = b.raw_header(header::HeaderValue::new(
                header::HeaderName::new_from_ascii_str("Importance"),
                self.importance.clone(),
            ));
        }
        if self.attachments.is_empty() {
            return Ok(b
                .header(header::ContentType::TEXT_HTML)
                .body(self.body_html.clone())?);
        }
        let mut parts = MultiPart::mixed().singlepart(SinglePart::html(self.body_html.clone()));
        for a in &self.attachments {
            let ct = header::ContentType::parse(&a.mime)
                .unwrap_or(header::ContentType::parse("application/octet-stream")?);
            parts = parts.singlepart(Attachment::new(a.name.clone()).body(a.data.clone(), ct));
        }
        Ok(b.multipart(parts)?)
    }
}

/// Split "a@x.de; Bob <b@x.de>, c@x.de" into individual addresses.
pub fn split_addresses(s: &str) -> Vec<String> {
    s.split([',', ';'])
        .map(|p| {
            let p = p.trim();
            match (p.find('<'), p.rfind('>')) {
                (Some(a), Some(b)) if a < b => p[a + 1..b].trim().to_string(),
                _ => p.to_string(),
            }
        })
        .filter(|p| !p.is_empty())
        .collect()
}

fn account(state: &AppState, id: i64) -> Result<Account> {
    db::get_account(&state.db(), id)?.ok_or_else(|| anyhow!("Konto {id} nicht gefunden"))
}

pub async fn sync(state: &AppState, account_id: i64) -> Result<()> {
    let acc = account(state, account_id)?;
    let res = match acc.kind {
        AccountKind::Local => Ok(()),
        AccountKind::Imap => imap::sync(state, account_id).await,
        AccountKind::Graph => graph::sync(state, account_id).await,
    };
    let err = res.as_ref().err().map(|e| format!("{e:#}"));
    db::set_sync_result(&state.db(), account_id, err.as_deref())?;
    if res.is_ok() && acc.kind != AccountKind::Local {
        // Rules only handle mail that arrives after the first sync.
        let applied = if acc.last_sync.is_none() {
            crate::rules::mark_all_handled(state, account_id)
        } else {
            crate::rules::apply(state, account_id, true)
                .await
                .map(|_| ())
        };
        if let Err(e) = applied {
            tracing::warn!("Regeln für {}: {e:#}", acc.email);
        }
    }
    res
}

pub async fn sync_all(state: &AppState) {
    let accounts = match db::list_accounts(&state.db()) {
        Ok(a) => a,
        Err(e) => return tracing::error!("Konten laden: {e:#}"),
    };
    for a in accounts
        .into_iter()
        .filter(|a| a.kind != AccountKind::Local)
    {
        if let Err(e) = sync(state, a.id).await {
            tracing::warn!("Sync {} fehlgeschlagen: {e:#}", a.email);
        }
    }
}

// ---------------------------------------------------------------- mail

#[derive(Debug, Clone, Default)]
pub struct MessagePatch {
    pub is_read: Option<bool>,
    pub is_flagged: Option<bool>,
    pub is_pinned: Option<bool>,
    pub categories: Option<Vec<String>>,
}

pub async fn patch_message(state: &AppState, id: i64, p: &MessagePatch) -> Result<()> {
    let msg =
        db::get_message(&state.db(), id)?.ok_or_else(|| anyhow!("Nachricht nicht gefunden"))?;
    let acc = account(state, msg.summary.account_id)?;
    match acc.kind {
        AccountKind::Local => {}
        AccountKind::Imap => {
            if p.is_read.is_some() || p.is_flagged.is_some() {
                imap::set_flags(state, acc.id, id, p.is_read, p.is_flagged).await?;
            }
        }
        AccountKind::Graph => {
            let (rid, _) = db::message_remote(&state.db(), id)?;
            graph::patch_message(
                state,
                acc.id,
                &rid,
                p.is_read,
                p.is_flagged,
                p.categories.as_deref(),
            )
            .await?;
        }
    }
    db::update_message_flags(
        &state.db(),
        id,
        p.is_read,
        p.is_flagged,
        p.is_pinned,
        p.categories.as_deref(),
    )
}

pub async fn move_message(state: &AppState, id: i64, folder_id: i64) -> Result<()> {
    let (rid, from_folder, acc, target) = {
        let c = state.db();
        let (rid, from_folder) = db::message_remote(&c, id)?;
        let target = db::get_folder(&c, folder_id)?.ok_or_else(|| anyhow!("Zielordner fehlt"))?;
        let msg = db::get_message(&c, id)?.ok_or_else(|| anyhow!("Nachricht nicht gefunden"))?;
        let acc =
            db::get_account(&c, msg.summary.account_id)?.ok_or_else(|| anyhow!("Konto fehlt"))?;
        (rid, from_folder, acc, target)
    };
    if from_folder == folder_id {
        return Ok(());
    }
    if target.account_id != acc.id {
        bail!("Verschieben zwischen Konten wird nicht unterstützt");
    }
    let new_rid = match acc.kind {
        AccountKind::Local => rid,
        AccountKind::Imap => {
            imap::move_message(state, acc.id, id, &target.remote_id).await?;
            // The new UID is only known after the next sync of the target folder.
            format!("moved-{rid}-{}", mail::now_utc())
        }
        AccountKind::Graph => graph::move_message(state, acc.id, &rid, &target.remote_id).await?,
    };
    db::move_message(&state.db(), id, folder_id, &new_rid)
}

/// Outlook semantics: delete moves to "Deleted Items"; deleting from there is permanent.
pub async fn delete_message(state: &AppState, id: i64) -> Result<()> {
    let (account_id, folder_id) = {
        let c = state.db();
        let m = db::get_message(&c, id)?.ok_or_else(|| anyhow!("Nachricht nicht gefunden"))?;
        (m.summary.account_id, m.summary.folder_id)
    };
    let trash = db::folder_by_role(&state.db(), account_id, "trash")?;
    match trash {
        Some(t) if t.id != folder_id => move_message(state, id, t.id).await,
        _ => {
            let acc = account(state, account_id)?;
            match acc.kind {
                AccountKind::Local => {}
                AccountKind::Imap => imap::delete_permanently(state, acc.id, id).await?,
                AccountKind::Graph => {
                    let (rid, _) = db::message_remote(&state.db(), id)?;
                    graph::delete_message(state, acc.id, &rid).await?;
                }
            }
            db::delete_message(&state.db(), id)
        }
    }
}

pub async fn send(state: &AppState, m: &OutgoingMail) -> Result<()> {
    if split_addresses(&m.to).is_empty() {
        bail!("Mindestens ein Empfänger ist erforderlich");
    }
    let acc = account(state, m.account_id)?;
    match acc.kind {
        AccountKind::Local => {}
        AccountKind::Imap => {
            imap::send(state, acc.id, &format!("{} <{}>", acc.name, acc.email), m).await?
        }
        AccountKind::Graph => graph::send(state, acc.id, m).await?,
    }
    // Mirror into the local "Sent" folder right away; remote accounts replace
    // this copy with the server's on the next sync.
    let sent = db::folder_by_role(&state.db(), acc.id, "sent")?;
    if let Some(sent) = sent {
        let text = mail::strip_html(&m.body_html);
        db::upsert_message(
            &state.db(),
            acc.id,
            sent.id,
            &NewMessage {
                remote_id: format!("local-{}", mail::now_utc()),
                subject: m.subject.clone(),
                from_name: acc.name.clone(),
                from_addr: acc.email.clone(),
                to: m.to.clone(),
                cc: m.cc.clone(),
                date: mail::now_utc(),
                preview: mail::make_preview(&text),
                body_html: m.body_html.clone(),
                body_text: text,
                is_read: true,
                importance: m.importance.clone(),
                has_attachments: !m.attachments.is_empty(),
                attachments: m.attachments.clone(),
                ..Default::default()
            },
        )?;
    }
    Ok(())
}

pub fn save_draft(state: &AppState, m: &OutgoingMail) -> Result<i64> {
    let c = state.db();
    let acc = db::get_account(&c, m.account_id)?.ok_or_else(|| anyhow!("Konto fehlt"))?;
    let drafts =
        db::folder_by_role(&c, acc.id, "drafts")?.ok_or_else(|| anyhow!("Kein Entwürfe-Ordner"))?;
    let text = mail::strip_html(&m.body_html);
    db::upsert_message(
        &c,
        acc.id,
        drafts.id,
        &NewMessage {
            remote_id: format!("draft-{}", mail::now_utc()),
            subject: if m.subject.is_empty() {
                "(kein Betreff)".into()
            } else {
                m.subject.clone()
            },
            from_name: acc.name.clone(),
            from_addr: acc.email.clone(),
            to: m.to.clone(),
            cc: m.cc.clone(),
            date: mail::now_utc(),
            preview: mail::make_preview(&text),
            body_html: m.body_html.clone(),
            body_text: text,
            is_read: true,
            has_attachments: !m.attachments.is_empty(),
            attachments: m.attachments.clone(),
            ..Default::default()
        },
    )
}

/// Make sure the attachments of a message are in the local cache
/// (Microsoft Graph only delivers them on request).
pub async fn ensure_attachments(state: &AppState, message_id: i64) -> Result<()> {
    let (acc, has, cached) = {
        let c = state.db();
        let m =
            db::get_message(&c, message_id)?.ok_or_else(|| anyhow!("Nachricht nicht gefunden"))?;
        (
            m.summary.account_id,
            m.summary.has_attachments,
            !db::list_attachments(&c, message_id)?.is_empty(),
        )
    };
    if !has || cached || kind_of(state, acc)? != AccountKind::Graph {
        return Ok(());
    }
    let (rid, _) = db::message_remote(&state.db(), message_id)?;
    let list = graph::load_attachments(state, acc, &rid).await?;
    db::replace_attachments(&state.db(), message_id, &list)
}

// ---------------------------------------------------------------- folders

pub async fn create_folder(
    state: &AppState,
    account_id: i64,
    parent: Option<i64>,
    name: &str,
) -> Result<i64> {
    let name = name.trim();
    if name.is_empty() {
        bail!("Bitte einen Ordnernamen eingeben");
    }
    let parent_remote = match parent {
        Some(id) => Some(
            db::get_folder(&state.db(), id)?
                .ok_or_else(|| anyhow!("Ordner fehlt"))?
                .remote_id,
        ),
        None => None,
    };
    let remote = match kind_of(state, account_id)? {
        AccountKind::Local => format!("local-{}-{}", name, mail::now_utc()),
        AccountKind::Imap => {
            imap::create_folder(state, account_id, parent_remote.as_deref(), name).await?
        }
        AccountKind::Graph => {
            graph::create_folder(state, account_id, parent_remote.as_deref(), name).await?
        }
    };
    db::upsert_folder(&state.db(), account_id, &remote, name, "other")
}

fn editable_folder(state: &AppState, id: i64) -> Result<Folder> {
    let f = db::get_folder(&state.db(), id)?.ok_or_else(|| anyhow!("Ordner fehlt"))?;
    if f.role != "other" {
        bail!("Standardordner können nicht umbenannt oder gelöscht werden");
    }
    Ok(f)
}

pub async fn rename_folder(state: &AppState, id: i64, name: &str) -> Result<()> {
    let name = name.trim();
    if name.is_empty() {
        bail!("Bitte einen Ordnernamen eingeben");
    }
    let f = editable_folder(state, id)?;
    let remote = match kind_of(state, f.account_id)? {
        AccountKind::Local => f.remote_id.clone(),
        AccountKind::Imap => imap::rename_folder(state, f.account_id, &f.remote_id, name).await?,
        AccountKind::Graph => {
            graph::rename_folder(state, f.account_id, &f.remote_id, name).await?;
            f.remote_id.clone()
        }
    };
    db::rename_folder(&state.db(), id, &remote, name)
}

pub async fn delete_folder(state: &AppState, id: i64) -> Result<()> {
    let f = editable_folder(state, id)?;
    match kind_of(state, f.account_id)? {
        AccountKind::Local => {}
        AccountKind::Imap => imap::delete_folder(state, f.account_id, &f.remote_id).await?,
        AccountKind::Graph => graph::delete_folder(state, f.account_id, &f.remote_id).await?,
    }
    db::delete_folder(&state.db(), id)
}

// ---------------------------------------------------------------- calendar / contacts / tasks

fn kind_of(state: &AppState, account_id: i64) -> Result<AccountKind> {
    Ok(account(state, account_id)?.kind)
}

pub async fn save_event(state: &AppState, mut e: Event) -> Result<i64> {
    if kind_of(state, e.account_id)? == AccountKind::Graph {
        e.remote_id = graph::save_event(state, &e).await?;
    }
    db::save_event(&state.db(), &e)
}

pub async fn delete_event(state: &AppState, id: i64) -> Result<()> {
    let e = db::get_event(&state.db(), id)?.ok_or_else(|| anyhow!("Termin nicht gefunden"))?;
    if kind_of(state, e.account_id)? == AccountKind::Graph && !e.remote_id.is_empty() {
        graph::delete_event(state, e.account_id, &e.remote_id).await?;
    }
    db::delete_event(&state.db(), id)
}

pub async fn save_contact(state: &AppState, mut c: Contact) -> Result<i64> {
    if kind_of(state, c.account_id)? == AccountKind::Graph {
        c.remote_id = graph::save_contact(state, &c).await?;
    }
    db::save_contact(&state.db(), &c)
}

pub async fn delete_contact(state: &AppState, id: i64) -> Result<()> {
    let c = db::get_contact(&state.db(), id)?.ok_or_else(|| anyhow!("Kontakt nicht gefunden"))?;
    if kind_of(state, c.account_id)? == AccountKind::Graph && !c.remote_id.is_empty() {
        graph::delete_contact(state, c.account_id, &c.remote_id).await?;
    }
    db::delete_contact(&state.db(), id)
}

pub async fn save_task(state: &AppState, mut t: Task) -> Result<i64> {
    if kind_of(state, t.account_id)? == AccountKind::Graph {
        let (rid, lid, lname) = graph::save_task(state, &t).await?;
        t.remote_id = rid;
        t.list_remote_id = lid;
        t.list_name = lname;
    }
    if t.list_name.is_empty() {
        t.list_name = "Aufgaben".into();
    }
    db::save_task(&state.db(), &t)
}

pub async fn delete_task(state: &AppState, id: i64) -> Result<()> {
    let t = db::get_task(&state.db(), id)?.ok_or_else(|| anyhow!("Aufgabe nicht gefunden"))?;
    if kind_of(state, t.account_id)? == AccountKind::Graph && !t.remote_id.is_empty() {
        graph::delete_task(state, &t).await?;
    }
    db::delete_task(&state.db(), id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_addresses() {
        assert_eq!(
            split_addresses("a@x.de; Bob <b@x.de>, , c@x.de"),
            vec!["a@x.de", "b@x.de", "c@x.de"]
        );
    }

    #[test]
    fn builds_mime() {
        let m = OutgoingMail {
            to: "bob@example.com".into(),
            subject: "Test".into(),
            body_html: "<p>Hi</p>".into(),
            importance: "high".into(),
            ..Default::default()
        };
        let raw =
            String::from_utf8(m.to_lettre("Me <me@example.com>").unwrap().formatted()).unwrap();
        assert!(raw.contains("Subject: Test"));
        assert!(raw.contains("Importance: high"));
        assert!(raw.contains("<p>Hi</p>"));
    }

    #[tokio::test]
    async fn local_delete_moves_to_trash_then_removes() {
        let state = AppState::in_memory();
        let (inbox, trash, id) = {
            let c = state.db();
            let acc = db::insert_account(&c, AccountKind::Local, "L", "l@x.de", "{}").unwrap();
            let inbox = db::upsert_folder(&c, acc, "inbox", "Posteingang", "inbox").unwrap();
            let trash = db::upsert_folder(&c, acc, "trash", "Gelöschte Elemente", "trash").unwrap();
            let id = db::upsert_message(
                &c,
                acc,
                inbox,
                &NewMessage {
                    remote_id: "1".into(),
                    ..Default::default()
                },
            )
            .unwrap();
            (inbox, trash, id)
        };
        delete_message(&state, id).await.unwrap();
        assert_eq!(
            db::get_message(&state.db(), id)
                .unwrap()
                .unwrap()
                .summary
                .folder_id,
            trash
        );
        assert_ne!(inbox, trash);
        delete_message(&state, id).await.unwrap();
        assert!(db::get_message(&state.db(), id).unwrap().is_none());
    }

    #[tokio::test]
    async fn local_folder_lifecycle() {
        let state = AppState::in_memory();
        crate::seed::ensure_local_account(&state.db()).unwrap();
        let acc = db::list_accounts(&state.db()).unwrap()[0].id;
        let id = create_folder(&state, acc, None, "Projekte").await.unwrap();
        rename_folder(&state, id, "Kunden").await.unwrap();
        assert_eq!(
            db::get_folder(&state.db(), id).unwrap().unwrap().name,
            "Kunden"
        );
        let inbox = db::folder_by_role(&state.db(), acc, "inbox")
            .unwrap()
            .unwrap();
        assert!(delete_folder(&state, inbox.id).await.is_err());
        delete_folder(&state, id).await.unwrap();
        assert!(db::get_folder(&state.db(), id).unwrap().is_none());
        assert!(create_folder(&state, acc, None, "  ").await.is_err());
    }

    #[tokio::test]
    async fn local_send_lands_in_sent() {
        let state = AppState::in_memory();
        crate::seed::ensure_local_account(&state.db()).unwrap();
        let acc = db::list_accounts(&state.db()).unwrap()[0].id;
        let m = OutgoingMail {
            account_id: acc,
            to: "bob@x.de".into(),
            subject: "Hi".into(),
            body_html: "<p>Hallo</p>".into(),
            ..Default::default()
        };
        let m = OutgoingMail {
            attachments: vec![NewAttachment {
                name: "a.txt".into(),
                mime: "text/plain".into(),
                data: b"x".to_vec(),
            }],
            ..m
        };
        send(&state, &m).await.unwrap();
        let sent = db::folder_by_role(&state.db(), acc, "sent")
            .unwrap()
            .unwrap();
        assert_eq!(sent.total, 1);
        let msgs = db::list_messages(
            &state.db(),
            &db::MessageQuery {
                folder_id: Some(sent.id),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            db::list_attachments(&state.db(), msgs[0].id).unwrap().len(),
            1
        );
        // The same mail as MIME carries the attachment.
        let raw = String::from_utf8(m.to_lettre("me@x.de").unwrap().formatted()).unwrap();
        assert!(raw.contains("multipart/mixed") && raw.contains("a.txt"));
        assert!(send(
            &state,
            &OutgoingMail {
                account_id: acc,
                ..Default::default()
            }
        )
        .await
        .is_err());
    }
}
