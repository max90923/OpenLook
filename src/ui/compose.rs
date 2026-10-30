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
    /// Forward to a fixed recipient (QuickStep "An Vorgesetzte(n)").
    ForwardTo(i64, String),
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
    /// Last selection in the body (char indices) for the format buttons.
    sel: (usize, usize),
    /// `sel` was changed by a format button and must be pushed into the editor.
    sel_dirty: bool,
    tab: usize,
    read_receipt: bool,
    delivery_receipt: bool,
    /// "Antworten richten an": Some = field shown.
    reply_to: Option<String>,
    /// "Übermittlung verzögern": not before this local date and time.
    send_at: Option<(chrono::NaiveDate, chrono::NaiveTime)>,
    file_menu: bool,
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
            sel: (0, 0),
            sel_dirty: false,
            tab: 0,
            read_receipt: false,
            delivery_receipt: false,
            reply_to: None,
            send_at: None,
            file_menu: false,
        }
    }

    fn to_outgoing(&self) -> OutgoingMail {
        OutgoingMail {
            account_id: self.account_id,
            to: self.to.clone(),
            cc: self.cc.clone(),
            bcc: self.bcc.clone(),
            subject: self.subject.clone(),
            body_html: super::html::markup_to_html(&self.body),
            importance: self.importance.clone(),
            attachments: self.attachments.clone(),
            read_receipt: self.read_receipt,
            delivery_receipt: self.delivery_receipt,
            reply_to: self.reply_to.clone().unwrap_or_default(),
            send_at: self
                .send_at
                .map(|(d, t)| super::local_to_utc_string(d.and_time(t))),
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
            | ComposeKind::ForwardTo(id, _)
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
            (ComposeKind::ForwardTo(_, to), Some(m)) => {
                c.to = to;
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
            "{} - Nachricht (HTML)",
            if c.subject.trim().is_empty() {
                "Unbenannt"
            } else {
                c.subject.trim()
            }
        );
        let accounts = self.data.accounts.clone();
        let contacts = self.data.contacts.clone();
        let groups = self.data.groups.clone();
        let signature = self.prefs.signature.clone();
        let mut send = false;
        let mut save = false;
        let mut discard = false;
        let closed = chrome::native_window(ctx, vid, &title, [900.0, 700.0], &t, |ui| {
            // Alt+S sends; drop the "s" the keyboard also typed so it doesn't land in the text.
            if ui.input(|i| super::pressed_alt(i, egui::Key::S)) {
                send = true;
                ui.ctx()
                    .input_mut(|i| i.events.retain(|e| !matches!(e, egui::Event::Text(_))));
            }
            let mut tab = c.tab;
            let file = chrome::item_ribbon_tabs(
                ui,
                &t,
                &["Nachricht", "Einfügen", "Optionen", "Text formatieren"],
                &mut tab,
                |ui, tab| match tab {
                    0 => {
                        group_names(ui, &t, c, &contacts, &groups);
                        group_basic_text(ui, &t, c);
                        chrome::group(ui, &t, "Einschließen", |ui| {
                            attach_button(ui, &t, c);
                            signature_button(ui, &t, c, &signature);
                        });
                        group_tags(ui, &t, c);
                    }
                    1 => {
                        chrome::group(ui, &t, "Einschließen", |ui| {
                            attach_button(ui, &t, c);
                            let r = chrome::large(
                                ui,
                                &t,
                                ph::IDENTIFICATION_CARD,
                                IC_BLUE,
                                "Visiten-\nkarte \u{E136}",
                                !contacts.is_empty(),
                            );
                            chrome::menu(&r, |ui| {
                                for k in &contacts {
                                    if ui
                                        .add(egui::Button::new(&k.display_name).frame(false))
                                        .clicked()
                                    {
                                        c.attachments.push(NewAttachment {
                                            name: format!(
                                                "{}.vcf",
                                                mail::safe_file_name(&k.display_name)
                                            ),
                                            mime: "text/vcard".into(),
                                            data: vcard(k).into_bytes(),
                                        });
                                        c.changed = true;
                                        ui.close();
                                    }
                                }
                            });
                            signature_button(ui, &t, c, &signature);
                        });
                        chrome::group(ui, &t, "Links", |ui| {
                            let r =
                                chrome::large(ui, &t, ph::GLOBE, IC_BLUE, "Link \u{E136}", true);
                            chrome::menu(&r, |ui| {
                                ui.label("Adresse:");
                                let id = egui::Id::new(("link_input", wid));
                                let mut url: String = ui
                                    .data(|d| d.get_temp(id))
                                    .unwrap_or_else(|| "https://".to_string());
                                let resp = ui
                                    .add(egui::TextEdit::singleline(&mut url).desired_width(280.0));
                                ui.data_mut(|d| d.insert_temp(id, url.clone()));
                                if ui.button("Einfügen").clicked()
                                    || (resp.lost_focus()
                                        && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                                {
                                    insert_at_cursor(c, &format!(" {} ", url.trim()));
                                    ui.data_mut(|d| d.remove::<String>(id));
                                    ui.close();
                                }
                            });
                        });
                        chrome::group(ui, &t, "Text", |ui| {
                            if chrome::large(ui, &t, ph::CLOCK, IC_BLUE, "Datum und\nUhrzeit", true)
                                .clicked()
                            {
                                insert_at_cursor(
                                    c,
                                    &chrono::Local::now().format("%d.%m.%Y %H:%M").to_string(),
                                );
                            }
                        });
                    }
                    2 => {
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
                        chrome::group(ui, &t, "Verlauf", |ui| {
                            chrome::column(ui, |ui| {
                                let check = |on: bool, label: &str| {
                                    if on {
                                        format!("{label}  \u{E182}")
                                    } else {
                                        label.to_string()
                                    }
                                };
                                if chrome::small(
                                    ui,
                                    &t,
                                    ph::ENVELOPE_SIMPLE_OPEN,
                                    IC_BLUE,
                                    &check(c.read_receipt, "Lesebestätigung anfordern"),
                                    true,
                                )
                                .clicked()
                                {
                                    c.read_receipt = !c.read_receipt;
                                }
                                if chrome::small(
                                    ui,
                                    &t,
                                    ph::PAPER_PLANE_RIGHT,
                                    IC_BLUE,
                                    &check(
                                        c.delivery_receipt,
                                        "Übermittlungsbestätigung anfordern",
                                    ),
                                    true,
                                )
                                .clicked()
                                {
                                    c.delivery_receipt = !c.delivery_receipt;
                                }
                            });
                        });
                        chrome::group(ui, &t, "Weitere Optionen", |ui| {
                            let label = if c.send_at.is_some() {
                                "Übermittlung\nverzögern \u{E182}"
                            } else {
                                "Übermittlung\nverzögern"
                            };
                            if chrome::large(ui, &t, ph::CLOCK, IC_BLUE, label, true).clicked() {
                                c.send_at = match c.send_at {
                                    Some(_) => None,
                                    None => {
                                        let next = chrono::Local::now().naive_local()
                                            + chrono::Duration::hours(1);
                                        Some((
                                            next.date(),
                                            chrono::NaiveTime::from_hms_opt(
                                                chrono::Timelike::hour(&next),
                                                0,
                                                0,
                                            )
                                            .unwrap(),
                                        ))
                                    }
                                };
                            }
                            let label = if c.reply_to.is_some() {
                                "Antworten\nrichten an \u{E182}"
                            } else {
                                "Antworten\nrichten an"
                            };
                            if chrome::large(ui, &t, ph::ARROW_BEND_UP_LEFT, IC_PURPLE, label, true)
                                .clicked()
                            {
                                c.reply_to = if c.reply_to.is_some() {
                                    None
                                } else {
                                    Some(String::new())
                                };
                            }
                        });
                        group_tags(ui, &t, c);
                    }
                    _ => group_basic_text(ui, &t, c),
                },
            );
            c.tab = tab;
            if file {
                c.file_menu = !c.file_menu;
            }
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
                                            &groups,
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
                        if let Some(rt) = &mut c.reply_to {
                            ui.horizontal(|ui| {
                                ui.add_sized([136.0, 24.0], egui::Label::new("Antworten an"));
                                ui.add(
                                    egui::TextEdit::singleline(rt)
                                        .hint_text("Adresse für Antworten")
                                        .desired_width(ui.available_width() - 16.0),
                                );
                            });
                        }
                        if let Some((d, tm)) = &mut c.send_at {
                            ui.horizontal(|ui| {
                                ui.add_sized([136.0, 24.0], egui::Label::new("Nicht senden vor"));
                                ui.add(
                                    egui_extras::DatePickerButton::new(d)
                                        .id_salt(&format!("send_at{wid}"))
                                        .format("%d.%m.%Y"),
                                );
                                egui::ComboBox::from_id_salt(("send_at_time", wid))
                                    .width(70.0)
                                    .selected_text(tm.format("%H:%M").to_string())
                                    .height(300.0)
                                    .show_ui(ui, |ui| {
                                        for i in 0..96 {
                                            let x = chrono::NaiveTime::from_hms_opt(
                                                i / 4,
                                                (i % 4) * 15,
                                                0,
                                            )
                                            .unwrap();
                                            ui.selectable_value(
                                                tm,
                                                x,
                                                x.format("%H:%M").to_string(),
                                            );
                                        }
                                    });
                            });
                        }
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
                                let (text, weak) = (t.text, t.text_weak);
                                let mut layouter =
                                    |ui: &egui::Ui, buf: &dyn egui::TextBuffer, wrap: f32| {
                                        ui.fonts_mut(|f| {
                                            f.layout_job(super::html::markup_job(
                                                buf.as_str(),
                                                14.0,
                                                text,
                                                weak,
                                                wrap,
                                            ))
                                        })
                                    };
                                let body_id = egui::Id::new(("compose-body", wid));
                                if c.sel_dirty {
                                    // Put the cursor/selection where the format button left it.
                                    let mut state =
                                        egui::text_edit::TextEditState::load(ui.ctx(), body_id)
                                            .unwrap_or_default();
                                    state.cursor.set_char_range(Some(
                                        egui::text::CCursorRange::two(
                                            egui::text::CCursor::new(c.sel.0.min(c.sel.1)),
                                            egui::text::CCursor::new(c.sel.0.max(c.sel.1)),
                                        ),
                                    ));
                                    state.store(ui.ctx(), body_id);
                                    ui.ctx().memory_mut(|m| m.request_focus(body_id));
                                    c.sel_dirty = false;
                                }
                                let out = egui::TextEdit::multiline(&mut c.body)
                                    .id(body_id)
                                    .frame(false)
                                    .lock_focus(true)
                                    .min_size(egui::vec2(w - 14.0, h - 8.0))
                                    .desired_width(w - 14.0)
                                    .layouter(&mut layouter)
                                    .show(ui);
                                if let Some(r) = out.cursor_range {
                                    c.sel = (r.primary.index, r.secondary.index);
                                }
                                c.changed |= out.response.changed();
                            });
                    });
                });

            if ui.input(|i| super::pressed(i, egui::Key::Enter, true, false)) {
                send = true;
            }
            for (key, marker) in [
                (egui::Key::B, super::html::BOLD),
                (egui::Key::I, super::html::ITALIC),
                (egui::Key::U, super::html::UNDERLINE),
            ] {
                if ui.input(|i| super::pressed(i, key, true, false)) {
                    c.sel = super::html::toggle_wrap(&mut c.body, c.sel, marker);
                    c.sel_dirty = true;
                    c.changed = true;
                }
            }
            if ui.input(|i| super::pressed(i, egui::Key::S, true, false)) {
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
                                let entries = groups
                                    .iter()
                                    .map(|g| {
                                        (
                                            format!("{}  {}", ph::USERS_THREE, g.name),
                                            g.members.join("; "),
                                        )
                                    })
                                    .chain(contacts.iter().filter(|k| !k.email.is_empty()).map(
                                        |k| {
                                            (
                                                format!("{}   <{}>", k.display_name, k.email),
                                                format!("{} <{}>", k.display_name, k.email),
                                            )
                                        },
                                    ));
                                for (label, insert) in entries {
                                    if ui.add(egui::Button::new(label).frame(false)).clicked() {
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
                                        target.push_str(&format!("{sep}{insert}; "));
                                    }
                                }
                            });
                    });
                if !open {
                    c.address_book = None;
                }
            }
            if c.file_menu {
                let mut open = true;
                egui::Window::new("Datei")
                    .open(&mut open)
                    .collapsible(false)
                    .resizable(false)
                    .anchor(egui::Align2::LEFT_TOP, egui::vec2(8.0, 60.0))
                    .show(ui.ctx(), |ui| {
                        ui.set_min_width(220.0);
                        if chrome::menu_item(ui, ph::FLOPPY_DISK, IC_BLUE, "Speichern (Strg+S)") {
                            save = true;
                            c.file_menu = false;
                        }
                        if chrome::menu_item(ui, ph::FLOPPY_DISK, IC_BLUE, "Speichern unter…") {
                            c.file_menu = false;
                            let from = accounts
                                .iter()
                                .find(|a| a.id == c.account_id)
                                .map(|a| format!("{} <{}>", a.name, a.email))
                                .unwrap_or_default();
                            let name = format!(
                                "{}.eml",
                                mail::safe_file_name(if c.subject.trim().is_empty() {
                                    "Nachricht"
                                } else {
                                    c.subject.trim()
                                })
                            );
                            if let Some(path) = rfd::FileDialog::new()
                                .set_title("Speichern unter")
                                .set_file_name(name)
                                .save_file()
                            {
                                if let Ok(m) = c.to_outgoing().to_lettre(&from) {
                                    let _ = std::fs::write(path, m.formatted());
                                }
                            }
                        }
                        if chrome::menu_item(ui, ph::X, IC_RED, "Schließen") {
                            c.file_menu = false;
                            if c.changed {
                                c.confirm_close = true
                            } else {
                                discard = true
                            }
                        }
                    });
                if !open {
                    c.file_menu = false;
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
            // A group name typed without picking it is expanded on send, like Outlook.
            for f in [&mut c.to, &mut c.cc, &mut c.bcc] {
                *f = expand_groups(f, &groups);
            }
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
        let menu_id = egui::Id::new(("read_file_menu", id));
        let mut close_requested = false;
        let mut close = chrome::native_window(ctx, vid, &title, [900.0, 720.0], &t, |ui| {
            let mut tab = 0;
            let file = chrome::item_ribbon_tabs(ui, &t, &["Nachricht"], &mut tab, |ui, _| {
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
                chrome::group(ui, &t, "Drucken", |ui| {
                    if chrome::large(ui, &t, ph::PRINTER, IC_GREY, "Drucken", true).clicked() {
                        acts.push(Act::Print(s.id));
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
            if file {
                ui.ctx().data_mut(|d| d.insert_temp(menu_id, true));
            }
            if ui
                .ctx()
                .data(|d| d.get_temp::<bool>(menu_id))
                .unwrap_or(false)
            {
                let mut open = true;
                egui::Window::new("Datei")
                    .open(&mut open)
                    .collapsible(false)
                    .resizable(false)
                    .anchor(egui::Align2::LEFT_TOP, egui::vec2(8.0, 60.0))
                    .show(ui.ctx(), |ui| {
                        ui.set_min_width(220.0);
                        if chrome::menu_item(ui, ph::FLOPPY_DISK, IC_BLUE, "Speichern unter…") {
                            ui.ctx().data_mut(|d| d.insert_temp(menu_id, false));
                            let name = format!("{}.eml", mail::safe_file_name(&s.subject));
                            if let Some(path) = rfd::FileDialog::new()
                                .set_title("Speichern unter")
                                .set_file_name(name)
                                .save_file()
                            {
                                let out = OutgoingMail {
                                    to: m.to.clone(),
                                    cc: m.cc.clone(),
                                    subject: s.subject.clone(),
                                    body_html: m.body_html.clone(),
                                    importance: s.importance.clone(),
                                    attachments: db::attachments_with_data(&self.st.db(), s.id)
                                        .unwrap_or_default(),
                                    ..Default::default()
                                };
                                let from = if s.from_name.is_empty() {
                                    s.from_addr.clone()
                                } else {
                                    format!("{} <{}>", s.from_name, s.from_addr)
                                };
                                if let Ok(msg) = out.to_lettre(&from) {
                                    let _ = std::fs::write(path, msg.formatted());
                                }
                            }
                        }
                        if chrome::menu_item(ui, ph::PRINTER, IC_GREY, "Drucken") {
                            ui.ctx().data_mut(|d| d.insert_temp(menu_id, false));
                            acts.push(Act::Print(s.id));
                        }
                        if chrome::menu_item(ui, ph::X, IC_RED, "Schließen") {
                            ui.ctx().data_mut(|d| d.insert_temp(menu_id, false));
                            close_requested = true;
                        }
                    });
                if !open {
                    ui.ctx().data_mut(|d| d.insert_temp(menu_id, false));
                }
            }
            mail::read_message(ui, &t, &m, &atts, false, &mut acts);
        });
        close |= close_requested;
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

/// Replace contact group names by their members.
pub(super) fn expand_groups(field: &str, groups: &[ContactGroup]) -> String {
    field
        .split([';', ','])
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(|p| {
            match groups
                .iter()
                .find(|g| g.name.to_lowercase() == p.to_lowercase())
            {
                Some(g) => g.members.join("; "),
                None => p.to_string(),
            }
        })
        .collect::<Vec<_>>()
        .join("; ")
}

/// Replace names without address by the matching contact or contact group
/// ("Namen überprüfen").
fn resolve_names(field: &str, contacts: &[Contact], groups: &[ContactGroup]) -> String {
    expand_groups(field, groups)
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
/// Enter or Tab takes the first suggestion, a click takes the clicked one.
pub(super) fn address_field(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash,
    value: &mut String,
    contacts: &[Contact],
    groups: &[ContactGroup],
    width: f32,
) -> bool {
    let resp = ui.add(
        egui::TextEdit::singleline(value)
            .id_salt(id)
            .desired_width(width),
    );
    let mut changed = resp.changed();
    let rect_id = resp.id.with("suggest_rect");
    let last_popup: Option<egui::Rect> = ui.data(|d| d.get_temp(rect_id));
    // Keep the list alive for the frame in which a click on it moves the focus away.
    let clicking_popup = last_popup.is_some_and(|r| {
        ui.input(|i| {
            i.pointer.interact_pos().is_some_and(|p| r.contains(p))
                && (i.pointer.any_down() || i.pointer.any_released())
        })
    });
    let confirm = resp.lost_focus()
        && ui.input(|i| i.key_pressed(egui::Key::Enter) || i.key_pressed(egui::Key::Tab));
    let token = value
        .rsplit([';', ','])
        .next()
        .unwrap_or("")
        .trim()
        .to_lowercase();
    let active = resp.has_focus() || clicking_popup || confirm;
    // (shown text, inserted text, address); groups insert all their members.
    let hits: Vec<(String, String, String)> = if active && !token.is_empty() && !token.contains('>')
    {
        groups
            .iter()
            .filter(|g| g.name.to_lowercase().contains(&token))
            .map(|g| {
                (
                    format!("{}  {}", ph::USERS_THREE, g.name),
                    g.members.join("; "),
                    String::new(),
                )
            })
            .chain(
                contacts
                    .iter()
                    .filter(|c| !c.email.is_empty())
                    .filter(|c| {
                        c.display_name.to_lowercase().contains(&token)
                            || c.email.to_lowercase().contains(&token)
                    })
                    .map(|c| {
                        (
                            format!("{}   <{}>", c.display_name, c.email),
                            format!("{} <{}>", c.display_name, c.email),
                            c.email.to_lowercase(),
                        )
                    }),
            )
            .take(6)
            .collect()
    } else {
        vec![]
    };
    let ctx = ui.ctx().clone();
    let take = |value: &mut String, insert: &str| {
        let keep = value
            .rfind([';', ','])
            .map(|i| value[..=i].to_string() + " ")
            .unwrap_or_default();
        *value = format!("{keep}{insert}; ");
        // Continue typing after the inserted name.
        let mut state = egui::text_edit::TextEditState::load(&ctx, resp.id).unwrap_or_default();
        state
            .cursor
            .set_char_range(Some(egui::text::CCursorRange::one(
                egui::text::CCursor::new(value.chars().count()),
            )));
        state.store(&ctx, resp.id);
    };
    if hits.is_empty() || hits.iter().any(|h| h.2 == token) {
        ui.data_mut(|d| d.remove::<egui::Rect>(rect_id));
        return changed;
    }
    if confirm {
        take(value, &hits[0].1);
        ui.data_mut(|d| d.remove::<egui::Rect>(rect_id));
        resp.request_focus();
        return true;
    }
    let area = egui::Area::new(resp.id.with("suggest"))
        .order(egui::Order::Foreground)
        .fixed_pos(resp.rect.left_bottom() + egui::vec2(0.0, 1.0))
        .show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_min_width(resp.rect.width().min(420.0));
                for (i, (label, insert, _)) in hits.iter().enumerate() {
                    let text = RichText::new(label);
                    let r = ui.add(
                        egui::Button::new(if i == 0 { text.strong() } else { text }).frame(false),
                    );
                    if r.clicked() || r.is_pointer_button_down_on() {
                        take(value, insert);
                        changed = true;
                        resp.request_focus();
                    }
                }
            });
        });
    ui.data_mut(|d| d.insert_temp(rect_id, area.response.rect));
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_contact_groups() {
        let groups = vec![ContactGroup {
            name: "Projektteam".into(),
            members: vec!["Anna <anna@x.de>".into(), "bob@x.de".into()],
            ..Default::default()
        }];
        assert_eq!(
            expand_groups("projektteam; carl@x.de", &groups),
            "Anna <anna@x.de>; bob@x.de; carl@x.de"
        );
        assert_eq!(
            resolve_names("Projektteam", &[], &groups),
            "Anna <anna@x.de>; bob@x.de"
        );
    }

    #[test]
    fn resolves_names_from_contacts() {
        let contacts = vec![Contact {
            display_name: "Anna Schmidt".into(),
            email: "anna@x.de".into(),
            ..Default::default()
        }];
        assert_eq!(
            resolve_names("anna; bob@x.de; Unbekannt", &contacts, &[]),
            "Anna Schmidt <anna@x.de>; bob@x.de; Unbekannt"
        );
    }
}

// ---------------------------------------------------------------- ribbon groups of the message window

fn group_names(
    ui: &mut egui::Ui,
    t: &super::theme::Theme,
    c: &mut Composer,
    contacts: &[Contact],
    groups: &[ContactGroup],
) {
    chrome::group(ui, t, "Namen", |ui| {
        if chrome::large(ui, t, ph::ADDRESS_BOOK, IC_BLUE, "Adressbuch", !c.sending).clicked() {
            c.address_book = Some(0);
        }
        if chrome::large(
            ui,
            t,
            ph::USER_CHECK,
            IC_BLUE,
            "Namen\nüberprüfen",
            !c.sending,
        )
        .clicked()
        {
            c.to = resolve_names(&c.to, contacts, groups);
            c.cc = resolve_names(&c.cc, contacts, groups);
            c.bcc = resolve_names(&c.bcc, contacts, groups);
        }
    });
}

fn group_basic_text(ui: &mut egui::Ui, t: &super::theme::Theme, c: &mut Composer) {
    chrome::group(ui, t, "Basistext", |ui| {
        chrome::column(ui, |ui| {
            ui.horizontal(|ui| {
                for (icon, marker, tip) in [
                    (ph::TEXT_B, super::html::BOLD, "Fett (Strg+B)"),
                    (ph::TEXT_ITALIC, super::html::ITALIC, "Kursiv (Strg+I)"),
                    (
                        ph::TEXT_UNDERLINE,
                        super::html::UNDERLINE,
                        "Unterstrichen (Strg+U)",
                    ),
                ] {
                    if chrome::small(ui, t, icon, t.text, "", !c.sending)
                        .on_hover_text(tip)
                        .clicked()
                    {
                        c.sel = super::html::toggle_wrap(&mut c.body, c.sel, marker);
                        c.sel_dirty = true;
                        c.changed = true;
                    }
                }
            });
            ui.horizontal(|ui| {
                if chrome::small(ui, t, ph::LIST_BULLETS, t.text, "", !c.sending)
                    .on_hover_text("Aufzählungszeichen")
                    .clicked()
                {
                    super::html::toggle_list(&mut c.body, c.sel, false);
                    c.sel_dirty = true;
                    c.changed = true;
                }
                if chrome::small(ui, t, ph::LIST_NUMBERS, t.text, "", !c.sending)
                    .on_hover_text("Nummerierung")
                    .clicked()
                {
                    super::html::toggle_list(&mut c.body, c.sel, true);
                    c.sel_dirty = true;
                    c.changed = true;
                }
            });
        });
    });
}

fn group_tags(ui: &mut egui::Ui, t: &super::theme::Theme, c: &mut Composer) {
    chrome::group(ui, t, "Kategorien", |ui| {
        chrome::column(ui, |ui| {
            let high = c.importance == "high";
            if chrome::small(
                ui,
                t,
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
                t,
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
}

fn attach_button(ui: &mut egui::Ui, t: &super::theme::Theme, c: &mut Composer) {
    if chrome::large(ui, t, ph::PAPERCLIP, IC_BLUE, "Datei\nanfügen", !c.sending).clicked() {
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
}

fn signature_button(ui: &mut egui::Ui, t: &super::theme::Theme, c: &mut Composer, signature: &str) {
    if chrome::large(
        ui,
        t,
        ph::SIGNATURE,
        IC_BLUE,
        "Signatur",
        !signature.is_empty(),
    )
    .clicked()
    {
        insert_at_cursor(c, &format!("\n\n{signature}"));
    }
}

/// Insert text at the editor's cursor (or replace the selection).
fn insert_at_cursor(c: &mut Composer, text: &str) {
    let (a, b) = (c.sel.0.min(c.sel.1), c.sel.0.max(c.sel.1));
    let byte = |ch: usize| {
        c.body
            .char_indices()
            .nth(ch)
            .map(|(i, _)| i)
            .unwrap_or(c.body.len())
    };
    let (ba, bb) = (byte(a), byte(b));
    c.body.replace_range(ba..bb, text);
    let pos = a + text.chars().count();
    c.sel = (pos, pos);
    c.sel_dirty = true;
    c.changed = true;
}

fn vcard_escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace(',', "\\,")
        .replace(';', "\\;")
        .replace('\n', "\\n")
}

/// Contact as vCard 3.0 ("Visitenkarte" attachment).
pub(super) fn vcard(c: &Contact) -> String {
    let (first, last) = c
        .display_name
        .rsplit_once(' ')
        .map(|(f, l)| (f.to_string(), l.to_string()))
        .unwrap_or((String::new(), c.display_name.clone()));
    let mut v = vec!["BEGIN:VCARD".to_string(), "VERSION:3.0".into()];
    v.push(format!(
        "N:{};{};;;",
        vcard_escape(&last),
        vcard_escape(&first)
    ));
    v.push(format!("FN:{}", vcard_escape(&c.display_name)));
    if !c.company.is_empty() {
        v.push(format!("ORG:{}", vcard_escape(&c.company)));
    }
    if !c.job_title.is_empty() {
        v.push(format!("TITLE:{}", vcard_escape(&c.job_title)));
    }
    if !c.email.is_empty() {
        v.push(format!("EMAIL;TYPE=INTERNET:{}", c.email));
    }
    if !c.phone.is_empty() {
        v.push(format!("TEL;TYPE=CELL:{}", vcard_escape(&c.phone)));
    }
    if !c.address.is_empty() {
        v.push(format!("ADR;TYPE=WORK:;;{};;;;", vcard_escape(&c.address)));
    }
    if !c.notes.is_empty() {
        v.push(format!("NOTE:{}", vcard_escape(&c.notes)));
    }
    v.push("END:VCARD".into());
    v.join("\r\n") + "\r\n"
}

#[cfg(test)]
mod more_tests {
    use super::*;

    #[test]
    fn builds_vcard() {
        let c = Contact {
            display_name: "Anna Schmidt".into(),
            email: "anna@x.de".into(),
            company: "Contoso, GmbH".into(),
            phone: "+49 1".into(),
            ..Default::default()
        };
        let v = vcard(&c);
        assert!(
            v.starts_with("BEGIN:VCARD\r\nVERSION:3.0\r\nN:Schmidt;Anna;;;\r\nFN:Anna Schmidt\r\n")
        );
        assert!(v.contains("ORG:Contoso\\, GmbH\r\n"));
        assert!(v.contains("EMAIL;TYPE=INTERNET:anna@x.de\r\n"));
        assert!(v.ends_with("END:VCARD\r\n"));
    }

    #[test]
    fn inserts_at_cursor() {
        let mut c = Composer::blank(1);
        c.body = "Hallo Welt".into();
        c.sel = (6, 6);
        insert_at_cursor(&mut c, "liebe ");
        assert_eq!(c.body, "Hallo liebe Welt");
        assert_eq!(c.sel, (12, 12));
    }
}
