//! IMAP (implicit TLS) + SMTP provider.

use std::sync::Arc;

use anyhow::{anyhow, bail, Context, Result};
use async_imap::types::{Flag, NameAttribute};
use futures::TryStreamExt;
use lettre::{AsyncSmtpTransport, AsyncTransport, Tokio1Executor};
use tokio::net::TcpStream;
use tokio_rustls::{client::TlsStream, rustls, TlsConnector};

use crate::{db, mail, models::*, providers::OutgoingMail, state::AppState};

/// Number of most recent messages cached per folder.
const WINDOW: u32 = 100;

type Session = async_imap::Session<TlsStream<TcpStream>>;

fn config(state: &AppState, account_id: i64) -> Result<ImapConfig> {
    let raw = crate::secrets::config(state, account_id)?;
    serde_json::from_str(&raw).context("IMAP-Konfiguration ungültig")
}

/// Extra trusted CA certificates (PEM) from `MAILCRAFT_CA_FILE`, e.g. for
/// company mail servers with an internal certificate authority.
fn extra_ca_pem() -> Option<Vec<u8>> {
    std::env::var_os("MAILCRAFT_CA_FILE").and_then(|p| std::fs::read(p).ok())
}

fn roots() -> Result<rustls::RootCertStore> {
    use rustls::pki_types::{pem::PemObject, CertificateDer};
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    if let Some(pem) = extra_ca_pem() {
        for cert in CertificateDer::pem_slice_iter(&pem) {
            roots.add(cert.context("MAILCRAFT_CA_FILE enthält kein gültiges Zertifikat")?)?;
        }
    }
    Ok(roots)
}

async fn connect(cfg: &ImapConfig) -> Result<Session> {
    let roots = roots()?;
    let tls = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    let tcp = TcpStream::connect((cfg.imap_host.as_str(), cfg.imap_port))
        .await
        .with_context(|| {
            format!(
                "Verbindung zu {}:{} fehlgeschlagen",
                cfg.imap_host, cfg.imap_port
            )
        })?;
    let name = rustls::pki_types::ServerName::try_from(cfg.imap_host.clone())?;
    let stream = TlsConnector::from(Arc::new(tls)).connect(name, tcp).await?;
    let mut client = async_imap::Client::new(stream);
    client
        .read_response()
        .await
        .ok_or_else(|| anyhow!("IMAP-Server hat keine Begrüßung gesendet"))??;
    client
        .login(&cfg.username, &cfg.password)
        .await
        .map_err(|(e, _)| anyhow!("IMAP-Anmeldung fehlgeschlagen: {e}"))
}

fn folder_role(name: &str, attrs: &[NameAttribute]) -> &'static str {
    for a in attrs {
        match a {
            NameAttribute::Sent => return "sent",
            NameAttribute::Drafts => return "drafts",
            NameAttribute::Trash => return "trash",
            NameAttribute::Junk => return "junk",
            NameAttribute::Archive => return "archive",
            _ => {}
        }
    }
    let n = name.to_ascii_lowercase();
    let last = n.rsplit(['/', '.']).next().unwrap_or(&n);
    match last {
        "inbox" => "inbox",
        "sent" | "sent items" | "sent messages" | "gesendet" | "gesendete elemente"
        | "gesendete objekte" => "sent",
        "drafts" | "entwürfe" | "entwurf" => "drafts",
        "trash" | "deleted" | "deleted items" | "papierkorb" | "gelöschte elemente" => "trash",
        "junk" | "spam" | "junk-e-mail" | "junk e-mail" => "junk",
        "archive" | "archiv" => "archive",
        _ => "other",
    }
}

fn display_name(remote: &str, role: &str) -> String {
    match role {
        "inbox" => "Posteingang".into(),
        "sent" => "Gesendete Elemente".into(),
        "drafts" => "Entwürfe".into(),
        "trash" => "Gelöschte Elemente".into(),
        "junk" => "Junk-E-Mail".into(),
        "archive" => "Archiv".into(),
        _ => remote.to_string(),
    }
}

pub async fn sync(state: &AppState, account_id: i64) -> Result<()> {
    let cfg = config(state, account_id)?;
    let mut s = connect(&cfg).await?;

    let names: Vec<_> = s.list(None, Some("*")).await?.try_collect().await?;
    let mut keep_folders = vec![];
    for n in &names {
        if n.attributes().contains(&NameAttribute::NoSelect) {
            continue;
        }
        let remote = n.name().to_string();
        let role = folder_role(&remote, n.attributes());
        let folder_id = db::upsert_folder(
            &state.db(),
            account_id,
            &remote,
            &display_name(&remote, role),
            role,
        )?;
        keep_folders.push(folder_id);
        sync_folder(state, &mut s, account_id, folder_id, &remote).await?;
    }
    db::prune_folders(&state.db(), account_id, &keep_folders)?;
    s.logout().await.ok();
    Ok(())
}

async fn sync_folder(
    state: &AppState,
    s: &mut Session,
    account_id: i64,
    folder_id: i64,
    remote: &str,
) -> Result<()> {
    let mbox = s.select(remote).await?;
    if mbox.exists == 0 {
        db::prune_messages(&state.db(), folder_id, &[])?;
        return Ok(());
    }
    let first = mbox.exists.saturating_sub(WINDOW - 1).max(1);
    let flags: Vec<_> = s
        .fetch(format!("{first}:{}", mbox.exists), "(UID FLAGS)")
        .await?
        .try_collect()
        .await?;

    let cached = db::remote_ids_in_folder(&state.db(), folder_id)?;
    let mut keep = vec![];
    let mut missing = vec![];
    for f in &flags {
        let Some(uid) = f.uid else { continue };
        let rid = uid.to_string();
        let seen = f.flags().any(|x| x == Flag::Seen);
        let flagged = f.flags().any(|x| x == Flag::Flagged);
        if cached.contains(&rid) {
            let c = state.db();
            let id: i64 = c.query_row(
                "SELECT id FROM messages WHERE folder_id = ? AND remote_id = ?",
                rusqlite::params![folder_id, rid],
                |r| r.get(0),
            )?;
            db::update_message_flags(&c, id, Some(seen), Some(flagged), None, None)?;
        } else {
            missing.push(uid);
        }
        keep.push(rid);
    }

    for chunk in missing.chunks(20) {
        let set = chunk
            .iter()
            .map(|u| u.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let bodies: Vec<_> = s
            .uid_fetch(set, "(UID FLAGS BODY.PEEK[])")
            .await?
            .try_collect()
            .await?;
        for f in bodies {
            let (Some(uid), Some(body)) = (f.uid, f.body()) else {
                continue;
            };
            let mut m = mail::parse_rfc822(uid.to_string(), body);
            m.is_read = f.flags().any(|x| x == Flag::Seen);
            m.is_flagged = f.flags().any(|x| x == Flag::Flagged);
            db::upsert_message(&state.db(), account_id, folder_id, &m)?;
        }
    }
    db::prune_messages(&state.db(), folder_id, &keep)?;
    Ok(())
}

fn remote_of(state: &AppState, message_id: i64) -> Result<(String, String)> {
    let c = state.db();
    let (uid, folder_id) = db::message_remote(&c, message_id)?;
    let folder = db::get_folder(&c, folder_id)?.ok_or_else(|| anyhow!("Ordner fehlt"))?;
    Ok((uid, folder.remote_id))
}

pub async fn set_flags(
    state: &AppState,
    account_id: i64,
    message_id: i64,
    read: Option<bool>,
    flagged: Option<bool>,
) -> Result<()> {
    let cfg = config(state, account_id)?;
    let (uid, folder) = remote_of(state, message_id)?;
    let mut s = connect(&cfg).await?;
    s.select(&folder).await?;
    for (flag, val) in [("\\Seen", read), ("\\Flagged", flagged)] {
        if let Some(v) = val {
            let q = format!("{}FLAGS ({flag})", if v { "+" } else { "-" });
            let _: Vec<_> = s.uid_store(&uid, q).await?.try_collect().await?;
        }
    }
    s.logout().await.ok();
    Ok(())
}

pub async fn move_message(
    state: &AppState,
    account_id: i64,
    message_id: i64,
    target_remote: &str,
) -> Result<()> {
    let cfg = config(state, account_id)?;
    let (uid, folder) = remote_of(state, message_id)?;
    let mut s = connect(&cfg).await?;
    s.select(&folder).await?;
    if s.uid_mv(&uid, target_remote).await.is_err() {
        // Server without MOVE extension: copy + delete.
        s.uid_copy(&uid, target_remote).await?;
        let _: Vec<_> = s
            .uid_store(&uid, "+FLAGS (\\Deleted)")
            .await?
            .try_collect()
            .await?;
        let _: Vec<_> = s.expunge().await?.try_collect().await?;
    }
    s.logout().await.ok();
    Ok(())
}

pub async fn delete_permanently(state: &AppState, account_id: i64, message_id: i64) -> Result<()> {
    let cfg = config(state, account_id)?;
    let (uid, folder) = remote_of(state, message_id)?;
    let mut s = connect(&cfg).await?;
    s.select(&folder).await?;
    let _: Vec<_> = s
        .uid_store(&uid, "+FLAGS (\\Deleted)")
        .await?
        .try_collect()
        .await?;
    let _: Vec<_> = s.expunge().await?.try_collect().await?;
    s.logout().await.ok();
    Ok(())
}

pub async fn send(
    state: &AppState,
    account_id: i64,
    from: &str,
    mail: &OutgoingMail,
) -> Result<()> {
    let cfg = config(state, account_id)?;
    let msg = mail.to_lettre(from)?;
    let creds = lettre::transport::smtp::authentication::Credentials::new(
        cfg.username.clone(),
        cfg.password.clone(),
    );
    let builder = match extra_ca_pem() {
        Some(pem) => {
            use lettre::transport::smtp::client::{Certificate, Tls, TlsParameters};
            let params = TlsParameters::builder(cfg.smtp_host.clone())
                .add_root_certificate(Certificate::from_pem(&pem)?)
                .build_rustls()?;
            let tls = if cfg.smtp_port == 465 {
                Tls::Wrapper(params)
            } else {
                Tls::Required(params)
            };
            AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&cfg.smtp_host).tls(tls)
        }
        None => match cfg.smtp_port {
            465 => AsyncSmtpTransport::<Tokio1Executor>::relay(&cfg.smtp_host)?,
            _ => AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&cfg.smtp_host)?,
        },
    };
    builder
        .port(cfg.smtp_port)
        .credentials(creds)
        .build()
        .send(msg.clone())
        .await
        .context("SMTP-Versand fehlgeschlagen")?;

    // Keep a copy in "Sent" (many servers do not do this automatically).
    let sent = db::folder_by_role(&state.db(), account_id, "sent")?;
    if let Some(sent) = sent {
        let mut s = connect(&cfg).await?;
        if let Err(e) = s
            .append(&sent.remote_id, Some("(\\Seen)"), None, msg.formatted())
            .await
        {
            tracing::warn!("Kopie in Gesendet fehlgeschlagen: {e}");
        }
        s.logout().await.ok();
    }
    Ok(())
}

/// Hierarchy delimiter of the server ("/" or ".").
async fn delimiter(s: &mut Session) -> Result<String> {
    let names: Vec<_> = s.list(Some(""), Some("\"\"")).await?.try_collect().await?;
    Ok(names
        .first()
        .and_then(|n| n.delimiter())
        .unwrap_or("/")
        .to_string())
}

/// Create a folder (below `parent`, if given); returns its full name.
pub async fn create_folder(
    state: &AppState,
    account_id: i64,
    parent: Option<&str>,
    name: &str,
) -> Result<String> {
    let cfg = config(state, account_id)?;
    let mut s = connect(&cfg).await?;
    let full = match parent {
        Some(p) => format!("{p}{}{name}", delimiter(&mut s).await?),
        None => name.to_string(),
    };
    s.create(&full).await?;
    let _ = s.subscribe(&full).await;
    s.logout().await.ok();
    Ok(full)
}

/// Rename a folder, keeping it below the same parent; returns the new full name.
pub async fn rename_folder(
    state: &AppState,
    account_id: i64,
    remote: &str,
    name: &str,
) -> Result<String> {
    let cfg = config(state, account_id)?;
    let mut s = connect(&cfg).await?;
    let d = delimiter(&mut s).await?;
    let full = match remote.rsplit_once(d.as_str()) {
        Some((parent, _)) => format!("{parent}{d}{name}"),
        None => name.to_string(),
    };
    s.rename(remote, &full).await?;
    let _ = s.subscribe(&full).await;
    s.logout().await.ok();
    Ok(full)
}

pub async fn delete_folder(state: &AppState, account_id: i64, remote: &str) -> Result<()> {
    let cfg = config(state, account_id)?;
    let mut s = connect(&cfg).await?;
    s.delete(remote).await?;
    s.logout().await.ok();
    Ok(())
}

/// Validate settings by logging in once.
pub async fn test_login(cfg: &ImapConfig) -> Result<()> {
    if cfg.imap_host.is_empty() || cfg.username.is_empty() {
        bail!("IMAP-Server und Benutzername sind erforderlich");
    }
    let mut s = connect(cfg).await?;
    s.logout().await.ok();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_roles() {
        assert_eq!(folder_role("INBOX", &[]), "inbox");
        assert_eq!(folder_role("INBOX.Sent", &[]), "sent");
        assert_eq!(folder_role("Gelöschte Elemente", &[]), "trash");
        assert_eq!(folder_role("Whatever", &[NameAttribute::Junk]), "junk");
        assert_eq!(folder_role("Projekte/2026", &[]), "other");
    }
}
