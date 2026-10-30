//! People module: contact list with card, business-card and list views,
//! and the contact window ("Contact").

use eframe::egui::{self, RichText, Sense};
use egui_phosphor::regular as ph;

use super::{
    chrome::{self, IC_BLUE, IC_GREY, IC_PURPLE, IC_RED},
    compose::ComposeKind,
    App, Module, Win,
};
use crate::{models::*, providers};

#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub enum PeopleMode {
    #[default]
    People,
    Cards,
    List,
}

#[derive(Default)]
pub struct PeopleView {
    selected: Option<i64>,
    /// Index of the selected contact group (instead of a contact).
    pub(super) selected_group: Option<usize>,
    mode: PeopleMode,
    favorites_only: bool,
}

impl PeopleView {
    pub(super) fn clear_contact(&mut self) {
        self.selected = None;
    }
}

pub struct ContactEditor {
    c: Contact,
}

impl App {
    pub fn new_contact(&mut self) {
        let c = Contact {
            account_id: self.data.default_pim_account(),
            ..Default::default()
        };
        self.open_window(Win::Contact(ContactEditor { c }));
    }

    fn filtered_contacts(&self) -> Vec<Contact> {
        let needle = self.search.trim().to_lowercase();
        self.data
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
            .collect()
    }

    pub fn people_ribbon(&mut self, ui: &mut egui::Ui) {
        if self.ribbon_tab != 0 {
            self.common_ribbon(ui);
            return;
        }
        let t = self.t;
        let sel = self
            .people
            .selected
            .and_then(|id| self.data.contacts.iter().find(|c| c.id == id))
            .cloned();
        chrome::group(ui, &t, "New", |ui| {
            if chrome::large(ui, &t, ph::USER_PLUS, IC_BLUE, "New\nContact", true).clicked() {
                self.new_contact();
            }
            if chrome::large(ui, &t, ph::USERS_FOUR, IC_BLUE, "New Contact\nGroup", true).clicked()
            {
                self.new_contact_group();
            }
        });
        let group = self.people.selected_group;
        chrome::group(ui, &t, "Delete", |ui| {
            if chrome::large(
                ui,
                &t,
                ph::X,
                IC_RED,
                "Delete",
                sel.is_some() || group.is_some(),
            )
            .clicked()
            {
                if let Some(i) = group {
                    self.delete_contact_group(i);
                    return;
                }
                let id = sel.as_ref().unwrap().id;
                self.people.selected = None;
                self.spawn(Some("Contact deleted."), move |st, _| async move {
                    providers::delete_contact(&st, id).await
                });
            }
        });
        chrome::group(ui, &t, "Communicate", |ui| {
            let has_mail = sel.as_ref().is_some_and(|c| !c.email.is_empty());
            if chrome::large(
                ui,
                &t,
                ph::ENVELOPE_SIMPLE,
                IC_BLUE,
                "Email",
                has_mail || group.is_some(),
            )
            .clicked()
            {
                if let Some(i) = group {
                    self.mail_to_group(i);
                    return;
                }
                let c = sel.as_ref().unwrap();
                self.switch_module(Module::Mail);
                self.open_composer(ComposeKind::To(format!("{} <{}>", c.display_name, c.email)));
            }
            if chrome::large(ui, &t, ph::CALENDAR_PLUS, IC_BLUE, "Meeting", has_mail).clicked()
            {
                let c = sel.as_ref().unwrap().clone();
                self.new_event_now();
                self.set_last_event_attendee(&c.email);
            }
        });
        chrome::group(ui, &t, "Current View", |ui| {
            for (m, icon, label) in [
                (PeopleMode::People, ph::USER_CIRCLE, "People"),
                (PeopleMode::Cards, ph::IDENTIFICATION_CARD, "Business Card"),
                (PeopleMode::List, ph::LIST, "List"),
            ] {
                let r = chrome::large(
                    ui,
                    &t,
                    icon,
                    if self.people.mode == m {
                        IC_BLUE
                    } else {
                        IC_GREY
                    },
                    label,
                    true,
                );
                if self.people.mode == m {
                    ui.painter().rect_stroke(
                        r.rect,
                        0,
                        egui::Stroke::new(1.0_f32, t.button_down),
                        egui::StrokeKind::Inside,
                    );
                }
                if r.clicked() {
                    self.people.mode = m;
                }
            }
        });
        chrome::group(ui, &t, "Tags", |ui| {
            if chrome::large(
                ui,
                &t,
                ph::STAR,
                IC_PURPLE,
                if sel.as_ref().is_some_and(|c| c.favorite) {
                    "Remove from\nFavorites"
                } else {
                    "Add to\nFavorites"
                },
                sel.is_some(),
            )
            .clicked()
            {
                let mut c = sel.clone().unwrap();
                c.favorite = !c.favorite;
                self.spawn(None, move |st, _| async move {
                    providers::save_contact(&st, c).await.map(|_| ())
                });
            }
        });
    }

    fn set_last_event_attendee(&mut self, email: &str) {
        if let Some(super::WinEntry {
            win: Win::Event(e), ..
        }) = self.windows.last_mut()
        {
            e.set_attendee(email);
        }
    }

    pub fn people_nav(&mut self, ui: &mut egui::Ui) {
        let t = self.t;
        ui.label(RichText::new(format!("{}  My Contacts", ph::CARET_DOWN)).strong());
        for (fav, label) in [(false, "Contacts"), (true, "Favorites")] {
            let active = self.people.favorites_only == fav;
            let w = ui.available_width();
            let (r, resp) = ui.allocate_exact_size(egui::vec2(w, 24.0), Sense::click());
            if active {
                ui.painter().rect_filled(r, 0, t.selected);
            } else if resp.hovered() {
                ui.painter().rect_filled(r, 0, t.hover);
            }
            ui.painter().text(
                r.left_center() + egui::vec2(26.0, 0.0),
                egui::Align2::LEFT_CENTER,
                label,
                egui::FontId::proportional(13.0),
                t.text,
            );
            if resp.clicked() {
                self.people.favorites_only = fav;
            }
        }
    }

    pub fn people_ui(&mut self, ui: &mut egui::Ui) {
        let t = self.t;
        let list = self.filtered_contacts();
        egui::Frame::new()
            .inner_margin(egui::Margin::symmetric(8, 8))
            .show(ui, |ui| {
                ui.allocate_ui(egui::vec2(ui.available_width().min(420.0), 26.0), |ui| {
                    super::search_box(ui, &t, &mut self.search, "Search Contacts (Ctrl+E)");
                });
            });
        match self.people.mode {
            PeopleMode::People => self.people_split(ui, &list),
            PeopleMode::Cards => self.people_cards(ui, &list),
            PeopleMode::List => self.people_table(ui, &list),
        }
    }

    fn people_split(&mut self, ui: &mut egui::Ui, list: &[Contact]) {
        let t = self.t;
        egui::SidePanel::left("people_list")
            .resizable(true)
            .default_width(320.0)
            .frame(
                egui::Frame::new()
                    .fill(t.pane)
                    .stroke(egui::Stroke::new(1.0_f32, t.border)),
            )
            .show_inside(ui, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("people_scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        if !self.people.favorites_only {
                            self.group_rows(ui);
                        }
                        let mut letter = None;
                        for c in list {
                            let l = c
                                .display_name
                                .chars()
                                .next()
                                .map(|c| c.to_uppercase().to_string())
                                .unwrap_or_default();
                            if letter.as_ref() != Some(&l) {
                                ui.add_space(4.0);
                                ui.horizontal(|ui| {
                                    ui.add_space(12.0);
                                    ui.label(RichText::new(&l).size(15.0).color(t.accent));
                                });
                                letter = Some(l);
                            }
                            let w = ui.available_width();
                            let (rect, resp) =
                                ui.allocate_exact_size(egui::vec2(w, 46.0), Sense::click());
                            if self.people.selected == Some(c.id) {
                                ui.painter().rect_filled(rect, 0, t.selected);
                            } else if resp.hovered() {
                                ui.painter().rect_filled(rect, 0, t.hover);
                            }
                            super::paint_avatar(
                                ui.painter(),
                                rect.left_center() + egui::vec2(28.0, 0.0),
                                16.0,
                                &c.display_name,
                            );
                            let name = if c.favorite {
                                format!("{}  ★", c.display_name)
                            } else {
                                c.display_name.clone()
                            };
                            ui.painter().text(
                                rect.left_center() + egui::vec2(54.0, -8.0),
                                egui::Align2::LEFT_CENTER,
                                name,
                                egui::FontId::proportional(13.5),
                                t.text,
                            );
                            let sub = if c.company.is_empty() {
                                c.email.clone()
                            } else {
                                c.company.clone()
                            };
                            ui.painter().text(
                                rect.left_center() + egui::vec2(54.0, 9.0),
                                egui::Align2::LEFT_CENTER,
                                sub,
                                egui::FontId::proportional(12.0),
                                t.text_weak,
                            );
                            if resp.clicked() {
                                self.people.selected = Some(c.id);
                                self.people.selected_group = None;
                            }
                            if resp.double_clicked() {
                                self.open_window(Win::Contact(ContactEditor { c: c.clone() }));
                            }
                        }
                        if list.is_empty() {
                            ui.add_space(24.0);
                            ui.vertical_centered(|ui| {
                                ui.label(
                                    RichText::new("We didn't find any contacts.")
                                        .color(t.text_weak),
                                )
                            });
                        }
                    });
            });
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(t.pane)
                    .inner_margin(egui::Margin::same(24)),
            )
            .show_inside(ui, |ui| {
                if let Some(i) = self.people.selected_group {
                    self.group_details(ui, i);
                    return;
                }
                let Some(c) = self
                    .people
                    .selected
                    .and_then(|id| self.data.contacts.iter().find(|c| c.id == id))
                    .cloned()
                else {
                    ui.centered_and_justified(|ui| {
                        ui.label(RichText::new("Select a contact.").color(t.text_weak))
                    });
                    return;
                };
                ui.horizontal(|ui| {
                    super::avatar(ui, 84.0, &c.display_name);
                    ui.add_space(14.0);
                    ui.vertical(|ui| {
                        ui.add_space(8.0);
                        ui.label(RichText::new(&c.display_name).size(26.0));
                        let role = [c.job_title.as_str(), c.company.as_str()]
                            .iter()
                            .filter(|s| !s.is_empty())
                            .cloned()
                            .collect::<Vec<_>>()
                            .join(", ");
                        if !role.is_empty() {
                            ui.label(RichText::new(role).color(t.text_weak));
                        }
                        ui.horizontal(|ui| {
                            if !c.email.is_empty()
                                && ui
                                    .add(egui::Button::new((
                                        RichText::new(ph::ENVELOPE_SIMPLE).color(IC_BLUE),
                                        "Send Email",
                                    )))
                                    .clicked()
                            {
                                self.switch_module(Module::Mail);
                                self.open_composer(ComposeKind::To(format!(
                                    "{} <{}>",
                                    c.display_name, c.email
                                )));
                            }
                            if ui
                                .add(egui::Button::new((
                                    RichText::new(ph::PENCIL_SIMPLE).color(IC_BLUE),
                                    "Edit",
                                )))
                                .clicked()
                            {
                                self.open_window(Win::Contact(ContactEditor { c: c.clone() }));
                            }
                        });
                    });
                });
                ui.add_space(20.0);
                ui.label(
                    RichText::new("Contact Information")
                        .size(15.0)
                        .color(t.accent),
                );
                ui.separator();
                egui::Grid::new("profile")
                    .num_columns(2)
                    .spacing([28.0, 10.0])
                    .show(ui, |ui| {
                        for (k, v) in [
                            ("Email", &c.email),
                            ("Mobile", &c.phone),
                            ("Company", &c.company),
                            ("Job title", &c.job_title),
                            ("Business Address", &c.address),
                            ("Notes", &c.notes),
                        ] {
                            if !v.is_empty() {
                                ui.label(RichText::new(k).color(t.text_weak));
                                ui.add(egui::Label::new(v.as_str()).selectable(true).wrap());
                                ui.end_row();
                            }
                        }
                    });
                let acc = self
                    .data
                    .account(c.account_id)
                    .map(|a| a.email.clone())
                    .unwrap_or_default();
                ui.add_space(14.0);
                ui.label(
                    RichText::new(format!("Saved in: Contacts – {acc}"))
                        .size(12.0)
                        .color(t.text_weak),
                );
            });
    }

    /// Outlook "Business Card" view.
    fn people_cards(&mut self, ui: &mut egui::Ui, list: &[Contact]) {
        let t = self.t;
        egui::ScrollArea::vertical()
            .id_salt("cards")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(12.0, 12.0);
                    ui.add_space(8.0);
                    for c in list {
                        let (r, resp) =
                            ui.allocate_exact_size(egui::vec2(300.0, 150.0), Sense::click());
                        let sel = self.people.selected == Some(c.id);
                        let p = ui.painter();
                        p.rect(
                            r,
                            0,
                            t.pane,
                            egui::Stroke::new(
                                if sel { 2.0_f32 } else { 1.0 },
                                if sel { t.accent } else { t.border },
                            ),
                            egui::StrokeKind::Inside,
                        );
                        p.rect_filled(
                            egui::Rect::from_min_size(r.min, egui::vec2(r.width(), 26.0)),
                            0,
                            t.accent.gamma_multiply(0.15),
                        );
                        p.text(
                            r.min + egui::vec2(10.0, 13.0),
                            egui::Align2::LEFT_CENTER,
                            &c.display_name,
                            egui::FontId::proportional(14.0),
                            t.text,
                        );
                        super::paint_avatar(
                            p,
                            r.min + egui::vec2(40.0, 76.0),
                            28.0,
                            &c.display_name,
                        );
                        let mut y = r.top() + 36.0;
                        for line in [&c.company, &c.job_title, &c.phone, &c.email, &c.address] {
                            if !line.is_empty() {
                                let g = super::one_line(
                                    p,
                                    line,
                                    egui::FontId::proportional(12.0),
                                    t.text,
                                    r.width() - 90.0,
                                );
                                p.galley(egui::pos2(r.left() + 80.0, y), g, t.text);
                                y += 18.0;
                            }
                        }
                        if resp.clicked() {
                            self.people.selected = Some(c.id);
                        }
                        if resp.double_clicked() {
                            self.open_window(Win::Contact(ContactEditor { c: c.clone() }));
                        }
                    }
                });
            });
    }

    /// Outlook "List" view.
    fn people_table(&mut self, ui: &mut egui::Ui, list: &[Contact]) {
        let t = self.t;
        let cols = [
            ("Full Name", 0.24),
            ("Company", 0.2),
            ("Email", 0.26),
            ("Mobile", 0.15),
            ("Job title", 0.15),
        ];
        let w = ui.available_width() - 16.0;
        let (head, _) = ui.allocate_exact_size(egui::vec2(w + 16.0, 24.0), Sense::hover());
        ui.painter().rect_filled(head, 0, t.ribbon);
        let mut x = head.left() + 8.0;
        for (name, f) in cols {
            ui.painter().text(
                egui::pos2(x + 6.0, head.center().y),
                egui::Align2::LEFT_CENTER,
                name,
                egui::FontId::proportional(12.5),
                t.text,
            );
            x += w * f;
            ui.painter()
                .vline(x, head.y_range(), egui::Stroke::new(1.0_f32, t.border));
        }
        egui::ScrollArea::vertical()
            .id_salt("ptable")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for c in list {
                    let (r, resp) =
                        ui.allocate_exact_size(egui::vec2(w + 16.0, 24.0), Sense::click());
                    if self.people.selected == Some(c.id) {
                        ui.painter().rect_filled(r, 0, t.selected);
                    } else if resp.hovered() {
                        ui.painter().rect_filled(r, 0, t.hover);
                    }
                    ui.painter().hline(
                        r.x_range(),
                        r.bottom() - 0.5,
                        egui::Stroke::new(1.0_f32, t.border.gamma_multiply(0.6)),
                    );
                    let mut x = r.left() + 8.0;
                    for (i, (_, f)) in cols.iter().enumerate() {
                        let v = [
                            &c.display_name,
                            &c.company,
                            &c.email,
                            &c.phone,
                            &c.job_title,
                        ][i];
                        let g = super::one_line(
                            ui.painter(),
                            v,
                            egui::FontId::proportional(12.5),
                            t.text,
                            w * f - 12.0,
                        );
                        ui.painter().galley(
                            egui::pos2(x + 6.0, r.center().y - g.size().y / 2.0),
                            g,
                            t.text,
                        );
                        x += w * f;
                    }
                    if resp.clicked() {
                        self.people.selected = Some(c.id);
                    }
                    if resp.double_clicked() {
                        self.open_window(Win::Contact(ContactEditor { c: c.clone() }));
                    }
                }
            });
    }

    // ------------------------------------------------------------ contact window

    pub(super) fn contact_window(
        &mut self,
        ctx: &egui::Context,
        vid: egui::ViewportId,
        ed: &mut ContactEditor,
    ) -> bool {
        let t = self.t;
        let c = &mut ed.c;
        let title = format!(
            "{} - Contact",
            if c.display_name.trim().is_empty() {
                "Untitled"
            } else {
                c.display_name.trim()
            }
        );
        let accounts = self.data.accounts.clone();
        let is_new = c.id == 0;
        let mut save = false;
        let mut delete = false;
        let mut mail = false;
        let closed = chrome::native_window(ctx, vid, &title, [840.0, 600.0], &t, |ui| {
            chrome::item_ribbon(ui, &t, "Contact", |ui| {
                chrome::group(ui, &t, "Actions", |ui| {
                    if chrome::large(
                        ui,
                        &t,
                        ph::FLOPPY_DISK,
                        IC_PURPLE,
                        "Save &\nClose",
                        true,
                    )
                    .clicked()
                    {
                        save = true;
                    }
                    if chrome::large(ui, &t, ph::X, IC_RED, "Delete", !is_new).clicked() {
                        delete = true;
                    }
                });
                chrome::group(ui, &t, "Communicate", |ui| {
                    if chrome::large(
                        ui,
                        &t,
                        ph::ENVELOPE_SIMPLE,
                        IC_BLUE,
                        "Email",
                        !c.email.is_empty(),
                    )
                    .clicked()
                    {
                        mail = true;
                    }
                });
                chrome::group(ui, &t, "Tags", |ui| {
                    if chrome::large(
                        ui,
                        &t,
                        ph::STAR,
                        IC_PURPLE,
                        if c.favorite {
                            "Favorite  \u{E182}"
                        } else {
                            "Favorite"
                        },
                        true,
                    )
                    .clicked()
                    {
                        c.favorite = !c.favorite;
                    }
                });
            });
            egui::Frame::new()
                .inner_margin(egui::Margin::symmetric(16, 12))
                .show(ui, |ui| {
                    ui.horizontal_top(|ui| {
                        ui.vertical(|ui| {
                            ui.set_width(460.0);
                            egui::Grid::new("contact_form")
                                .num_columns(2)
                                .spacing([10.0, 7.0])
                                .show(ui, |ui| {
                                    for (label, field) in [
                                        ("Full Name…", &mut c.display_name),
                                        ("Company", &mut c.company),
                                        ("Job title", &mut c.job_title),
                                        ("E-mail…", &mut c.email),
                                        ("Mobile…", &mut c.phone),
                                        ("Business…", &mut c.address),
                                    ] {
                                        ui.label(label);
                                        ui.add(
                                            egui::TextEdit::singleline(field).desired_width(310.0),
                                        );
                                        ui.end_row();
                                    }
                                    if is_new {
                                        ui.label("Save in");
                                        egui::ComboBox::from_id_salt("c_acc")
                                            .width(310.0)
                                            .selected_text(
                                                accounts
                                                    .iter()
                                                    .find(|a| a.id == c.account_id)
                                                    .map(|a| a.email.clone())
                                                    .unwrap_or_default(),
                                            )
                                            .show_ui(ui, |ui| {
                                                for a in &accounts {
                                                    ui.selectable_value(
                                                        &mut c.account_id,
                                                        a.id,
                                                        &a.email,
                                                    );
                                                }
                                            });
                                        ui.end_row();
                                    }
                                });
                        });
                        // Business card preview, like Outlook's contact form.
                        let (r, _) =
                            ui.allocate_exact_size(egui::vec2(280.0, 160.0), Sense::hover());
                        let p = ui.painter();
                        p.rect(
                            r,
                            0,
                            t.pane,
                            egui::Stroke::new(1.0_f32, t.border),
                            egui::StrokeKind::Inside,
                        );
                        super::paint_avatar(
                            p,
                            r.min + egui::vec2(46.0, 56.0),
                            32.0,
                            &c.display_name,
                        );
                        let mut y = r.top() + 14.0;
                        for (i, line) in [
                            &c.display_name,
                            &c.company,
                            &c.job_title,
                            &c.phone,
                            &c.email,
                        ]
                        .into_iter()
                        .enumerate()
                        {
                            if !line.is_empty() {
                                let size = if i == 0 { 14.0 } else { 12.0 };
                                let g = super::one_line(
                                    p,
                                    line,
                                    egui::FontId::proportional(size),
                                    t.text,
                                    r.width() - 100.0,
                                );
                                p.galley(egui::pos2(r.left() + 92.0, y), g, t.text);
                                y += size + 6.0;
                            }
                        }
                    });
                    ui.add_space(8.0);
                    ui.label(RichText::new("Notes").color(t.text_weak));
                    let (w, h) = (ui.available_width(), ui.available_height().max(80.0));
                    ui.add_sized([w, h - 4.0], egui::TextEdit::multiline(&mut c.notes));
                });
        });
        if mail {
            let to = format!("{} <{}>", c.display_name, c.email);
            self.open_composer(ComposeKind::To(to));
        }
        if save {
            if c.display_name.trim().is_empty() {
                c.display_name = c.email.clone();
            }
            if c.display_name.trim().is_empty() {
                self.toast("Please enter a name or an email address.", true);
                return false;
            }
            let c = c.clone();
            self.spawn(Some("Contact saved."), move |st, _| async move {
                providers::save_contact(&st, c).await.map(|_| ())
            });
            return true;
        }
        if delete {
            let id = c.id;
            self.spawn(Some("Contact deleted."), move |st, _| async move {
                providers::delete_contact(&st, id).await
            });
            return true;
        }
        closed
    }
}
