//! People module: address book with profile card and editor.

use eframe::egui::{self, RichText, Sense};

use super::{compose::ComposeKind, App, BRAND, DANGER};
use crate::{models::*, providers};

#[derive(Default)]
pub struct PeopleView {
    selected: Option<i64>,
    favorites_only: bool,
    editor: Option<Contact>,
}

impl App {
    pub fn people_ui(&mut self, ui: &mut egui::Ui) {
        let mut compose_to = None;
        let mut delete = None;
        let mut toggle_fav = None;

        egui::TopBottomPanel::top("people_ribbon")
            .frame(super::pane_frame(ui).inner_margin(egui::Margin::symmetric(8, 6)))
            .show_inside(ui, |ui| {
                ui.horizontal(|ui| {
                    if super::primary_button(ui, "👤  Neuer Kontakt").clicked() {
                        self.people.editor = Some(Contact {
                            account_id: self.data.local_account_id(),
                            ..Default::default()
                        });
                    }
                    ui.separator();
                    ui.selectable_value(&mut self.people.favorites_only, false, "Alle Kontakte");
                    ui.selectable_value(&mut self.people.favorites_only, true, "★ Favoriten");
                });
            });
        ui.add_space(8.0);

        let needle = self.search.trim().to_lowercase();
        let list: Vec<Contact> = self
            .data
            .contacts
            .iter()
            .filter(|c| !self.people.favorites_only || c.favorite)
            .filter(|c| {
                needle.is_empty()
                    || [&c.display_name, &c.email, &c.company, &c.phone]
                        .iter()
                        .any(|f| f.to_lowercase().contains(&needle))
            })
            .cloned()
            .collect();

        egui::SidePanel::left("people_list")
            .resizable(true)
            .default_width(340.0)
            .frame(super::pane_frame(ui))
            .show_inside(ui, |ui| {
                egui::Frame::new()
                    .inner_margin(egui::Margin::symmetric(14, 10))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new(format!("Kontakte ({})", list.len()))
                                .size(18.0)
                                .strong(),
                        );
                    });
                ui.separator();
                egui::ScrollArea::vertical()
                    .id_salt("people_scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let mut letter = None;
                        for c in &list {
                            let l = c
                                .display_name
                                .chars()
                                .next()
                                .map(|c| c.to_uppercase().to_string())
                                .unwrap_or_default();
                            if letter.as_ref() != Some(&l) {
                                ui.horizontal(|ui| {
                                    ui.add_space(14.0);
                                    ui.label(RichText::new(&l).strong().color(BRAND));
                                });
                                letter = Some(l);
                            }
                            let w = ui.available_width();
                            let (rect, resp) =
                                ui.allocate_exact_size(egui::vec2(w, 52.0), Sense::click());
                            let v = ui.visuals();
                            if self.people.selected == Some(c.id) {
                                ui.painter().rect_filled(rect, 0, v.selection.bg_fill);
                            } else if resp.hovered() {
                                ui.painter()
                                    .rect_filled(rect, 0, v.widgets.hovered.weak_bg_fill);
                            }
                            super::paint_avatar(
                                ui.painter(),
                                rect.left_center() + egui::vec2(32.0, 0.0),
                                18.0,
                                &c.display_name,
                            );
                            let name = if c.favorite {
                                format!("{} ★", c.display_name)
                            } else {
                                c.display_name.clone()
                            };
                            ui.painter().text(
                                rect.left_center() + egui::vec2(60.0, -9.0),
                                egui::Align2::LEFT_CENTER,
                                name,
                                egui::FontId::proportional(14.0),
                                v.text_color(),
                            );
                            let sub = if c.email.is_empty() {
                                c.company.clone()
                            } else {
                                c.email.clone()
                            };
                            ui.painter().text(
                                rect.left_center() + egui::vec2(60.0, 10.0),
                                egui::Align2::LEFT_CENTER,
                                sub,
                                egui::FontId::proportional(12.0),
                                v.weak_text_color(),
                            );
                            if resp.clicked() {
                                self.people.selected = Some(c.id);
                            }
                            if resp.double_clicked() {
                                self.people.editor = Some(c.clone());
                            }
                        }
                        if list.is_empty() {
                            ui.add_space(30.0);
                            ui.vertical_centered(|ui| {
                                ui.label(RichText::new("Keine Kontakte gefunden.").weak())
                            });
                        }
                    });
            });

        egui::CentralPanel::default()
            .frame(super::pane_frame(ui).inner_margin(egui::Margin::same(28)))
            .show_inside(ui, |ui| {
                let Some(c) = self
                    .people
                    .selected
                    .and_then(|id| self.data.contacts.iter().find(|c| c.id == id))
                    .cloned()
                else {
                    ui.centered_and_justified(|ui| {
                        ui.label(
                            RichText::new("👤\n\nWählen Sie einen Kontakt aus.")
                                .size(16.0)
                                .weak(),
                        )
                    });
                    return;
                };
                ui.horizontal(|ui| {
                    super::avatar(ui, 80.0, &c.display_name);
                    ui.add_space(12.0);
                    ui.vertical(|ui| {
                        ui.add_space(10.0);
                        ui.label(RichText::new(&c.display_name).size(24.0).strong());
                        let role = [c.job_title.as_str(), c.company.as_str()]
                            .iter()
                            .filter(|s| !s.is_empty())
                            .cloned()
                            .collect::<Vec<_>>()
                            .join(" · ");
                        if !role.is_empty() {
                            ui.label(RichText::new(role).weak());
                        }
                        let acc = self
                            .data
                            .account(c.account_id)
                            .map(|a| a.email.clone())
                            .unwrap_or_default();
                        ui.label(
                            RichText::new(format!("Gespeichert in: {acc}"))
                                .small()
                                .weak(),
                        );
                    });
                });
                ui.add_space(16.0);
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(!c.email.is_empty(), egui::Button::new("✉  E-Mail senden"))
                        .clicked()
                    {
                        compose_to = Some(format!("{} <{}>", c.display_name, c.email));
                    }
                    if ui.button("✏  Bearbeiten").clicked() {
                        self.people.editor = Some(c.clone());
                    }
                    if ui
                        .button(if c.favorite {
                            "★  Aus Favoriten entfernen"
                        } else {
                            "☆  Zu Favoriten"
                        })
                        .clicked()
                    {
                        toggle_fav = Some(c.clone());
                    }
                    if ui
                        .button(RichText::new("🗑  Löschen").color(DANGER))
                        .clicked()
                    {
                        delete = Some(c.id);
                    }
                });
                ui.add_space(20.0);
                ui.label(RichText::new("Kontaktinformationen").strong());
                ui.add_space(6.0);
                egui::Grid::new("profile")
                    .num_columns(2)
                    .spacing([24.0, 10.0])
                    .show(ui, |ui| {
                        for (k, v) in [
                            ("E-Mail", &c.email),
                            ("Telefon", &c.phone),
                            ("Firma", &c.company),
                            ("Position", &c.job_title),
                            ("Adresse", &c.address),
                            ("Notizen", &c.notes),
                        ] {
                            if !v.is_empty() {
                                ui.label(RichText::new(k).weak());
                                ui.add(egui::Label::new(v.as_str()).selectable(true).wrap());
                                ui.end_row();
                            }
                        }
                    });
            });

        if let Some(to) = compose_to {
            self.module = super::Module::Mail;
            self.open_composer(ComposeKind::To(to));
        }
        if let Some(mut c) = toggle_fav {
            c.favorite = !c.favorite;
            self.spawn(None, move |st, _| async move {
                providers::save_contact(&st, c).await.map(|_| ())
            });
        }
        if let Some(id) = delete {
            self.people.selected = None;
            self.spawn(Some("Kontakt gelöscht"), move |st, _| async move {
                providers::delete_contact(&st, id).await
            });
        }
    }

    pub fn contact_editor_ui(&mut self, ctx: &egui::Context) {
        let Some(c) = &mut self.people.editor else {
            return;
        };
        let mut open = true;
        let mut save = false;
        let is_new = c.id == 0;
        let accounts = self.data.accounts.clone();
        egui::Window::new(if is_new {
            "Neuer Kontakt"
        } else {
            "Kontakt bearbeiten"
        })
        .open(&mut open)
        .collapsible(false)
        .default_width(480.0)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .show(ctx, |ui| {
            egui::Grid::new("contact_form")
                .num_columns(2)
                .spacing([12.0, 8.0])
                .show(ui, |ui| {
                    if is_new {
                        ui.label("Speichern in");
                        egui::ComboBox::from_id_salt("c_acc")
                            .selected_text(
                                accounts
                                    .iter()
                                    .find(|a| a.id == c.account_id)
                                    .map(|a| a.email.clone())
                                    .unwrap_or_default(),
                            )
                            .show_ui(ui, |ui| {
                                for a in accounts.iter().filter(|a| a.kind != AccountKind::Imap) {
                                    ui.selectable_value(&mut c.account_id, a.id, &a.email);
                                }
                            });
                        ui.end_row();
                    }
                    for (label, field) in [
                        ("Name", &mut c.display_name),
                        ("E-Mail", &mut c.email),
                        ("Telefon", &mut c.phone),
                        ("Firma", &mut c.company),
                        ("Position", &mut c.job_title),
                        ("Adresse", &mut c.address),
                    ] {
                        ui.label(label);
                        ui.add(egui::TextEdit::singleline(field).desired_width(320.0));
                        ui.end_row();
                    }
                    ui.label("Notizen");
                    ui.add(
                        egui::TextEdit::multiline(&mut c.notes)
                            .desired_rows(3)
                            .desired_width(320.0),
                    );
                    ui.end_row();
                    ui.label("");
                    ui.checkbox(&mut c.favorite, "Favorit");
                    ui.end_row();
                });
            ui.add_space(8.0);
            if super::primary_button(ui, "💾  Speichern").clicked() {
                save = true;
            }
        });
        if save {
            if c.display_name.trim().is_empty() {
                c.display_name = c.email.clone();
            }
            if c.display_name.trim().is_empty() {
                self.toast("Bitte einen Namen oder eine E-Mail-Adresse angeben.", true);
                return;
            }
            let c = self.people.editor.take().unwrap();
            self.spawn(Some("Kontakt gespeichert"), move |st, _| async move {
                providers::save_contact(&st, c).await.map(|_| ())
            });
        } else if !open {
            self.people.editor = None;
        }
    }
}
