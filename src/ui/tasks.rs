//! To Do module: smart lists (Mein Tag, Wichtig, Geplant, Aufgaben) plus
//! every task list of the connected accounts.

use chrono::NaiveDate;
use eframe::egui::{self, Color32, RichText, Sense};

use super::{App, BRAND, DANGER};
use crate::{models::*, providers};

#[derive(Clone, PartialEq, Eq, Default)]
pub enum TaskList {
    MyDay,
    Important,
    Planned,
    #[default]
    All,
    /// (account id, list name)
    Named(i64, String),
}

#[derive(Default)]
pub struct TasksView {
    list: TaskList,
    new_title: String,
    selected: Option<i64>,
    show_completed: bool,
    detail: Option<Task>,
}

impl TaskList {
    fn title(&self) -> String {
        match self {
            TaskList::MyDay => "☀ Mein Tag".into(),
            TaskList::Important => "★ Wichtig".into(),
            TaskList::Planned => "📅 Geplant".into(),
            TaskList::All => "🏠 Aufgaben".into(),
            TaskList::Named(_, n) => format!("☰ {n}"),
        }
    }

    fn matches(&self, t: &Task) -> bool {
        match self {
            TaskList::MyDay => t.my_day || t.due == super::today().format("%Y-%m-%d").to_string(),
            TaskList::Important => t.important,
            TaskList::Planned => !t.due.is_empty(),
            TaskList::All => true,
            TaskList::Named(a, n) => t.account_id == *a && &t.list_name == n,
        }
    }
}

impl App {
    pub fn tasks_ui(&mut self, ui: &mut egui::Ui) {
        let mut save: Vec<Task> = vec![];
        let mut delete = None;

        // Lists pane.
        egui::SidePanel::left("task_lists")
            .exact_width(250.0)
            .resizable(false)
            .frame(egui::Frame::new())
            .show_inside(ui, |ui| {
                for l in [
                    TaskList::MyDay,
                    TaskList::Important,
                    TaskList::Planned,
                    TaskList::All,
                ] {
                    let n = self
                        .data
                        .tasks
                        .iter()
                        .filter(|t| !t.completed && l.matches(t))
                        .count();
                    self.task_list_row(ui, l, n);
                }
                ui.separator();
                let mut named: Vec<(i64, String)> = self
                    .data
                    .tasks
                    .iter()
                    .map(|t| (t.account_id, t.list_name.clone()))
                    .collect();
                named.sort();
                named.dedup();
                for (acc, name) in named {
                    let l = TaskList::Named(acc, name);
                    let n = self
                        .data
                        .tasks
                        .iter()
                        .filter(|t| !t.completed && l.matches(t))
                        .count();
                    self.task_list_row(ui, l, n);
                }
            });

        // Detail pane.
        if self.tasks.selected.is_some() {
            if self.tasks.detail.as_ref().map(|d| d.id) != self.tasks.selected {
                self.tasks.detail = self
                    .data
                    .tasks
                    .iter()
                    .find(|t| Some(t.id) == self.tasks.selected)
                    .cloned();
            }
            if let Some(d) = &mut self.tasks.detail {
                let mut close = false;
                egui::SidePanel::right("task_detail")
                    .exact_width(340.0)
                    .resizable(false)
                    .frame(super::pane_frame(ui).inner_margin(egui::Margin::same(16)))
                    .show_inside(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.checkbox(&mut d.completed, "");
                            ui.add(
                                egui::TextEdit::singleline(&mut d.title)
                                    .font(egui::TextStyle::Heading)
                                    .desired_width(240.0),
                            );
                            if ui.button(if d.important { "★" } else { "☆" }).clicked() {
                                d.important = !d.important;
                            }
                        });
                        ui.add_space(10.0);
                        ui.checkbox(&mut d.my_day, "☀ Zu „Mein Tag“ hinzufügen");
                        ui.add_space(6.0);
                        let mut has_due = !d.due.is_empty();
                        ui.horizontal(|ui| {
                            ui.checkbox(&mut has_due, "📅 Fälligkeitsdatum");
                            if has_due {
                                let mut date = NaiveDate::parse_from_str(&d.due, "%Y-%m-%d")
                                    .unwrap_or_else(|_| super::today());
                                ui.add(
                                    egui_extras::DatePickerButton::new(&mut date)
                                        .id_salt("due")
                                        .format("%d.%m.%Y"),
                                );
                                d.due = date.format("%Y-%m-%d").to_string();
                            } else {
                                d.due.clear();
                            }
                        });
                        ui.add_space(6.0);
                        ui.label(RichText::new(format!("Liste: {}", d.list_name)).weak());
                        ui.add_space(6.0);
                        ui.add(
                            egui::TextEdit::multiline(&mut d.notes)
                                .hint_text("Notiz hinzufügen")
                                .desired_rows(8)
                                .desired_width(f32::INFINITY),
                        );
                        ui.add_space(10.0);
                        ui.horizontal(|ui| {
                            if super::primary_button(ui, "💾  Speichern").clicked() {
                                save.push(d.clone());
                            }
                            if ui
                                .button(RichText::new("🗑  Löschen").color(DANGER))
                                .clicked()
                            {
                                delete = Some(d.id);
                            }
                            if ui.button("Schließen").clicked() {
                                close = true;
                            }
                        });
                    });
                if close {
                    self.tasks.selected = None;
                    self.tasks.detail = None;
                }
            }
        }

        // Main list.
        egui::CentralPanel::default()
            .frame(super::pane_frame(ui).inner_margin(egui::Margin::symmetric(24, 18)))
            .show_inside(ui, |ui| {
                ui.label(
                    RichText::new(self.tasks.list.title())
                        .size(24.0)
                        .strong()
                        .color(BRAND),
                );
                if self.tasks.list == TaskList::MyDay {
                    let t = super::today();
                    ui.label(
                        RichText::new(format!(
                            "{}, {}. {}",
                            super::WEEKDAYS_LONG
                                [chrono::Datelike::weekday(&t).num_days_from_monday() as usize],
                            chrono::Datelike::day(&t),
                            super::MONTHS[chrono::Datelike::month0(&t) as usize]
                        ))
                        .weak(),
                    );
                }
                ui.add_space(10.0);
                let add = ui.add(
                    egui::TextEdit::singleline(&mut self.tasks.new_title)
                        .hint_text("+  Aufgabe hinzufügen (Enter)")
                        .desired_width(f32::INFINITY)
                        .margin(egui::Margin::symmetric(10, 10)),
                );
                if add.lost_focus()
                    && ui.input(|i| i.key_pressed(egui::Key::Enter))
                    && !self.tasks.new_title.trim().is_empty()
                {
                    let (account_id, list_name, list_remote_id) = match &self.tasks.list {
                        TaskList::Named(a, n) => {
                            let rid = self
                                .data
                                .tasks
                                .iter()
                                .find(|t| t.account_id == *a && &t.list_name == n)
                                .map(|t| t.list_remote_id.clone())
                                .unwrap_or_default();
                            (*a, n.clone(), rid)
                        }
                        _ => (
                            self.data.local_account_id(),
                            "Aufgaben".to_string(),
                            String::new(),
                        ),
                    };
                    save.push(Task {
                        account_id,
                        list_name,
                        list_remote_id,
                        title: self.tasks.new_title.trim().to_string(),
                        important: self.tasks.list == TaskList::Important,
                        my_day: self.tasks.list == TaskList::MyDay,
                        due: if self.tasks.list == TaskList::Planned {
                            super::today().format("%Y-%m-%d").to_string()
                        } else {
                            String::new()
                        },
                        ..Default::default()
                    });
                    self.tasks.new_title.clear();
                    add.request_focus();
                }
                ui.add_space(10.0);
                let needle = self.search.trim().to_lowercase();
                let visible: Vec<Task> = self
                    .data
                    .tasks
                    .iter()
                    .filter(|t| self.tasks.list.matches(t))
                    .filter(|t| {
                        needle.is_empty()
                            || t.title.to_lowercase().contains(&needle)
                            || t.notes.to_lowercase().contains(&needle)
                    })
                    .cloned()
                    .collect();
                egui::ScrollArea::vertical()
                    .id_salt("tasks_scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for t in visible.iter().filter(|t| !t.completed) {
                            self.task_row(ui, t, &mut save);
                        }
                        let done: Vec<&Task> = visible.iter().filter(|t| t.completed).collect();
                        if !done.is_empty() {
                            ui.add_space(12.0);
                            let arrow = if self.tasks.show_completed {
                                "⏷"
                            } else {
                                "⏵"
                            };
                            if ui
                                .add(
                                    egui::Button::new(
                                        RichText::new(format!(
                                            "{arrow} Abgeschlossen ({})",
                                            done.len()
                                        ))
                                        .strong(),
                                    )
                                    .frame(false),
                                )
                                .clicked()
                            {
                                self.tasks.show_completed = !self.tasks.show_completed;
                            }
                            if self.tasks.show_completed {
                                for t in done {
                                    self.task_row(ui, t, &mut save);
                                }
                            }
                        }
                        if visible.is_empty() {
                            ui.add_space(40.0);
                            ui.vertical_centered(|ui| {
                                ui.label(
                                    RichText::new("✔\n\nKeine Aufgaben – genieß den Tag!")
                                        .size(15.0)
                                        .weak(),
                                )
                            });
                        }
                    });
            });

        for t in save {
            if self.tasks.detail.as_ref().is_some_and(|d| d.id == t.id) {
                self.tasks.detail = Some(t.clone());
            }
            self.spawn(None, move |st, _| async move {
                providers::save_task(&st, t).await.map(|_| ())
            });
        }
        if let Some(id) = delete {
            self.tasks.selected = None;
            self.tasks.detail = None;
            self.spawn(Some("Aufgabe gelöscht"), move |st, _| async move {
                providers::delete_task(&st, id).await
            });
        }
    }

    fn task_list_row(&mut self, ui: &mut egui::Ui, l: TaskList, count: usize) {
        let active = self.tasks.list == l;
        let w = ui.available_width();
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, 32.0), Sense::click());
        let v = ui.visuals();
        if active {
            ui.painter().rect_filled(rect, 4, v.selection.bg_fill);
        } else if resp.hovered() {
            ui.painter()
                .rect_filled(rect, 4, v.widgets.hovered.weak_bg_fill);
        }
        ui.painter().text(
            rect.left_center() + egui::vec2(12.0, 0.0),
            egui::Align2::LEFT_CENTER,
            l.title(),
            egui::FontId::proportional(14.0),
            v.text_color(),
        );
        if count > 0 {
            ui.painter().text(
                rect.right_center() - egui::vec2(12.0, 0.0),
                egui::Align2::RIGHT_CENTER,
                count.to_string(),
                egui::FontId::proportional(12.0),
                v.weak_text_color(),
            );
        }
        if resp.clicked() {
            self.tasks.list = l;
        }
    }

    fn task_row(&mut self, ui: &mut egui::Ui, t: &Task, save: &mut Vec<Task>) {
        let w = ui.available_width();
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, 48.0), Sense::click());
        let v = ui.visuals().clone();
        let fill = if self.tasks.selected == Some(t.id) {
            v.selection.bg_fill
        } else if resp.hovered() {
            v.widgets.hovered.weak_bg_fill
        } else {
            v.faint_bg_color
        };
        ui.painter()
            .rect_filled(rect.shrink2(egui::vec2(0.0, 2.0)), 4, fill);
        // Completion circle.
        let c = rect.left_center() + egui::vec2(20.0, 0.0);
        let circle = egui::Rect::from_center_size(c, egui::vec2(22.0, 22.0));
        let cr = ui.interact(circle, ui.id().with(("done", t.id)), Sense::click());
        if t.completed {
            ui.painter().circle_filled(c, 10.0, BRAND);
            ui.painter().text(
                c,
                egui::Align2::CENTER_CENTER,
                "✔",
                egui::FontId::proportional(12.0),
                Color32::WHITE,
            );
        } else {
            ui.painter()
                .circle_stroke(c, 10.0, egui::Stroke::new(1.5_f32, BRAND));
            if cr.hovered() {
                ui.painter().text(
                    c,
                    egui::Align2::CENTER_CENTER,
                    "✔",
                    egui::FontId::proportional(12.0),
                    BRAND,
                );
            }
        }
        // Title + meta line.
        let title_color = if t.completed {
            v.weak_text_color()
        } else {
            v.text_color()
        };
        let mut title = egui::text::LayoutJob::default();
        title.append(
            &t.title,
            0.0,
            egui::TextFormat {
                font_id: egui::FontId::proportional(14.0),
                color: title_color,
                strikethrough: if t.completed {
                    egui::Stroke::new(1.0_f32, title_color)
                } else {
                    egui::Stroke::NONE
                },
                ..Default::default()
            },
        );
        let g = ui.painter().layout_job(title);
        ui.painter()
            .galley(rect.left_center() + egui::vec2(42.0, -17.0), g, title_color);
        let mut meta = vec![t.list_name.clone()];
        let mut overdue = false;
        if let Ok(d) = NaiveDate::parse_from_str(&t.due, "%Y-%m-%d") {
            let today = super::today();
            overdue = d < today && !t.completed;
            meta.push(match (d - today).num_days() {
                0 => "📅 Heute".into(),
                1 => "📅 Morgen".into(),
                -1 => "📅 Gestern".into(),
                _ => format!("📅 {}", d.format("%d.%m.%Y")),
            });
        }
        if t.my_day {
            meta.push("☀ Mein Tag".into());
        }
        if !t.notes.is_empty() {
            meta.push("🗒".into());
        }
        ui.painter().text(
            rect.left_center() + egui::vec2(42.0, 10.0),
            egui::Align2::LEFT_CENTER,
            meta.join("  ·  "),
            egui::FontId::proportional(12.0),
            if overdue { DANGER } else { v.weak_text_color() },
        );
        // Importance star.
        let sc = rect.right_center() - egui::vec2(22.0, 0.0);
        let star = egui::Rect::from_center_size(sc, egui::vec2(24.0, 24.0));
        let sr = ui.interact(star, ui.id().with(("star", t.id)), Sense::click());
        ui.painter().text(
            sc,
            egui::Align2::CENTER_CENTER,
            if t.important { "★" } else { "☆" },
            egui::FontId::proportional(18.0),
            if t.important {
                BRAND
            } else {
                v.weak_text_color()
            },
        );

        if cr.clicked() {
            save.push(Task {
                completed: !t.completed,
                ..t.clone()
            });
        } else if sr.clicked() {
            save.push(Task {
                important: !t.important,
                ..t.clone()
            });
        } else if resp.clicked() {
            self.tasks.selected = Some(t.id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smart_lists() {
        let today = crate::ui::today().format("%Y-%m-%d").to_string();
        let t = Task {
            due: today,
            ..Default::default()
        };
        assert!(TaskList::MyDay.matches(&t));
        assert!(TaskList::Planned.matches(&t));
        assert!(!TaskList::Important.matches(&t));
        let n = Task {
            account_id: 2,
            list_name: "Einkauf".into(),
            ..Default::default()
        };
        assert!(TaskList::Named(2, "Einkauf".into()).matches(&n));
        assert!(!TaskList::Named(1, "Einkauf".into()).matches(&n));
    }
}
