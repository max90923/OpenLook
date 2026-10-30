//! Quick Steps: several actions on the selected message with one click,
//! as in Outlook's Home ribbon.

use eframe::egui::{self, RichText};
use egui_phosphor::regular as ph;
use serde::{Deserialize, Serialize};

use super::{
    chrome::{self, IC_BLUE, IC_RED},
    compose::ComposeKind,
    App, Win, CATEGORIES,
};
use crate::{
    db,
    providers::{self, MessagePatch},
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StepAction {
    MoveTo { folder: Option<i64> },
    MarkRead,
    MarkUnread,
    Flag,
    ClearFlag,
    Category { name: String },
    Delete,
    Reply,
    ReplyAll,
    ForwardTo { to: String },
    NewMailTo { to: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuickStep {
    pub name: String,
    pub actions: Vec<StepAction>,
}

type ActionKind = (&'static str, fn() -> StepAction);

const ACTION_KINDS: [ActionKind; 11] = [
    ("Move to folder", || StepAction::MoveTo { folder: None }),
    ("Mark as read", || StepAction::MarkRead),
    ("Mark as unread", || StepAction::MarkUnread),
    ("Flag message", || StepAction::Flag),
    ("Clear flags on message", || StepAction::ClearFlag),
    ("Categorize message", || StepAction::Category {
        name: String::new(),
    }),
    ("Delete message", || StepAction::Delete),
    ("Reply", || StepAction::Reply),
    ("Reply All", || StepAction::ReplyAll),
    ("Forward to", || StepAction::ForwardTo { to: String::new() }),
    ("New Message to", || StepAction::NewMailTo {
        to: String::new(),
    }),
];

impl StepAction {
    fn label(&self) -> &'static str {
        let idx = match self {
            StepAction::MoveTo { .. } => 0,
            StepAction::MarkRead => 1,
            StepAction::MarkUnread => 2,
            StepAction::Flag => 3,
            StepAction::ClearFlag => 4,
            StepAction::Category { .. } => 5,
            StepAction::Delete => 6,
            StepAction::Reply => 7,
            StepAction::ReplyAll => 8,
            StepAction::ForwardTo { .. } => 9,
            StepAction::NewMailTo { .. } => 10,
        };
        ACTION_KINDS[idx].0
    }

    /// Needs settings before it can run (Outlook's "First Time Setup").
    fn incomplete(&self) -> bool {
        match self {
            StepAction::MoveTo { folder } => folder.is_none(),
            StepAction::Category { name } => name.is_empty(),
            StepAction::ForwardTo { to } | StepAction::NewMailTo { to } => to.trim().is_empty(),
            _ => false,
        }
    }
}

impl QuickStep {
    pub fn icon(&self) -> &'static str {
        match self.actions.first() {
            Some(StepAction::MoveTo { .. }) => ph::FOLDER_SIMPLE,
            Some(StepAction::ForwardTo { .. }) => ph::ARROW_BEND_UP_RIGHT,
            Some(StepAction::NewMailTo { .. }) => ph::ENVELOPE_SIMPLE,
            Some(StepAction::Reply | StepAction::ReplyAll) => ph::ARROW_BEND_UP_LEFT,
            Some(StepAction::Delete) => ph::X,
            Some(StepAction::Flag | StepAction::ClearFlag) => ph::FLAG,
            Some(StepAction::Category { .. }) => ph::TAG,
            _ => ph::CHECK,
        }
    }

    fn incomplete(&self) -> bool {
        self.actions.is_empty() || self.actions.iter().any(StepAction::incomplete)
    }
}

/// Outlook's default Quick Steps.
pub fn defaults() -> Vec<QuickStep> {
    vec![
        QuickStep {
            name: "Move to: ?".into(),
            actions: vec![StepAction::MoveTo { folder: None }],
        },
        QuickStep {
            name: "To Manager".into(),
            actions: vec![StepAction::ForwardTo { to: String::new() }],
        },
        QuickStep {
            name: "Team Email".into(),
            actions: vec![StepAction::NewMailTo { to: String::new() }],
        },
        QuickStep {
            name: "Done".into(),
            actions: vec![
                StepAction::MarkRead,
                StepAction::ClearFlag,
                StepAction::MoveTo { folder: None },
            ],
        },
        QuickStep {
            name: "Reply & Delete".into(),
            actions: vec![StepAction::Reply, StepAction::Delete],
        },
    ]
}

pub fn load(c: &rusqlite::Connection) -> Vec<QuickStep> {
    db::get_settings(c)
        .ok()
        .and_then(|s| s.get("quicksteps").cloned())
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_else(defaults)
}

pub struct QuickStepsWin {
    steps: Vec<QuickStep>,
    sel: usize,
    /// Run this step on the message after saving (first use).
    run_after: Option<(usize, i64)>,
}

impl App {
    /// Ribbon gallery: two columns of three steps, plus "Create New".
    pub(super) fn quicksteps_gallery(&mut self, ui: &mut egui::Ui, selected: Option<i64>) {
        let t = self.t;
        let steps = quicksteps_of(self);
        for column in steps.chunks(3).take(2).enumerate() {
            let (ci, chunk) = column;
            chrome::column(ui, |ui| {
                for (k, s) in chunk.iter().enumerate() {
                    let idx = ci * 3 + k;
                    if chrome::small(ui, &t, s.icon(), IC_BLUE, &s.name, selected.is_some())
                        .clicked()
                    {
                        self.run_quickstep(idx, selected.unwrap());
                    }
                }
            });
        }
        chrome::column(ui, |ui| {
            if chrome::small(ui, &t, ph::PLUS, IC_BLUE, "Create New", true).clicked() {
                let mut steps = quicksteps_of(self);
                steps.push(QuickStep {
                    name: "New Quick Step".into(),
                    actions: vec![StepAction::MoveTo { folder: None }],
                });
                let sel = steps.len() - 1;
                self.open_window(Win::QuickSteps(QuickStepsWin {
                    steps,
                    sel,
                    run_after: None,
                }));
            }
            if chrome::small(ui, &t, ph::GEAR, IC_BLUE, "Manage", true).clicked() {
                self.open_window(Win::QuickSteps(QuickStepsWin {
                    steps: quicksteps_of(self),
                    sel: 0,
                    run_after: None,
                }));
            }
        });
    }

    /// Collapsed gallery: one large button with the steps in a menu.
    pub(super) fn quicksteps_button(&mut self, ui: &mut egui::Ui, selected: Option<i64>) {
        let t = self.t;
        let steps = quicksteps_of(self);
        let r = chrome::large(
            ui,
            &t,
            ph::LIGHTNING,
            IC_BLUE,
            "Quick Steps\n\u{E136}",
            true,
        );
        let mut run = None;
        let mut manage = false;
        chrome::menu(&r, |ui| {
            for (i, s) in steps.iter().enumerate() {
                if ui
                    .add_enabled(
                        selected.is_some(),
                        egui::Button::new(format!("{}  {}", s.icon(), s.name)).frame(false),
                    )
                    .clicked()
                {
                    run = Some(i);
                    ui.close();
                }
            }
            ui.separator();
            if chrome::menu_item(ui, ph::GEAR, IC_BLUE, "Manage Quick Steps…") {
                manage = true;
            }
        });
        if let (Some(i), Some(m)) = (run, selected) {
            self.run_quickstep(i, m);
        }
        if manage {
            self.open_window(Win::QuickSteps(QuickStepsWin {
                steps,
                sel: 0,
                run_after: None,
            }));
        }
    }

    pub(super) fn run_quickstep(&mut self, idx: usize, message: i64) {
        let steps = quicksteps_of(self);
        let Some(step) = steps.get(idx).cloned() else {
            return;
        };
        if step.incomplete() {
            // First use: ask for the missing settings, then run.
            self.open_window(Win::QuickSteps(QuickStepsWin {
                steps,
                sel: idx,
                run_after: Some((idx, message)),
            }));
            return;
        }
        // Windows first (they need the message), then the server actions in order.
        for a in &step.actions {
            match a {
                StepAction::Reply => self.open_composer(ComposeKind::Reply(message)),
                StepAction::ReplyAll => self.open_composer(ComposeKind::ReplyAll(message)),
                StepAction::ForwardTo { to } => {
                    self.open_composer(ComposeKind::ForwardTo(message, to.clone()))
                }
                StepAction::NewMailTo { to } => self.open_composer(ComposeKind::To(to.clone())),
                _ => {}
            }
        }
        let actions = step.actions.clone();
        let name = step.name.clone();
        self.spawn(
            Some(&format!("Quick Step \"{name}\" completed.")),
            move |st, _| async move {
                for a in actions {
                    let cats = db::get_message(&st.db(), message)?
                        .map(|m| m.summary.categories)
                        .unwrap_or_default();
                    match a {
                        StepAction::MoveTo { folder: Some(f) } => {
                            providers::move_message(&st, message, f).await?
                        }
                        StepAction::MarkRead => {
                            providers::patch_message(
                                &st,
                                message,
                                &MessagePatch {
                                    is_read: Some(true),
                                    ..Default::default()
                                },
                            )
                            .await?
                        }
                        StepAction::MarkUnread => {
                            providers::patch_message(
                                &st,
                                message,
                                &MessagePatch {
                                    is_read: Some(false),
                                    ..Default::default()
                                },
                            )
                            .await?
                        }
                        StepAction::Flag => {
                            providers::patch_message(
                                &st,
                                message,
                                &MessagePatch {
                                    is_flagged: Some(true),
                                    ..Default::default()
                                },
                            )
                            .await?
                        }
                        StepAction::ClearFlag => {
                            providers::patch_message(
                                &st,
                                message,
                                &MessagePatch {
                                    is_flagged: Some(false),
                                    ..Default::default()
                                },
                            )
                            .await?
                        }
                        StepAction::Category { name } => {
                            let mut c = cats;
                            if !c.contains(&name) {
                                c.push(name);
                            }
                            providers::patch_message(
                                &st,
                                message,
                                &MessagePatch {
                                    categories: Some(c),
                                    ..Default::default()
                                },
                            )
                            .await?
                        }
                        StepAction::Delete => providers::delete_message(&st, message).await?,
                        _ => {}
                    }
                }
                Ok(())
            },
        );
    }

    pub(super) fn quicksteps_window(
        &mut self,
        ctx: &egui::Context,
        vid: egui::ViewportId,
        w: &mut QuickStepsWin,
    ) -> bool {
        let t = self.t;
        let folders = self.data.folders.clone();
        let accounts = self.data.accounts.clone();
        let contacts = self.data.contacts.clone();
        let mut ok = false;
        let mut cancel = false;
        let title = if w.run_after.is_some() {
            "First Time Setup"
        } else {
            "Manage Quick Steps"
        };
        let closed = chrome::native_window(ctx, vid, title, [820.0, 520.0], &t, |ui| {
            egui::TopBottomPanel::bottom("qs_buttons")
                .frame(egui::Frame::new().inner_margin(egui::Margin::same(12)))
                .show_inside(ui, |ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add_sized([90.0, 26.0], egui::Button::new("Cancel"))
                            .clicked()
                        {
                            cancel = true;
                        }
                        if ui
                            .add_sized(
                                [90.0, 26.0],
                                egui::Button::new(if w.run_after.is_some() {
                                    "Save"
                                } else {
                                    "OK"
                                }),
                            )
                            .clicked()
                        {
                            ok = true;
                        }
                    });
                });
            if w.run_after.is_none() {
                egui::SidePanel::left("qs_list")
                    .exact_width(240.0)
                    .resizable(false)
                    .show_inside(ui, |ui| {
                        ui.add_space(8.0);
                        for (i, s) in w.steps.iter().enumerate() {
                            if ui
                                .selectable_label(w.sel == i, format!("  {}", s.name))
                                .clicked()
                            {
                                w.sel = i;
                            }
                        }
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            if ui.button(format!("{}  New", ph::PLUS)).clicked() {
                                w.steps.push(QuickStep {
                                    name: "New Quick Step".into(),
                                    actions: vec![StepAction::MarkRead],
                                });
                                w.sel = w.steps.len() - 1;
                            }
                            if ui
                                .add_enabled(
                                    !w.steps.is_empty(),
                                    egui::Button::new((
                                        RichText::new(ph::X).color(IC_RED),
                                        "Delete",
                                    )),
                                )
                                .clicked()
                            {
                                w.steps.remove(w.sel);
                                w.sel = w.sel.saturating_sub(1);
                            }
                        });
                        if ui.button("Reset to Defaults").clicked() {
                            w.steps = defaults();
                            w.sel = 0;
                        }
                    });
            }
            egui::CentralPanel::default().frame(egui::Frame::new().inner_margin(egui::Margin::same(16))).show_inside(ui, |ui| {
                let Some(step) = w.steps.get_mut(w.sel) else { return };
                if w.run_after.is_some() {
                    ui.label("Choose the settings for this Quick Step. They are saved the first time you use it.");
                    ui.add_space(8.0);
                }
                ui.horizontal(|ui| {
                    ui.label("Name:");
                    ui.add(egui::TextEdit::singleline(&mut step.name).desired_width(360.0));
                });
                ui.add_space(8.0);
                ui.label(RichText::new("Actions").strong());
                let mut remove = None;
                for (i, a) in step.actions.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        egui::ComboBox::from_id_salt(("qs_action", i)).width(230.0).selected_text(a.label()).show_ui(ui, |ui| {
                            for (label, make) in ACTION_KINDS {
                                if ui.selectable_label(a.label() == label, label).clicked() {
                                    *a = make();
                                }
                            }
                        });
                        match a {
                            StepAction::MoveTo { folder } => {
                                let text = folder
                                    .and_then(|id| folders.iter().find(|f| f.id == id))
                                    .map(|f| format!("{} ({})", f.name, accounts.iter().find(|x| x.id == f.account_id).map(|x| x.email.as_str()).unwrap_or("")))
                                    .unwrap_or_else(|| "Choose folder".into());
                                egui::ComboBox::from_id_salt(("qs_folder", i)).width(300.0).selected_text(text).show_ui(ui, |ui| {
                                    for f in &folders {
                                        let acc = accounts.iter().find(|x| x.id == f.account_id).map(|x| x.email.as_str()).unwrap_or("");
                                        ui.selectable_value(folder, Some(f.id), format!("{} ({acc})", f.name));
                                    }
                                });
                            }
                            StepAction::Category { name } => {
                                egui::ComboBox::from_id_salt(("qs_cat", i)).selected_text(if name.is_empty() { "Choose category" } else { name.as_str() }).show_ui(ui, |ui| {
                                    for (c, _) in CATEGORIES {
                                        ui.selectable_value(name, c.to_string(), c);
                                    }
                                });
                            }
                            StepAction::ForwardTo { to } | StepAction::NewMailTo { to } => {
                                ui.add(egui::TextEdit::singleline(to).hint_text("Email address").desired_width(260.0));
                                if let Some(c) = contacts.iter().find(|c| !to.is_empty() && !to.contains('@') && c.display_name.to_lowercase().contains(&to.to_lowercase()) && !c.email.is_empty()) {
                                    if ui.small_button(format!("{} <{}>", c.display_name, c.email)).clicked() {
                                        *to = format!("{} <{}>", c.display_name, c.email);
                                    }
                                }
                            }
                            _ => {}
                        }
                        if ui.add(egui::Button::new(RichText::new(ph::X).color(IC_RED)).frame(false)).on_hover_text("Remove action").clicked() {
                            remove = Some(i);
                        }
                    });
                }
                if let Some(i) = remove {
                    step.actions.remove(i);
                }
                if ui.button(format!("{}  Add Action", ph::PLUS)).clicked() {
                    step.actions.push(StepAction::MarkRead);
                }
                if step.incomplete() {
                    ui.add_space(6.0);
                    ui.label(RichText::new(format!("{}  Please fill in all settings.", ph::WARNING)).color(t.danger));
                }
            });
        });
        if ok {
            if w.run_after.is_some() && w.steps.get(w.sel).is_some_and(QuickStep::incomplete) {
                self.toast("Please fill in all settings of the Quick Step.", true);
                return false;
            }
            self.save_setting(
                "quicksteps",
                serde_json::to_value(&w.steps).unwrap_or_default(),
            );
            if let Some((idx, message)) = w.run_after {
                self.run_quickstep(idx, message);
            }
            return true;
        }
        cancel || closed
    }
}

fn quicksteps_of(app: &App) -> Vec<QuickStep> {
    load(&app.st.db())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_need_setup_and_roundtrip() {
        let d = defaults();
        assert_eq!(d.len(), 5);
        assert!(d[0].incomplete() && d[3].incomplete() && !d[4].incomplete());
        let json = serde_json::to_value(&d).unwrap();
        let back: Vec<QuickStep> = serde_json::from_value(json).unwrap();
        assert_eq!(back, d);
    }
}
