//! Mail module: folder pane, message list and reading pane.

use chrono::Datelike;
use eframe::egui::{self, Color32, RichText, Sense};

use super::{compose::ComposeKind, App, BRAND, CATEGORIES, DANGER};
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

#[derive(Default)]
pub struct MailView {
    pub folder_id: Option<i64>,
    filter: Filter,
    messages: Vec<MessageSummary>,
    selected: Option<i64>,
    current: Option<Message>,
}

enum Act {
    Folder(i64),
    Select(i64),
    Patch(i64, MessagePatch),
    Move(i64, i64),
    Delete(i64),
    Compose(ComposeKind),
    MarkFolderRead(i64),
    OpenInBrowser,
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
        let q = db::MessageQuery {
            folder_id: if self.search.trim().is_empty() {
                self.mail.folder_id
            } else {
                None
            },
            search: Some(self.search.clone()),
            unread_only: self.mail.filter == Filter::Unread,
            flagged_only: self.mail.filter == Filter::Flagged,
            limit: 1000,
        };
        let c = self.st.db();
        self.mail.messages = db::list_messages(&c, &q).unwrap_or_default();
        self.mail.current = self
            .mail
            .selected
            .and_then(|id| db::get_message(&c, id).ok().flatten());
        if self.mail.current.is_none() {
            self.mail.selected = None;
        }
    }

    fn mail_apply(&mut self, acts: Vec<Act>) {
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
                    if self
                        .mail
                        .current
                        .as_ref()
                        .is_some_and(|m| !m.summary.is_read)
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
                Act::Patch(id, p) => self.patch_message(id, p),
                Act::Move(id, folder) => {
                    if self.mail.selected == Some(id) {
                        self.select_next(id);
                    }
                    self.spawn(Some("Nachricht verschoben"), move |st, _| async move {
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
                Act::Toast(t) => self.toast(t, true),
                Act::MarkFolderRead(folder) => {
                    let ids: Vec<i64> = self
                        .mail
                        .messages
                        .iter()
                        .filter(|m| m.folder_id == folder && !m.is_read)
                        .map(|m| m.id)
                        .collect();
                    self.spawn(Some("Alle als gelesen markiert"), move |st, _| async move {
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
                    });
                }
                Act::OpenInBrowser => {
                    if let Some(m) = &self.mail.current {
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

    fn folder_for_role(&self, account_id: i64, role: &str) -> Option<i64> {
        self.data
            .folders
            .iter()
            .find(|f| f.account_id == account_id && f.role == role)
            .map(|f| f.id)
    }

    pub fn mail_ui(&mut self, ui: &mut egui::Ui) {
        let mut acts = vec![];
        self.mail_shortcuts(ui.ctx(), &mut acts);

        egui::TopBottomPanel::top("mail_ribbon")
            .frame(super::pane_frame(ui).inner_margin(egui::Margin::symmetric(8, 6)))
            .show_inside(ui, |ui| self.mail_ribbon(ui, &mut acts));
        ui.add_space(8.0);

        egui::SidePanel::left("folders")
            .resizable(true)
            .default_width(230.0)
            .width_range(160.0..=360.0)
            .frame(egui::Frame::new())
            .show_inside(ui, |ui| self.folder_pane(ui, &mut acts));

        egui::SidePanel::left("message_list")
            .resizable(true)
            .default_width(400.0)
            .width_range(280.0..=640.0)
            .frame(super::pane_frame(ui))
            .show_inside(ui, |ui| self.message_list(ui, &mut acts));

        egui::CentralPanel::default()
            .frame(super::pane_frame(ui).inner_margin(egui::Margin::same(0)))
            .show_inside(ui, |ui| self.reading_pane(ui, &mut acts));

        self.mail_apply(acts);
    }

    fn mail_shortcuts(&self, ctx: &egui::Context, acts: &mut Vec<Act>) {
        if ctx.wants_keyboard_input() {
            return;
        }
        let sel = self.mail.current.as_ref().map(|m| &m.summary);
        ctx.input(|i| {
            if i.key_pressed(egui::Key::N) && !i.modifiers.any() {
                acts.push(Act::Compose(ComposeKind::New));
            }
            if let Some(m) = sel {
                if i.key_pressed(egui::Key::Delete) {
                    acts.push(Act::Delete(m.id));
                }
                if i.key_pressed(egui::Key::R) && !i.modifiers.command {
                    acts.push(Act::Compose(if i.modifiers.shift {
                        ComposeKind::ReplyAll(m.id)
                    } else {
                        ComposeKind::Reply(m.id)
                    }));
                }
                if i.key_pressed(egui::Key::F) && !i.modifiers.command {
                    acts.push(Act::Compose(ComposeKind::Forward(m.id)));
                }
                if i.key_pressed(egui::Key::Q) {
                    acts.push(Act::Patch(
                        m.id,
                        MessagePatch {
                            is_read: Some(!m.is_read),
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
            }
            let pos = sel.and_then(|m| self.mail.messages.iter().position(|x| x.id == m.id));
            if i.key_pressed(egui::Key::ArrowDown) {
                let next = pos.map(|p| p + 1).unwrap_or(0);
                if let Some(m) = self.mail.messages.get(next) {
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

    fn mail_ribbon(&self, ui: &mut egui::Ui, acts: &mut Vec<Act>) {
        let sel = self.mail.current.as_ref().map(|m| m.summary.clone());
        let has = sel.is_some();
        ui.horizontal_wrapped(|ui| {
            if super::primary_button(ui, "✉  Neue E-Mail").clicked() {
                acts.push(Act::Compose(ComposeKind::New));
            }
            ui.separator();
            if super::ribbon_button(ui, "🗑", "Löschen", has).clicked() {
                acts.push(Act::Delete(sel.as_ref().unwrap().id));
            }
            if super::ribbon_button(ui, "🗄", "Archivieren", has).clicked() {
                let m = sel.as_ref().unwrap();
                match self.folder_for_role(m.account_id, "archive") {
                    Some(f) => acts.push(Act::Move(m.id, f)),
                    None => acts.push(Act::Toast("Dieses Konto hat keinen Archivordner".into())),
                }
            }
            if super::ribbon_button(ui, "⚠", "Junk", has).clicked() {
                let m = sel.as_ref().unwrap();
                if let Some(f) = self.folder_for_role(m.account_id, "junk") {
                    acts.push(Act::Move(m.id, f));
                }
            }
            ui.add_enabled_ui(has, |ui| {
                ui.menu_button("📁  Verschieben", |ui| {
                    if let Some(m) = &sel {
                        self.move_menu(ui, m, acts);
                    }
                });
            });
            ui.separator();
            for (icon, label, kind) in [
                ("↩", "Antworten", 0),
                ("↩↩", "Allen antworten", 1),
                ("↪", "Weiterleiten", 2),
            ] {
                if super::ribbon_button(ui, icon, label, has).clicked() {
                    let id = sel.as_ref().unwrap().id;
                    acts.push(Act::Compose(match kind {
                        0 => ComposeKind::Reply(id),
                        1 => ComposeKind::ReplyAll(id),
                        _ => ComposeKind::Forward(id),
                    }));
                }
            }
            ui.separator();
            let read_label = if sel.as_ref().is_some_and(|m| m.is_read) {
                "Ungelesen"
            } else {
                "Gelesen"
            };
            if super::ribbon_button(ui, "✉", read_label, has).clicked() {
                let m = sel.as_ref().unwrap();
                acts.push(Act::Patch(
                    m.id,
                    MessagePatch {
                        is_read: Some(!m.is_read),
                        ..Default::default()
                    },
                ));
            }
            if super::ribbon_button(ui, "⚑", "Kennzeichnen", has).clicked() {
                let m = sel.as_ref().unwrap();
                acts.push(Act::Patch(
                    m.id,
                    MessagePatch {
                        is_flagged: Some(!m.is_flagged),
                        ..Default::default()
                    },
                ));
            }
            if super::ribbon_button(ui, "📌", "Anheften", has).clicked() {
                let m = sel.as_ref().unwrap();
                acts.push(Act::Patch(
                    m.id,
                    MessagePatch {
                        is_pinned: Some(!m.is_pinned),
                        ..Default::default()
                    },
                ));
            }
            ui.add_enabled_ui(has, |ui| {
                ui.menu_button("🏷  Kategorisieren", |ui| {
                    if let Some(m) = &sel {
                        category_menu(ui, m, acts);
                    }
                });
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
            if ui.button(&f.name).clicked() {
                acts.push(Act::Move(m.id, f.id));
                ui.close();
            }
        }
    }

    fn folder_pane(&self, ui: &mut egui::Ui, acts: &mut Vec<Act>) {
        egui::ScrollArea::vertical()
            .id_salt("folders_scroll")
            .show(ui, |ui| {
                // Favourites: every inbox, like Outlook's default favourites.
                ui.label(RichText::new("★ Favoriten").strong());
                for f in self.data.folders.iter().filter(|f| f.role == "inbox") {
                    let acc = self
                        .data
                        .account(f.account_id)
                        .map(|a| a.email.as_str())
                        .unwrap_or_default();
                    self.folder_row(ui, f, &format!("Posteingang – {acc}"), acts);
                }
                ui.add_space(8.0);
                for acc in &self.data.accounts {
                    let header = RichText::new(format!(
                        "{}  {}",
                        match acc.kind {
                            AccountKind::Local => "🖴",
                            AccountKind::Imap => "📨",
                            AccountKind::Graph => "Ⓜ",
                        },
                        acc.email
                    ))
                    .strong();
                    egui::CollapsingHeader::new(header)
                        .id_salt(("acc", acc.id))
                        .default_open(true)
                        .show(ui, |ui| {
                            if let Some(e) = &acc.last_error {
                                ui.label(RichText::new(format!("⚠ {e}")).small().color(DANGER))
                                    .on_hover_text(e);
                            }
                            for f in self.data.folders.iter().filter(|f| f.account_id == acc.id) {
                                self.folder_row(ui, f, &f.name, acts);
                            }
                        });
                }
            });
    }

    fn folder_row(&self, ui: &mut egui::Ui, f: &Folder, label: &str, acts: &mut Vec<Act>) {
        let icon = match f.role.as_str() {
            "inbox" => "📥",
            "sent" => "📤",
            "drafts" => "📝",
            "trash" => "🗑",
            "junk" => "⚠",
            "archive" => "🗄",
            _ => "📁",
        };
        let active = self.mail.folder_id == Some(f.id) && self.search.trim().is_empty();
        let w = ui.available_width();
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, 28.0), Sense::click());
        let hovered_drop = resp.dnd_hover_payload::<i64>().is_some();
        let visuals = ui.visuals();
        if active {
            ui.painter().rect_filled(rect, 4, visuals.selection.bg_fill);
        } else if resp.hovered() || hovered_drop {
            ui.painter()
                .rect_filled(rect, 4, visuals.widgets.hovered.weak_bg_fill);
        }
        if hovered_drop {
            ui.painter().rect_stroke(
                rect,
                4,
                egui::Stroke::new(1.5_f32, BRAND),
                egui::StrokeKind::Inside,
            );
        }
        let text_color = visuals.text_color();
        let p = ui.painter();
        p.text(
            rect.left_center() + egui::vec2(10.0, 0.0),
            egui::Align2::LEFT_CENTER,
            icon,
            egui::FontId::proportional(14.0),
            text_color,
        );
        let name = super::one_line(
            p,
            label,
            egui::FontId::proportional(14.0),
            text_color,
            w - 74.0,
        );
        p.galley(
            rect.left_center() + egui::vec2(34.0, -name.size().y / 2.0),
            name,
            text_color,
        );
        if f.unread > 0 && f.role != "sent" && f.role != "drafts" {
            ui.painter().text(
                rect.right_center() - egui::vec2(10.0, 0.0),
                egui::Align2::RIGHT_CENTER,
                f.unread.to_string(),
                egui::FontId::proportional(12.0),
                BRAND,
            );
        } else if f.role == "drafts" && f.total > 0 {
            ui.painter().text(
                rect.right_center() - egui::vec2(10.0, 0.0),
                egui::Align2::RIGHT_CENTER,
                f.total.to_string(),
                egui::FontId::proportional(12.0),
                visuals.weak_text_color(),
            );
        }
        if let Some(id) = resp.dnd_release_payload::<i64>() {
            acts.push(Act::Move(*id, f.id));
        }
        if resp.clicked() {
            acts.push(Act::Folder(f.id));
        }
        resp.context_menu(|ui| {
            if ui.button("✉  Alle als gelesen markieren").clicked() {
                acts.push(Act::Folder(f.id));
                acts.push(Act::MarkFolderRead(f.id));
                ui.close();
            }
        });
    }

    fn message_list(&mut self, ui: &mut egui::Ui, acts: &mut Vec<Act>) {
        let title = if !self.search.trim().is_empty() {
            format!("Suchergebnisse für „{}“", self.search.trim())
        } else {
            self.mail
                .folder_id
                .and_then(|id| self.data.folders.iter().find(|f| f.id == id))
                .map(|f| f.name.clone())
                .unwrap_or_else(|| "E-Mail".into())
        };
        egui::Frame::new()
            .inner_margin(egui::Margin {
                left: 14,
                right: 14,
                top: 12,
                bottom: 4,
            })
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(title).size(18.0).strong());
                });
                ui.horizontal(|ui| {
                    let mut changed = false;
                    for (f, label) in [
                        (Filter::All, "Alle"),
                        (Filter::Unread, "Ungelesen"),
                        (Filter::Flagged, "Gekennzeichnet"),
                    ] {
                        if ui.selectable_label(self.mail.filter == f, label).clicked() {
                            self.mail.filter = f;
                            changed = true;
                        }
                    }
                    if changed {
                        self.reload_messages();
                    }
                });
            });
        ui.separator();

        if self.mail.messages.is_empty() {
            ui.centered_and_justified(|ui| {
                ui.label(RichText::new("📭\n\nHier ist nichts.").size(16.0).weak());
            });
            return;
        }
        let in_search = !self.search.trim().is_empty();
        egui::ScrollArea::vertical()
            .id_salt("msg_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let mut last_group = "";
                for m in &self.mail.messages {
                    let group = if m.is_pinned {
                        "Angeheftet"
                    } else {
                        date_group(&m.date)
                    };
                    if group != last_group {
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            ui.add_space(14.0);
                            ui.label(RichText::new(group).small().strong().weak());
                        });
                        last_group = group;
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
    }

    fn message_row(
        &self,
        ui: &mut egui::Ui,
        m: &MessageSummary,
        folder_name: Option<&str>,
        acts: &mut Vec<Act>,
    ) {
        let selected = self.mail.selected == Some(m.id);
        let w = ui.available_width();
        let h = if m.categories.is_empty() { 68.0 } else { 86.0 };
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, h), Sense::click_and_drag());
        resp.dnd_set_drag_payload(m.id);
        let v = ui.visuals().clone();
        let p = ui.painter_at(rect);
        if selected {
            p.rect_filled(rect, 0, v.selection.bg_fill);
        } else if resp.hovered() {
            p.rect_filled(rect, 0, v.widgets.hovered.weak_bg_fill);
        }
        if !m.is_read {
            p.rect_filled(
                egui::Rect::from_min_size(rect.min, egui::vec2(3.0, h)),
                0,
                BRAND,
            );
        }
        p.line_segment(
            [
                rect.left_bottom() + egui::vec2(12.0, -0.5),
                rect.right_bottom() - egui::vec2(0.0, 0.5),
            ],
            v.widgets.noninteractive.bg_stroke,
        );
        let name = if m.from_name.is_empty() {
            &m.from_addr
        } else {
            &m.from_name
        };
        super::paint_avatar(&p, rect.left_top() + egui::vec2(30.0, 26.0), 16.0, name);

        let x = rect.left() + 56.0;
        let right = rect.right() - 12.0;
        let text = v.text_color();
        let weak = v.weak_text_color();
        // Line 1: sender + date
        let date = super::short_date(&m.date);
        let dg = p.layout_no_wrap(date, egui::FontId::proportional(12.0), weak);
        let date_w = dg.size().x;
        p.galley(egui::pos2(right - date_w, rect.top() + 9.0), dg, weak);
        let from = super::one_line(
            &p,
            name,
            egui::FontId::proportional(14.5),
            text,
            right - x - date_w - 10.0,
        );
        p.galley(egui::pos2(x, rect.top() + 7.0), from, text);
        // Line 2: subject + icons
        let mut icons = String::new();
        if m.importance == "high" {
            icons.push('❗');
        }
        if m.has_attachments {
            icons.push('📎');
        }
        if m.is_pinned {
            icons.push('📌');
        }
        let mut icon_w = 0.0;
        if m.is_flagged {
            let g = p.layout_no_wrap("⚑".into(), egui::FontId::proportional(14.0), DANGER);
            icon_w += g.size().x + 4.0;
            p.galley(
                egui::pos2(right - icon_w + 4.0, rect.top() + 27.0),
                g,
                DANGER,
            );
        }
        if !icons.is_empty() {
            let g = p.layout_no_wrap(icons, egui::FontId::proportional(13.0), weak);
            icon_w += g.size().x + 4.0;
            p.galley(egui::pos2(right - icon_w + 4.0, rect.top() + 27.0), g, weak);
        }
        let subj_color = if m.is_read { text } else { BRAND };
        let subject = match folder_name {
            Some(f) => format!("{}  ·  {f}", m.subject),
            None => m.subject.clone(),
        };
        let sg = super::one_line(
            &p,
            &subject,
            egui::FontId::proportional(13.5),
            subj_color,
            right - x - icon_w - 4.0,
        );
        p.galley(egui::pos2(x, rect.top() + 27.0), sg, subj_color);
        // Line 3: preview
        let pg = super::one_line(
            &p,
            &m.preview,
            egui::FontId::proportional(12.5),
            weak,
            right - x,
        );
        p.galley(egui::pos2(x, rect.top() + 46.0), pg, weak);
        // Line 4: categories
        let mut cx = x;
        for c in &m.categories {
            let g = p.layout_no_wrap(c.clone(), egui::FontId::proportional(11.0), Color32::WHITE);
            let r = egui::Rect::from_min_size(
                egui::pos2(cx, rect.top() + 65.0),
                g.size() + egui::vec2(10.0, 2.0),
            );
            p.rect_filled(r, 3, super::category_color(c));
            p.galley(r.min + egui::vec2(5.0, 1.0), g, Color32::WHITE);
            cx = r.right() + 4.0;
        }

        if resp.clicked() {
            acts.push(Act::Select(m.id));
        }
        if resp.double_clicked()
            && m.folder_id == self.folder_for_role(m.account_id, "drafts").unwrap_or(-1)
        {
            acts.push(Act::Compose(ComposeKind::EditDraft(m.id)));
        }
        resp.context_menu(|ui| {
            if ui.button("↩  Antworten").clicked() {
                acts.push(Act::Compose(ComposeKind::Reply(m.id)));
                ui.close();
            }
            if ui.button("↩↩  Allen antworten").clicked() {
                acts.push(Act::Compose(ComposeKind::ReplyAll(m.id)));
                ui.close();
            }
            if ui.button("↪  Weiterleiten").clicked() {
                acts.push(Act::Compose(ComposeKind::Forward(m.id)));
                ui.close();
            }
            ui.separator();
            if ui
                .button(if m.is_read {
                    "✉  Als ungelesen markieren"
                } else {
                    "✉  Als gelesen markieren"
                })
                .clicked()
            {
                acts.push(Act::Patch(
                    m.id,
                    MessagePatch {
                        is_read: Some(!m.is_read),
                        ..Default::default()
                    },
                ));
                ui.close();
            }
            if ui
                .button(if m.is_flagged {
                    "⚑  Kennzeichnung entfernen"
                } else {
                    "⚑  Kennzeichnen"
                })
                .clicked()
            {
                acts.push(Act::Patch(
                    m.id,
                    MessagePatch {
                        is_flagged: Some(!m.is_flagged),
                        ..Default::default()
                    },
                ));
                ui.close();
            }
            if ui
                .button(if m.is_pinned {
                    "📌  Lösen"
                } else {
                    "📌  Anheften"
                })
                .clicked()
            {
                acts.push(Act::Patch(
                    m.id,
                    MessagePatch {
                        is_pinned: Some(!m.is_pinned),
                        ..Default::default()
                    },
                ));
                ui.close();
            }
            ui.menu_button("🏷  Kategorisieren", |ui| category_menu(ui, m, acts));
            ui.menu_button("📁  Verschieben", |ui| self.move_menu(ui, m, acts));
            ui.separator();
            if ui
                .button(RichText::new("🗑  Löschen").color(DANGER))
                .clicked()
            {
                acts.push(Act::Delete(m.id));
                ui.close();
            }
        });
    }

    fn reading_pane(&self, ui: &mut egui::Ui, acts: &mut Vec<Act>) {
        let Some(m) = &self.mail.current else {
            ui.centered_and_justified(|ui| {
                ui.label(
                    RichText::new("✉\n\nWählen Sie ein Element aus, um es zu lesen.")
                        .size(16.0)
                        .weak(),
                );
            });
            return;
        };
        let s = &m.summary;
        egui::ScrollArea::vertical().id_salt(("reader", s.id)).auto_shrink([false, false]).show(ui, |ui| {
            egui::Frame::new().inner_margin(egui::Margin::symmetric(20, 16)).show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(&s.subject).size(20.0).strong());
                    for c in &s.categories {
                        ui.label(RichText::new(format!(" {c} ")).small().color(Color32::WHITE).background_color(super::category_color(c)));
                    }
                });
                ui.add_space(10.0);
                egui::Frame::new()
                    .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
                    .corner_radius(6)
                    .inner_margin(egui::Margin::same(14))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            let name = if s.from_name.is_empty() { &s.from_addr } else { &s.from_name };
                            super::avatar(ui, 40.0, name);
                            ui.vertical(|ui| {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(name).strong());
                                    if !s.from_name.is_empty() {
                                        ui.label(RichText::new(format!("<{}>", s.from_addr)).weak());
                                    }
                                });
                                if !m.to.is_empty() {
                                    ui.label(RichText::new(format!("An: {}", m.to)).small().weak());
                                }
                                if !m.cc.is_empty() {
                                    ui.label(RichText::new(format!("Cc: {}", m.cc)).small().weak());
                                }
                                ui.label(RichText::new(super::long_date(&s.date)).small().weak());
                            });
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                                let b = |ui: &mut egui::Ui, t: &str, tip: &str| {
                                    ui.add(egui::Button::new(RichText::new(t).size(16.0).color(BRAND)).frame(false)).on_hover_text(tip).clicked()
                                };
                                if b(ui, "🌐", "Im Browser anzeigen (vollständiges HTML)") {
                                    acts.push(Act::OpenInBrowser);
                                }
                                if b(ui, "↪", "Weiterleiten") {
                                    acts.push(Act::Compose(ComposeKind::Forward(s.id)));
                                }
                                if b(ui, "↩↩", "Allen antworten") {
                                    acts.push(Act::Compose(ComposeKind::ReplyAll(s.id)));
                                }
                                if b(ui, "↩", "Antworten") {
                                    acts.push(Act::Compose(ComposeKind::Reply(s.id)));
                                }
                            });
                        });
                        if s.importance == "high" {
                            ui.add_space(6.0);
                            ui.label(RichText::new("❗ Diese Nachricht wurde mit hoher Wichtigkeit gesendet.").color(DANGER).small());
                        }
                        if s.has_attachments {
                            ui.label(RichText::new("📎 Diese Nachricht hat Anlagen – zum Öffnen „Im Browser anzeigen“ oder im Webmail öffnen.").small().weak());
                        }
                        ui.add_space(8.0);
                        ui.separator();
                        ui.add_space(8.0);
                        let body = if m.body_text.trim().is_empty() { "(kein Inhalt)" } else { m.body_text.as_str() };
                        ui.add(egui::Label::new(RichText::new(body).size(14.5)).selectable(true).wrap());
                    });
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if ui.button("↩  Antworten").clicked() {
                        acts.push(Act::Compose(ComposeKind::Reply(s.id)));
                    }
                    if ui.button("↩↩  Allen antworten").clicked() {
                        acts.push(Act::Compose(ComposeKind::ReplyAll(s.id)));
                    }
                    if ui.button("↪  Weiterleiten").clicked() {
                        acts.push(Act::Compose(ComposeKind::Forward(s.id)));
                    }
                });
            });
        });
    }
}

fn category_menu(ui: &mut egui::Ui, m: &MessageSummary, acts: &mut Vec<Act>) {
    for (name, color) in CATEGORIES {
        let mut on = m.categories.iter().any(|c| c == name);
        if ui
            .checkbox(&mut on, RichText::new(format!("■ {name}")).color(color))
            .changed()
        {
            let mut cats: Vec<String> = m
                .categories
                .iter()
                .filter(|c| *c != name)
                .cloned()
                .collect();
            if on {
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
    if ui.button("Alle Kategorien entfernen").clicked() {
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

/// Outlook groups the list into "Heute", "Gestern", "Diese Woche", …
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
        "Diese Woche"
    } else if d >= week_start - chrono::Duration::days(7) {
        "Letzte Woche"
    } else if d.month() == today.month() && d.year() == today.year() {
        "Diesen Monat"
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
}
