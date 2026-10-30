//! "Rules and Alerts" window.

use eframe::egui::{self, RichText, Sense};
use egui_phosphor::regular as ph;

use super::{
    chrome::{self, IC_RED},
    App, Win, CATEGORIES,
};
use crate::{
    db,
    rules::{self, Action, Field, Rule},
};

pub struct RulesWin {
    rules: Vec<Rule>,
    sel: usize,
}

impl App {
    /// Open the rules window; with a message, a new rule for its sender is prefilled.
    pub fn open_rules(&mut self, from_message: Option<i64>) {
        let mut list = rules::load(&self.st.db());
        if let Some(m) =
            from_message.and_then(|id| db::get_message(&self.st.db(), id).ok().flatten())
        {
            let who = if m.summary.from_name.is_empty() {
                m.summary.from_addr.clone()
            } else {
                m.summary.from_name.clone()
            };
            list.push(Rule {
                name: format!("From {who}"),
                enabled: true,
                account_id: Some(m.summary.account_id),
                field: Field::From,
                contains: m.summary.from_addr.clone(),
                action: Action::Move,
                ..Default::default()
            });
        }
        let sel = list.len().saturating_sub(1);
        self.open_window(Win::Rules(RulesWin { rules: list, sel }));
    }

    pub(super) fn rules_window(
        &mut self,
        ctx: &egui::Context,
        vid: egui::ViewportId,
        w: &mut RulesWin,
    ) -> bool {
        let t = self.t;
        let accounts = self.data.accounts.clone();
        let folders = self.data.folders.clone();
        let mut ok = false;
        let mut apply_now = false;
        let mut cancel = false;
        let closed =
            chrome::native_window(ctx, vid, "Rules and Alerts", [860.0, 560.0], &t, |ui| {
                egui::TopBottomPanel::bottom("rules_buttons")
                    .frame(egui::Frame::new().inner_margin(egui::Margin::same(12)))
                    .show_inside(ui, |ui| {
                        ui.horizontal(|ui| {
                            if ui
                                .button(format!("{}  Run Rules Now", ph::PLAY))
                                .on_hover_text(
                                    "Saves the rules and runs them on all messages in the Inbox",
                                )
                                .clicked()
                            {
                                apply_now = true;
                            }
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui
                                        .add_sized([90.0, 26.0], egui::Button::new("Cancel"))
                                        .clicked()
                                    {
                                        cancel = true;
                                    }
                                    if ui
                                        .add_sized([90.0, 26.0], egui::Button::new("OK"))
                                        .clicked()
                                    {
                                        ok = true;
                                    }
                                },
                            );
                        });
                    });
                egui::CentralPanel::default()
                    .frame(egui::Frame::new().inner_margin(egui::Margin::same(12)))
                    .show_inside(ui, |ui| {
                        ui.horizontal(|ui| {
                            if ui.button(format!("{}  New Rule…", ph::PLUS)).clicked() {
                                w.rules.push(Rule {
                                    name: format!("Rule {}", w.rules.len() + 1),
                                    enabled: true,
                                    ..Default::default()
                                });
                                w.sel = w.rules.len() - 1;
                            }
                            if ui
                                .add_enabled(
                                    !w.rules.is_empty(),
                                    egui::Button::new((
                                        RichText::new(ph::X).color(IC_RED),
                                        "Delete",
                                    )),
                                )
                                .clicked()
                            {
                                w.rules.remove(w.sel);
                                w.sel = w.sel.saturating_sub(1);
                            }
                            if ui
                                .add_enabled(w.sel > 0, egui::Button::new(ph::ARROW_UP))
                                .on_hover_text("Move Up")
                                .clicked()
                            {
                                w.rules.swap(w.sel, w.sel - 1);
                                w.sel -= 1;
                            }
                            if ui
                                .add_enabled(
                                    w.sel + 1 < w.rules.len(),
                                    egui::Button::new(ph::ARROW_DOWN),
                                )
                                .on_hover_text("Move Down")
                                .clicked()
                            {
                                w.rules.swap(w.sel, w.sel + 1);
                                w.sel += 1;
                            }
                        });
                        ui.add_space(6.0);
                        // Rule list.
                        let (head, _) = ui.allocate_exact_size(
                            egui::vec2(ui.available_width(), 22.0),
                            Sense::hover(),
                        );
                        ui.painter().rect_filled(head, 0, t.ribbon);
                        ui.painter().text(
                            head.left_center() + egui::vec2(8.0, 0.0),
                            egui::Align2::LEFT_CENTER,
                            "Rule (applied in the order shown)",
                            egui::FontId::proportional(12.5),
                            t.text,
                        );
                        egui::ScrollArea::vertical()
                            .max_height(170.0)
                            .auto_shrink([false, true])
                            .show(ui, |ui| {
                                for (i, r) in w.rules.iter_mut().enumerate() {
                                    ui.horizontal(|ui| {
                                        ui.checkbox(&mut r.enabled, "");
                                        if ui.selectable_label(w.sel == i, &r.name).clicked() {
                                            w.sel = i;
                                        }
                                    });
                                }
                                if w.rules.is_empty() {
                                    ui.label(
                                        RichText::new("There are no rules.").color(t.text_weak),
                                    );
                                }
                            });
                        ui.separator();
                        // Description / editor of the selected rule.
                        let Some(r) = w.rules.get_mut(w.sel) else {
                            return;
                        };
                        ui.label(RichText::new("Rule description").strong());
                        egui::Grid::new("rule_edit")
                            .num_columns(2)
                            .spacing([10.0, 8.0])
                            .show(ui, |ui| {
                                ui.label("Name");
                                ui.add(
                                    egui::TextEdit::singleline(&mut r.name).desired_width(360.0),
                                );
                                ui.end_row();
                                ui.label("Apply this rule after the message arrives in");
                                egui::ComboBox::from_id_salt("rule_acc")
                                    .width(360.0)
                                    .selected_text(
                                        r.account_id
                                            .and_then(|a| accounts.iter().find(|x| x.id == a))
                                            .map(|a| a.email.clone())
                                            .unwrap_or_else(|| "all accounts".into()),
                                    )
                                    .show_ui(ui, |ui| {
                                        ui.selectable_value(
                                            &mut r.account_id,
                                            None,
                                            "all accounts",
                                        );
                                        for a in &accounts {
                                            ui.selectable_value(
                                                &mut r.account_id,
                                                Some(a.id),
                                                &a.email,
                                            );
                                        }
                                    });
                                ui.end_row();
                                ui.label("Condition");
                                ui.horizontal(|ui| {
                                    egui::ComboBox::from_id_salt("rule_field")
                                        .selected_text(r.field.label())
                                        .show_ui(ui, |ui| {
                                            for f in [
                                                Field::From,
                                                Field::Subject,
                                                Field::To,
                                                Field::Body,
                                            ] {
                                                ui.selectable_value(&mut r.field, f, f.label());
                                            }
                                        });
                                    ui.add(
                                        egui::TextEdit::singleline(&mut r.contains)
                                            .hint_text("Text")
                                            .desired_width(200.0),
                                    );
                                });
                                ui.end_row();
                                ui.label("Action");
                                ui.horizontal(|ui| {
                                    egui::ComboBox::from_id_salt("rule_action")
                                        .selected_text(r.action.label())
                                        .show_ui(ui, |ui| {
                                            for a in [
                                                Action::Move,
                                                Action::MarkRead,
                                                Action::Flag,
                                                Action::Category,
                                                Action::Delete,
                                            ] {
                                                ui.selectable_value(&mut r.action, a, a.label());
                                            }
                                        });
                                    match r.action {
                                        Action::Move => {
                                            let candidates: Vec<_> = folders
                                                .iter()
                                                .filter(|f| {
                                                    r.account_id.is_none_or(|a| a == f.account_id)
                                                        && f.role != "inbox"
                                                })
                                                .collect();
                                            egui::ComboBox::from_id_salt("rule_folder")
                                                .width(200.0)
                                                .selected_text(
                                                    r.folder_id
                                                        .and_then(|id| {
                                                            folders.iter().find(|f| f.id == id)
                                                        })
                                                        .map(|f| f.name.clone())
                                                        .unwrap_or_else(|| "Choose folder".into()),
                                                )
                                                .show_ui(ui, |ui| {
                                                    for f in candidates {
                                                        let acc = accounts
                                                            .iter()
                                                            .find(|a| a.id == f.account_id)
                                                            .map(|a| a.email.as_str())
                                                            .unwrap_or_default();
                                                        ui.selectable_value(
                                                            &mut r.folder_id,
                                                            Some(f.id),
                                                            format!("{} ({acc})", f.name),
                                                        );
                                                    }
                                                });
                                        }
                                        Action::Category => {
                                            egui::ComboBox::from_id_salt("rule_cat")
                                                .selected_text(if r.category.is_empty() {
                                                    "Choose category"
                                                } else {
                                                    r.category.as_str()
                                                })
                                                .show_ui(ui, |ui| {
                                                    for (c, _) in CATEGORIES {
                                                        ui.selectable_value(
                                                            &mut r.category,
                                                            c.to_string(),
                                                            c,
                                                        );
                                                    }
                                                });
                                        }
                                        _ => {}
                                    }
                                });
                                ui.end_row();
                            });
                        let incomplete = r.contains.trim().is_empty()
                            || (r.action == Action::Move && r.folder_id.is_none());
                        if incomplete {
                            ui.label(
                                RichText::new(format!(
                                    "{}  The rule is incomplete and will not be applied.",
                                    ph::WARNING
                                ))
                                .color(t.danger),
                            );
                        }
                        ui.label(
                            RichText::new(
                                "Rules run on new messages in the Inbox after every Send/Receive.",
                            )
                            .size(12.0)
                            .color(t.text_weak),
                        );
                    });
            });
        if ok || apply_now {
            let res = rules::save(&self.st.db(), &w.rules);
            if let Err(e) = res {
                self.toast(format!("{e:#}"), true);
                return false;
            }
            if apply_now {
                let accounts: Vec<i64> = self.data.accounts.iter().map(|a| a.id).collect();
                self.spawn(Some("Rules have been run."), move |st, _| async move {
                    for a in accounts {
                        rules::apply(&st, a, false).await?;
                    }
                    Ok(())
                });
                return false;
            }
            self.toast("Rules saved.", false);
            return true;
        }
        cancel || closed
    }
}
