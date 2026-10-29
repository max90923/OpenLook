//! Mail module: ribbon, folder tree, message list and reading pane.

use std::collections::HashSet;

use chrono::Datelike;
use eframe::egui::{self, RichText, Sense};
use egui_phosphor::regular as ph;

use super::{
    chrome::{self, IC_BLUE, IC_GREEN, IC_GREY, IC_PURPLE, IC_RED, IC_YELLOW},
    compose::ComposeKind,
    html, App, ReadingPane, Win, CATEGORIES,
};
use crate::{
    db,
    models::*,
    providers::{self, MessagePatch},
};

#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub enum Filter {
    #[default]
    All,
    Unread,
    Flagged,
}

#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub enum Sort {
    #[default]
    Date,
    From,
    Subject,
}

#[derive(Default)]
pub struct MailView {
    pub folder_id: Option<i64>,
    filter: Filter,
    sort: Sort,
    ascending: bool,
    search_all: bool,
    messages: Vec<MessageSummary>,
    selected: Option<i64>,
    current: Option<Message>,
    atts: Vec<Attachment>,
    collapsed: HashSet<&'static str>,
    favorites_closed: bool,
    closed_accounts: HashSet<i64>,
}

impl MailView {
    pub fn count(&self) -> usize {
        self.messages.len()
    }

    pub fn unread(&self) -> usize {
        self.messages.iter().filter(|m| !m.is_read).count()
    }
}

pub(super) enum Act {
    Folder(i64),
    Select(i64),
    Open(i64),
    Patch(i64, MessagePatch),
    Move(i64, i64),
    Delete(i64),
    Compose(ComposeKind),
    MarkFolderRead(i64),
    OpenInBrowser(i64),
    OpenAttachment(Attachment),
    SaveAttachment(Attachment),
    SaveAllAttachments(i64),
    Toast(String),
}

impl App {
    pub fn reload_messages(&mut self) {
        if self
            .mail
            .folder_id
            .is_none_or(|id| !self.data.folders.iter().any(|f| f.id == id))
        {
            self.mail.folder_id = self
                .data
                .folders
                .iter()
                .find(|f| f.role == "inbox")
                .map(|f| f.id);
        }
        let searching = !self.search.trim().is_empty();
        let q = db::MessageQuery {
            folder_id: if searching && self.mail.search_all {
                None
            } else {
                self.mail.folder_id
            },
            search: Some(self.search.clone()),
            unread_only: self.mail.filter == Filter::Unread,
            flagged_only: self.mail.filter == Filter::Flagged,
            limit: 2000,
        };
        let c = self.st.db();
        let mut list = db::list_messages(&c, &q).unwrap_or_default();
        match self.mail.sort {
            Sort::Date => {
                if self.mail.ascending {
                    list.reverse();
                }
            }
            Sort::From => list.sort_by_key(|m| sender(m).to_lowercase()),
            Sort::Subject => list.sort_by_key(|m| strip_prefixes(&m.subject).to_lowercase()),
        }
        if self.mail.sort != Sort::Date && !self.mail.ascending {
            list.reverse();
        }
        self.mail.messages = list;
        self.mail.current = self
            .mail
            .selected
            .and_then(|id| db::get_message(&c, id).ok().flatten());
        self.mail.atts = self
            .mail
            .current
            .as_ref()
            .and_then(|m| db::list_attachments(&c, m.summary.id).ok())
            .unwrap_or_default();
        if self.mail.current.is_none() {
            self.mail.selected = None;
        }
    }

    pub(super) fn mail_apply(&mut self, acts: Vec<Act>) {
        for a in acts {
            match a {
                Act::Folder(id) => {
                    self.mail.folder_id = Some(id);
                    self.mail.selected = None;
                    self.search.clear();
                    self.reload_messages();
                }
                Act::Select(id) => {
                    self.mail.selected = Some(id);
                    self.reload_messages();
                    let missing = self
                        .mail
                        .current
                        .as_ref()
                        .is_some_and(|m| m.summary.has_attachments)
                        && self.mail.atts.is_empty();
                    if missing {
                        self.spawn(None, move |st, _| async move {
                            providers::ensure_attachments(&st, id).await
                        });
                    }
                    let unread = self
                        .mail
                        .current
                        .as_ref()
                        .is_some_and(|m| !m.summary.is_read);
                    if unread && self.prefs.mark_read && self.prefs.reading_pane != ReadingPane::Off
                    {
                        self.patch_message(
                            id,
                            MessagePatch {
                                is_read: Some(true),
                                ..Default::default()
                            },
                        );
                    }
                }
                Act::Open(id) => {
                    let draft = self
                        .mail
                        .messages
                        .iter()
                        .find(|m| m.id == id)
                        .is_some_and(|m| {
                            self.folder_for_role(m.account_id, "drafts") == Some(m.folder_id)
                        });
                    if draft {
                        self.open_composer(ComposeKind::EditDraft(id));
                    } else {
                        self.open_window(Win::Compose(super::compose::Composer::reader(id)));
                        self.patch_message(
                            id,
                            MessagePatch {
                                is_read: Some(true),
                                ..Default::default()
                            },
                        );
                    }
                }
                Act::Patch(id, p) => self.patch_message(id, p),
                Act::Move(id, folder) => {
                    if self.mail.selected == Some(id) {
                        self.select_next(id);
                    }
                    self.spawn(Some("Element verschoben."), move |st, _| async move {
                        providers::move_message(&st, id, folder).await
                    });
                }
                Act::Delete(id) => {
                    if self.mail.selected == Some(id) {
                        self.select_next(id);
                    }
                    self.spawn(None, move |st, _| async move {
                        providers::delete_message(&st, id).await
                    });
                }
                Act::Compose(kind) => self.open_composer(kind),
                Act::MarkFolderRead(folder) => {
                    let ids: Vec<i64> = {
                        let c = self.st.db();
                        db::list_messages(
                            &c,
                            &db::MessageQuery {
                                folder_id: Some(folder),
                                unread_only: true,
                                limit: 5000,
                                ..Default::default()
                            },
                        )
                        .unwrap_or_default()
                        .into_iter()
                        .map(|m| m.id)
                        .collect()
                    };
                    self.spawn(
                        Some("Alle Elemente als gelesen markiert."),
                        move |st, _| async move {
                            for id in ids {
                                providers::patch_message(
                                    &st,
                                    id,
                                    &MessagePatch {
                                        is_read: Some(true),
                                        ..Default::default()
                                    },
                                )
                                .await?;
                            }
                            Ok(())
                        },
                    );
                }
                Act::OpenInBrowser(id) => {
                    let m = db::get_message(&self.st.db(), id).ok().flatten();
                    if let Some(m) = m {
                        let path =
                            std::env::temp_dir().join(format!("mailcraft-{}.html", m.summary.id));
                        let html = format!(
                            "<!doctype html><meta charset=\"utf-8\"><title>{}</title>{}",
                            crate::mail::escape_html(&m.summary.subject),
                            m.body_html
                        );
                        match std::fs::write(&path, html) {
                            Ok(()) => super::open_external(&path.to_string_lossy()),
                            Err(e) => self.toast(format!("{e}"), true),
                        }
                    }
                }
                Act::OpenAttachment(a) => {
                    let res = db::attachment_data(&self.st.db(), a.id).and_then(|data| {
                        let dir = std::env::temp_dir()
                            .join(format!("mailcraft-anlagen-{}", a.message_id));
                        std::fs::create_dir_all(&dir)?;
                        let path = dir.join(safe_file_name(&a.name));
                        std::fs::write(&path, data)?;
                        Ok(path)
                    });
                    match res {
                        Ok(path) => super::open_external(&path.to_string_lossy()),
                        Err(e) => {
                            self.toast(format!("Anlage konnte nicht geöffnet werden: {e:#}"), true)
                        }
                    }
                }
                Act::SaveAttachment(a) => {
                    if let Some(path) = rfd::FileDialog::new()
                        .set_title("Anlage speichern")
                        .set_file_name(safe_file_name(&a.name))
                        .save_file()
                    {
                        let res = db::attachment_data(&self.st.db(), a.id)
                            .and_then(|d| Ok(std::fs::write(&path, d)?));
                        match res {
                            Ok(()) => self.toast(format!("Gespeichert: {}", path.display()), false),
                            Err(e) => self.toast(format!("{e:#}"), true),
                        }
                    }
                }
                Act::SaveAllAttachments(id) => {
                    if let Some(dir) = rfd::FileDialog::new()
                        .set_title("Alle Anlagen speichern")
                        .pick_folder()
                    {
                        let res = db::attachments_with_data(&self.st.db(), id).and_then(|list| {
                            for a in &list {
                                std::fs::write(dir.join(safe_file_name(&a.name)), &a.data)?;
                            }
                            Ok(list.len())
                        });
                        match res {
                            Ok(n) => self.toast(
                                format!("{n} Anlage(n) gespeichert in {}", dir.display()),
                                false,
                            ),
                            Err(e) => self.toast(format!("{e:#}"), true),
                        }
                    }
                }
                Act::Toast(t) => self.toast(t, true),
            }
        }
    }

    fn patch_message(&mut self, id: i64, p: MessagePatch) {
        // Optimistic local update so the list reacts immediately.
        let _ = db::update_message_flags(
            &self.st.db(),
            id,
            p.is_read,
            p.is_flagged,
            p.is_pinned,
            p.categories.as_deref(),
        );
        self.reload_messages();
        self.data.folders = db::list_folders(&self.st.db()).unwrap_or_default();
        self.spawn(None, move |st, _| async move {
            providers::patch_message(&st, id, &p).await
        });
    }

    /// Select the message after `id` (like Outlook does after delete/move).
    fn select_next(&mut self, id: i64) {
        let pos = self.mail.messages.iter().position(|m| m.id == id);
        self.mail.selected = pos
            .and_then(|p| {
                self.mail
                    .messages
                    .get(p + 1)
                    .or(p.checked_sub(1).and_then(|q| self.mail.messages.get(q)))
            })
            .map(|m| m.id);
    }

    pub(super) fn folder_for_role(&self, account_id: i64, role: &str) -> Option<i64> {
        self.data
            .folders
            .iter()
            .find(|f| f.account_id == account_id && f.role == role)
            .map(|f| f.id)
    }

    // ------------------------------------------------------------ ribbon

    pub fn mail_ribbon(&mut self, ui: &mut egui::Ui) {
        if self.ribbon_tab != 0 {
            let done = self.common_ribbon(ui);
            if !done && self.ribbon_tab == 3 {
                self.mail_view_ribbon(ui);
            }
            return;
        }
        let t = self.t;
        let sel = self.mail.current.as_ref().map(|m| m.summary.clone());
        let has = sel.is_some();
        let mut acts = vec![];
        chrome::group(ui, &t, "Neu", |ui| {
            if chrome::large(ui, &t, ph::ENVELOPE_SIMPLE, IC_BLUE, "Neue\nE-Mail", true).clicked() {
                acts.push(Act::Compose(ComposeKind::New));
            }
            let r = chrome::large(
                ui,
                &t,
                ph::FILE_PLUS,
                IC_YELLOW,
                "Neue\nElemente \u{E136}",
                true,
            );
            chrome::menu(&r, |ui| {
                if chrome::menu_item(ui, ph::ENVELOPE_SIMPLE, IC_BLUE, "E-Mail-Nachricht") {
                    acts.push(Act::Compose(ComposeKind::New));
                }
                if chrome::menu_item(ui, ph::CALENDAR_BLANK, IC_BLUE, "Termin") {
                    self.new_event_now();
                }
                if chrome::menu_item(ui, ph::USER, IC_BLUE, "Kontakt") {
                    self.new_contact();
                }
                if chrome::menu_item(ui, ph::CHECK_SQUARE_OFFSET, IC_GREEN, "Aufgabe") {
                    self.new_task();
                }
            });
        });
        chrome::group(ui, &t, "Löschen", |ui| {
            if chrome::large(ui, &t, ph::X, IC_RED, "Löschen", has).clicked() {
                acts.push(Act::Delete(sel.as_ref().unwrap().id));
            }
            if chrome::large(ui, &t, ph::ARCHIVE, IC_GREY, "Archivieren", has).clicked() {
                let m = sel.as_ref().unwrap();
                match self.folder_for_role(m.account_id, "archive") {
                    Some(f) => acts.push(Act::Move(m.id, f)),
                    None => acts.push(Act::Toast(
                        "Für dieses Konto ist kein Archivordner vorhanden.".into(),
                    )),
                }
            }
            chrome::column(ui, |ui| {
                let r = chrome::small(ui, &t, ph::PROHIBIT, IC_RED, "Junk-E-Mail \u{E136}", has);
                chrome::menu(&r, |ui| {
                    if chrome::menu_item(ui, ph::PROHIBIT, IC_RED, "In Junk-E-Mail verschieben") {
                        let m = sel.as_ref().unwrap();
                        if let Some(f) = self.folder_for_role(m.account_id, "junk") {
                            acts.push(Act::Move(m.id, f));
                        }
                    }
                    if chrome::menu_item(ui, ph::TRAY, IC_BLUE, "Keine Junk-E-Mail") {
                        let m = sel.as_ref().unwrap();
                        if let Some(f) = self.folder_for_role(m.account_id, "inbox") {
                            acts.push(Act::Move(m.id, f));
                        }
                    }
                });
            });
        });
        chrome::group(ui, &t, "Antworten", |ui| {
            for (icon, label, kind) in [
                (ph::ARROW_BEND_UP_LEFT, "Antworten", 0),
                (ph::ARROW_BEND_DOUBLE_UP_LEFT, "Allen\nantworten", 1),
                (ph::ARROW_BEND_UP_RIGHT, "Weiterleiten", 2),
            ] {
                if chrome::large(ui, &t, icon, IC_PURPLE, label, has).clicked() {
                    let id = sel.as_ref().unwrap().id;
                    acts.push(Act::Compose(match kind {
                        0 => ComposeKind::Reply(id),
                        1 => ComposeKind::ReplyAll(id),
                        _ => ComposeKind::Forward(id),
                    }));
                }
            }
        });
        chrome::group(ui, &t, "Verschieben", |ui| {
            let r = chrome::large(
                ui,
                &t,
                ph::FOLDER_SIMPLE,
                IC_YELLOW,
                "Verschieben\n\u{E136}",
                has,
            );
            if let Some(m) = &sel {
                chrome::menu(&r, |ui| self.move_menu(ui, m, &mut acts));
            }
        });
        chrome::group(ui, &t, "Kategorien", |ui| {
            chrome::column(ui, |ui| {
                let read = sel.as_ref().is_some_and(|m| m.is_read);
                if chrome::small(
                    ui,
                    &t,
                    if read {
                        ph::ENVELOPE_SIMPLE
                    } else {
                        ph::ENVELOPE_SIMPLE_OPEN
                    },
                    IC_BLUE,
                    "Ungelesen/Gelesen",
                    has,
                )
                .clicked()
                {
                    let m = sel.as_ref().unwrap();
                    acts.push(Act::Patch(
                        m.id,
                        MessagePatch {
                            is_read: Some(!m.is_read),
                            ..Default::default()
                        },
                    ));
                }
                let r = chrome::small(ui, &t, ph::TAG, IC_PURPLE, "Kategorisieren \u{E136}", has);
                if let Some(m) = &sel {
                    chrome::menu(&r, |ui| category_menu(ui, m, &mut acts));
                }
                let r = chrome::small(ui, &t, ph::FLAG, IC_RED, "Zur Nachverfolgung \u{E136}", has);
                if let Some(m) = &sel {
                    chrome::menu(&r, |ui| {
                        if chrome::menu_item(ui, ph::FLAG, IC_RED, "Kennzeichnen") {
                            acts.push(Act::Patch(
                                m.id,
                                MessagePatch {
                                    is_flagged: Some(true),
                                    ..Default::default()
                                },
                            ));
                        }
                        if chrome::menu_item(
                            ui,
                            ph::CHECK,
                            IC_GREEN,
                            "Als erledigt markieren / Kennzeichnung löschen",
                        ) {
                            acts.push(Act::Patch(
                                m.id,
                                MessagePatch {
                                    is_flagged: Some(false),
                                    ..Default::default()
                                },
                            ));
                        }
                    });
                }
            });
            chrome::column(ui, |ui| {
                let pinned = sel.as_ref().is_some_and(|m| m.is_pinned);
                if chrome::small(
                    ui,
                    &t,
                    ph::PUSH_PIN,
                    IC_BLUE,
                    if pinned { "Lösen" } else { "Anheften" },
                    has,
                )
                .clicked()
                {
                    let m = sel.as_ref().unwrap();
                    acts.push(Act::Patch(
                        m.id,
                        MessagePatch {
                            is_pinned: Some(!m.is_pinned),
                            ..Default::default()
                        },
                    ));
                }
            });
        });
        chrome::group(ui, &t, "Suchen", |ui| {
            chrome::column(ui, |ui| {
                if chrome::small(ui, &t, ph::ADDRESS_BOOK, IC_BLUE, "Adressbuch", true).clicked() {
                    self.switch_module(super::Module::People);
                }
                let r = chrome::small(ui, &t, ph::FUNNEL, IC_BLUE, "E-Mail filtern \u{E136}", true);
                chrome::menu(&r, |ui| {
                    for (f, label, icon) in [
                        (Filter::All, "Alle E-Mails", ph::ENVELOPE_SIMPLE),
                        (Filter::Unread, "Ungelesen", ph::ENVELOPE_SIMPLE_OPEN),
                        (Filter::Flagged, "Gekennzeichnet", ph::FLAG),
                    ] {
                        if chrome::menu_item(ui, icon, IC_BLUE, label) {
                            self.mail.filter = f;
                            self.dirty = true;
                        }
                    }
                });
            });
        });
        self.mail_apply(acts);
    }

    fn mail_view_ribbon(&mut self, ui: &mut egui::Ui) {
        let t = self.t;
        chrome::group(ui, &t, "Layout", |ui| {
            let r = chrome::large(ui, &t, ph::LAYOUT, IC_BLUE, "Lesebereich\n\u{E136}", true);
            chrome::menu(&r, |ui| {
                for (p, label, key) in [
                    (ReadingPane::Right, "Rechts", "right"),
                    (ReadingPane::Bottom, "Unten", "bottom"),
                    (ReadingPane::Off, "Aus", "off"),
                ] {
                    if ui.radio(self.prefs.reading_pane == p, label).clicked() {
                        self.prefs.reading_pane = p;
                        self.save_setting("reading_pane", serde_json::json!(key));
                        ui.close();
                    }
                }
            });
        });
        chrome::group(ui, &t, "Nachrichtenvorschau", |ui| {
            let r = chrome::large(
                ui,
                &t,
                ph::ROWS,
                IC_BLUE,
                "Nachrichten-\nvorschau \u{E136}",
                true,
            );
            chrome::menu(&r, |ui| {
                for (v, label) in [(false, "Aus"), (true, "1 Zeile")] {
                    if ui.radio(self.prefs.preview == v, label).clicked() {
                        self.prefs.preview = v;
                        self.save_setting("preview", serde_json::json!(v));
                        ui.close();
                    }
                }
            });
        });
        chrome::group(ui, &t, "Anordnung", |ui| {
            chrome::column(ui, |ui| {
                for (s, label, icon) in [
                    (Sort::Date, "Datum", ph::CALENDAR_BLANK),
                    (Sort::From, "Von", ph::USER),
                    (Sort::Subject, "Betreff", ph::TEXT_AA),
                ] {
                    let txt = if self.mail.sort == s {
                        format!("{label}  \u{E182}")
                    } else {
                        label.to_string()
                    };
                    if chrome::small(ui, &t, icon, IC_BLUE, &txt, true).clicked() {
                        self.mail.sort = s;
                        self.dirty = true;
                    }
                }
            });
            chrome::column(ui, |ui| {
                if chrome::small(
                    ui,
                    &t,
                    ph::ARROWS_DOWN_UP,
                    IC_BLUE,
                    "Sortierung umkehren",
                    true,
                )
                .clicked()
                {
                    self.mail.ascending = !self.mail.ascending;
                    self.dirty = true;
                }
            });
        });
    }

    fn move_menu(&self, ui: &mut egui::Ui, m: &MessageSummary, acts: &mut Vec<Act>) {
        for f in self
            .data
            .folders
            .iter()
            .filter(|f| f.account_id == m.account_id && f.id != m.folder_id)
        {
            if chrome::menu_item(ui, folder_icon(&f.role), IC_YELLOW, &f.name) {
                acts.push(Act::Move(m.id, f.id));
            }
        }
    }

    // ------------------------------------------------------------ folder pane

    pub fn mail_folders(&mut self, ui: &mut egui::Ui) {
        let mut acts = vec![];
        egui::ScrollArea::vertical()
            .id_salt("folders_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                let arrow = if self.mail.favorites_closed {
                    ph::CARET_RIGHT
                } else {
                    ph::CARET_DOWN
                };
                if ui
                    .add(
                        egui::Button::new(RichText::new(format!("{arrow}  Favoriten")).strong())
                            .frame(false),
                    )
                    .clicked()
                {
                    self.mail.favorites_closed = !self.mail.favorites_closed;
                }
                if !self.mail.favorites_closed {
                    for role in ["inbox", "sent", "trash"] {
                        for f in self.data.folders.iter().filter(|f| f.role == role) {
                            let multi = self.data.accounts.len() > 1 && role == "inbox";
                            let label = if multi {
                                format!(
                                    "{} – {}",
                                    f.name,
                                    self.data
                                        .account(f.account_id)
                                        .map(|a| a.name.as_str())
                                        .unwrap_or_default()
                                )
                            } else {
                                f.name.clone()
                            };
                            self.folder_row(ui, f, &label, 14.0, &mut acts);
                        }
                        if self.data.accounts.len() > 1 {
                            break;
                        }
                    }
                }
                ui.add_space(10.0);
                for acc in self.data.accounts.clone() {
                    let closed = self.mail.closed_accounts.contains(&acc.id);
                    let arrow = if closed {
                        ph::CARET_RIGHT
                    } else {
                        ph::CARET_DOWN
                    };
                    let r = ui.add(
                        egui::Button::new(
                            RichText::new(format!("{arrow}  {}", acc.email)).strong(),
                        )
                        .frame(false)
                        .truncate(),
                    );
                    if r.clicked() {
                        if closed {
                            self.mail.closed_accounts.remove(&acc.id);
                        } else {
                            self.mail.closed_accounts.insert(acc.id);
                        }
                    }
                    if let Some(e) = &acc.last_error {
                        ui.horizontal(|ui| {
                            ui.add_space(22.0);
                            ui.label(
                                RichText::new(format!("{}  Getrennt", ph::WARNING))
                                    .small()
                                    .color(self.t.danger),
                            )
                            .on_hover_text(e);
                        });
                    }
                    if !closed {
                        for f in self.data.folders.iter().filter(|f| f.account_id == acc.id) {
                            self.folder_row(ui, f, &f.name, 26.0, &mut acts);
                        }
                    }
                    ui.add_space(8.0);
                }
            });
        self.mail_apply(acts);
    }

    fn folder_row(
        &self,
        ui: &mut egui::Ui,
        f: &Folder,
        label: &str,
        indent: f32,
        acts: &mut Vec<Act>,
    ) {
        let t = &self.t;
        let active = self.mail.folder_id == Some(f.id)
            && (self.search.trim().is_empty() || !self.mail.search_all);
        let w = ui.available_width();
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, 24.0), Sense::click());
        let drop = resp.dnd_hover_payload::<i64>().is_some();
        if active {
            ui.painter().rect_filled(rect, 0, t.selected);
        } else if resp.hovered() || drop {
            ui.painter().rect_filled(rect, 0, t.hover);
        }
        if drop {
            ui.painter().rect_stroke(
                rect,
                0,
                egui::Stroke::new(1.0_f32, t.accent),
                egui::StrokeKind::Inside,
            );
        }
        let p = ui.painter();
        let show_count = f.role != "sent" && f.role != "drafts";
        let bold = f.unread > 0 && show_count;
        let font = if bold {
            chrome::bold_font(13.0)
        } else {
            egui::FontId::proportional(13.0)
        };
        let name = super::one_line(p, label, font, t.text, w - indent - 50.0);
        let name_w = name.size().x;
        p.galley(
            egui::pos2(rect.left() + indent, rect.center().y - name.size().y / 2.0),
            name,
            t.text,
        );
        let count = if show_count && f.unread > 0 {
            Some(f.unread.to_string())
        } else if f.role == "drafts" && f.total > 0 {
            Some(format!("[{}]", f.total))
        } else {
            None
        };
        if let Some(c) = count {
            p.text(
                egui::pos2(rect.left() + indent + name_w + 8.0, rect.center().y),
                egui::Align2::LEFT_CENTER,
                c,
                egui::FontId::proportional(13.0),
                t.accent,
            );
        }
        if let Some(id) = resp.dnd_release_payload::<i64>() {
            acts.push(Act::Move(*id, f.id));
        }
        if resp.clicked() {
            acts.push(Act::Folder(f.id));
        }
        resp.context_menu(|ui| {
            if chrome::menu_item(
                ui,
                ph::ENVELOPE_SIMPLE_OPEN,
                IC_BLUE,
                "Alle als gelesen markieren",
            ) {
                acts.push(Act::MarkFolderRead(f.id));
            }
        });
    }

    // ------------------------------------------------------------ content

    pub fn mail_ui(&mut self, ui: &mut egui::Ui) {
        let mut acts = vec![];
        self.mail_shortcuts(ui.ctx(), &mut acts);
        let t = self.t;
        let list_frame = egui::Frame::new()
            .fill(t.pane)
            .stroke(egui::Stroke::new(1.0_f32, t.border));
        match self.prefs.reading_pane {
            ReadingPane::Right => {
                egui::SidePanel::left("message_list")
                    .resizable(true)
                    .default_width(380.0)
                    .width_range(260.0..=700.0)
                    .frame(list_frame)
                    .show_inside(ui, |ui| self.message_list(ui, &mut acts));
                egui::CentralPanel::default()
                    .frame(egui::Frame::new().fill(t.pane))
                    .show_inside(ui, |ui| self.reading_pane(ui, &mut acts));
            }
            ReadingPane::Bottom => {
                egui::TopBottomPanel::top("message_list_top")
                    .resizable(true)
                    .default_height(300.0)
                    .height_range(140.0..=700.0)
                    .frame(list_frame)
                    .show_inside(ui, |ui| self.message_list(ui, &mut acts));
                egui::CentralPanel::default()
                    .frame(egui::Frame::new().fill(t.pane))
                    .show_inside(ui, |ui| self.reading_pane(ui, &mut acts));
            }
            ReadingPane::Off => {
                egui::CentralPanel::default()
                    .frame(list_frame)
                    .show_inside(ui, |ui| self.message_list(ui, &mut acts));
            }
        }
        self.mail_apply(acts);
    }

    fn mail_shortcuts(&self, ctx: &egui::Context, acts: &mut Vec<Act>) {
        if ctx.wants_keyboard_input() {
            return;
        }
        let sel = self.mail.current.as_ref().map(|m| &m.summary);
        ctx.input(|i| {
            let cmd = i.modifiers.command;
            if let Some(m) = sel {
                if i.key_pressed(egui::Key::Delete) {
                    acts.push(Act::Delete(m.id));
                }
                if cmd && i.key_pressed(egui::Key::R) {
                    acts.push(Act::Compose(if i.modifiers.shift {
                        ComposeKind::ReplyAll(m.id)
                    } else {
                        ComposeKind::Reply(m.id)
                    }));
                }
                if cmd && i.key_pressed(egui::Key::F) {
                    acts.push(Act::Compose(ComposeKind::Forward(m.id)));
                }
                if cmd && i.key_pressed(egui::Key::Q) {
                    acts.push(Act::Patch(
                        m.id,
                        MessagePatch {
                            is_read: Some(true),
                            ..Default::default()
                        },
                    ));
                }
                if cmd && i.key_pressed(egui::Key::U) {
                    acts.push(Act::Patch(
                        m.id,
                        MessagePatch {
                            is_read: Some(false),
                            ..Default::default()
                        },
                    ));
                }
                if i.key_pressed(egui::Key::Insert) {
                    acts.push(Act::Patch(
                        m.id,
                        MessagePatch {
                            is_flagged: Some(!m.is_flagged),
                            ..Default::default()
                        },
                    ));
                }
                if i.key_pressed(egui::Key::Enter) {
                    acts.push(Act::Open(m.id));
                }
            }
            let pos = sel.and_then(|m| self.mail.messages.iter().position(|x| x.id == m.id));
            if i.key_pressed(egui::Key::ArrowDown) {
                if let Some(m) = self.mail.messages.get(pos.map(|p| p + 1).unwrap_or(0)) {
                    acts.push(Act::Select(m.id));
                }
            }
            if i.key_pressed(egui::Key::ArrowUp) {
                if let Some(m) = pos
                    .and_then(|p| p.checked_sub(1))
                    .and_then(|p| self.mail.messages.get(p))
                {
                    acts.push(Act::Select(m.id));
                }
            }
        });
    }

    fn message_list(&mut self, ui: &mut egui::Ui, acts: &mut Vec<Act>) {
        let t = self.t;
        egui::Frame::new()
            .inner_margin(egui::Margin {
                left: 8,
                right: 8,
                top: 8,
                bottom: 2,
            })
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let hint = if self.mail.search_all {
                        "Alle Postfächer durchsuchen (Strg+E)"
                    } else {
                        "Aktuellen Ordner durchsuchen (Strg+E)"
                    };
                    let w = ui.available_width() - 130.0;
                    ui.allocate_ui(egui::vec2(w, 26.0), |ui| {
                        if super::search_box(ui, &t, &mut self.search, hint).changed() {
                            self.dirty = true;
                        }
                    });
                    egui::ComboBox::from_id_salt("search_scope")
                        .width(120.0)
                        .selected_text(if self.mail.search_all {
                            "Alle Postfächer"
                        } else {
                            "Aktueller Ordner"
                        })
                        .show_ui(ui, |ui| {
                            if ui
                                .selectable_value(
                                    &mut self.mail.search_all,
                                    false,
                                    "Aktueller Ordner",
                                )
                                .clicked()
                                | ui.selectable_value(
                                    &mut self.mail.search_all,
                                    true,
                                    "Alle Postfächer",
                                )
                                .clicked()
                            {
                                self.dirty = true;
                            }
                        });
                });
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    for (f, label) in [(Filter::All, "Alle"), (Filter::Unread, "Ungelesen")] {
                        let active = self.mail.filter == f;
                        let text = RichText::new(label).size(13.0).color(if active {
                            t.accent
                        } else {
                            t.text
                        });
                        let r = ui.add(
                            egui::Button::new(if active { text.underline() } else { text })
                                .frame(false),
                        );
                        if r.clicked() {
                            self.mail.filter = f;
                            self.dirty = true;
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let dir = match (self.mail.sort, self.mail.ascending) {
                            (Sort::Date, false) => format!("Neueste {}", ph::ARROW_DOWN),
                            (Sort::Date, true) => format!("Älteste {}", ph::ARROW_UP),
                            (_, false) => format!("A bis Z {}", ph::ARROW_DOWN),
                            (_, true) => format!("Z bis A {}", ph::ARROW_UP),
                        };
                        if ui
                            .add(egui::Button::new(RichText::new(dir).size(12.0)).frame(false))
                            .clicked()
                        {
                            self.mail.ascending = !self.mail.ascending;
                            self.dirty = true;
                        }
                        let by = match self.mail.sort {
                            Sort::Date => "Nach Datum",
                            Sort::From => "Nach Von",
                            Sort::Subject => "Nach Betreff",
                        };
                        let r = ui.add(
                            egui::Button::new(
                                RichText::new(format!("{by} {}", ph::CARET_DOWN)).size(12.0),
                            )
                            .frame(false),
                        );
                        chrome::menu(&r, |ui| {
                            for (s, label) in [
                                (Sort::Date, "Datum"),
                                (Sort::From, "Von"),
                                (Sort::Subject, "Betreff"),
                            ] {
                                if ui.radio(self.mail.sort == s, label).clicked() {
                                    self.mail.sort = s;
                                    self.dirty = true;
                                    ui.close();
                                }
                            }
                        });
                    });
                });
            });
        ui.painter().hline(
            ui.max_rect().x_range(),
            ui.cursor().top(),
            egui::Stroke::new(1.0_f32, t.border),
        );

        if self.mail.messages.is_empty() {
            ui.add_space(30.0);
            ui.vertical_centered(|ui| {
                let msg = if self.search.trim().is_empty() {
                    "In dieser Ansicht gibt es keine Elemente."
                } else {
                    "Es wurden keine Ergebnisse gefunden."
                };
                ui.label(RichText::new(msg).color(t.text_weak));
            });
            return;
        }
        let in_search = !self.search.trim().is_empty() && self.mail.search_all;
        let messages = std::mem::take(&mut self.mail.messages);
        egui::ScrollArea::vertical()
            .id_salt("msg_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                let mut last_group = "";
                let mut hidden = false;
                for m in &messages {
                    let group = if m.is_pinned {
                        "Angeheftet"
                    } else if self.mail.sort == Sort::Date {
                        date_group(&m.date)
                    } else {
                        ""
                    };
                    if group != last_group && !group.is_empty() {
                        last_group = group;
                        hidden = self.mail.collapsed.contains(group);
                        let arrow = if hidden {
                            ph::CARET_RIGHT
                        } else {
                            ph::CARET_DOWN
                        };
                        let r = ui.add(
                            egui::Button::new(
                                RichText::new(format!("  {arrow}  {group}"))
                                    .size(12.5)
                                    .strong()
                                    .color(t.text),
                            )
                            .frame(false)
                            .min_size(egui::vec2(ui.available_width(), 26.0)),
                        );
                        if r.clicked() {
                            if hidden {
                                self.mail.collapsed.remove(group);
                            } else {
                                self.mail.collapsed.insert(group);
                            }
                            hidden = !hidden;
                        }
                    }
                    if hidden {
                        continue;
                    }
                    let folder_name = in_search
                        .then(|| {
                            self.data
                                .folders
                                .iter()
                                .find(|f| f.id == m.folder_id)
                                .map(|f| f.name.as_str())
                        })
                        .flatten();
                    self.message_row(ui, m, folder_name, acts);
                }
            });
        self.mail.messages = messages;
    }

    fn message_row(
        &self,
        ui: &mut egui::Ui,
        m: &MessageSummary,
        folder_name: Option<&str>,
        acts: &mut Vec<Act>,
    ) {
        let t = &self.t;
        let selected = self.mail.selected == Some(m.id);
        let w = ui.available_width();
        let h = if self.prefs.preview { 62.0 } else { 44.0 };
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, h), Sense::click_and_drag());
        resp.dnd_set_drag_payload(m.id);
        let p = ui.painter_at(rect);
        if selected {
            p.rect_filled(rect, 0, t.selected);
        } else if resp.hovered() {
            p.rect_filled(rect, 0, t.hover);
        }
        if !m.is_read {
            p.rect_filled(
                egui::Rect::from_min_size(
                    rect.min + egui::vec2(0.0, 2.0),
                    egui::vec2(3.0, h - 4.0),
                ),
                0,
                t.accent,
            );
        }
        p.hline(
            rect.x_range(),
            rect.bottom() - 0.5,
            egui::Stroke::new(1.0_f32, t.border.gamma_multiply(0.6)),
        );

        let x = rect.left() + 14.0;
        let right = rect.right() - 24.0;
        let unread = !m.is_read;
        // Line 1: sender, right side: categories, attachment, flag/delete.
        let mut rx = right;
        let hover = resp.hovered() || selected;
        let mut icon_hits: Vec<(egui::Rect, u8)> = vec![];
        let flag_color = if m.is_flagged { t.flag } else { t.text_weak };
        if m.is_flagged || hover {
            let r = egui::Rect::from_center_size(
                egui::pos2(rx - 7.0, rect.top() + 13.0),
                egui::vec2(16.0, 16.0),
            );
            p.text(
                r.center(),
                egui::Align2::CENTER_CENTER,
                if m.is_flagged {
                    egui_phosphor::fill::FLAG
                } else {
                    ph::FLAG
                },
                chrome::fill_font(14.0),
                flag_color,
            );
            icon_hits.push((r, 1));
        }
        rx -= 20.0;
        if hover {
            let r = egui::Rect::from_center_size(
                egui::pos2(rx - 7.0, rect.top() + 13.0),
                egui::vec2(16.0, 16.0),
            );
            p.text(
                r.center(),
                egui::Align2::CENTER_CENTER,
                ph::X,
                egui::FontId::proportional(14.0),
                t.text_weak,
            );
            icon_hits.push((r, 0));
            rx -= 20.0;
        }
        if m.has_attachments {
            p.text(
                egui::pos2(rx - 7.0, rect.top() + 13.0),
                egui::Align2::CENTER_CENTER,
                ph::PAPERCLIP,
                egui::FontId::proportional(13.0),
                t.text_weak,
            );
            rx -= 18.0;
        }
        for c in m.categories.iter().rev() {
            let r = egui::Rect::from_center_size(
                egui::pos2(rx - 6.0, rect.top() + 13.0),
                egui::vec2(10.0, 10.0),
            );
            p.rect_filled(r, 1, super::category_color(c));
            rx -= 14.0;
        }
        let name_color = t.text;
        let name = sender(m);
        let g = super::one_line(
            &p,
            name,
            if unread {
                chrome::bold_font(14.0)
            } else {
                egui::FontId::proportional(14.0)
            },
            name_color,
            rx - x - 6.0,
        );
        p.galley(egui::pos2(x, rect.top() + 5.0), g, name_color);

        // Line 2: subject + date.
        let date = super::short_date(&m.date);
        let date_color = if unread { t.accent } else { t.text_weak };
        let dg = p.layout_no_wrap(date, egui::FontId::proportional(12.0), date_color);
        let dw = dg.size().x;
        p.galley(egui::pos2(right - dw, rect.top() + 25.0), dg, date_color);
        let mut sx = x;
        if m.importance == "high" {
            p.text(
                egui::pos2(sx, rect.top() + 32.0),
                egui::Align2::LEFT_CENTER,
                "!",
                egui::FontId::proportional(14.0),
                t.danger,
            );
            sx += 9.0;
        }
        let subject = match folder_name {
            Some(f) => format!("{}  ({f})", m.subject),
            None => m.subject.clone(),
        };
        let subj_color = if unread { t.accent } else { t.text };
        let sg = super::one_line(
            &p,
            &subject,
            if unread {
                chrome::bold_font(13.0)
            } else {
                egui::FontId::proportional(13.0)
            },
            subj_color,
            right - sx - dw - 10.0,
        );
        p.galley(egui::pos2(sx, rect.top() + 24.0), sg, subj_color);
        // Line 3: preview.
        if self.prefs.preview {
            let pg = super::one_line(
                &p,
                &m.preview,
                egui::FontId::proportional(12.5),
                t.text_weak,
                right - x,
            );
            p.galley(egui::pos2(x, rect.top() + 42.0), pg, t.text_weak);
        }

        let pointer = resp.interact_pointer_pos();
        let hit = pointer.and_then(|pos| {
            icon_hits
                .iter()
                .find(|(r, _)| r.expand(3.0).contains(pos))
                .map(|(_, k)| *k)
        });
        if resp.clicked() {
            match hit {
                Some(0) => acts.push(Act::Delete(m.id)),
                Some(_) => acts.push(Act::Patch(
                    m.id,
                    MessagePatch {
                        is_flagged: Some(!m.is_flagged),
                        ..Default::default()
                    },
                )),
                None => acts.push(Act::Select(m.id)),
            }
        }
        if resp.double_clicked() && hit.is_none() {
            acts.push(Act::Open(m.id));
        }
        resp.context_menu(|ui| {
            if chrome::menu_item(ui, ph::ARROW_BEND_UP_LEFT, IC_PURPLE, "Antworten") {
                acts.push(Act::Compose(ComposeKind::Reply(m.id)));
            }
            if chrome::menu_item(
                ui,
                ph::ARROW_BEND_DOUBLE_UP_LEFT,
                IC_PURPLE,
                "Allen antworten",
            ) {
                acts.push(Act::Compose(ComposeKind::ReplyAll(m.id)));
            }
            if chrome::menu_item(ui, ph::ARROW_BEND_UP_RIGHT, IC_PURPLE, "Weiterleiten") {
                acts.push(Act::Compose(ComposeKind::Forward(m.id)));
            }
            ui.separator();
            if chrome::menu_item(
                ui,
                ph::ENVELOPE_SIMPLE_OPEN,
                IC_BLUE,
                if m.is_read {
                    "Als ungelesen markieren"
                } else {
                    "Als gelesen markieren"
                },
            ) {
                acts.push(Act::Patch(
                    m.id,
                    MessagePatch {
                        is_read: Some(!m.is_read),
                        ..Default::default()
                    },
                ));
            }
            ui.menu_button(format!("{}  Kategorisieren", ph::TAG), |ui| {
                category_menu(ui, m, acts)
            });
            if chrome::menu_item(
                ui,
                ph::FLAG,
                t.flag,
                if m.is_flagged {
                    "Als erledigt markieren"
                } else {
                    "Zur Nachverfolgung"
                },
            ) {
                acts.push(Act::Patch(
                    m.id,
                    MessagePatch {
                        is_flagged: Some(!m.is_flagged),
                        ..Default::default()
                    },
                ));
            }
            if chrome::menu_item(
                ui,
                ph::PUSH_PIN,
                IC_BLUE,
                if m.is_pinned { "Lösen" } else { "Anheften" },
            ) {
                acts.push(Act::Patch(
                    m.id,
                    MessagePatch {
                        is_pinned: Some(!m.is_pinned),
                        ..Default::default()
                    },
                ));
            }
            ui.menu_button(format!("{}  Verschieben", ph::FOLDER_SIMPLE), |ui| {
                self.move_menu(ui, m, acts)
            });
            ui.separator();
            if chrome::menu_item(ui, ph::X, IC_RED, "Löschen") {
                acts.push(Act::Delete(m.id));
            }
        });
    }

    fn reading_pane(&self, ui: &mut egui::Ui, acts: &mut Vec<Act>) {
        let Some(m) = &self.mail.current else {
            ui.centered_and_justified(|ui| {
                ui.label(
                    RichText::new("Wählen Sie ein Element aus, um es zu lesen.")
                        .size(14.0)
                        .color(self.t.text_weak),
                );
            });
            return;
        };
        read_message(ui, &self.t, m, &self.mail.atts, true, acts);
    }
}

/// Message header + body, used by the reading pane and the message window.
pub(super) fn read_message(
    ui: &mut egui::Ui,
    t: &super::theme::Theme,
    m: &Message,
    atts: &[Attachment],
    actions: bool,
    acts: &mut Vec<Act>,
) {
    let s = &m.summary;
    egui::ScrollArea::vertical()
        .id_salt(("reader", s.id))
        .auto_shrink([false, false])
        .show(ui, |ui| {
            egui::Frame::new()
                .inner_margin(egui::Margin {
                    left: 18,
                    right: 18,
                    top: 10,
                    bottom: 18,
                })
                .show(ui, |ui| {
                    if actions {
                        ui.horizontal(|ui| {
                            for (icon, label, kind) in [
                                (ph::ARROW_BEND_UP_LEFT, "Antworten", 0),
                                (ph::ARROW_BEND_DOUBLE_UP_LEFT, "Allen antworten", 1),
                                (ph::ARROW_BEND_UP_RIGHT, "Weiterleiten", 2),
                            ] {
                                let r = ui.add(
                                    egui::Button::new((
                                        RichText::new(icon).color(IC_PURPLE).size(15.0),
                                        label,
                                    ))
                                    .frame(false),
                                );
                                if r.clicked() {
                                    acts.push(Act::Compose(match kind {
                                        0 => ComposeKind::Reply(s.id),
                                        1 => ComposeKind::ReplyAll(s.id),
                                        _ => ComposeKind::Forward(s.id),
                                    }));
                                }
                            }
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui
                                        .add(
                                            egui::Button::new(
                                                RichText::new(ph::GLOBE).color(IC_BLUE),
                                            )
                                            .frame(false),
                                        )
                                        .on_hover_text("Im Browser anzeigen (Original-HTML)")
                                        .clicked()
                                    {
                                        acts.push(Act::OpenInBrowser(s.id));
                                    }
                                },
                            );
                        });
                        ui.add_space(6.0);
                    }
                    ui.horizontal(|ui| {
                        let name = if s.from_name.is_empty() {
                            &s.from_addr
                        } else {
                            &s.from_name
                        };
                        super::avatar(ui, 48.0, name);
                        ui.add_space(6.0);
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 2.0;
                            ui.label(
                                RichText::new(super::long_date(&s.date))
                                    .size(12.0)
                                    .color(t.text_weak),
                            );
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(name).size(16.0));
                                if !s.from_name.is_empty() {
                                    ui.label(
                                        RichText::new(format!("<{}>", s.from_addr))
                                            .size(12.0)
                                            .color(t.text_weak),
                                    );
                                }
                            });
                            ui.label(RichText::new(&s.subject).size(13.5).strong());
                            if !m.to.is_empty() {
                                ui.label(
                                    RichText::new(format!("An   {}", m.to))
                                        .size(12.0)
                                        .color(t.text_weak),
                                );
                            }
                            if !m.cc.is_empty() {
                                ui.label(
                                    RichText::new(format!("Cc   {}", m.cc))
                                        .size(12.0)
                                        .color(t.text_weak),
                                );
                            }
                        });
                    });
                    ui.add_space(6.0);
                    let info = |ui: &mut egui::Ui, icon: &str, color: egui::Color32, text: &str| {
                        egui::Frame::new()
                            .fill(t.pane_alt)
                            .inner_margin(egui::Margin::symmetric(8, 3))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.label(
                                    RichText::new(format!("{icon}  {text}"))
                                        .size(12.0)
                                        .color(color),
                                );
                            });
                    };
                    if s.importance == "high" {
                        info(
                            ui,
                            "!",
                            t.danger,
                            "Diese Nachricht wurde mit der Wichtigkeit \"Hoch\" gesendet.",
                        );
                    }
                    if s.is_flagged {
                        info(ui, ph::FLAG, t.flag, "Zur Nachverfolgung.");
                    }
                    for c in &s.categories {
                        egui::Frame::new()
                            .fill(super::category_color(c))
                            .inner_margin(egui::Margin::symmetric(8, 2))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.label(RichText::new(c).size(12.0).color(egui::Color32::WHITE));
                            });
                    }
                    if !atts.is_empty() {
                        ui.add_space(4.0);
                        ui.horizontal_wrapped(|ui| {
                            for a in atts {
                                let r = ui.add(egui::Button::new((
                                    RichText::new(file_icon(&a.mime)).color(IC_BLUE).size(18.0),
                                    format!("{}\n{}", a.name, human_size(a.size)),
                                )));
                                if r.double_clicked() {
                                    acts.push(Act::OpenAttachment(a.clone()));
                                }
                                r.on_hover_text(
                                    "Doppelklick zum Öffnen, Rechtsklick für weitere Optionen",
                                )
                                .context_menu(|ui| {
                                    if chrome::menu_item(ui, ph::FOLDER_OPEN, IC_BLUE, "Öffnen") {
                                        acts.push(Act::OpenAttachment(a.clone()));
                                    }
                                    if chrome::menu_item(
                                        ui,
                                        ph::FLOPPY_DISK,
                                        IC_BLUE,
                                        "Speichern unter…",
                                    ) {
                                        acts.push(Act::SaveAttachment(a.clone()));
                                    }
                                    if chrome::menu_item(
                                        ui,
                                        ph::FLOPPY_DISK,
                                        IC_BLUE,
                                        "Alle Anlagen speichern…",
                                    ) {
                                        acts.push(Act::SaveAllAttachments(a.message_id));
                                    }
                                });
                            }
                        });
                    } else if s.has_attachments {
                        info(ui, ph::PAPERCLIP, t.text_weak, "Anlagen werden geladen …");
                    }
                    ui.add_space(8.0);
                    ui.separator();
                    ui.add_space(8.0);
                    // Parsed HTML is cached per message to keep repaints cheap.
                    let key = egui::Id::new(("html", s.id, m.body_html.len()));
                    let blocks = ui
                        .data(|d| d.get_temp::<std::sync::Arc<Vec<html::Block>>>(key))
                        .unwrap_or_else(|| {
                            let parsed = if m.body_html.trim().is_empty() {
                                html::parse(&crate::mail::text_to_html(&m.body_text))
                            } else {
                                html::parse(&m.body_html)
                            };
                            let parsed = std::sync::Arc::new(parsed);
                            ui.data_mut(|d| d.insert_temp(key, parsed.clone()));
                            parsed
                        });
                    if blocks.is_empty() {
                        ui.label(RichText::new("(kein Inhalt)").color(t.text_weak));
                    } else if let Some(link) =
                        html::show(ui, &blocks, t.text, t.text_weak, t.accent)
                    {
                        match link.strip_prefix("mailto:") {
                            Some(addr) => acts.push(Act::Compose(ComposeKind::To(
                                addr.split('?').next().unwrap_or("").to_string(),
                            ))),
                            None if link.starts_with("http://") || link.starts_with("https://") => {
                                super::open_external(&link)
                            }
                            None => {}
                        }
                    }
                });
        });
}

fn file_icon(mime: &str) -> &'static str {
    match mime.split('/').next().unwrap_or("") {
        "image" => ph::FILE_IMAGE,
        "text" => ph::FILE_TEXT,
        "audio" => ph::FILE_AUDIO,
        "video" => ph::FILE_VIDEO,
        _ if mime.contains("pdf") => ph::FILE_PDF,
        _ if mime.contains("zip") => ph::FILE_ZIP,
        _ => ph::FILE,
    }
}

pub(super) fn human_size(bytes: i64) -> String {
    match bytes {
        b if b < 1024 => format!("{b} Bytes"),
        b if b < 1024 * 1024 => format!("{} KB", b / 1024),
        b => format!("{:.1} MB", b as f64 / 1024.0 / 1024.0),
    }
}

/// Strip path separators and other characters that are invalid in file names.
pub(super) fn safe_file_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if r#"/\:*?"<>|"#.contains(c) || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect();
    if cleaned.trim().is_empty() {
        "Anlage".into()
    } else {
        cleaned
    }
}

fn sender(m: &MessageSummary) -> &str {
    if m.from_name.is_empty() {
        &m.from_addr
    } else {
        &m.from_name
    }
}

fn strip_prefixes(s: &str) -> &str {
    let mut s = s.trim();
    loop {
        let lower = s.to_lowercase();
        match ["aw:", "re:", "wg:", "fw:", "fwd:"]
            .iter()
            .find(|p| lower.starts_with(*p))
        {
            Some(p) => s = s[p.len()..].trim_start(),
            None => return s,
        }
    }
}

pub fn folder_icon(role: &str) -> &'static str {
    match role {
        "inbox" => ph::TRAY,
        "sent" => ph::PAPER_PLANE_RIGHT,
        "drafts" => ph::NOTE_PENCIL,
        "trash" => ph::TRASH,
        "junk" => ph::PROHIBIT,
        "archive" => ph::ARCHIVE,
        _ => ph::FOLDER_SIMPLE,
    }
}

pub(super) fn category_menu(ui: &mut egui::Ui, m: &MessageSummary, acts: &mut Vec<Act>) {
    for (name, color) in CATEGORIES {
        let on = m.categories.iter().any(|c| c == name);
        let label = if on {
            format!("{name}  \u{E182}")
        } else {
            name.to_string()
        };
        if ui
            .add(
                egui::Button::new((
                    RichText::new("\u{E45E}")
                        .family(egui::FontFamily::Name("fill".into()))
                        .color(color)
                        .size(16.0),
                    label,
                ))
                .frame(false),
            )
            .clicked()
        {
            let mut cats: Vec<String> = m
                .categories
                .iter()
                .filter(|c| *c != name)
                .cloned()
                .collect();
            if !on {
                cats.push(name.to_string());
            }
            acts.push(Act::Patch(
                m.id,
                MessagePatch {
                    categories: Some(cats),
                    ..Default::default()
                },
            ));
            ui.close();
        }
    }
    ui.separator();
    if ui.button("Alle Kategorien löschen").clicked() {
        acts.push(Act::Patch(
            m.id,
            MessagePatch {
                categories: Some(vec![]),
                ..Default::default()
            },
        ));
        ui.close();
    }
}

/// Outlook groups the list into "Heute", "Gestern", "Mittwoch", "Letzte Woche", …
fn date_group(date: &str) -> &'static str {
    let Some(d) = super::parse_utc(date) else {
        return "Älter";
    };
    let today = super::today();
    let d = d.date_naive();
    let days = (today - d).num_days();
    let week_start = today - chrono::Duration::days(today.weekday().num_days_from_monday() as i64);
    if days <= 0 {
        "Heute"
    } else if days == 1 {
        "Gestern"
    } else if d >= week_start {
        super::WEEKDAYS_LONG[d.weekday().num_days_from_monday() as usize]
    } else if d >= week_start - chrono::Duration::days(7) {
        "Letzte Woche"
    } else if d >= week_start - chrono::Duration::days(14) {
        "Vor zwei Wochen"
    } else if d.month() == today.month() && d.year() == today.year() {
        "Anfang dieses Monats"
    } else {
        "Älter"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_today() {
        assert_eq!(date_group(&crate::mail::now_utc()), "Heute");
        assert_eq!(date_group("2001-01-01T00:00:00Z"), "Älter");
    }

    #[test]
    fn sanitizes_file_names() {
        assert_eq!(safe_file_name("../a/b:c.txt"), ".._a_b_c.txt");
        assert_eq!(safe_file_name("  "), "Anlage");
        assert_eq!(human_size(2048), "2 KB");
    }

    #[test]
    fn strips_reply_prefixes() {
        assert_eq!(strip_prefixes("AW: WG: Re: Hallo"), "Hallo");
        assert_eq!(strip_prefixes("Rechnung"), "Rechnung");
    }
}
