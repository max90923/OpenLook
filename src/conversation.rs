//! Conversation tools: "Clean Up" and "Ignore".

use crate::models::Message;

/// Messages with the same subject (without RE:/FW:/Fwd: prefixes, or the German AW:/WG:) form a conversation.
pub fn key(subject: &str) -> String {
    let mut s = subject.trim();
    loop {
        let lower = s.to_lowercase();
        match ["aw:", "re:", "wg:", "fw:", "fwd:"]
            .iter()
            .find(|p| lower.starts_with(*p))
        {
            Some(p) => s = s[p.len()..].trim_start(),
            None => return s.to_lowercase(),
        }
    }
}

fn normalized(text: &str) -> String {
    text.split_whitespace()
        .filter(|w| !w.starts_with('>'))
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Ids of messages whose whole text is contained in a later message of the
/// same conversation – Outlook's "Clean Up Conversation".
pub fn redundant(messages: &[Message]) -> Vec<i64> {
    let mut out = vec![];
    for m in messages {
        let own = normalized(&m.body_text);
        if own.len() < 20 {
            continue;
        }
        let k = key(&m.summary.subject);
        let quoted = messages.iter().any(|later| {
            later.summary.id != m.summary.id
                && later.summary.account_id == m.summary.account_id
                && later.summary.date > m.summary.date
                && key(&later.summary.subject) == k
                && normalized(&later.body_text).contains(&own)
        });
        if quoted {
            out.push(m.summary.id);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::MessageSummary;

    fn msg(id: i64, subject: &str, date: &str, body: &str) -> Message {
        Message {
            summary: MessageSummary {
                id,
                account_id: 1,
                folder_id: 1,
                subject: subject.into(),
                from_name: String::new(),
                from_addr: String::new(),
                date: date.into(),
                preview: String::new(),
                is_read: true,
                is_flagged: false,
                is_pinned: false,
                has_attachments: false,
                importance: "normal".into(),
                categories: vec![],
            },
            to: String::new(),
            cc: String::new(),
            body_html: String::new(),
            body_text: body.into(),
        }
    }

    #[test]
    fn keys_ignore_prefixes() {
        assert_eq!(key("AW: WG: Quote"), "quote");
        assert_eq!(key("RE: FW: Quote"), "quote");
        assert_eq!(key("Re: quote"), "quote");
        assert_eq!(key("Fwd: Quote"), "quote");
    }

    #[test]
    fn finds_quoted_messages() {
        let first = msg(
            1,
            "Quote",
            "2026-10-01T10:00:00Z",
            "Could you send us a quote by Friday?",
        );
        let reply = msg(
            2,
            "RE: Quote",
            "2026-10-02T10:00:00Z",
            "Sure, please find it attached.\n\nFrom: Lea\nSubject: Quote\n\nCould you send us a quote by Friday?",
        );
        let other = msg(
            3,
            "Invoice",
            "2026-10-03T10:00:00Z",
            "Could you send us a quote by Friday?",
        );
        assert_eq!(redundant(&[first, reply, other]), vec![1]);
    }
}
