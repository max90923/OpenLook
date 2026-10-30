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
        "Lokales Postfach",
        "ich@openlook.local",
        "{}",
    )?;
    let mut ids = std::collections::HashMap::new();
    for (rid, name, role) in [
        ("inbox", "Posteingang", "inbox"),
        ("drafts", "Entwürfe", "drafts"),
        ("sent", "Gesendete Elemente", "sent"),
        ("trash", "Gelöschte Elemente", "trash"),
        ("junk", "Junk-E-Mail", "junk"),
        ("archive", "Archiv", "archive"),
    ] {
        ids.insert(role, db::upsert_folder(c, acc, rid, name, role)?);
    }

    let now = Utc::now();
    let fmt = |d: chrono::DateTime<Utc>| d.format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let mails = [
        ("Willkommen bei OpenLook", "OpenLook Team", "team@openlook.local", 0, false, true,
         "<h2>Willkommen!</h2><p>Dies ist dein lokales Postfach. Verbinde ein echtes Konto über <b>Datei &gt; Konto hinzufügen</b>:</p><ul><li><b>Microsoft 365 / Outlook.com</b> über Microsoft Graph</li><li><b>Jedes IMAP/SMTP-Konto</b> (Gmail, GMX, web.de, Posteo …)</li></ul><p>Tastenkombinationen wie in Outlook: <code>Strg+N</code> neue E-Mail, <code>Strg+R</code> antworten, <code>Strg+F</code> weiterleiten, <code>Entf</code> löschen, <code>Strg+Q</code> als gelesen markieren, <code>Strg+E</code> suchen, <code>F9</code> Senden/Empfangen.</p>"),
        ("Projekt-Kickoff am Donnerstag", "Anna Schmidt", "anna.schmidt@example.com", 2, true, false,
         "<p>Hallo,</p><p>anbei die Agenda für den Kickoff. Bitte bringt eure Fragen mit.</p><p>Viele Grüße<br>Anna</p>"),
        ("Rechnung 2026-1042", "Buchhaltung", "rechnung@example.com", 26, false, false,
         "<p>Sehr geehrte Damen und Herren,</p><p>im Anhang finden Sie die Rechnung für Oktober.</p>"),
        ("Re: Urlaubsplanung", "Jonas Weber", "jonas.weber@example.com", 50, false, false,
         "<p>Passt für mich, ich trage es in den Teamkalender ein.</p><p>Jonas</p>"),
        ("Dein Wochenbericht", "Insights", "insights@example.com", 120, false, false,
         "<p>Du hast diese Woche 12 Besprechungen besucht und 48 E-Mails gesendet.</p>"),
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
                to: "ich@openlook.local".into(),
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
                        name: "Rechnung_2026-1042.txt".into(),
                        mime: "text/plain".into(),
                        data: "Rechnung 2026-1042\nLeistungszeitraum: Oktober 2026\nBetrag: 1.190,00 EUR\n".as_bytes().to_vec(),
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
            "Blaue Kategorie",
        ),
        (
            "Projekt-Kickoff",
            at(2, 12, 0),
            at(2, 13, 30),
            "Raum 4.12",
            "Orange Kategorie",
        ),
        (
            "Mittagessen mit Jonas",
            at(1, 11, 0),
            at(1, 12, 0),
            "Kantine",
            "Grüne Kategorie",
        ),
        (
            "1:1 mit Anna",
            at(3, 14, 0),
            at(3, 14, 30),
            "",
            "Lila Kategorie",
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
            "Projektleiterin",
            "+49 30 1234567",
        ),
        (
            "Jonas Weber",
            "jonas.weber@example.com",
            "Contoso GmbH",
            "Entwickler",
            "+49 40 7654321",
        ),
        (
            "Lea Fischer",
            "lea.fischer@example.com",
            "Fabrikam AG",
            "Einkauf",
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
        ("Agenda für Kickoff vorbereiten", Some(1), true),
        ("Rechnung prüfen", Some(3), false),
        ("Reisekosten einreichen", None, false),
    ] {
        db::save_task(
            c,
            &Task {
                account_id: acc,
                list_name: "Aufgaben".into(),
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
