//! SQLite storage. The database is the local cache for every account; providers
//! sync remote data into it and the UI reads exclusively from it.

use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension, Row};

use crate::models::*;

const SCHEMA: &str = r#"
PRAGMA foreign_keys = ON;
PRAGMA journal_mode = WAL;

CREATE TABLE IF NOT EXISTS accounts (
    id          INTEGER PRIMARY KEY,
    kind        TEXT NOT NULL,
    name        TEXT NOT NULL,
    email       TEXT NOT NULL,
    config      TEXT NOT NULL DEFAULT '{}',
    last_sync   TEXT,
    last_error  TEXT
);

CREATE TABLE IF NOT EXISTS folders (
    id          INTEGER PRIMARY KEY,
    account_id  INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    remote_id   TEXT NOT NULL,
    name        TEXT NOT NULL,
    role        TEXT NOT NULL DEFAULT 'other',
    UNIQUE(account_id, remote_id)
);

CREATE TABLE IF NOT EXISTS messages (
    id              INTEGER PRIMARY KEY,
    account_id      INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    folder_id       INTEGER NOT NULL REFERENCES folders(id) ON DELETE CASCADE,
    remote_id       TEXT NOT NULL,
    subject         TEXT NOT NULL DEFAULT '',
    from_name       TEXT NOT NULL DEFAULT '',
    from_addr       TEXT NOT NULL DEFAULT '',
    to_addrs        TEXT NOT NULL DEFAULT '',
    cc_addrs        TEXT NOT NULL DEFAULT '',
    date            TEXT NOT NULL DEFAULT '',
    preview         TEXT NOT NULL DEFAULT '',
    body_html       TEXT NOT NULL DEFAULT '',
    body_text       TEXT NOT NULL DEFAULT '',
    is_read         INTEGER NOT NULL DEFAULT 0,
    is_flagged      INTEGER NOT NULL DEFAULT 0,
    is_pinned       INTEGER NOT NULL DEFAULT 0,
    has_attachments INTEGER NOT NULL DEFAULT 0,
    importance      TEXT NOT NULL DEFAULT 'normal',
    categories      TEXT NOT NULL DEFAULT '',
    UNIQUE(folder_id, remote_id)
);
CREATE INDEX IF NOT EXISTS idx_messages_folder_date ON messages(folder_id, date DESC);

CREATE TABLE IF NOT EXISTS events (
    id          INTEGER PRIMARY KEY,
    account_id  INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    remote_id   TEXT NOT NULL DEFAULT '',
    title       TEXT NOT NULL DEFAULT '',
    start       TEXT NOT NULL,
    end         TEXT NOT NULL,
    all_day     INTEGER NOT NULL DEFAULT 0,
    location    TEXT NOT NULL DEFAULT '',
    body        TEXT NOT NULL DEFAULT '',
    attendees   TEXT NOT NULL DEFAULT '',
    show_as     TEXT NOT NULL DEFAULT 'busy',
    category    TEXT NOT NULL DEFAULT '',
    reminder    INTEGER NOT NULL DEFAULT 15
);
CREATE INDEX IF NOT EXISTS idx_events_start ON events(start);

CREATE TABLE IF NOT EXISTS contacts (
    id           INTEGER PRIMARY KEY,
    account_id   INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    remote_id    TEXT NOT NULL DEFAULT '',
    display_name TEXT NOT NULL DEFAULT '',
    email        TEXT NOT NULL DEFAULT '',
    phone        TEXT NOT NULL DEFAULT '',
    company      TEXT NOT NULL DEFAULT '',
    job_title    TEXT NOT NULL DEFAULT '',
    address      TEXT NOT NULL DEFAULT '',
    notes        TEXT NOT NULL DEFAULT '',
    favorite     INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS tasks (
    id             INTEGER PRIMARY KEY,
    account_id     INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    remote_id      TEXT NOT NULL DEFAULT '',
    list_name      TEXT NOT NULL DEFAULT 'Aufgaben',
    list_remote_id TEXT NOT NULL DEFAULT '',
    title          TEXT NOT NULL DEFAULT '',
    due            TEXT NOT NULL DEFAULT '',
    completed      INTEGER NOT NULL DEFAULT 0,
    important      INTEGER NOT NULL DEFAULT 0,
    my_day         INTEGER NOT NULL DEFAULT 0,
    notes          TEXT NOT NULL DEFAULT ''
);

CREATE TABLE IF NOT EXISTS attachments (
    id          INTEGER PRIMARY KEY,
    message_id  INTEGER NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    mime        TEXT NOT NULL DEFAULT '',
    size        INTEGER NOT NULL DEFAULT 0,
    data        BLOB NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_attachments_message ON attachments(message_id);

CREATE TABLE IF NOT EXISTS notes (
    id       INTEGER PRIMARY KEY,
    body     TEXT NOT NULL DEFAULT '',
    color    TEXT NOT NULL DEFAULT 'yellow',
    modified TEXT NOT NULL DEFAULT ''
);

CREATE TABLE IF NOT EXISTS journal (
    id      INTEGER PRIMARY KEY,
    kind    TEXT NOT NULL DEFAULT 'Telefonanruf',
    subject TEXT NOT NULL DEFAULT '',
    company TEXT NOT NULL DEFAULT '',
    start   TEXT NOT NULL,
    minutes INTEGER NOT NULL DEFAULT 0,
    notes   TEXT NOT NULL DEFAULT ''
);

CREATE TABLE IF NOT EXISTS settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
"#;

/// Columns added after the first release; created on older databases.
const MIGRATIONS: [(&str, &str, &str); 3] = [
    ("events", "recurrence", "TEXT NOT NULL DEFAULT ''"),
    ("events", "response", "TEXT NOT NULL DEFAULT ''"),
    ("events", "organizer", "TEXT NOT NULL DEFAULT ''"),
];

pub fn open(path: &str) -> Result<Connection> {
    let conn = Connection::open(path)?;
    conn.execute_batch(SCHEMA)?;
    for (table, column, decl) in MIGRATIONS {
        let exists: bool = conn.query_row(
            &format!("SELECT EXISTS(SELECT 1 FROM pragma_table_info('{table}') WHERE name = ?)"),
            [column],
            |r| r.get(0),
        )?;
        if !exists {
            conn.execute(
                &format!("ALTER TABLE {table} ADD COLUMN {column} {decl}"),
                [],
            )?;
        }
    }
    Ok(conn)
}

// ---------------------------------------------------------------- accounts

fn row_account(r: &Row) -> rusqlite::Result<Account> {
    Ok(Account {
        id: r.get(0)?,
        kind: AccountKind::parse(&r.get::<_, String>(1)?),
        name: r.get(2)?,
        email: r.get(3)?,
        last_sync: r.get(4)?,
        last_error: r.get(5)?,
    })
}

pub fn list_accounts(c: &Connection) -> Result<Vec<Account>> {
    let mut st =
        c.prepare("SELECT id, kind, name, email, last_sync, last_error FROM accounts ORDER BY id")?;
    let rows = st
        .query_map([], row_account)?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

pub fn get_account(c: &Connection, id: i64) -> Result<Option<Account>> {
    Ok(c.query_row(
        "SELECT id, kind, name, email, last_sync, last_error FROM accounts WHERE id = ?",
        [id],
        row_account,
    )
    .optional()?)
}

pub fn insert_account(
    c: &Connection,
    kind: AccountKind,
    name: &str,
    email: &str,
    config: &str,
) -> Result<i64> {
    c.execute(
        "INSERT INTO accounts (kind, name, email, config) VALUES (?, ?, ?, ?)",
        params![kind.as_str(), name, email, config],
    )?;
    Ok(c.last_insert_rowid())
}

pub fn account_config(c: &Connection, id: i64) -> Result<String> {
    Ok(
        c.query_row("SELECT config FROM accounts WHERE id = ?", [id], |r| {
            r.get(0)
        })?,
    )
}

pub fn set_account_config(c: &Connection, id: i64, config: &str) -> Result<()> {
    c.execute(
        "UPDATE accounts SET config = ? WHERE id = ?",
        params![config, id],
    )?;
    Ok(())
}

pub fn set_sync_result(c: &Connection, id: i64, error: Option<&str>) -> Result<()> {
    if error.is_some() {
        c.execute(
            "UPDATE accounts SET last_error = ? WHERE id = ?",
            params![error, id],
        )?;
    } else {
        c.execute(
            "UPDATE accounts SET last_error = NULL, last_sync = ? WHERE id = ?",
            params![crate::mail::now_utc(), id],
        )?;
    }
    Ok(())
}

pub fn delete_account(c: &Connection, id: i64) -> Result<()> {
    c.execute("DELETE FROM accounts WHERE id = ?", [id])?;
    Ok(())
}

// ---------------------------------------------------------------- folders

pub fn list_folders(c: &Connection) -> Result<Vec<Folder>> {
    let mut st = c.prepare(
        "SELECT f.id, f.account_id, f.remote_id, f.name, f.role,
                (SELECT COUNT(*) FROM messages m WHERE m.folder_id = f.id AND m.is_read = 0),
                (SELECT COUNT(*) FROM messages m WHERE m.folder_id = f.id)
         FROM folders f
         ORDER BY f.account_id,
                  CASE f.role WHEN 'inbox' THEN 0 WHEN 'drafts' THEN 1 WHEN 'sent' THEN 2
                              WHEN 'trash' THEN 3 WHEN 'junk' THEN 4 WHEN 'archive' THEN 5 ELSE 6 END,
                  f.name",
    )?;
    let rows = st
        .query_map([], |r| {
            Ok(Folder {
                id: r.get(0)?,
                account_id: r.get(1)?,
                remote_id: r.get(2)?,
                name: r.get(3)?,
                role: r.get(4)?,
                unread: r.get(5)?,
                total: r.get(6)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

pub fn get_folder(c: &Connection, id: i64) -> Result<Option<Folder>> {
    Ok(list_folders(c)?.into_iter().find(|f| f.id == id))
}

pub fn folder_by_role(c: &Connection, account_id: i64, role: &str) -> Result<Option<Folder>> {
    Ok(list_folders(c)?
        .into_iter()
        .find(|f| f.account_id == account_id && f.role == role))
}

/// Insert or update a folder by remote id, returning the local id.
pub fn upsert_folder(
    c: &Connection,
    account_id: i64,
    remote_id: &str,
    name: &str,
    role: &str,
) -> Result<i64> {
    c.execute(
        "INSERT INTO folders (account_id, remote_id, name, role) VALUES (?, ?, ?, ?)
         ON CONFLICT(account_id, remote_id) DO UPDATE SET name = excluded.name, role = excluded.role",
        params![account_id, remote_id, name, role],
    )?;
    Ok(c.query_row(
        "SELECT id FROM folders WHERE account_id = ? AND remote_id = ?",
        params![account_id, remote_id],
        |r| r.get(0),
    )?)
}

pub fn rename_folder(c: &Connection, id: i64, remote_id: &str, name: &str) -> Result<()> {
    c.execute(
        "UPDATE folders SET remote_id = ?, name = ? WHERE id = ?",
        params![remote_id, name, id],
    )?;
    Ok(())
}

pub fn delete_folder(c: &Connection, id: i64) -> Result<()> {
    c.execute("DELETE FROM folders WHERE id = ?", [id])?;
    Ok(())
}

/// Remove folders of an account that are no longer present remotely.
pub fn prune_folders(c: &Connection, account_id: i64, keep: &[i64]) -> Result<()> {
    let ids: Vec<String> = keep.iter().map(|i| i.to_string()).collect();
    c.execute(
        &format!(
            "DELETE FROM folders WHERE account_id = ? AND id NOT IN ({})",
            if ids.is_empty() {
                "0".to_string()
            } else {
                ids.join(",")
            }
        ),
        [account_id],
    )?;
    Ok(())
}

// ---------------------------------------------------------------- messages

const SUMMARY_COLS: &str =
    "id, account_id, folder_id, subject, from_name, from_addr, date, preview,
     is_read, is_flagged, is_pinned, has_attachments, importance, categories";

fn row_summary(r: &Row) -> rusqlite::Result<MessageSummary> {
    let cats: String = r.get(13)?;
    Ok(MessageSummary {
        id: r.get(0)?,
        account_id: r.get(1)?,
        folder_id: r.get(2)?,
        subject: r.get(3)?,
        from_name: r.get(4)?,
        from_addr: r.get(5)?,
        date: r.get(6)?,
        preview: r.get(7)?,
        is_read: r.get(8)?,
        is_flagged: r.get(9)?,
        is_pinned: r.get(10)?,
        has_attachments: r.get(11)?,
        importance: r.get(12)?,
        categories: cats
            .split(',')
            .filter(|s| !s.is_empty())
            .map(String::from)
            .collect(),
    })
}

#[derive(Debug, Default)]
pub struct MessageQuery {
    pub folder_id: Option<i64>,
    pub search: Option<String>,
    pub unread_only: bool,
    pub flagged_only: bool,
    pub limit: i64,
}

pub fn list_messages(c: &Connection, q: &MessageQuery) -> Result<Vec<MessageSummary>> {
    let mut sql = format!("SELECT {SUMMARY_COLS} FROM messages WHERE 1=1");
    let mut args: Vec<rusqlite::types::Value> = vec![];
    if let Some(f) = q.folder_id {
        sql.push_str(" AND folder_id = ?");
        args.push(f.into());
    }
    if let Some(s) = q.search.as_deref().filter(|s| !s.trim().is_empty()) {
        sql.push_str(
            " AND (subject LIKE ? OR from_name LIKE ? OR from_addr LIKE ? OR body_text LIKE ? OR to_addrs LIKE ?)",
        );
        let pat = format!("%{}%", s.trim());
        for _ in 0..5 {
            args.push(pat.clone().into());
        }
    }
    if q.unread_only {
        sql.push_str(" AND is_read = 0");
    }
    if q.flagged_only {
        sql.push_str(" AND is_flagged = 1");
    }
    sql.push_str(" ORDER BY is_pinned DESC, date DESC LIMIT ?");
    args.push(if q.limit > 0 { q.limit } else { 500 }.into());
    let mut st = c.prepare(&sql)?;
    let rows = st
        .query_map(rusqlite::params_from_iter(args), row_summary)?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

pub fn get_message(c: &Connection, id: i64) -> Result<Option<Message>> {
    Ok(c.query_row(
        &format!("SELECT {SUMMARY_COLS}, to_addrs, cc_addrs, body_html, body_text FROM messages WHERE id = ?"),
        [id],
        |r| {
            Ok(Message {
                summary: row_summary(r)?,
                to: r.get(14)?,
                cc: r.get(15)?,
                body_html: r.get(16)?,
                body_text: r.get(17)?,
            })
        },
    )
    .optional()?)
}

pub fn message_remote(c: &Connection, id: i64) -> Result<(String, i64)> {
    Ok(c.query_row(
        "SELECT remote_id, folder_id FROM messages WHERE id = ?",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?)
}

pub fn upsert_message(
    c: &Connection,
    account_id: i64,
    folder_id: i64,
    m: &NewMessage,
) -> Result<i64> {
    let existed: bool = c.query_row(
        "SELECT EXISTS(SELECT 1 FROM messages WHERE folder_id = ? AND remote_id = ?)",
        params![folder_id, m.remote_id],
        |r| r.get(0),
    )?;
    c.execute(
        "INSERT INTO messages (account_id, folder_id, remote_id, subject, from_name, from_addr, to_addrs,
                               cc_addrs, date, preview, body_html, body_text, is_read, is_flagged,
                               has_attachments, importance)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(folder_id, remote_id) DO UPDATE SET
             is_read = excluded.is_read, is_flagged = excluded.is_flagged,
             subject = excluded.subject, preview = excluded.preview",
        params![
            account_id, folder_id, m.remote_id, m.subject, m.from_name, m.from_addr, m.to, m.cc,
            m.date, m.preview, m.body_html, m.body_text, m.is_read, m.is_flagged,
            m.has_attachments, if m.importance.is_empty() { "normal" } else { &m.importance },
        ],
    )?;
    let id: i64 = c.query_row(
        "SELECT id FROM messages WHERE folder_id = ? AND remote_id = ?",
        params![folder_id, m.remote_id],
        |r| r.get(0),
    )?;
    if !existed && !m.attachments.is_empty() {
        replace_attachments(c, id, &m.attachments)?;
    }
    Ok(id)
}

pub fn replace_attachments(c: &Connection, message_id: i64, list: &[NewAttachment]) -> Result<()> {
    c.execute("DELETE FROM attachments WHERE message_id = ?", [message_id])?;
    for a in list {
        c.execute(
            "INSERT INTO attachments (message_id, name, mime, size, data) VALUES (?, ?, ?, ?, ?)",
            params![message_id, a.name, a.mime, a.data.len() as i64, a.data],
        )?;
    }
    if !list.is_empty() {
        c.execute(
            "UPDATE messages SET has_attachments = 1 WHERE id = ?",
            [message_id],
        )?;
    }
    Ok(())
}

pub fn list_attachments(c: &Connection, message_id: i64) -> Result<Vec<Attachment>> {
    let mut st = c.prepare(
        "SELECT id, message_id, name, mime, size FROM attachments WHERE message_id = ? ORDER BY id",
    )?;
    let rows = st
        .query_map([message_id], |r| {
            Ok(Attachment {
                id: r.get(0)?,
                message_id: r.get(1)?,
                name: r.get(2)?,
                mime: r.get(3)?,
                size: r.get(4)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

/// All attachments of a message including their content (for forwarding).
pub fn attachments_with_data(c: &Connection, message_id: i64) -> Result<Vec<NewAttachment>> {
    let mut st =
        c.prepare("SELECT name, mime, data FROM attachments WHERE message_id = ? ORDER BY id")?;
    let rows = st
        .query_map([message_id], |r| {
            Ok(NewAttachment {
                name: r.get(0)?,
                mime: r.get(1)?,
                data: r.get(2)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

pub fn attachment_data(c: &Connection, id: i64) -> Result<Vec<u8>> {
    Ok(
        c.query_row("SELECT data FROM attachments WHERE id = ?", [id], |r| {
            r.get(0)
        })?,
    )
}

/// Remove messages in a folder whose remote id is not in `keep`.
pub fn prune_messages(c: &Connection, folder_id: i64, keep: &[String]) -> Result<()> {
    let mut st = c.prepare("SELECT id, remote_id FROM messages WHERE folder_id = ?")?;
    let existing: Vec<(i64, String)> = st
        .query_map([folder_id], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    for (id, rid) in existing {
        if !keep.contains(&rid) {
            c.execute("DELETE FROM messages WHERE id = ?", [id])?;
        }
    }
    Ok(())
}

pub fn remote_ids_in_folder(c: &Connection, folder_id: i64) -> Result<Vec<String>> {
    let mut st = c.prepare("SELECT remote_id FROM messages WHERE folder_id = ?")?;
    let rows = st
        .query_map([folder_id], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

pub fn update_message_flags(
    c: &Connection,
    id: i64,
    is_read: Option<bool>,
    is_flagged: Option<bool>,
    is_pinned: Option<bool>,
    categories: Option<&[String]>,
) -> Result<()> {
    if let Some(v) = is_read {
        c.execute(
            "UPDATE messages SET is_read = ? WHERE id = ?",
            params![v, id],
        )?;
    }
    if let Some(v) = is_flagged {
        c.execute(
            "UPDATE messages SET is_flagged = ? WHERE id = ?",
            params![v, id],
        )?;
    }
    if let Some(v) = is_pinned {
        c.execute(
            "UPDATE messages SET is_pinned = ? WHERE id = ?",
            params![v, id],
        )?;
    }
    if let Some(v) = categories {
        c.execute(
            "UPDATE messages SET categories = ? WHERE id = ?",
            params![v.join(","), id],
        )?;
    }
    Ok(())
}

pub fn move_message(c: &Connection, id: i64, folder_id: i64, new_remote_id: &str) -> Result<()> {
    c.execute(
        "UPDATE messages SET folder_id = ?, remote_id = ? WHERE id = ?",
        params![folder_id, new_remote_id, id],
    )?;
    Ok(())
}

pub fn delete_message(c: &Connection, id: i64) -> Result<()> {
    c.execute("DELETE FROM messages WHERE id = ?", [id])?;
    Ok(())
}

// ---------------------------------------------------------------- events

fn row_event(r: &Row) -> rusqlite::Result<Event> {
    Ok(Event {
        id: r.get(0)?,
        account_id: r.get(1)?,
        remote_id: r.get(2)?,
        title: r.get(3)?,
        start: r.get(4)?,
        end: r.get(5)?,
        all_day: r.get(6)?,
        location: r.get(7)?,
        body: r.get(8)?,
        attendees: r.get(9)?,
        show_as: r.get(10)?,
        category: r.get(11)?,
        reminder: r.get(12)?,
        recurrence: r.get(13)?,
        response: r.get(14)?,
        organizer: r.get(15)?,
    })
}

const EVENT_COLS: &str =
    "id, account_id, remote_id, title, start, end, all_day, location, body, attendees, show_as, category, reminder,
     recurrence, response, organizer";

pub fn list_events(c: &Connection, from: &str, to: &str) -> Result<Vec<Event>> {
    let mut st = c.prepare(&format!(
        "SELECT {EVENT_COLS} FROM events
         WHERE (end >= ?1 AND start <= ?2) OR (recurrence != '' AND start <= ?2) ORDER BY start"
    ))?;
    let rows: Vec<Event> = st
        .query_map([from, to], row_event)?
        .collect::<rusqlite::Result<_>>()?;
    Ok(crate::recur::expand(rows, from, to))
}

pub fn get_event(c: &Connection, id: i64) -> Result<Option<Event>> {
    Ok(c.query_row(
        &format!("SELECT {EVENT_COLS} FROM events WHERE id = ?"),
        [id],
        row_event,
    )
    .optional()?)
}

pub fn save_event(c: &Connection, e: &Event) -> Result<i64> {
    if e.id > 0 {
        c.execute(
            "UPDATE events SET remote_id=?, title=?, start=?, end=?, all_day=?, location=?, body=?,
                    attendees=?, show_as=?, category=?, reminder=?, recurrence=?, response=?, organizer=? WHERE id=?",
            params![
                e.remote_id,
                e.title,
                e.start,
                e.end,
                e.all_day,
                e.location,
                e.body,
                e.attendees,
                e.show_as,
                e.category,
                e.reminder,
                e.recurrence,
                e.response,
                e.organizer,
                e.id
            ],
        )?;
        Ok(e.id)
    } else {
        c.execute(
            "INSERT INTO events (account_id, remote_id, title, start, end, all_day, location, body,
                                 attendees, show_as, category, reminder, recurrence, response, organizer)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                e.account_id,
                e.remote_id,
                e.title,
                e.start,
                e.end,
                e.all_day,
                e.location,
                e.body,
                e.attendees,
                e.show_as,
                e.category,
                e.reminder,
                e.recurrence,
                e.response,
                e.organizer
            ],
        )?;
        Ok(c.last_insert_rowid())
    }
}

pub fn delete_event(c: &Connection, id: i64) -> Result<()> {
    c.execute("DELETE FROM events WHERE id = ?", [id])?;
    Ok(())
}

// ---------------------------------------------------------------- contacts

fn row_contact(r: &Row) -> rusqlite::Result<Contact> {
    Ok(Contact {
        id: r.get(0)?,
        account_id: r.get(1)?,
        remote_id: r.get(2)?,
        display_name: r.get(3)?,
        email: r.get(4)?,
        phone: r.get(5)?,
        company: r.get(6)?,
        job_title: r.get(7)?,
        address: r.get(8)?,
        notes: r.get(9)?,
        favorite: r.get(10)?,
    })
}

const CONTACT_COLS: &str =
    "id, account_id, remote_id, display_name, email, phone, company, job_title, address, notes, favorite";

pub fn list_contacts(c: &Connection) -> Result<Vec<Contact>> {
    let mut st = c.prepare(&format!(
        "SELECT {CONTACT_COLS} FROM contacts ORDER BY display_name COLLATE NOCASE"
    ))?;
    let rows = st
        .query_map([], row_contact)?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

pub fn get_contact(c: &Connection, id: i64) -> Result<Option<Contact>> {
    Ok(c.query_row(
        &format!("SELECT {CONTACT_COLS} FROM contacts WHERE id = ?"),
        [id],
        row_contact,
    )
    .optional()?)
}

pub fn save_contact(c: &Connection, x: &Contact) -> Result<i64> {
    if x.id > 0 {
        c.execute(
            "UPDATE contacts SET remote_id=?, display_name=?, email=?, phone=?, company=?, job_title=?,
                    address=?, notes=?, favorite=? WHERE id=?",
            params![x.remote_id, x.display_name, x.email, x.phone, x.company, x.job_title, x.address,
                    x.notes, x.favorite, x.id],
        )?;
        Ok(x.id)
    } else {
        c.execute(
            "INSERT INTO contacts (account_id, remote_id, display_name, email, phone, company, job_title,
                                   address, notes, favorite)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![x.account_id, x.remote_id, x.display_name, x.email, x.phone, x.company, x.job_title,
                    x.address, x.notes, x.favorite],
        )?;
        Ok(c.last_insert_rowid())
    }
}

pub fn delete_contact(c: &Connection, id: i64) -> Result<()> {
    c.execute("DELETE FROM contacts WHERE id = ?", [id])?;
    Ok(())
}

// ---------------------------------------------------------------- tasks

fn row_task(r: &Row) -> rusqlite::Result<Task> {
    Ok(Task {
        id: r.get(0)?,
        account_id: r.get(1)?,
        remote_id: r.get(2)?,
        list_name: r.get(3)?,
        list_remote_id: r.get(4)?,
        title: r.get(5)?,
        due: r.get(6)?,
        completed: r.get(7)?,
        important: r.get(8)?,
        my_day: r.get(9)?,
        notes: r.get(10)?,
    })
}

const TASK_COLS: &str =
    "id, account_id, remote_id, list_name, list_remote_id, title, due, completed, important, my_day, notes";

pub fn list_tasks(c: &Connection) -> Result<Vec<Task>> {
    let mut st = c.prepare(&format!(
        "SELECT {TASK_COLS} FROM tasks ORDER BY completed, CASE WHEN due = '' THEN 1 ELSE 0 END, due, id"
    ))?;
    let rows = st
        .query_map([], row_task)?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

pub fn get_task(c: &Connection, id: i64) -> Result<Option<Task>> {
    Ok(c.query_row(
        &format!("SELECT {TASK_COLS} FROM tasks WHERE id = ?"),
        [id],
        row_task,
    )
    .optional()?)
}

pub fn save_task(c: &Connection, t: &Task) -> Result<i64> {
    if t.id > 0 {
        c.execute(
            "UPDATE tasks SET remote_id=?, list_name=?, list_remote_id=?, title=?, due=?, completed=?,
                    important=?, my_day=?, notes=? WHERE id=?",
            params![t.remote_id, t.list_name, t.list_remote_id, t.title, t.due, t.completed, t.important,
                    t.my_day, t.notes, t.id],
        )?;
        Ok(t.id)
    } else {
        c.execute(
            "INSERT INTO tasks (account_id, remote_id, list_name, list_remote_id, title, due, completed,
                                important, my_day, notes)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![t.account_id, t.remote_id, t.list_name, t.list_remote_id, t.title, t.due, t.completed,
                    t.important, t.my_day, t.notes],
        )?;
        Ok(c.last_insert_rowid())
    }
}

pub fn delete_task(c: &Connection, id: i64) -> Result<()> {
    c.execute("DELETE FROM tasks WHERE id = ?", [id])?;
    Ok(())
}

// ---------------------------------------------------------------- remote item replacement

/// Replace all remotely-synced items of `table` for an account (local-only rows,
/// i.e. those with an empty remote id, are kept).
pub fn clear_remote_items(c: &Connection, table: &str, account_id: i64) -> Result<()> {
    debug_assert!(matches!(table, "events" | "contacts" | "tasks"));
    c.execute(
        &format!("DELETE FROM {table} WHERE account_id = ? AND remote_id != ''"),
        [account_id],
    )?;
    Ok(())
}

// ---------------------------------------------------------------- notes & journal

pub fn list_notes(c: &Connection) -> Result<Vec<Note>> {
    let mut st = c.prepare("SELECT id, body, color, modified FROM notes ORDER BY modified DESC")?;
    let rows = st
        .query_map([], |r| {
            Ok(Note {
                id: r.get(0)?,
                body: r.get(1)?,
                color: r.get(2)?,
                modified: r.get(3)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

pub fn save_note(c: &Connection, n: &Note) -> Result<i64> {
    let now = crate::mail::now_utc();
    if n.id > 0 {
        c.execute(
            "UPDATE notes SET body = ?, color = ?, modified = ? WHERE id = ?",
            params![n.body, n.color, now, n.id],
        )?;
        Ok(n.id)
    } else {
        c.execute(
            "INSERT INTO notes (body, color, modified) VALUES (?, ?, ?)",
            params![n.body, n.color, now],
        )?;
        Ok(c.last_insert_rowid())
    }
}

pub fn delete_note(c: &Connection, id: i64) -> Result<()> {
    c.execute("DELETE FROM notes WHERE id = ?", [id])?;
    Ok(())
}

pub fn list_journal(c: &Connection) -> Result<Vec<JournalEntry>> {
    let mut st = c.prepare(
        "SELECT id, kind, subject, company, start, minutes, notes FROM journal ORDER BY start DESC",
    )?;
    let rows = st
        .query_map([], |r| {
            Ok(JournalEntry {
                id: r.get(0)?,
                kind: r.get(1)?,
                subject: r.get(2)?,
                company: r.get(3)?,
                start: r.get(4)?,
                minutes: r.get(5)?,
                notes: r.get(6)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

pub fn save_journal(c: &Connection, j: &JournalEntry) -> Result<i64> {
    if j.id > 0 {
        c.execute(
            "UPDATE journal SET kind=?, subject=?, company=?, start=?, minutes=?, notes=? WHERE id=?",
            params![j.kind, j.subject, j.company, j.start, j.minutes, j.notes, j.id],
        )?;
        Ok(j.id)
    } else {
        c.execute(
            "INSERT INTO journal (kind, subject, company, start, minutes, notes) VALUES (?, ?, ?, ?, ?, ?)",
            params![j.kind, j.subject, j.company, j.start, j.minutes, j.notes],
        )?;
        Ok(c.last_insert_rowid())
    }
}

pub fn delete_journal(c: &Connection, id: i64) -> Result<()> {
    c.execute("DELETE FROM journal WHERE id = ?", [id])?;
    Ok(())
}

// ---------------------------------------------------------------- settings

pub fn get_settings(c: &Connection) -> Result<serde_json::Map<String, serde_json::Value>> {
    let mut st = c.prepare("SELECT key, value FROM settings")?;
    let mut map = serde_json::Map::new();
    for row in st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
        let (k, v) = row?;
        map.insert(
            k,
            serde_json::from_str(&v).unwrap_or(serde_json::Value::String(v)),
        );
    }
    Ok(map)
}

pub fn set_setting(c: &Connection, key: &str, value: &serde_json::Value) -> Result<()> {
    c.execute(
        "INSERT INTO settings (key, value) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value.to_string()],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem() -> Connection {
        open(":memory:").unwrap()
    }

    #[test]
    fn folders_and_messages_roundtrip() {
        let c = mem();
        let acc = insert_account(&c, AccountKind::Local, "Test", "t@example.com", "{}").unwrap();
        let inbox = upsert_folder(&c, acc, "INBOX", "Posteingang", "inbox").unwrap();
        assert_eq!(
            upsert_folder(&c, acc, "INBOX", "Posteingang", "inbox").unwrap(),
            inbox
        );

        let m = NewMessage {
            remote_id: "1".into(),
            subject: "Hallo Welt".into(),
            from_name: "Alice".into(),
            date: "2026-01-01T10:00:00Z".into(),
            ..Default::default()
        };
        let id = upsert_message(&c, acc, inbox, &m).unwrap();
        assert_eq!(list_folders(&c).unwrap()[0].unread, 1);

        update_message_flags(&c, id, Some(true), Some(true), None, Some(&["Rot".into()])).unwrap();
        let got = get_message(&c, id).unwrap().unwrap();
        assert!(got.summary.is_read && got.summary.is_flagged);
        assert_eq!(got.summary.categories, vec!["Rot"]);

        let found = list_messages(
            &c,
            &MessageQuery {
                search: Some("welt".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(found.len(), 1);
        let none = list_messages(
            &c,
            &MessageQuery {
                search: Some("nope".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(none.is_empty());

        prune_messages(&c, inbox, &[]).unwrap();
        assert!(get_message(&c, id).unwrap().is_none());
    }

    #[test]
    fn attachments_roundtrip() {
        let c = mem();
        let acc = insert_account(&c, AccountKind::Local, "T", "t@x.de", "{}").unwrap();
        let f = upsert_folder(&c, acc, "inbox", "Posteingang", "inbox").unwrap();
        let m = NewMessage {
            remote_id: "1".into(),
            attachments: vec![NewAttachment {
                name: "a.txt".into(),
                mime: "text/plain".into(),
                data: b"hallo".to_vec(),
            }],
            ..Default::default()
        };
        let id = upsert_message(&c, acc, f, &m).unwrap();
        // A re-sync must not duplicate attachments.
        upsert_message(&c, acc, f, &m).unwrap();
        let list = list_attachments(&c, id).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].size, 5);
        assert_eq!(attachment_data(&c, list[0].id).unwrap(), b"hallo");
        assert!(
            get_message(&c, id)
                .unwrap()
                .unwrap()
                .summary
                .has_attachments
        );
    }

    #[test]
    fn notes_and_journal_roundtrip() {
        let c = mem();
        let id = save_note(
            &c,
            &Note {
                body: "Einkaufen\nMilch".into(),
                color: "yellow".into(),
                ..Default::default()
            },
        )
        .unwrap();
        save_note(
            &c,
            &Note {
                id,
                body: "Einkaufen\nBrot".into(),
                color: "blue".into(),
                ..Default::default()
            },
        )
        .unwrap();
        let notes = list_notes(&c).unwrap();
        assert_eq!((notes.len(), notes[0].color.as_str()), (1, "blue"));
        assert!(!notes[0].modified.is_empty());
        delete_note(&c, id).unwrap();
        assert!(list_notes(&c).unwrap().is_empty());

        let j = JournalEntry {
            kind: "Telefonanruf".into(),
            subject: "Anruf Kunde".into(),
            start: "2026-10-29T10:00:00Z".into(),
            minutes: 12,
            ..Default::default()
        };
        let jid = save_journal(&c, &j).unwrap();
        assert_eq!(list_journal(&c).unwrap()[0], JournalEntry { id: jid, ..j });
        delete_journal(&c, jid).unwrap();
        assert!(list_journal(&c).unwrap().is_empty());
    }

    #[test]
    fn events_range_query() {
        let c = mem();
        let acc = insert_account(&c, AccountKind::Local, "Test", "t@example.com", "{}").unwrap();
        let e = Event {
            account_id: acc,
            title: "Meeting".into(),
            start: "2026-03-10T09:00:00Z".into(),
            end: "2026-03-10T10:00:00Z".into(),
            ..Default::default()
        };
        save_event(&c, &e).unwrap();
        assert_eq!(
            list_events(&c, "2026-03-10T00:00:00Z", "2026-03-11T00:00:00Z")
                .unwrap()
                .len(),
            1
        );
        assert!(
            list_events(&c, "2026-03-11T00:00:00Z", "2026-03-12T00:00:00Z")
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn recurring_events_expand_in_range() {
        let c = mem();
        let acc = insert_account(&c, AccountKind::Local, "T", "t@x.de", "{}").unwrap();
        save_event(
            &c,
            &Event {
                account_id: acc,
                title: "Jour fixe".into(),
                start: "2026-01-05T09:00:00Z".into(),
                end: "2026-01-05T10:00:00Z".into(),
                recurrence: "weekly;1;1;".into(),
                ..Default::default()
            },
        )
        .unwrap();
        let march = list_events(&c, "2026-03-01T00:00:00Z", "2026-04-01T00:00:00Z").unwrap();
        assert_eq!(march.len(), 5); // Mondays in March 2026: 2, 9, 16, 23, 30
        assert!(march.iter().all(|e| e.title == "Jour fixe"));
    }

    #[test]
    fn migrates_old_event_table() {
        let path =
            std::env::temp_dir().join(format!("mailcraft-migrate-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        {
            let c = Connection::open(&path).unwrap();
            c.execute_batch("CREATE TABLE events (id INTEGER PRIMARY KEY, account_id INTEGER, remote_id TEXT, title TEXT, start TEXT, end TEXT, all_day INTEGER, location TEXT, body TEXT, attendees TEXT, show_as TEXT, category TEXT, reminder INTEGER)").unwrap();
        }
        let c = open(&path.to_string_lossy()).unwrap();
        assert!(
            list_events(&c, "2026-01-01T00:00:00Z", "2026-02-01T00:00:00Z")
                .unwrap()
                .is_empty()
        );
        drop(c);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn clear_remote_keeps_local_items() {
        let c = mem();
        let acc = insert_account(&c, AccountKind::Graph, "G", "g@example.com", "{}").unwrap();
        save_task(
            &c,
            &Task {
                account_id: acc,
                title: "lokal".into(),
                ..Default::default()
            },
        )
        .unwrap();
        save_task(
            &c,
            &Task {
                account_id: acc,
                title: "remote".into(),
                remote_id: "x".into(),
                ..Default::default()
            },
        )
        .unwrap();
        clear_remote_items(&c, "tasks", acc).unwrap();
        let tasks = list_tasks(&c).unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].title, "lokal");
    }
}
