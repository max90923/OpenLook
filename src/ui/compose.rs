//! Compose windows (new mail, reply, reply all, forward, drafts).

use eframe::egui::{self, RichText};

use super::{App, Done, DANGER};
use crate::{
    db, mail,
    models::*,
    providers::{self, OutgoingMail},
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
    pub id: u64,
    pub sending: bool,
    account_id: i64,
    to: String,
    cc: String,
    bcc: String,
    show_cc: bool,
    subject: String,
    body: String,
    importance: String,
    /// Draft this composer was opened from (deleted after sending).
    draft_id: Option<i64>,
}

impl App {
    pub fn open_composer(&mut self, kind: ComposeKind) {
        let default_account = self
            .mail
            .folder_id
            .and_then(|f| self.data.folders.iter().find(|x| x.id == f))
            .map(|f| f.account_id)
            .unwrap_or_else(|| self.data.local_account_id());
        let sig = if self.data.signature.trim().is_empty() {
            String::new()
        } else {
            format!("\n\n{}", self.data.signature)
        };
        let mut c = Composer {
            id: self.next_composer,
            sending: false,
            account_id: default_account,
            to: String::new(),
            cc: String::new(),
            bcc: String::new(),
            show_cc: false,
            subject: String::new(),
            body: sig.clone(),
            importance: "normal".into(),
            draft_id: None,
        };
        self.next_composer += 1;

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
                c.show_cc = !c.cc.is_empty();
                c.subject = prefixed("AW:", &m.summary.subject);
                c.body = quote(&m);
            }
            (ComposeKind::Forward(_), Some(m)) => {
                c.subject = prefixed("WG:", &m.summary.subject);
                c.body = quote(&m);
            }
            (ComposeKind::EditDraft(id), Some(m)) => {
                c.to = m.to.clone();
                c.cc = m.cc.clone();
                c.show_cc = !c.cc.is_empty();
                c.subject = if m.summary.subject == "(kein Betreff)" {
                    String::new()
                } else {
                    m.summary.subject.clone()
                };
                c.body = m.body_text.clone();
                c.draft_id = Some(id);
            }
            _ => {}
        }
        self.composers.push(c);
    }

    pub fn composers_ui(&mut self, ctx: &egui::Context) {
        let mut send = vec![];
        let mut save = vec![];
        let mut close = vec![];
        let accounts = self.data.accounts.clone();
        let contacts = self.data.contacts.clone();
        for (i, c) in self.composers.iter_mut().enumerate() {
            let mut open = true;
            let title = if c.subject.is_empty() {
                "Neue Nachricht".to_string()
            } else {
                c.subject.clone()
            };
            egui::Window::new(title)
                .id(egui::Id::new(("composer", c.id)))
                .open(&mut open)
                .default_size([720.0, 560.0])
                .default_pos(
                    ctx.content_rect().center() - egui::vec2(360.0, 300.0)
                        + egui::vec2(24.0, 24.0) * i as f32,
                )
                .resizable(true)
                .max_width(ctx.content_rect().width() - 80.0)
                .collapsible(true)
                .show(ctx, |ui| {
                    ui.add_enabled_ui(!c.sending, |ui| {
                        ui.horizontal(|ui| {
                            if super::primary_button(
                                ui,
                                if c.sending {
                                    "⏳ Wird gesendet…"
                                } else {
                                    "📤  Senden"
                                },
                            )
                            .clicked()
                                || ui.input(|i| {
                                    i.modifiers.command && i.key_pressed(egui::Key::Enter)
                                })
                            {
                                send.push(c.id);
                            }
                            if ui.button("💾 Entwurf speichern").clicked() {
                                save.push(c.id);
                            }
                            if ui
                                .button(RichText::new("🗑 Verwerfen").color(DANGER))
                                .clicked()
                            {
                                close.push(c.id);
                            }
                            ui.separator();
                            egui::ComboBox::from_id_salt(("imp", c.id))
                                .selected_text(match c.importance.as_str() {
                                    "high" => "❗ Hohe Wichtigkeit",
                                    "low" => "⬇ Niedrige Wichtigkeit",
                                    _ => "Normale Wichtigkeit",
                                })
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(
                                        &mut c.importance,
                                        "normal".into(),
                                        "Normale Wichtigkeit",
                                    );
                                    ui.selectable_value(
                                        &mut c.importance,
                                        "high".into(),
                                        "❗ Hohe Wichtigkeit",
                                    );
                                    ui.selectable_value(
                                        &mut c.importance,
                                        "low".into(),
                                        "⬇ Niedrige Wichtigkeit",
                                    );
                                });
                        });
                        ui.separator();
                        let label = |ui: &mut egui::Ui, t: &str| {
                            ui.add_sized([56.0, 22.0], egui::Label::new(RichText::new(t).weak()));
                        };
                        ui.horizontal(|ui| {
                            label(ui, "Von");
                            egui::ComboBox::from_id_salt(("from", c.id))
                                .width(320.0)
                                .selected_text(
                                    accounts
                                        .iter()
                                        .find(|a| a.id == c.account_id)
                                        .map(|a| a.email.clone())
                                        .unwrap_or_default(),
                                )
                                .show_ui(ui, |ui| {
                                    for a in &accounts {
                                        ui.selectable_value(&mut c.account_id, a.id, &a.email);
                                    }
                                });
                        });
                        ui.horizontal(|ui| {
                            label(ui, "An");
                            address_field(ui, ("to", c.id), &mut c.to, &contacts);
                            if !c.show_cc && ui.small_button("Cc/Bcc").clicked() {
                                c.show_cc = true;
                            }
                        });
                        if c.show_cc {
                            ui.horizontal(|ui| {
                                label(ui, "Cc");
                                address_field(ui, ("cc", c.id), &mut c.cc, &contacts);
                            });
                            ui.horizontal(|ui| {
                                label(ui, "Bcc");
                                address_field(ui, ("bcc", c.id), &mut c.bcc, &contacts);
                            });
                        }
                        ui.horizontal(|ui| {
                            label(ui, "Betreff");
                            ui.add(
                                egui::TextEdit::singleline(&mut c.subject)
                                    .hint_text("Betreff hinzufügen")
                                    .desired_width(ui.available_width() - 16.0),
                            );
                        });
                        ui.separator();
                        // Fixed sizes: widgets sized from `available_*` inside a
                        // window would otherwise make it grow every frame.
                        let (w, h) = (ui.available_width() - 4.0, ui.available_height().max(300.0));
                        egui::ScrollArea::vertical()
                            .id_salt(("body", c.id))
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                ui.add_sized(
                                    [w - 12.0, h - 8.0],
                                    egui::TextEdit::multiline(&mut c.body)
                                        .hint_text("Nachricht eingeben…")
                                        .lock_focus(true),
                                );
                            });
                    });
                });
            if !open {
                close.push(c.id);
            }
        }
        for id in save {
            let Some(c) = self.composers.iter().find(|c| c.id == id) else {
                continue;
            };
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
                    if let Some(c) = self.composers.iter_mut().find(|c| c.id == id) {
                        c.draft_id = Some(new_id);
                    }
                    self.toast("Entwurf gespeichert", false);
                    self.dirty = true;
                }
                Err(e) => self.toast(format!("{e:#}"), true),
            }
        }
        for id in send {
            let Some(c) = self.composers.iter_mut().find(|c| c.id == id) else {
                continue;
            };
            if c.sending {
                continue;
            }
            c.sending = true;
            let m = c.to_outgoing();
            let draft = c.draft_id;
            self.busy += 1;
            let (st, tx, ctx) = (self.st.clone(), self.tx.clone(), self.ctx.clone());
            self.rt.spawn(async move {
                let res = providers::send(&st, &m).await;
                let msg = match res {
                    Ok(()) => {
                        if let Some(d) = draft {
                            let _ = db::delete_message(&st.db(), d);
                        }
                        Done::Sent(id)
                    }
                    Err(e) => Done::SendFailed(id, format!("{e:#}")),
                };
                let _ = tx.send(msg);
                ctx.request_repaint();
            });
        }
        self.composers.retain(|c| !close.contains(&c.id));
    }
}

impl Composer {
    fn to_outgoing(&self) -> OutgoingMail {
        OutgoingMail {
            account_id: self.account_id,
            to: self.to.clone(),
            cc: self.cc.clone(),
            bcc: self.bcc.clone(),
            subject: self.subject.clone(),
            body_html: mail::text_to_html(&self.body),
            importance: self.importance.clone(),
        }
    }
}

/// Recipient field with suggestions from the address book.
fn address_field(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash,
    value: &mut String,
    contacts: &[Contact],
) {
    let resp = ui.add(
        egui::TextEdit::singleline(value)
            .id_salt(id)
            .hint_text("Name oder E-Mail-Adresse; mehrere mit ; trennen")
            .desired_width(ui.available_width() - 80.0),
    );
    let token = value
        .rsplit([';', ','])
        .next()
        .unwrap_or("")
        .trim()
        .to_lowercase();
    if !resp.has_focus() || token.is_empty() || token.contains('@') && token.ends_with('>') {
        return;
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
        return;
    }
    egui::Area::new(resp.id.with("suggest"))
        .order(egui::Order::Foreground)
        .fixed_pos(resp.rect.left_bottom() + egui::vec2(0.0, 2.0))
        .show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_min_width(resp.rect.width());
                for c in hits {
                    let r = ui.add(
                        egui::Button::new(format!("{}  <{}>", c.display_name, c.email))
                            .frame(false),
                    );
                    if r.clicked() || r.is_pointer_button_down_on() {
                        let keep = value
                            .rfind([';', ','])
                            .map(|i| value[..=i].to_string() + " ")
                            .unwrap_or_default();
                        *value = format!("{keep}{} <{}>; ", c.display_name, c.email);
                        resp.request_focus();
                    }
                }
            });
        });
}
