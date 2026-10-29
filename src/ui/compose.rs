//! Message windows: read a message ("Nachricht") or write one
//! (new, reply, reply all, forward, draft) – each in its own OS window.

use eframe::egui::{self, RichText};
use egui_phosphor::regular as ph;

use super::{
    chrome::{self, IC_BLUE, IC_GREY, IC_PURPLE, IC_RED, IC_YELLOW},
    mail::{self, Act},
    App, Done, Win,
};
use crate::{
    db,
    models::*,
    providers::{self, MessagePatch, OutgoingMail},
};

pub enum ComposeKind {
    New,
    To(String),
    Reply(i64),
    ReplyAll(i64),
    Forward(i64),
    EditDraft(i64),
}

pub struct Composer {
    pub sending: bool,
    /// Some(id) = read-only message window for that message.
    read: Option<i64>,
    account_id: i64,
    to: String,
    cc: String,
    bcc: String,
    show_bcc: bool,
    subject: String,
    body: String,
    importance: String,
    /// Draft this window was opened from (deleted after sending).
    draft_id: Option<i64>,
    attachments: Vec<NewAttachment>,
    changed: bool,
    confirm_close: bool,
    address_book: Option<u8>,
}

impl Composer {
    pub fn reader(id: i64) -> Self {
        let mut c = Self::blank(0);
        c.read = Some(id);
        c
    }

    fn blank(account_id: i64) -> Self {
        Composer {
            sending: false,
            read: None,
            account_id,
            to: String::new(),
            cc: String::new(),
            bcc: String::new(),
            show_bcc: false,
            subject: String::new(),
            body: String::new(),
            importance: "normal".into(),
            draft_id: None,
            changed: false,
            confirm_close: false,
            address_book: None,
            attachments: vec![],
        }
    }

    fn to_outgoing(&self) -> OutgoingMail {
        OutgoingMail {
            account_id: self.account_id,
            to: self.to.clone(),
            cc: self.cc.clone(),
            bcc: self.bcc.clone(),
            subject: self.subject.clone(),
            body_html: crate::mail::text_to_html(&self.body),
            importance: self.importance.clone(),
            attachments: self.attachments.clone(),
        }
    }
}

impl App {
    pub fn open_composer(&mut self, kind: ComposeKind) {
        let default_account = self
            .current_account()
            .unwrap_or_else(|| self.data.local_account_id());
        let sig = if self.prefs.signature.trim().is_empty() {
            String::new()
        } else {
            format!("\n\n{}", self.prefs.signature)
        };
        let mut c = Composer::blank(default_account);
        c.body = sig.clone();

        let original = match &kind {
            ComposeKind::Reply(id)
            | ComposeKind::ReplyAll(id)
            | ComposeKind::Forward(id)
            | ComposeKind::EditDraft(id) => db::get_message(&self.st.db(), *id).ok().flatten(),
            _ => None,
        };
        if let Some(m) = &original {
            c.account_id = m.summary.account_id;
        }
        let me = self
            .data
            .account(c.account_id)
            .map(|a| a.email.to_lowercase())
            .unwrap_or_default();
        // Outlook's quoted header block.
        let quote = |m: &Message| {
            let header = format!(
                "\n\n________________________________\nVon: {} <{}>\nGesendet: {}\nAn: {}\n{}Betreff: {}\n\n",
                m.summary.from_name,
                m.summary.from_addr,
                super::long_date(&m.summary.date),
                m.to,
                if m.cc.is_empty() { String::new() } else { format!("Cc: {}\n", m.cc) },
                m.summary.subject
            );
            format!("{sig}{header}{}", m.body_text)
        };
        let prefixed = |p: &str, s: &str| {
            if s.to_lowercase().starts_with(&p.to_lowercase()) {
                s.to_string()
            } else {
                format!("{p} {s}")
            }
        };
        match (kind, original) {
            (ComposeKind::New, _) => {}
            (ComposeKind::To(addr), _) => c.to = addr,
            (ComposeKind::Reply(_), Some(m)) => {
                c.to = m.summary.from_addr.clone();
                c.subject = prefixed("AW:", &m.summary.subject);
                c.body = quote(&m);
            }
            (ComposeKind::ReplyAll(_), Some(m)) => {
                let others: Vec<String> =
                    providers::split_addresses(&format!("{}, {}", m.to, m.cc))
                        .into_iter()
                        .filter(|a| {
                            a.to_lowercase() != me
                                && a.to_lowercase() != m.summary.from_addr.to_lowercase()
                        })
                        .collect();
                c.to = m.summary.from_addr.clone();
                c.cc = others.join("; ");
                c.subject = prefixed("AW:", &m.summary.subject);
                c.body = quote(&m);
            }
            (ComposeKind::Forward(_), Some(m)) => {
                c.subject = prefixed("WG:", &m.summary.subject);
                c.body = quote(&m);
                c.attachments =
                    db::attachments_with_data(&self.st.db(), m.summary.id).unwrap_or_default();
            }
            (ComposeKind::EditDraft(id), Some(m)) => {
                c.to = m.to.clone();
                c.cc = m.cc.clone();
                c.subject = if m.summary.subject == "(kein Betreff)" {
                    String::new()
                } else {
                    m.summary.subject.clone()
                };
                c.body = m.body_text.clone();
                c.draft_id = Some(id);
                c.attachments = db::attachments_with_data(&self.st.db(), id).unwrap_or_default();
            }
            _ => {}
        }
        self.open_window(Win::Compose(c));
    }

    /// Returns true when the window should close.
    pub(super) fn compose_window(
        &mut self,
        ctx: &egui::Context,
        vid: egui::ViewportId,
        wid: u64,
        c: &mut Composer,
    ) -> bool {
        if let Some(id) = c.read {
            return self.read_window(ctx, vid, id);
        }
        let t = self.t;
        let title = format!(
            "{} - Nachricht (Nur-Text)",
            if c.subject.trim().is_empty() {
                "Unbenannt"
            } else {
                c.subject.trim()
            }
        );
        let accounts = self.data.accounts.clone();
        let contacts = self.data.contacts.clone();
        let signature = self.prefs.signature.clone();
        let mut send = false;
        let mut save = false;
        let mut discard = false;
        let closed = chrome::native_window(ctx, vid, &title, [900.0, 700.0], &t, |ui| {
            chrome::item_ribbon(ui, &t, "Nachricht", |ui| {
                chrome::group(ui, &t, "Namen", |ui| {
                    if chrome::large(ui, &t, ph::ADDRESS_BOOK, IC_BLUE, "Adressbuch", !c.sending)
                        .clicked()
                    {
                        c.address_book = Some(0);
                    }
                    if chrome::large(
                        ui,
                        &t,
                        ph::USER_CHECK,
                        IC_BLUE,
                        "Namen\nüberprüfen",
                        !c.sending,
                    )
                    .clicked()
                    {
                        c.to = resolve_names(&c.to, &contacts);
                        c.cc = resolve_names(&c.cc, &contacts);
                        c.bcc = resolve_names(&c.bcc, &contacts);
                    }
                });
                chrome::group(ui, &t, "Einschließen", |ui| {
                    if chrome::large(ui, &t, ph::PAPERCLIP, IC_BLUE, "Datei\nanfügen", !c.sending)
                        .clicked()
                    {
                        if let Some(files) = rfd::FileDialog::new()
                            .set_title("Datei einfügen")
                            .pick_files()
                        {
                            for f in files {
                                match std::fs::read(&f) {
                                    Ok(data) => c.attachments.push(NewAttachment {
                                        name: f
                                            .file_name()
                                            .map(|n| n.to_string_lossy().into_owned())
                                            .unwrap_or_else(|| "Anlage".into()),
                                        mime: mime_for(&f),
                                        data,
                                    }),
                                    Err(e) => tracing::warn!("{}: {e}", f.display()),
                                }
                                c.changed = true;
                            }
                        }
                    }
                    if chrome::large(
                        ui,
                        &t,
                        ph::SIGNATURE,
                        IC_BLUE,
                        "Signatur",
                        !signature.is_empty(),
                    )
                    .clicked()
                    {
                        c.body.push_str(&format!("\n\n{signature}"));
                        c.changed = true;
                    }
                    chrome::column(ui, |ui| {
                        if chrome::small(ui, &t, ph::CLOCK, IC_BLUE, "Datum und Uhrzeit", true)
                            .clicked()
                        {
                            c.body.push_str(
                                &chrono::Local::now().format("%d.%m.%Y %H:%M").to_string(),
                            );
                            c.changed = true;
                        }
                    });
                });
                chrome::group(ui, &t, "Kategorien", |ui| {
                    chrome::column(ui, |ui| {
                        let high = c.importance == "high";
                        if chrome::small(
                            ui,
                            &t,
                            "!",
                            IC_RED,
                            if high {
                                "Wichtigkeit: hoch  \u{E182}"
                            } else {
                                "Wichtigkeit: hoch"
                            },
                            true,
                        )
                        .clicked()
                        {
                            c.importance = if high { "normal".into() } else { "high".into() };
                        }
                        let low = c.importance == "low";
                        if chrome::small(
                            ui,
                            &t,
                            ph::ARROW_DOWN,
                            IC_BLUE,
                            if low {
                                "Wichtigkeit: niedrig  \u{E182}"
                            } else {
                                "Wichtigkeit: niedrig"
                            },
                            true,
                        )
                        .clicked()
                        {
                            c.importance = if low { "normal".into() } else { "low".into() };
                        }
                    });
                });
                chrome::group(ui, &t, "Felder anzeigen", |ui| {
                    if chrome::large(
                        ui,
                        &t,
                        ph::USERS,
                        IC_GREY,
                        if c.show_bcc { "Bcc  \u{E182}" } else { "Bcc" },
                        true,
                    )
                    .clicked()
                    {
                        c.show_bcc = !c.show_bcc;
                    }
                });
            });

            egui::Frame::new()
                .inner_margin(egui::Margin::symmetric(12, 10))
                .show(ui, |ui| {
                    ui.add_enabled_ui(!c.sending, |ui| {
                        ui.horizontal_top(|ui| {
                            // Big "Senden" button left of the address rows, like Outlook.
                            let rows = if c.show_bcc { 4.0 } else { 3.0 };
                            let (r, resp) = ui.allocate_exact_size(
                                egui::vec2(64.0, rows * 30.0 - 6.0),
                                egui::Sense::click(),
                            );
                            let bg = if resp.hovered() {
                                t.button_hover
                            } else {
                                t.ribbon
                            };
                            ui.painter().rect(
                                r,
                                0,
                                bg,
                                egui::Stroke::new(1.0_f32, t.border),
                                egui::StrokeKind::Inside,
                            );
                            ui.painter().text(
                                r.center() - egui::vec2(0.0, 10.0),
                                egui::Align2::CENTER_CENTER,
                                ph::PAPER_PLANE_RIGHT,
                                chrome::fill_font(24.0),
                                IC_BLUE,
                            );
                            ui.painter().text(
                                r.center() + egui::vec2(0.0, 16.0),
                                egui::Align2::CENTER_CENTER,
                                if c.sending { "…" } else { "Senden" },
                                egui::FontId::proportional(12.5),
                                t.text,
                            );
                            if resp.on_hover_text("Senden (Alt+S)").clicked() {
                                send = true;
                            }
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 4.0;
                                let w = ui.available_width();
                                ui.horizontal(|ui| {
                                    let from = accounts
                                        .iter()
                                        .find(|a| a.id == c.account_id)
                                        .map(|a| a.email.clone())
                                        .unwrap_or_default();
                                    let r = ui.add_sized(
                                        [72.0, 26.0],
                                        egui::Button::new(format!("Von {}", ph::CARET_DOWN)),
                                    );
                                    chrome::menu(&r, |ui| {
                                        for a in &accounts {
                                            if ui.radio(c.account_id == a.id, &a.email).clicked() {
                                                c.account_id = a.id;
                                                ui.close();
                                            }
                                        }
                                    });
                                    ui.label(from);
                                });
                                let mut book = None;
                                for (i, label) in ["An…", "Cc…", "Bcc…"].into_iter().enumerate()
                                {
                                    if i == 2 && !c.show_bcc {
                                        continue;
                                    }
                                    ui.horizontal(|ui| {
                                        if ui
                                            .add_sized([72.0, 26.0], egui::Button::new(label))
                                            .clicked()
                                        {
                                            book = Some(i as u8);
                                        }
                                        let field = match i {
                                            0 => &mut c.to,
                                            1 => &mut c.cc,
                                            _ => &mut c.bcc,
                                        };
                                        if address_field(
                                            ui,
                                            ("addr", wid, i),
                                            field,
                                            &contacts,
                                            w - 84.0,
                                        ) {
                                            c.changed = true;
                                        }
                                    });
                                }
                                if book.is_some() {
                                    c.address_book = book;
                                }
                            });
                        });
                        ui.horizontal(|ui| {
                            ui.add_sized([136.0, 24.0], egui::Label::new("Betreff"));
                            let r = ui.add(
                                egui::TextEdit::singleline(&mut c.subject)
                                    .desired_width(ui.available_width() - 4.0),
                            );
                            c.changed |= r.changed();
                        });
                        if !c.attachments.is_empty() {
                            ui.horizontal_wrapped(|ui| {
                                ui.add_sized([136.0, 24.0], egui::Label::new("Angefügt"));
                                let mut remove = None;
                                for (i, a) in c.attachments.iter().enumerate() {
                                    let r = ui.add(egui::Button::new((
                                        RichText::new(ph::PAPERCLIP).color(IC_BLUE),
                                        format!(
                                            "{} ({})",
                                            a.name,
                                            mail::human_size(a.data.len() as i64)
                                        ),
                                        RichText::new(ph::X).color(t.text_weak),
                                    )));
                                    if r.on_hover_text("Anlage entfernen").clicked() {
                                        remove = Some(i);
                                    }
                                }
                                if let Some(i) = remove {
                                    c.attachments.remove(i);
                                    c.changed = true;
                                }
                            });
                        }
                        ui.add_space(6.0);
                        let (w, h) = (ui.available_width(), ui.available_height().max(200.0));
                        egui::ScrollArea::vertical()
                            .id_salt(("body", wid))
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                let r = ui.add_sized(
                                    [w - 14.0, h - 8.0],
                                    egui::TextEdit::multiline(&mut c.body)
                                        .frame(false)
                                        .lock_focus(true),
                                );
                                c.changed |= r.changed();
                            });
                    });
                });

            if ui.input(|i| {
                (i.modifiers.alt && i.key_pressed(egui::Key::S))
                    || (i.modifiers.command && i.key_pressed(egui::Key::Enter))
            }) {
                send = true;
            }
            if ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::S)) {
                save = true;
            }
            if let Some(field) = c.address_book {
                let mut open = true;
                egui::Window::new("Namen auswählen: Kontakte")
                    .open(&mut open)
                    .collapsible(false)
                    .resizable(false)
                    .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                    .show(ui.ctx(), |ui| {
                        ui.label("Doppelklick (oder Klick) fügt den Namen hinzu:");
                        egui::ScrollArea::vertical()
                            .max_height(300.0)
                            .show(ui, |ui| {
                                for k in contacts.iter().filter(|k| !k.email.is_empty()) {
                                    if ui
                                        .add(
                                            egui::Button::new(format!(
                                                "{}   <{}>",
                                                k.display_name, k.email
                                            ))
                                            .frame(false),
                                        )
                                        .clicked()
                                    {
                                        let target = match field {
                                            0 => &mut c.to,
                                            1 => &mut c.cc,
                                            _ => &mut c.bcc,
                                        };
                                        let sep = if target.trim().is_empty()
                                            || target.trim_end().ends_with(';')
                                        {
                                            ""
                                        } else {
                                            "; "
                                        };
                                        target.push_str(&format!(
                                            "{sep}{} <{}>; ",
                                            k.display_name, k.email
                                        ));
                                    }
                                }
                            });
                    });
                if !open {
                    c.address_book = None;
                }
            }
            if c.confirm_close {
                egui::Window::new("MailCraft")
                    .collapsible(false)
                    .resizable(false)
                    .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                    .show(ui.ctx(), |ui| {
                        ui.label("Möchten Sie die Änderungen speichern?");
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            if ui.button("     Ja     ").clicked() {
                                save = true;
                                discard = true;
                            }
                            if ui.button("    Nein    ").clicked() {
                                discard = true;
                            }
                            if ui.button(" Abbrechen ").clicked() {
                                c.confirm_close = false;
                            }
                        });
                    });
            }
        });

        if save {
            let m = c.to_outgoing();
            let old = c.draft_id;
            let res = providers::save_draft(&self.st, &m).and_then(|new_id| {
                if let Some(old) = old {
                    db::delete_message(&self.st.db(), old)?;
                }
                Ok(new_id)
            });
            match res {
                Ok(new_id) => {
                    c.draft_id = Some(new_id);
                    c.changed = false;
                    self.toast("Element gespeichert.", false);
                    self.dirty = true;
                }
                Err(e) => self.toast(format!("{e:#}"), true),
            }
        }
        if send && !c.sending {
            c.sending = true;
            let m = c.to_outgoing();
            let draft = c.draft_id;
            self.busy += 1;
            let (st, tx, ctx) = (self.st.clone(), self.tx.clone(), self.ctx.clone());
            self.rt.spawn(async move {
                let msg = match providers::send(&st, &m).await {
                    Ok(()) => {
                        if let Some(d) = draft {
                            let _ = db::delete_message(&st.db(), d);
                        }
                        Done::Sent(wid)
                    }
                    Err(e) => Done::SendFailed(wid, format!("{e:#}")),
                };
                let _ = tx.send(msg);
                ctx.request_repaint();
            });
        }
        if discard {
            return true;
        }
        if closed {
            if c.changed && !c.sending {
                c.confirm_close = true;
                ctx.send_viewport_cmd_to(vid, egui::ViewportCommand::CancelClose);
                return false;
            }
            return true;
        }
        false
    }

    /// Read-only message window with Outlook's "Nachricht" ribbon.
    fn read_window(&mut self, ctx: &egui::Context, vid: egui::ViewportId, id: i64) -> bool {
        let t = self.t;
        let Some(m) = db::get_message(&self.st.db(), id).ok().flatten() else {
            return true;
        };
        let s = m.summary.clone();
        let atts = db::list_attachments(&self.st.db(), id).unwrap_or_default();
        let title = format!("{} - Nachricht", s.subject);
        let folders: Vec<Folder> = self
            .data
            .folders
            .iter()
            .filter(|f| f.account_id == s.account_id && f.id != s.folder_id)
            .cloned()
            .collect();
        let mut acts = vec![];
        let mut close = chrome::native_window(ctx, vid, &title, [900.0, 720.0], &t, |ui| {
            chrome::item_ribbon(ui, &t, "Nachricht", |ui| {
                chrome::group(ui, &t, "Löschen", |ui| {
                    if chrome::large(ui, &t, ph::X, IC_RED, "Löschen", true).clicked() {
                        acts.push(Act::Delete(s.id));
                    }
                    if chrome::large(ui, &t, ph::ARCHIVE, IC_GREY, "Archivieren", true).clicked() {
                        if let Some(f) = folders.iter().find(|f| f.role == "archive") {
                            acts.push(Act::Move(s.id, f.id));
                        }
                    }
                });
                chrome::group(ui, &t, "Antworten", |ui| {
                    if chrome::large(ui, &t, ph::ARROW_BEND_UP_LEFT, IC_PURPLE, "Antworten", true)
                        .clicked()
                    {
                        acts.push(Act::Compose(ComposeKind::Reply(s.id)));
                    }
                    if chrome::large(
                        ui,
                        &t,
                        ph::ARROW_BEND_DOUBLE_UP_LEFT,
                        IC_PURPLE,
                        "Allen\nantworten",
                        true,
                    )
                    .clicked()
                    {
                        acts.push(Act::Compose(ComposeKind::ReplyAll(s.id)));
                    }
                    if chrome::large(
                        ui,
                        &t,
                        ph::ARROW_BEND_UP_RIGHT,
                        IC_PURPLE,
                        "Weiterleiten",
                        true,
                    )
                    .clicked()
                    {
                        acts.push(Act::Compose(ComposeKind::Forward(s.id)));
                    }
                });
                chrome::group(ui, &t, "Verschieben", |ui| {
                    let r = chrome::large(
                        ui,
                        &t,
                        ph::FOLDER_SIMPLE,
                        IC_YELLOW,
                        "Verschieben\n\u{E136}",
                        true,
                    );
                    chrome::menu(&r, |ui| {
                        for f in &folders {
                            if chrome::menu_item(ui, mail::folder_icon(&f.role), IC_YELLOW, &f.name)
                            {
                                acts.push(Act::Move(s.id, f.id));
                            }
                        }
                    });
                });
                chrome::group(ui, &t, "Kategorien", |ui| {
                    chrome::column(ui, |ui| {
                        if chrome::small(
                            ui,
                            &t,
                            ph::ENVELOPE_SIMPLE,
                            IC_BLUE,
                            "Als ungelesen markieren",
                            true,
                        )
                        .clicked()
                        {
                            acts.push(Act::Patch(
                                s.id,
                                MessagePatch {
                                    is_read: Some(false),
                                    ..Default::default()
                                },
                            ));
                        }
                        let r = chrome::small(
                            ui,
                            &t,
                            ph::TAG,
                            IC_PURPLE,
                            "Kategorisieren \u{E136}",
                            true,
                        );
                        chrome::menu(&r, |ui| mail::category_menu(ui, &s, &mut acts));
                        if chrome::small(
                            ui,
                            &t,
                            ph::FLAG,
                            IC_RED,
                            if s.is_flagged {
                                "Als erledigt markieren"
                            } else {
                                "Zur Nachverfolgung"
                            },
                            true,
                        )
                        .clicked()
                        {
                            acts.push(Act::Patch(
                                s.id,
                                MessagePatch {
                                    is_flagged: Some(!s.is_flagged),
                                    ..Default::default()
                                },
                            ));
                        }
                    });
                });
            });
            mail::read_message(ui, &t, &m, &atts, false, &mut acts);
        });
        // Delete/move closes the window, like Outlook.
        if acts
            .iter()
            .any(|a| matches!(a, Act::Delete(_) | Act::Move(..)))
        {
            close = true;
        }
        self.mail_apply(acts);
        close
    }
}

/// MIME type from the file extension (for attachments).
fn mime_for(path: &std::path::Path) -> String {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    match ext.as_str() {
        "pdf" => "application/pdf",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "txt" | "log" => "text/plain",
        "csv" => "text/csv",
        "htm" | "html" => "text/html",
        "zip" => "application/zip",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xls" => "application/vnd.ms-excel",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "ppt" => "application/vnd.ms-powerpoint",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "ics" => "text/calendar",
        _ => "application/octet-stream",
    }
    .into()
}

/// Replace names without address by the matching contact ("Namen überprüfen").
fn resolve_names(field: &str, contacts: &[Contact]) -> String {
    field
        .split([';', ','])
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(|p| {
            if p.contains('@') {
                return p.to_string();
            }
            let lower = p.to_lowercase();
            match contacts
                .iter()
                .find(|c| !c.email.is_empty() && c.display_name.to_lowercase().contains(&lower))
            {
                Some(c) => format!("{} <{}>", c.display_name, c.email),
                None => p.to_string(),
            }
        })
        .collect::<Vec<_>>()
        .join("; ")
}

/// Recipient field with suggestions from the address book ("AutoVervollständigen").
fn address_field(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash,
    value: &mut String,
    contacts: &[Contact],
    width: f32,
) -> bool {
    let resp = ui.add(
        egui::TextEdit::singleline(value)
            .id_salt(id)
            .desired_width(width),
    );
    let mut changed = resp.changed();
    let token = value
        .rsplit([';', ','])
        .next()
        .unwrap_or("")
        .trim()
        .to_lowercase();
    if !resp.has_focus() || token.is_empty() || token.contains('>') {
        return changed;
    }
    let hits: Vec<&Contact> = contacts
        .iter()
        .filter(|c| !c.email.is_empty())
        .filter(|c| {
            c.display_name.to_lowercase().contains(&token)
                || c.email.to_lowercase().contains(&token)
        })
        .take(6)
        .collect();
    if hits.is_empty() || hits.iter().any(|c| c.email.to_lowercase() == token) {
        return changed;
    }
    egui::Area::new(resp.id.with("suggest"))
        .order(egui::Order::Foreground)
        .fixed_pos(resp.rect.left_bottom() + egui::vec2(0.0, 1.0))
        .show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_min_width(resp.rect.width().min(420.0));
                for c in hits {
                    let r = ui.add(
                        egui::Button::new(RichText::new(format!(
                            "{}   <{}>",
                            c.display_name, c.email
                        )))
                        .frame(false),
                    );
                    if r.clicked() || r.is_pointer_button_down_on() {
                        let keep = value
                            .rfind([';', ','])
                            .map(|i| value[..=i].to_string() + " ")
                            .unwrap_or_default();
                        *value = format!("{keep}{} <{}>; ", c.display_name, c.email);
                        changed = true;
                        resp.request_focus();
                    }
                }
            });
        });
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_names_from_contacts() {
        let contacts = vec![Contact {
            display_name: "Anna Schmidt".into(),
            email: "anna@x.de".into(),
            ..Default::default()
        }];
        assert_eq!(
            resolve_names("anna; bob@x.de; Unbekannt", &contacts),
            "Anna Schmidt <anna@x.de>; bob@x.de; Unbekannt"
        );
    }
}
