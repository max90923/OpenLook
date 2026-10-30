//! Contact groups (distribution lists): shown in the People module, edited
//! in the "Contact Group" window and expanded in recipient fields.

use eframe::egui::{self, RichText, Sense};
use egui_phosphor::regular as ph;

use super::{
    chrome::{self, IC_BLUE, IC_PURPLE, IC_RED},
    compose::ComposeKind,
    App, Module, Win,
};
use crate::models::ContactGroup;

pub struct GroupEditor {
    /// Index in `Data::groups`; None for a new group.
    index: Option<usize>,
    g: ContactGroup,
    add: String,
    selected: Option<usize>,
}

/// Split "a <x>; b <y>" into single members, skipping duplicates.
fn add_members(g: &mut ContactGroup, field: &str) {
    for p in field
        .split([';', ','])
        .map(str::trim)
        .filter(|p| !p.is_empty())
    {
        if !g.members.iter().any(|m| m.eq_ignore_ascii_case(p)) {
            g.members.push(p.to_string());
        }
    }
}

/// ("Name", "address") of a member written as "Name <address>".
pub(super) fn split_member(m: &str) -> (&str, &str) {
    match m.rsplit_once('<') {
        Some((name, addr)) => (name.trim(), addr.trim_end_matches('>').trim()),
        None => (m.trim(), m.trim()),
    }
}

impl App {
    pub fn new_contact_group(&mut self) {
        self.open_window(Win::ContactGroup(GroupEditor {
            index: None,
            g: ContactGroup::default(),
            add: String::new(),
            selected: None,
        }));
    }

    pub(super) fn edit_contact_group(&mut self, index: usize) {
        if let Some(g) = self.data.groups.get(index).cloned() {
            self.open_window(Win::ContactGroup(GroupEditor {
                index: Some(index),
                g,
                add: String::new(),
                selected: None,
            }));
        }
    }

    pub(super) fn save_groups(&mut self, groups: Vec<ContactGroup>) {
        self.save_setting("contact_groups", serde_json::json!(groups));
        self.data.groups = groups;
    }

    pub(super) fn delete_contact_group(&mut self, index: usize) {
        let mut groups = self.data.groups.clone();
        if index < groups.len() {
            groups.remove(index);
            self.people.selected_group = None;
            self.save_groups(groups);
            self.toast("Contact group deleted.", false);
        }
    }

    pub(super) fn mail_to_group(&mut self, index: usize) {
        if let Some(g) = self.data.groups.get(index) {
            let to = g.members.join("; ");
            self.switch_module(Module::Mail);
            self.open_composer(ComposeKind::To(to));
        }
    }

    /// Group rows at the top of the contact list.
    pub(super) fn group_rows(&mut self, ui: &mut egui::Ui) {
        let t = self.t;
        let needle = self.search.trim().to_lowercase();
        let mut open = None;
        for (i, g) in self.data.groups.iter().enumerate() {
            if !needle.is_empty() && !g.name.to_lowercase().contains(&needle) {
                continue;
            }
            let w = ui.available_width();
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, 46.0), Sense::click());
            if self.people.selected_group == Some(i) {
                ui.painter().rect_filled(rect, 0, t.selected);
            } else if resp.hovered() {
                ui.painter().rect_filled(rect, 0, t.hover);
            }
            let c = rect.left_center() + egui::vec2(28.0, 0.0);
            ui.painter().circle_filled(c, 16.0, t.accent);
            ui.painter().text(
                c,
                egui::Align2::CENTER_CENTER,
                ph::USERS,
                egui::FontId::proportional(17.0),
                egui::Color32::WHITE,
            );
            ui.painter().text(
                rect.left_center() + egui::vec2(54.0, -8.0),
                egui::Align2::LEFT_CENTER,
                &g.name,
                egui::FontId::proportional(13.5),
                t.text,
            );
            ui.painter().text(
                rect.left_center() + egui::vec2(54.0, 9.0),
                egui::Align2::LEFT_CENTER,
                format!("Contact Group · {} Members", g.members.len()),
                egui::FontId::proportional(12.0),
                t.text_weak,
            );
            if resp.clicked() {
                self.people.selected_group = Some(i);
                self.people.clear_contact();
            }
            if resp.double_clicked() {
                open = Some(i);
            }
        }
        if let Some(i) = open {
            self.edit_contact_group(i);
        }
    }

    /// Right pane for the selected group.
    pub(super) fn group_details(&mut self, ui: &mut egui::Ui, index: usize) {
        let t = self.t;
        let Some(g) = self.data.groups.get(index).cloned() else {
            return;
        };
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(egui::vec2(84.0, 84.0), Sense::hover());
            ui.painter().circle_filled(r.center(), 42.0, t.accent);
            ui.painter().text(
                r.center(),
                egui::Align2::CENTER_CENTER,
                ph::USERS,
                egui::FontId::proportional(40.0),
                egui::Color32::WHITE,
            );
            ui.add_space(14.0);
            ui.vertical(|ui| {
                ui.add_space(8.0);
                ui.label(RichText::new(&g.name).size(26.0));
                ui.label(RichText::new("Contact Group").color(t.text_weak));
                ui.horizontal(|ui| {
                    if ui
                        .add(egui::Button::new((
                            RichText::new(ph::ENVELOPE_SIMPLE).color(IC_BLUE),
                            "Send Email",
                        )))
                        .clicked()
                    {
                        self.mail_to_group(index);
                    }
                    if ui
                        .add(egui::Button::new((
                            RichText::new(ph::PENCIL_SIMPLE).color(IC_BLUE),
                            "Edit",
                        )))
                        .clicked()
                    {
                        self.edit_contact_group(index);
                    }
                });
            });
        });
        ui.add_space(20.0);
        ui.label(
            RichText::new(format!("Members ({})", g.members.len()))
                .size(15.0)
                .color(t.accent),
        );
        ui.separator();
        for m in &g.members {
            let (name, addr) = split_member(m);
            ui.horizontal(|ui| {
                super::avatar(ui, 28.0, name);
                ui.add_space(6.0);
                ui.label(name);
                ui.label(RichText::new(addr).color(t.text_weak));
            });
        }
    }

    pub(super) fn group_window(
        &mut self,
        ctx: &egui::Context,
        vid: egui::ViewportId,
        ed: &mut GroupEditor,
    ) -> bool {
        let t = self.t;
        let contacts = self.data.contacts.clone();
        let title = format!(
            "{} - Contact Group",
            if ed.g.name.trim().is_empty() {
                "Untitled"
            } else {
                ed.g.name.trim()
            }
        );
        let (mut save, mut delete, mut mail) = (false, false, false);
        let closed = chrome::native_window(ctx, vid, &title, [720.0, 560.0], &t, |ui| {
            chrome::item_ribbon(ui, &t, "Contact Group", |ui| {
                chrome::group(ui, &t, "Actions", |ui| {
                    if chrome::large(ui, &t, ph::FLOPPY_DISK, IC_PURPLE, "Save &\nClose", true)
                        .clicked()
                    {
                        save = true;
                    }
                    if chrome::large(ui, &t, ph::X, IC_RED, "Delete\nGroup", ed.index.is_some())
                        .clicked()
                    {
                        delete = true;
                    }
                });
                chrome::group(ui, &t, "Members", |ui| {
                    if chrome::large(
                        ui,
                        &t,
                        ph::USER_MINUS,
                        IC_RED,
                        "Remove\nMember",
                        ed.selected.is_some(),
                    )
                    .clicked()
                    {
                        if let Some(i) = ed.selected.take() {
                            ed.g.members.remove(i);
                        }
                    }
                });
                chrome::group(ui, &t, "Communicate", |ui| {
                    if chrome::large(
                        ui,
                        &t,
                        ph::ENVELOPE_SIMPLE,
                        IC_BLUE,
                        "Email",
                        !ed.g.members.is_empty(),
                    )
                    .clicked()
                    {
                        mail = true;
                    }
                });
            });
            egui::Frame::new()
                .inner_margin(egui::Margin::symmetric(16, 12))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.add_sized([150.0, 24.0], egui::Label::new("Name:"));
                        ui.add(egui::TextEdit::singleline(&mut ed.g.name).desired_width(380.0));
                    });
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.add_sized([150.0, 24.0], egui::Label::new("Add Members:"));
                        super::compose::address_field(
                            ui,
                            "group_add",
                            &mut ed.add,
                            &contacts,
                            &[],
                            380.0,
                        );
                        if ui.button("Add").clicked() && !ed.add.trim().is_empty() {
                            add_members(&mut ed.g, &ed.add);
                            ed.add.clear();
                        }
                    });
                    ui.add_space(10.0);
                    let w = ui.available_width();
                    let header = |ui: &mut egui::Ui, a: &str, b: &str| {
                        let (r, _) = ui.allocate_exact_size(egui::vec2(w, 24.0), Sense::hover());
                        ui.painter().rect_filled(r, 0, t.pane);
                        for (x, s) in [(8.0, a), (w * 0.45, b)] {
                            ui.painter().text(
                                r.left_center() + egui::vec2(x, 0.0),
                                egui::Align2::LEFT_CENTER,
                                s,
                                egui::FontId::proportional(12.5),
                                t.text_weak,
                            );
                        }
                    };
                    header(ui, "Name", "Email");
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.spacing_mut().item_spacing.y = 0.0;
                            for (i, m) in ed.g.members.iter().enumerate() {
                                let (name, addr) = split_member(m);
                                let (r, resp) =
                                    ui.allocate_exact_size(egui::vec2(w, 26.0), Sense::click());
                                if ed.selected == Some(i) {
                                    ui.painter().rect_filled(r, 0, t.selected);
                                } else if resp.hovered() {
                                    ui.painter().rect_filled(r, 0, t.hover);
                                }
                                for (x, s) in [(8.0, name), (w * 0.45, addr)] {
                                    ui.painter().text(
                                        r.left_center() + egui::vec2(x, 0.0),
                                        egui::Align2::LEFT_CENTER,
                                        s,
                                        egui::FontId::proportional(13.0),
                                        t.text,
                                    );
                                }
                                if resp.clicked() {
                                    ed.selected = Some(i);
                                }
                            }
                            if ed.g.members.is_empty() {
                                ui.add_space(12.0);
                                ui.label(
                                    RichText::new("This group has no members yet.")
                                        .color(t.text_weak),
                                );
                            }
                        });
                });
        });
        if mail {
            self.open_composer(ComposeKind::To(ed.g.members.join("; ")));
        }
        if save {
            if !ed.add.trim().is_empty() {
                add_members(&mut ed.g, &ed.add);
            }
            ed.g.name = ed.g.name.trim().to_string();
            if ed.g.name.is_empty() {
                self.toast("Please enter a name for the contact group.", true);
                return false;
            }
            let mut groups = self.data.groups.clone();
            match ed.index.filter(|&i| i < groups.len()) {
                Some(i) => groups[i] = ed.g.clone(),
                None => groups.push(ed.g.clone()),
            }
            groups.sort_by_key(|g| g.name.to_lowercase());
            self.save_groups(groups);
            self.toast("Contact group saved.", false);
            return true;
        }
        if delete {
            if let Some(i) = ed.index {
                self.delete_contact_group(i);
            }
            return true;
        }
        closed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adds_members_once() {
        let mut g = ContactGroup::default();
        add_members(&mut g, "Anna <anna@x.de>; bob@x.de, ");
        add_members(&mut g, "anna <ANNA@x.de>; Bob@x.de");
        assert_eq!(g.members, vec!["Anna <anna@x.de>", "bob@x.de"]);
        assert_eq!(split_member(&g.members[0]), ("Anna", "anna@x.de"));
        assert_eq!(split_member("bob@x.de"), ("bob@x.de", "bob@x.de"));
    }
}
