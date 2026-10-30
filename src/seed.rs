//! Creates the built-in local account with demo content on first start,
//! so the UI is usable before any real account is connected.

use anyhow::Result;
use chrono::{Duration, Utc};
use rusqlite::Connection;

use crate::{db, mail, models::*};

pub fn ensure_local_account(c: &Connection) -> Result<()> {
    if db::list_accounts(c)?
        .iter()
        .any(|a| a.kind == AccountKind::Local)
    {
        return Ok(());
    }
    let acc = db::insert_account(
        c,
        AccountKind::Local,
        "Local Mailbox",
        "me@openlook.local",
        "{}",
    )?;
    let mut ids = std::collections::HashMap::new();
    for (rid, name, role) in [
        ("inbox", "Inbox", "inbox"),
        ("drafts", "Drafts", "drafts"),
        ("sent", "Sent Items", "sent"),
        ("trash", "Deleted Items", "trash"),
        ("junk", "Junk Email", "junk"),
        ("archive", "Archive", "archive"),
    ] {
        ids.insert(role, db::upsert_folder(c, acc, rid, name, role)?);
    }

    let now = Utc::now();
    let fmt = |d: chrono::DateTime<Utc>| d.format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let mails = [
        ("Welcome to OpenLook", "OpenLook Team", "team@openlook.local", 0, false, true,
         "<h2>Welcome!</h2><p>This is your local mailbox. Connect a real account via <b>File &gt; Add Account</b>:</p><ul><li><b>Microsoft 365 / Outlook.com</b> via Microsoft Graph</li><li><b>Any IMAP/SMTP account</b> (Gmail, GMX, web.de, Posteo …)</li></ul><p>The keyboard shortcuts you already know: <code>Ctrl+N</code> new email, <code>Ctrl+R</code> reply, <code>Ctrl+F</code> forward, <code>Del</code> delete, <code>Ctrl+Q</code> mark as read, <code>Ctrl+E</code> search, <code>F9</code> Send / Receive.</p>"),
        ("Project kick-off on Thursday", "Anna Schmidt", "anna.schmidt@example.com", 2, true, false,
         "<p>Hi all,</p><p>please find attached the agenda for the kick-off. Bring your questions along.</p><p>Best regards<br>Anna</p>"),
        ("Invoice 2026-1042", "Accounts", "invoice@example.com", 26, false, false,
         "<p>Dear Sir or Madam,</p><p>please find attached the invoice for October.</p>"),
        ("Re: Vacation planning", "Jonas Weber", "jonas.weber@example.com", 50, false, false,
         "<p>Works for me, I'll add it to the team calendar.</p><p>Jonas</p>"),
        ("Your weekly report", "Insights", "insights@example.com", 120, false, false,
         "<p>This week you attended 12 meetings and sent 48 emails.</p>"),
    ];
    for (i, (subject, from_name, from_addr, hours_ago, flagged, unread, html)) in
        mails.into_iter().enumerate()
    {
        let text = mail::strip_html(html);
        db::upsert_message(
            c,
            acc,
            ids["inbox"],
            &NewMessage {
                remote_id: format!("seed-{i}"),
                subject: subject.into(),
                from_name: from_name.into(),
                from_addr: from_addr.into(),
                to: "me@openlook.local".into(),
                date: fmt(now - Duration::hours(hours_ago)),
                preview: mail::make_preview(&text),
                body_html: html.into(),
                body_text: text,
                is_read: !unread && i != 1,
                is_flagged: flagged,
                importance: if i == 1 {
                    "high".into()
                } else {
                    "normal".into()
                },
                // The invoice comes with a document, like the real thing.
                attachments: if i == 2 {
                    vec![NewAttachment {
                        name: "Invoice_2026-1042.txt".into(),
                        mime: "text/plain".into(),
                        data: "Invoice 2026-1042\nService period: October 2026\nAmount: EUR 1,190.00\n".as_bytes().to_vec(),
                    }]
                } else {
                    vec![]
                },
                ..Default::default()
            },
        )?;
    }

    let today = now.date_naive();
    let at = |days: i64, h: u32, m: u32| {
        fmt((today + Duration::days(days))
            .and_hms_opt(h, m, 0)
            .unwrap()
            .and_utc())
    };
    for (title, start, end, loc, cat) in [
        (
            "Daily Stand-up",
            at(0, 8, 0),
            at(0, 8, 15),
            "Teams",
            "Blue Category",
        ),
        (
            "Project kick-off",
            at(2, 12, 0),
            at(2, 13, 30),
            "Room 4.12",
            "Orange Category",
        ),
        (
            "Lunch with Jonas",
            at(1, 11, 0),
            at(1, 12, 0),
            "Canteen",
            "Green Category",
        ),
        (
            "1:1 with Anna",
            at(3, 14, 0),
            at(3, 14, 30),
            "",
            "Purple Category",
        ),
    ] {
        db::save_event(
            c,
            &Event {
                account_id: acc,
                title: title.into(),
                start,
                end,
                location: loc.into(),
                show_as: "busy".into(),
                category: cat.into(),
                reminder: 15,
                ..Default::default()
            },
        )?;
    }

    for (name, email, company, title, phone) in [
        (
            "Anna Schmidt",
            "anna.schmidt@example.com",
            "Contoso GmbH",
            "Project Manager",
            "+49 30 1234567",
        ),
        (
            "Jonas Weber",
            "jonas.weber@example.com",
            "Contoso GmbH",
            "Developer",
            "+49 40 7654321",
        ),
        (
            "Lea Fischer",
            "lea.fischer@example.com",
            "Fabrikam AG",
            "Purchasing",
            "",
        ),
    ] {
        db::save_contact(
            c,
            &Contact {
                account_id: acc,
                display_name: name.into(),
                email: email.into(),
                company: company.into(),
                job_title: title.into(),
                phone: phone.into(),
                ..Default::default()
            },
        )?;
    }

    for (title, due_days, important) in [
        ("Prepare kick-off agenda", Some(1), true),
        ("Check invoice", Some(3), false),
        ("Submit travel expenses", None, false),
    ] {
        db::save_task(
            c,
            &Task {
                account_id: acc,
                list_name: "Tasks".into(),
                title: title.into(),
                due: due_days
                    .map(|d| (today + Duration::days(d)).format("%Y-%m-%d").to_string())
                    .unwrap_or_default(),
                important,
                ..Default::default()
            },
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seeds_once() {
        let c = db::open(":memory:").unwrap();
        ensure_local_account(&c).unwrap();
        ensure_local_account(&c).unwrap();
        assert_eq!(db::list_accounts(&c).unwrap().len(), 1);
        assert_eq!(db::list_folders(&c).unwrap().len(), 6);
        assert_eq!(db::list_contacts(&c).unwrap().len(), 3);
    }
}
