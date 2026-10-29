//! MIME helpers shared by the providers.

use mail_parser::{Address, MessageParser, MimeHeaders};

use crate::models::NewAttachment;

use crate::models::NewMessage;

/// Normalize any RFC 3339 / RFC 2822 date string to `YYYY-MM-DDTHH:MM:SSZ`, so
/// stored dates sort and compare correctly as strings.
pub fn to_utc(date: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(date)
        .or_else(|_| chrono::DateTime::parse_from_rfc2822(date))
        .map(|d| {
            d.with_timezone(&chrono::Utc)
                .format("%Y-%m-%dT%H:%M:%SZ")
                .to_string()
        })
        .unwrap_or_else(|_| date.to_string())
}

pub fn now_utc() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

fn addr_list(a: Option<&Address>) -> String {
    a.map(|a| {
        a.iter()
            .map(|x| match (x.name(), x.address()) {
                (Some(n), Some(e)) => format!("{n} <{e}>"),
                (None, Some(e)) => e.to_string(),
                (Some(n), None) => n.to_string(),
                _ => String::new(),
            })
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(", ")
    })
    .unwrap_or_default()
}

/// Build a short single-line preview from a plain-text body.
pub fn make_preview(text: &str) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    flat.chars().take(160).collect()
}

/// Very small HTML-to-text converter for previews and search of HTML-only mails.
pub fn strip_html(html: &str) -> String {
    let mut out = String::with_capacity(html.len() / 2);
    let mut in_tag = false;
    let lower = html.to_ascii_lowercase();
    let mut skip_until: Option<&str> = None;
    let mut i = 0;
    let bytes = html.as_bytes();
    while i < bytes.len() {
        if let Some(end) = skip_until {
            if lower[i..].starts_with(end) {
                i += end.len();
                skip_until = None;
            } else {
                i += 1;
            }
            continue;
        }
        if lower[i..].starts_with("<style") {
            skip_until = Some("</style>");
            continue;
        }
        if lower[i..].starts_with("<script") {
            skip_until = Some("</script>");
            continue;
        }
        let ch = html[i..].chars().next().unwrap();
        match ch {
            '<' => {
                in_tag = true;
                let tag: String = lower[i + 1..]
                    .trim_start_matches('/')
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric())
                    .collect();
                let block = matches!(
                    tag.as_str(),
                    "br" | "p"
                        | "div"
                        | "li"
                        | "tr"
                        | "h1"
                        | "h2"
                        | "h3"
                        | "h4"
                        | "h5"
                        | "h6"
                        | "blockquote"
                        | "hr"
                );
                out.push(if block { '\n' } else { ' ' });
            }
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
        i += ch.len_utf8();
    }
    let out = out
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"");
    // Tidy up: trim each line and collapse runs of blank lines.
    let mut tidy = String::with_capacity(out.len());
    let mut blank = 0;
    for line in out
        .lines()
        .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
    {
        if line.is_empty() {
            blank += 1;
            if blank > 1 {
                continue;
            }
        } else {
            blank = 0;
        }
        tidy.push_str(&line);
        tidy.push('\n');
    }
    tidy.trim().to_string()
}

pub fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Turn a plain-text body into minimal HTML.
pub fn text_to_html(s: &str) -> String {
    format!(
        "<div style=\"white-space:pre-wrap\">{}</div>",
        escape_html(s)
    )
}

/// Parse a raw RFC 822 message.
pub fn parse_rfc822(remote_id: String, raw: &[u8]) -> NewMessage {
    let Some(msg) = MessageParser::default().parse(raw) else {
        return NewMessage {
            remote_id,
            subject: "(nicht lesbare Nachricht)".into(),
            ..Default::default()
        };
    };
    let from = msg.from().and_then(|a| a.first());
    let body_text = msg.body_text(0).map(|c| c.into_owned()).unwrap_or_default();
    let body_html = msg.body_html(0).map(|c| c.into_owned()).unwrap_or_default();
    let text_for_preview = if body_text.is_empty() {
        strip_html(&body_html)
    } else {
        body_text.clone()
    };
    let importance = match msg
        .header("Importance")
        .and_then(|h| h.as_text())
        .map(|s| s.to_ascii_lowercase())
        .as_deref()
    {
        Some("high") => "high",
        Some("low") => "low",
        _ => "normal",
    };
    NewMessage {
        remote_id,
        subject: msg.subject().unwrap_or("(kein Betreff)").to_string(),
        from_name: from.and_then(|a| a.name()).unwrap_or_default().to_string(),
        from_addr: from
            .and_then(|a| a.address())
            .unwrap_or_default()
            .to_string(),
        to: addr_list(msg.to()),
        cc: addr_list(msg.cc()),
        date: msg
            .date()
            .map(|d| to_utc(&d.to_rfc3339()))
            .unwrap_or_default(),
        preview: make_preview(&text_for_preview),
        body_html: if body_html.is_empty() {
            text_to_html(&body_text)
        } else {
            body_html
        },
        body_text: text_for_preview,
        has_attachments: msg.attachment_count() > 0,
        attachments: msg
            .attachments()
            .enumerate()
            .map(|(i, a)| NewAttachment {
                name: a
                    .attachment_name()
                    .map(String::from)
                    .unwrap_or_else(|| format!("Anlage {}", i + 1)),
                mime: a
                    .content_type()
                    .map(|ct| format!("{}/{}", ct.ctype(), ct.subtype().unwrap_or("octet-stream")))
                    .unwrap_or_else(|| "application/octet-stream".into()),
                data: a.contents().to_vec(),
            })
            .collect(),
        importance: importance.into(),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_simple_mail() {
        let raw = b"From: Alice Example <alice@example.com>\r\nTo: bob@example.com\r\nSubject: Hallo\r\nDate: Tue, 3 Mar 2026 10:00:00 +0100\r\nImportance: High\r\n\r\nZeile eins\r\nZeile zwei\r\n";
        let m = parse_rfc822("7".into(), raw);
        assert_eq!(m.subject, "Hallo");
        assert_eq!(m.from_name, "Alice Example");
        assert_eq!(m.from_addr, "alice@example.com");
        assert_eq!(m.to, "bob@example.com");
        assert_eq!(m.date, "2026-03-03T09:00:00Z");
        assert_eq!(m.importance, "high");
        assert_eq!(m.preview, "Zeile eins Zeile zwei");
        assert!(m.body_html.contains("Zeile eins"));
    }

    #[test]
    fn keeps_line_breaks() {
        assert_eq!(
            strip_html("<p>Hallo</p><p>Welt<br>zwei</p>"),
            "Hallo\n\nWelt\nzwei"
        );
    }

    #[test]
    fn parses_attachments() {
        let raw = b"From: a@x.de\r\nSubject: Anlage\r\nMIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=XX\r\n\r\n--XX\r\nContent-Type: text/plain\r\n\r\nText\r\n--XX\r\nContent-Type: application/pdf\r\nContent-Disposition: attachment; filename=\"r.pdf\"\r\nContent-Transfer-Encoding: base64\r\n\r\nSGFsbG8=\r\n--XX--\r\n";
        let m = parse_rfc822("1".into(), raw);
        assert!(m.has_attachments);
        assert_eq!(m.attachments.len(), 1);
        assert_eq!(m.attachments[0].name, "r.pdf");
        assert_eq!(m.attachments[0].mime, "application/pdf");
        assert_eq!(m.attachments[0].data, b"Hallo");
    }

    #[test]
    fn strips_html() {
        assert_eq!(
            make_preview(&strip_html("<style>p{}</style><p>Hi &amp; <b>du</b></p>")),
            "Hi & du"
        );
    }
}
