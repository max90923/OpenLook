//! Tasks module: classic table view ("Simple List") with the
//! "Click here to add a new Task" row, and the task window.

use chrono::{Datelike, Duration, NaiveDate};
use eframe::egui::{self, RichText, Sense};
use egui_phosphor::regular as ph;

use super::{
    chrome::{self, IC_BLUE, IC_GREEN, IC_PURPLE, IC_RED},
    App, Win,
};
use crate::{models::*, providers};

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum TaskFilter {
    #[default]
    All,
    Active,
    Completed,
    Today,
    Overdue,
}

#[derive(Default)]
pub struct TasksView {
    filter: TaskFilter,
    /// (account id, list name); None = all lists ("To-Do List").
    list: Option<(i64, String)>,
    selected: Option<i64>,
    new_title: String,
}

pub struct TaskEditor {
    t: Task,
    has_due: bool,
    due: NaiveDate,
}

impl TaskEditor {
    fn new(t: Task) -> Self {
        let due = NaiveDate::parse_from_str(&t.due, "%Y-%m-%d").ok();
        Self {
            has_due: due.is_some(),
            due: due.unwrap_or_else(super::today),
            t,
        }
    }
}

fn due_date(t: &Task) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(&t.due, "%Y-%m-%d").ok()
}

impl App {
    pub fn new_task(&mut self) {
        let (account_id, list_name) = match &self.tasks.list {
            Some((a, n)) => (*a, n.clone()),
            None => (self.data.default_pim_account(), String::new()),
        };
        let list_remote_id = self.list_remote_id(account_id, &list_name);
        self.open_window(Win::Task(TaskEditor::new(Task {
            account_id,
            list_name,
            list_remote_id,
            ..Default::default()
        })));
    }

    fn list_remote_id(&self, account_id: i64, list_name: &str) -> String {
        self.data
            .tasks
            .iter()
            .find(|t| t.account_id == account_id && t.list_name == list_name)
            .map(|t| t.list_remote_id.clone())
            .unwrap_or_default()
    }

    fn visible_tasks(&self) -> Vec<Task> {
        let today = super::today();
        let needle = self.search.trim().to_lowercase();
        self.data
            .tasks
            .iter()
            .filter(|t| {
                self.tasks
                    .list
                    .as_ref()
                    .is_none_or(|(a, n)| t.account_id == *a && &t.list_name == n)
            })
            .filter(|t| match self.tasks.filter {
                TaskFilter::All => true,
                TaskFilter::Active => !t.completed,
                TaskFilter::Completed => t.completed,
                TaskFilter::Today => !t.completed && due_date(t).is_some_and(|d| d <= today),
                TaskFilter::Overdue => !t.completed && due_date(t).is_some_and(|d| d < today),
            })
            .filter(|t| {
                needle.is_empty()
                    || t.title.to_lowercase().contains(&needle)
                    || t.notes.to_lowercase().contains(&needle)
            })
            .cloned()
            .collect()
    }

    fn save_task(&mut self, t: Task, msg: Option<&str>) {
        self.spawn(msg, move |st, _| async move {
            providers::save_task(&st, t).await.map(|_| ())
        });
    }

    pub fn tasks_ribbon(&mut self, ui: &mut egui::Ui) {
        if self.ribbon_tab != 0 {
            self.common_ribbon(ui);
            return;
        }
        let t = self.t;
        let sel = self
            .tasks
            .selected
            .and_then(|id| self.data.tasks.iter().find(|x| x.id == id))
            .cloned();
        chrome::group(ui, &t, "New", |ui| {
            if chrome::large(
                ui,
                &t,
                ph::CHECK_SQUARE_OFFSET,
                IC_GREEN,
                "New\nTask",
                true,
            )
            .clicked()
            {
                self.new_task();
            }
        });
        chrome::group(ui, &t, "Delete", |ui| {
            if chrome::large(ui, &t, ph::X, IC_RED, "Delete", sel.is_some()).clicked() {
                let id = sel.as_ref().unwrap().id;
                self.tasks.selected = None;
                self.spawn(Some("Task deleted."), move |st, _| async move {
                    providers::delete_task(&st, id).await
                });
            }
        });
        chrome::group(ui, &t, "Manage Task", |ui| {
            if chrome::large(
                ui,
                &t,
                ph::CHECK,
                IC_GREEN,
                "Mark\nComplete",
                sel.as_ref().is_some_and(|x| !x.completed),
            )
            .clicked()
            {
                let mut x = sel.clone().unwrap();
                x.completed = true;
                self.save_task(x, None);
            }
        });
        chrome::group(ui, &t, "Follow Up", |ui| {
            let today = super::today();
            let monday_next =
                today + Duration::days(7 - today.weekday().num_days_from_monday() as i64);
            let friday = monday_next - Duration::days(3);
            let options: [(&str, Option<NaiveDate>); 5] = [
                ("Today", Some(today)),
                ("Tomorrow", Some(today + Duration::days(1))),
                ("This Week", Some(friday.max(today))),
                ("Next Week", Some(monday_next)),
                ("No Date", None),
            ];
            chrome::column(ui, |ui| {
                for (label, d) in &options[..3] {
                    if chrome::small(ui, &t, ph::FLAG, IC_RED, label, sel.is_some()).clicked() {
                        let mut x = sel.clone().unwrap();
                        x.due = d
                            .map(|d| d.format("%Y-%m-%d").to_string())
                            .unwrap_or_default();
                        self.save_task(x, None);
                    }
                }
            });
            chrome::column(ui, |ui| {
                for (label, d) in &options[3..] {
                    if chrome::small(ui, &t, ph::FLAG, IC_RED, label, sel.is_some()).clicked() {
                        let mut x = sel.clone().unwrap();
                        x.due = d
                            .map(|d| d.format("%Y-%m-%d").to_string())
                            .unwrap_or_default();
                        self.save_task(x, None);
                    }
                }
            });
        });
        chrome::group(ui, &t, "Current View", |ui| {
            chrome::column(ui, |ui| {
                for (f, label) in [
                    (TaskFilter::All, "Simple List"),
                    (TaskFilter::Active, "Active"),
                    (TaskFilter::Completed, "Completed"),
                ] {
                    let txt = if self.tasks.filter == f {
                        format!("{label}  \u{E182}")
                    } else {
                        label.to_string()
                    };
                    if chrome::small(ui, &t, ph::LIST_CHECKS, IC_BLUE, &txt, true).clicked() {
                        self.tasks.filter = f;
                    }
                }
            });
            chrome::column(ui, |ui| {
                for (f, label) in [
                    (TaskFilter::Today, "Today"),
                    (TaskFilter::Overdue, "Overdue"),
                ] {
                    let txt = if self.tasks.filter == f {
                        format!("{label}  \u{E182}")
                    } else {
                        label.to_string()
                    };
                    if chrome::small(ui, &t, ph::CALENDAR_CHECK, IC_BLUE, &txt, true).clicked() {
                        self.tasks.filter = f;
                    }
                }
            });
        });
    }

    pub fn tasks_nav(&mut self, ui: &mut egui::Ui) {
        let t = self.t;
        ui.label(RichText::new(format!("{}  My Tasks", ph::CARET_DOWN)).strong());
        let mut lists: Vec<(i64, String)> = self
            .data
            .tasks
            .iter()
            .map(|t| (t.account_id, t.list_name.clone()))
            .collect();
        lists.sort();
        lists.dedup();
        let entries: Vec<(Option<(i64, String)>, String)> =
            std::iter::once((None, "To-Do List".to_string()))
                .chain(lists.into_iter().map(|(a, n)| {
                    let acc = self
                        .data
                        .account(a)
                        .map(|x| x.email.clone())
                        .unwrap_or_default();
                    let label = if self.data.accounts.len() > 1 {
                        format!("{n} – {acc}")
                    } else {
                        n.clone()
                    };
                    (Some((a, n)), label)
                }))
                .collect();
        for (key, label) in entries {
            let active = self.tasks.list == key;
            let w = ui.available_width();
            let (r, resp) = ui.allocate_exact_size(egui::vec2(w, 24.0), Sense::click());
            if active {
                ui.painter().rect_filled(r, 0, t.selected);
            } else if resp.hovered() {
                ui.painter().rect_filled(r, 0, t.hover);
            }
            let g = super::one_line(
                ui.painter(),
                &label,
                egui::FontId::proportional(13.0),
                t.text,
                w - 30.0,
            );
            ui.painter().galley(
                egui::pos2(r.left() + 26.0, r.center().y - g.size().y / 2.0),
                g,
                t.text,
            );
            if resp.clicked() {
                self.tasks.list = key;
            }
        }
    }

    pub fn tasks_ui(&mut self, ui: &mut egui::Ui) {
        let t = self.t;
        let tasks = self.visible_tasks();
        let today = super::today();
        egui::Frame::new()
            .inner_margin(egui::Margin::symmetric(8, 8))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let title = self
                        .tasks
                        .list
                        .as_ref()
                        .map(|(_, n)| n.clone())
                        .unwrap_or_else(|| "To-Do List".into());
                    ui.label(RichText::new(title).size(17.0));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.allocate_ui(egui::vec2(320.0, 26.0), |ui| {
                            super::search_box(
                                ui,
                                &t,
                                &mut self.search,
                                "Search To-Do List (Ctrl+E)",
                            );
                        });
                    });
                });
            });

        // Columns as in Outlook's "Simple List".
        let w = ui.available_width() - 12.0;
        let cols: [(&str, f32); 6] = [
            ("", 28.0),
            ("!", 22.0),
            (
                "Task Subject",
                w - 28.0 - 22.0 - 110.0 - 150.0 - 140.0 - 34.0,
            ),
            ("Due Date", 110.0),
            ("In Folder", 150.0),
            ("Categories", 140.0),
        ];
        let (head, _) = ui.allocate_exact_size(egui::vec2(w + 12.0, 24.0), Sense::hover());
        ui.painter().rect_filled(head, 0, t.ribbon);
        let mut x = head.left() + 6.0;
        for (name, cw) in cols {
            ui.painter().text(
                egui::pos2(x + 6.0, head.center().y),
                egui::Align2::LEFT_CENTER,
                name,
                egui::FontId::proportional(12.5),
                t.text,
            );
            x += cw;
            ui.painter()
                .vline(x, head.y_range(), egui::Stroke::new(1.0_f32, t.border));
        }
        chrome::paint_icon(
            ui,
            ph::FLAG_BANNER,
            t.text,
            egui::pos2(x + 16.0, head.center().y),
            14.0,
            1.0,
        );

        let mut save: Vec<Task> = vec![];
        let mut open = None;
        egui::ScrollArea::vertical()
            .id_salt("tasks_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                // "Click here to add a new task" row.
                ui.horizontal(|ui| {
                    ui.add_space(62.0);
                    let r = ui.add(
                        egui::TextEdit::singleline(&mut self.tasks.new_title)
                            .hint_text("Click here to add a new Task")
                            .desired_width(w - 70.0)
                            .frame(false),
                    );
                    if r.lost_focus()
                        && ui.input(|i| i.key_pressed(egui::Key::Enter))
                        && !self.tasks.new_title.trim().is_empty()
                    {
                        let (account_id, list_name) = match &self.tasks.list {
                            Some((a, n)) => (*a, n.clone()),
                            None => (self.data.default_pim_account(), String::new()),
                        };
                        save.push(Task {
                            account_id,
                            list_remote_id: self.list_remote_id(account_id, &list_name),
                            list_name,
                            title: self.tasks.new_title.trim().to_string(),
                            ..Default::default()
                        });
                        self.tasks.new_title.clear();
                        r.request_focus();
                    }
                });
                ui.painter().hline(
                    ui.max_rect().x_range(),
                    ui.cursor().top(),
                    egui::Stroke::new(1.0_f32, t.border),
                );
                for task in &tasks {
                    let (r, resp) =
                        ui.allocate_exact_size(egui::vec2(w + 12.0, 26.0), Sense::click());
                    let sel = self.tasks.selected == Some(task.id);
                    if sel {
                        ui.painter().rect_filled(r, 0, t.selected);
                    } else if resp.hovered() {
                        ui.painter().rect_filled(r, 0, t.hover);
                    }
                    ui.painter().hline(
                        r.x_range(),
                        r.bottom() - 0.5,
                        egui::Stroke::new(1.0_f32, t.border.gamma_multiply(0.6)),
                    );
                    let overdue = !task.completed && due_date(task).is_some_and(|d| d < today);
                    let color = if task.completed {
                        t.text_weak
                    } else if overdue {
                        t.danger
                    } else {
                        t.text
                    };
                    let mut x = r.left() + 6.0;
                    let cy = r.center().y;
                    // Checkbox.
                    let cb = egui::Rect::from_center_size(
                        egui::pos2(x + 14.0, cy),
                        egui::vec2(14.0, 14.0),
                    );
                    ui.painter().rect_stroke(
                        cb,
                        0,
                        egui::Stroke::new(1.0_f32, t.text_weak),
                        egui::StrokeKind::Inside,
                    );
                    if task.completed {
                        ui.painter().text(
                            cb.center(),
                            egui::Align2::CENTER_CENTER,
                            ph::CHECK,
                            egui::FontId::proportional(12.0),
                            t.text,
                        );
                    }
                    x += cols[0].1;
                    if task.important {
                        ui.painter().text(
                            egui::pos2(x + 8.0, cy),
                            egui::Align2::LEFT_CENTER,
                            "!",
                            egui::FontId::proportional(14.0),
                            t.danger,
                        );
                    }
                    x += cols[1].1;
                    let mut job = egui::text::LayoutJob::default();
                    job.append(
                        &task.title,
                        0.0,
                        egui::TextFormat {
                            font_id: egui::FontId::proportional(13.0),
                            color,
                            strikethrough: if task.completed {
                                egui::Stroke::new(1.0_f32, color)
                            } else {
                                egui::Stroke::NONE
                            },
                            ..Default::default()
                        },
                    );
                    job.wrap = egui::text::TextWrapping {
                        max_width: cols[2].1 - 12.0,
                        max_rows: 1,
                        break_anywhere: true,
                        overflow_character: Some('…'),
                    };
                    let g = ui.painter().layout_job(job);
                    ui.painter()
                        .galley(egui::pos2(x + 6.0, cy - g.size().y / 2.0), g, color);
                    x += cols[2].1;
                    let due = due_date(task)
                        .map(|d| {
                            format!(
                                "{} {}",
                                super::WEEKDAYS[d.weekday().num_days_from_monday() as usize],
                                d.format("%d/%m/%Y")
                            )
                        })
                        .unwrap_or_else(|| "None".into());
                    ui.painter().text(
                        egui::pos2(x + 6.0, cy),
                        egui::Align2::LEFT_CENTER,
                        due,
                        egui::FontId::proportional(12.5),
                        color,
                    );
                    x += cols[3].1;
                    let folder = super::one_line(
                        ui.painter(),
                        &task.list_name,
                        egui::FontId::proportional(12.5),
                        t.text_weak,
                        cols[4].1 - 12.0,
                    );
                    ui.painter().galley(
                        egui::pos2(x + 6.0, cy - folder.size().y / 2.0),
                        folder,
                        t.text_weak,
                    );
                    x += cols[4].1 + cols[5].1;
                    let flag = egui::Rect::from_center_size(
                        egui::pos2(x + 16.0, cy),
                        egui::vec2(18.0, 18.0),
                    );
                    let (icon, fc) = if task.completed {
                        (ph::CHECK, IC_GREEN)
                    } else if task.due.is_empty() {
                        (ph::FLAG_BANNER, t.text_weak)
                    } else {
                        (ph::FLAG, t.flag)
                    };
                    chrome::paint_icon(ui, icon, fc, flag.center(), 15.0, 1.0);

                    let pos = resp.interact_pointer_pos();
                    if resp.clicked() {
                        if pos.is_some_and(|p| {
                            cb.expand(4.0).contains(p) || flag.expand(4.0).contains(p)
                        }) {
                            save.push(Task {
                                completed: !task.completed,
                                ..task.clone()
                            });
                        } else {
                            self.tasks.selected = Some(task.id);
                        }
                    }
                    if resp.double_clicked() {
                        open = Some(task.clone());
                    }
                    resp.context_menu(|ui| {
                        if chrome::menu_item(
                            ui,
                            ph::CHECK,
                            IC_GREEN,
                            if task.completed {
                                "Mark Not Complete"
                            } else {
                                "Mark Complete"
                            },
                        ) {
                            save.push(Task {
                                completed: !task.completed,
                                ..task.clone()
                            });
                        }
                        if chrome::menu_item(
                            ui,
                            "!",
                            IC_RED,
                            if task.important {
                                "Normal Importance"
                            } else {
                                "High Importance"
                            },
                        ) {
                            save.push(Task {
                                important: !task.important,
                                ..task.clone()
                            });
                        }
                        if chrome::menu_item(ui, ph::FOLDER_OPEN, IC_BLUE, "Open") {
                            open = Some(task.clone());
                        }
                    });
                }
                if tasks.is_empty() {
                    ui.add_space(20.0);
                    ui.vertical_centered(|ui| {
                        ui.label(
                            RichText::new("There are no items to show in this view.")
                                .color(t.text_weak),
                        )
                    });
                }
            });
        for x in save {
            self.save_task(x, None);
        }
        if let Some(x) = open {
            self.open_window(Win::Task(TaskEditor::new(x)));
        }
    }

    pub(super) fn task_window(
        &mut self,
        ctx: &egui::Context,
        vid: egui::ViewportId,
        ed: &mut TaskEditor,
    ) -> bool {
        let th = self.t;
        let title = format!(
            "{} - Task",
            if ed.t.title.trim().is_empty() {
                "Untitled"
            } else {
                ed.t.title.trim()
            }
        );
        let is_new = ed.t.id == 0;
        let accounts = self.data.accounts.clone();
        let mut save = false;
        let mut delete = false;
        let closed = chrome::native_window(ctx, vid, &title, [760.0, 540.0], &th, |ui| {
            chrome::item_ribbon(ui, &th, "Task", |ui| {
                chrome::group(ui, &th, "Actions", |ui| {
                    if chrome::large(
                        ui,
                        &th,
                        ph::FLOPPY_DISK,
                        IC_PURPLE,
                        "Save &\nClose",
                        true,
                    )
                    .clicked()
                    {
                        save = true;
                    }
                    if chrome::large(ui, &th, ph::X, IC_RED, "Delete", !is_new).clicked() {
                        delete = true;
                    }
                });
                chrome::group(ui, &th, "Manage Task", |ui| {
                    if chrome::large(
                        ui,
                        &th,
                        ph::CHECK,
                        IC_GREEN,
                        "Mark\nComplete",
                        !ed.t.completed,
                    )
                    .clicked()
                    {
                        ed.t.completed = true;
                        save = true;
                    }
                });
                chrome::group(ui, &th, "Tags", |ui| {
                    chrome::column(ui, |ui| {
                        if chrome::small(
                            ui,
                            &th,
                            "!",
                            IC_RED,
                            if ed.t.important {
                                "High Importance  \u{E182}"
                            } else {
                                "High Importance"
                            },
                            true,
                        )
                        .clicked()
                        {
                            ed.t.important = !ed.t.important;
                        }
                    });
                });
            });
            egui::Frame::new()
                .inner_margin(egui::Margin::symmetric(16, 12))
                .show(ui, |ui| {
                    egui::Grid::new("task_form")
                        .num_columns(2)
                        .spacing([12.0, 8.0])
                        .show(ui, |ui| {
                            ui.label("Subject");
                            ui.add(
                                egui::TextEdit::singleline(&mut ed.t.title).desired_width(500.0),
                            );
                            ui.end_row();
                            ui.label("Due date");
                            ui.horizontal(|ui| {
                                ui.checkbox(&mut ed.has_due, "");
                                ui.add_enabled(
                                    ed.has_due,
                                    egui_extras::DatePickerButton::new(&mut ed.due)
                                        .id_salt("task_due")
                                        .format("%d/%m/%Y"),
                                );
                                if !ed.has_due {
                                    ui.label(RichText::new("None").color(th.text_weak));
                                }
                            });
                            ui.end_row();
                            ui.label("Status");
                            egui::ComboBox::from_id_salt("task_status")
                                .selected_text(if ed.t.completed {
                                    "Completed"
                                } else {
                                    "Not Started"
                                })
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(
                                        &mut ed.t.completed,
                                        false,
                                        "Not Started",
                                    );
                                    ui.selectable_value(&mut ed.t.completed, true, "Completed");
                                });
                            ui.end_row();
                            ui.label("Priority");
                            egui::ComboBox::from_id_salt("task_prio")
                                .selected_text(if ed.t.important { "High" } else { "Normal" })
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(&mut ed.t.important, false, "Normal");
                                    ui.selectable_value(&mut ed.t.important, true, "High");
                                });
                            ui.end_row();
                            if is_new {
                                ui.label("Save in");
                                egui::ComboBox::from_id_salt("task_acc")
                                    .selected_text(
                                        accounts
                                            .iter()
                                            .find(|a| a.id == ed.t.account_id)
                                            .map(|a| a.email.clone())
                                            .unwrap_or_default(),
                                    )
                                    .show_ui(ui, |ui| {
                                        for a in &accounts {
                                            if ui
                                                .selectable_value(
                                                    &mut ed.t.account_id,
                                                    a.id,
                                                    &a.email,
                                                )
                                                .clicked()
                                            {
                                                ed.t.list_name.clear();
                                                ed.t.list_remote_id.clear();
                                            }
                                        }
                                    });
                                ui.end_row();
                            }
                        });
                    ui.add_space(6.0);
                    ui.separator();
                    let (w, h) = (ui.available_width(), ui.available_height().max(100.0));
                    ui.add_sized(
                        [w, h - 4.0],
                        egui::TextEdit::multiline(&mut ed.t.notes).frame(false),
                    );
                });
        });
        if save {
            if ed.t.title.trim().is_empty() {
                self.toast("Please enter a subject.", true);
                return false;
            }
            let mut task = ed.t.clone();
            task.due = if ed.has_due {
                ed.due.format("%Y-%m-%d").to_string()
            } else {
                String::new()
            };
            self.save_task(task, Some("Task saved."));
            return true;
        }
        if delete {
            let id = ed.t.id;
            self.spawn(Some("Task deleted."), move |st, _| async move {
                providers::delete_task(&st, id).await
            });
            return true;
        }
        closed
    }
}
