//! Automatic replies for IMAP accounts via ManageSieve (RFC 5804), the
//! standard way mail servers (Dovecot/Pigeonhole, Cyrus, many providers)
//! let clients install a Sieve "vacation" script.

use std::sync::Arc;

use anyhow::{anyhow, bail, Context, Result};
use base64::Engine;
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader},
    net::TcpStream,
};
use tokio_rustls::{rustls, TlsConnector};

use crate::{models::ImapConfig, providers::graph::AutoReply};

pub const PORT: u16 = 4190;
const SCRIPT: &str = "openlook";
/// First line of the generated script carries the settings as JSON so they
/// can be shown again exactly as entered.
const MARKER: &str = "# openlook:";

struct Conn<S> {
    io: BufReader<S>,
}

impl<S: AsyncRead + AsyncWrite + Unpin> Conn<S> {
    async fn line(&mut self) -> Result<String> {
        let mut l = String::new();
        if self.io.read_line(&mut l).await? == 0 {
            bail!("ManageSieve-Verbindung unerwartet beendet");
        }
        Ok(l.trim_end_matches(['\r', '\n']).to_string())
    }

    /// Read lines until OK/NO/BYE; returns the collected data lines.
    async fn response(&mut self) -> Result<Vec<String>> {
        let mut data = vec![];
        loop {
            let l = self.line().await?;
            let upper = l.to_ascii_uppercase();
            if upper.starts_with("OK") {
                return Ok(data);
            }
            if upper.starts_with("NO") || upper.starts_with("BYE") {
                bail!("ManageSieve: {l}");
            }
            // Literal {n} or {n+}: read n bytes verbatim.
            if let Some(n) = l.strip_prefix('{').and_then(|r| {
                r.trim_end_matches('}')
                    .trim_end_matches('+')
                    .parse::<usize>()
                    .ok()
            }) {
                let mut buf = vec![0; n];
                self.io.read_exact(&mut buf).await?;
                data.push(String::from_utf8_lossy(&buf).into_owned());
                continue;
            }
            data.push(l);
        }
    }

    async fn cmd(&mut self, c: &str) -> Result<Vec<String>> {
        self.io
            .get_mut()
            .write_all(format!("{c}\r\n").as_bytes())
            .await?;
        self.io.get_mut().flush().await?;
        self.response().await
    }
}

fn quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Connect, STARTTLS, authenticate; runs `f` with the session.
async fn session<T>(
    cfg: &ImapConfig,
    f: impl AsyncFnOnce(&mut Conn<tokio_rustls::client::TlsStream<TcpStream>>) -> Result<T>,
) -> Result<T> {
    let tcp = TcpStream::connect((cfg.imap_host.as_str(), PORT))
        .await
        .with_context(|| format!("ManageSieve auf {}:{PORT} nicht erreichbar – der Server unterstützt keine Abwesenheitsnotizen über ManageSieve", cfg.imap_host))?;
    let mut plain = Conn {
        io: BufReader::new(tcp),
    };
    let caps = plain.response().await?.join("\n").to_ascii_uppercase();
    if !caps.contains("\"STARTTLS\"") {
        bail!("Der ManageSieve-Server bietet kein STARTTLS an");
    }
    plain.cmd("STARTTLS").await?;
    let tcp = plain.io.into_inner();
    let tls = rustls::ClientConfig::builder()
        .with_root_certificates(super::imap::roots()?)
        .with_no_client_auth();
    let name = rustls::pki_types::ServerName::try_from(cfg.imap_host.clone())?;
    let stream = TlsConnector::from(Arc::new(tls)).connect(name, tcp).await?;
    let mut conn = Conn {
        io: BufReader::new(stream),
    };
    conn.response().await?; // capabilities again after TLS
    let auth = base64::engine::general_purpose::STANDARD
        .encode(format!("\0{}\0{}", cfg.username, cfg.password));
    conn.cmd(&format!("AUTHENTICATE \"PLAIN\" {}", quote(&auth)))
        .await
        .map_err(|e| anyhow!("ManageSieve-Anmeldung fehlgeschlagen: {e}"))?;
    let res = f(&mut conn).await;
    let _ = conn.cmd("LOGOUT").await;
    res
}

/// Sieve script for the vacation settings. Dates are compared per day.
pub fn script(a: &AutoReply) -> String {
    let settings = serde_json::json!({ "scheduled": a.scheduled, "start": a.start, "end": a.end, "text": a.internal });
    let text: String = a
        .internal
        .lines()
        .map(|l| {
            if l.starts_with('.') {
                format!(".{l}")
            } else {
                l.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\r\n");
    let vacation =
        format!("vacation :days 1 :subject \"Automatische Antwort\" text:\r\n{text}\r\n.\r\n;\r\n");
    let body = if a.scheduled {
        let day = |s: &str| s.chars().take(10).collect::<String>();
        format!(
            "if allof (currentdate :value \"ge\" \"date\" \"{}\", currentdate :value \"le\" \"date\" \"{}\") {{\r\n{vacation}}}\r\n",
            day(&a.start),
            day(&a.end)
        )
    } else {
        vacation
    };
    format!("{MARKER}{settings}\r\nrequire [\"vacation\", \"date\", \"relational\"];\r\n{body}")
}

fn parse(script: &str) -> Option<AutoReply> {
    let json = script.lines().next()?.strip_prefix(MARKER)?;
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    Some(AutoReply {
        enabled: true,
        scheduled: v["scheduled"].as_bool().unwrap_or(false),
        start: v["start"].as_str().unwrap_or_default().to_string(),
        end: v["end"].as_str().unwrap_or_default().to_string(),
        internal: v["text"].as_str().unwrap_or_default().to_string(),
        external: String::new(),
    })
}

pub async fn get(cfg: &ImapConfig) -> Result<AutoReply> {
    session(cfg, async |c| {
        let list = c.cmd("LISTSCRIPTS").await?;
        let active = list
            .iter()
            .any(|l| l.starts_with(&quote(SCRIPT)) && l.to_ascii_uppercase().contains("ACTIVE"));
        if !list.iter().any(|l| l.starts_with(&quote(SCRIPT))) {
            return Ok(AutoReply::default());
        }
        let data = c.cmd(&format!("GETSCRIPT {}", quote(SCRIPT))).await?;
        let mut a = data.first().and_then(|s| parse(s)).unwrap_or_default();
        a.enabled = active;
        Ok(a)
    })
    .await
}

pub async fn set(cfg: &ImapConfig, a: &AutoReply) -> Result<()> {
    session(cfg, async |c| {
        if !a.enabled {
            // Deactivate (an empty name switches all scripts off).
            c.cmd("SETACTIVE \"\"").await?;
            return Ok(());
        }
        let s = script(a);
        c.cmd(&format!(
            "PUTSCRIPT {} {{{}+}}\r\n{s}",
            quote(SCRIPT),
            s.len()
        ))
        .await?;
        c.cmd(&format!("SETACTIVE {}", quote(SCRIPT))).await?;
        Ok(())
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_roundtrip() {
        let a = AutoReply {
            enabled: true,
            scheduled: true,
            start: "2026-12-20T08:00:00".into(),
            end: "2027-01-06T08:00:00".into(),
            internal: "Bin im Urlaub.\n.Punkt am Anfang".into(),
            external: String::new(),
        };
        let s = script(&a);
        assert!(s.contains("require [\"vacation\", \"date\", \"relational\"];"));
        assert!(s.contains("currentdate :value \"ge\" \"date\" \"2026-12-20\""));
        assert!(
            s.contains("\r\n..Punkt am Anfang\r\n.\r\n;"),
            "Punkt-Zeilen werden verdoppelt"
        );
        assert_eq!(parse(&s).unwrap(), a);
    }
}
