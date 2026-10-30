//! To-Do Bar: calendar peek with upcoming appointments, favourite
//! people and open tasks at the right of the mail module (View >
//! To-Do Bar).

use chrono::{Datelike, Duration, NaiveDate};
use eframe::egui::{self, RichText, Sense};
use egui_phosphor::regular as ph;

use super::{calendar::local, compose::ComposeKind, App, WEEKDAYS, WEEKDAYS_LONG};
use crate::{db, models::*, providers};

#[derive(Default)]
pub struct TodoBar {
    month: Option<NaiveDate>,
    new_task: String,
}

impl App {
    pub(super) fn todo_bar_visible(&self) -> bool {
        self.prefs.todo_calendar || self.prefs.todo_people || self.prefs.todo_tasks
    }

    pub(super) fn todo_bar(&mut self, ui: &mut egui::Ui) {
        let t = self.t;
        egui::ScrollArea::vertical()
            .id_salt("todo_bar")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(6.0);
                if self.prefs.todo_calendar {
                    self.todo_calendar(ui);
                    ui.add_space(10.0);
                    ui.separator();
                }
                if self.prefs.todo_people {
                    ui.label(RichText::new("People").size(15.0));
                    let favorites: Vec<Contact> = self
                        .data
                        .contacts
                        .iter()
                        .filter(|c| c.favorite && !c.email.is_empty())
                        .cloned()
                        .collect();
                    if favorites.is_empty() {
                        ui.label(
                            RichText::new(
                                "Add contacts to Favorites to see them here.",
                            )
                            .size(12.0)
                            .color(t.text_weak),
                        );
                    }
                    for c in favorites {
                        let (r, resp) = ui.allocate_exact_size(
                            egui::vec2(ui.available_width(), 34.0),
                            Sense::click(),
                        );
                        if resp.hovered() {
                            ui.painter().rect_filled(r, 0, t.hover);
                        }
                        super::paint_avatar(
                            ui.painter(),
                            r.left_center() + egui::vec2(16.0, 0.0),
                            13.0,
                            &c.display_name,
                        );
                        ui.painter().text(
                            r.left_center() + egui::vec2(36.0, 0.0),
                            egui::Align2::LEFT_CENTER,
                            &c.display_name,
                            egui::FontId::proportional(13.0),
                            t.text,
                        );
                        if resp
                            .on_hover_text(format!("Email {}", c.email))
                            .clicked()
                        {
                            self.open_composer(ComposeKind::To(format!(
                                "{} <{}>",
                                c.display_name, c.email
                            )));
                        }
                    }
                    ui.add_space(10.0);
                    ui.separator();
                }
                if self.prefs.todo_tasks {
                    self.todo_tasks(ui);
                }
            });
    }

    fn todo_calendar(&mut self, ui: &mut egui::Ui) {
        let t = self.t;
        let today = super::today();
        let month = self.todo.month.unwrap_or(today.with_day(1).unwrap());
        ui.horizontal(|ui| {
            if ui
                .add(egui::Button::new(ph::CARET_LEFT).frame(false))
                .clicked()
            {
                self.todo.month = Some(prev_month(month));
            }
            ui.label(
                RichText::new(format!(
                    "{} {}",
                    super::MONTHS[month.month0() as usize],
                    month.year()
                ))
                .strong(),
            );
            if ui
                .add(egui::Button::new(ph::CARET_RIGHT).frame(false))
                .clicked()
            {
                self.todo.month = Some(next_month(month));
            }
        });
        let w = ui.available_width();
        let cell = (w / 7.0).min(32.0);
        let (rect, _) =
            ui.allocate_exact_size(egui::vec2(cell * 7.0, cell * 0.75 * 7.0), Sense::hover());
        let ch = cell * 0.75;
        for (i, d) in WEEKDAYS.iter().enumerate() {
            ui.painter().text(
                egui::pos2(rect.left() + cell * (i as f32 + 0.5), rect.top() + ch / 2.0),
                egui::Align2::CENTER_CENTER,
                &d[..1],
                egui::FontId::proportional(11.0),
                t.text_weak,
            );
        }
        let start = month - Duration::days(month.weekday().num_days_from_monday() as i64);
        let from = super::local_to_utc_string(start.and_hms_opt(0, 0, 0).unwrap());
        let to =
            super::local_to_utc_string((start + Duration::days(42)).and_hms_opt(0, 0, 0).unwrap());
        let busy: Vec<NaiveDate> = db::list_events(&self.st.db(), &from, &to)
            .unwrap_or_default()
            .iter()
            .map(|e| local(&e.start).date())
            .collect();
        for i in 0..42 {
            let day = start + Duration::days(i);
            let r = egui::Rect::from_min_size(
                egui::pos2(
                    rect.left() + cell * (i % 7) as f32,
                    rect.top() + ch * (1 + i / 7) as f32,
                ),
                egui::vec2(cell, ch),
            );
            let resp = ui.interact(r, ui.id().with(("todo_day", i)), Sense::click());
            if day == today {
                ui.painter().rect_stroke(
                    r.shrink(1.0),
                    0,
                    egui::Stroke::new(1.5_f32, t.today),
                    egui::StrokeKind::Inside,
                );
            } else if resp.hovered() {
                ui.painter().rect_filled(r, 0, t.hover);
            }
            let color = if day.month() != month.month() {
                t.text_weak.gamma_multiply(0.6)
            } else {
                t.text
            };
            let font = if busy.contains(&day) {
                super::chrome::bold_font(11.5)
            } else {
                egui::FontId::proportional(11.5)
            };
            ui.painter().text(
                r.center(),
                egui::Align2::CENTER_CENTER,
                day.day().to_string(),
                font,
                color,
            );
            if resp.clicked() {
                self.cal.go_to(day);
                self.switch_module(super::Module::Calendar);
            }
        }
        ui.add_space(8.0);
        // Upcoming appointments of the next days, grouped like Outlook's peek.
        let now = chrono::Local::now().naive_local();
        let from = super::local_to_utc_string(now - Duration::hours(1));
        let to =
            super::local_to_utc_string((today + Duration::days(7)).and_hms_opt(0, 0, 0).unwrap());
        let events = db::list_events(&self.st.db(), &from, &to).unwrap_or_default();
        if events.is_empty() {
            ui.label(
                RichText::new("No upcoming appointments.")
                    .size(12.0)
                    .color(t.text_weak),
            );
        }
        let mut last = None;
        let mut open = None;
        for e in events.iter().filter(|e| local(&e.end) > now).take(8) {
            let day = local(&e.start).date();
            if last != Some(day) {
                let label = match (day - today).num_days() {
                    0 => "TODAY".to_string(),
                    1 => "TOMORROW".to_string(),
                    _ => format!(
                        "{} {}",
                        WEEKDAYS_LONG[day.weekday().num_days_from_monday() as usize].to_uppercase(),
                        day.format("%d/%m")
                    ),
                };
                ui.label(RichText::new(label).size(11.0).color(t.text_weak));
                last = Some(day);
            }
            let (r, resp) =
                ui.allocate_exact_size(egui::vec2(ui.available_width(), 38.0), Sense::click());
            if resp.hovered() {
                ui.painter().rect_filled(r, 0, t.hover);
            }
            let color = if e.category.is_empty() {
                super::chrome::IC_BLUE
            } else {
                super::category_color(&e.category)
            };
            ui.painter().rect_filled(
                egui::Rect::from_min_size(r.min + egui::vec2(2.0, 3.0), egui::vec2(3.0, 32.0)),
                0,
                color,
            );
            let time = if e.all_day {
                "All day".to_string()
            } else {
                format!(
                    "{} - {}",
                    local(&e.start).format("%H:%M"),
                    local(&e.end).format("%H:%M")
                )
            };
            let g = super::one_line(
                ui.painter(),
                &e.title,
                egui::FontId::proportional(13.0),
                t.text,
                r.width() - 14.0,
            );
            ui.painter()
                .galley(r.min + egui::vec2(10.0, 3.0), g, t.text);
            let sub = if e.location.is_empty() {
                time
            } else {
                format!("{time}  {}", e.location)
            };
            let g = super::one_line(
                ui.painter(),
                &sub,
                egui::FontId::proportional(11.5),
                t.text_weak,
                r.width() - 14.0,
            );
            ui.painter()
                .galley(r.min + egui::vec2(10.0, 21.0), g, t.text_weak);
            if resp.double_clicked() {
                open = Some(e.clone());
            }
        }
        if let Some(e) = open {
            self.edit_event(&e);
        }
    }

    fn todo_tasks(&mut self, ui: &mut egui::Ui) {
        let t = self.t;
        ui.label(RichText::new("Tasks").size(15.0));
        let r = ui.add(
            egui::TextEdit::singleline(&mut self.todo.new_task)
                .hint_text("Type a new task")
                .desired_width(f32::INFINITY),
        );
        if r.lost_focus()
            && ui.input(|i| i.key_pressed(egui::Key::Enter))
            && !self.todo.new_task.trim().is_empty()
        {
            let task = Task {
                account_id: self.data.default_pim_account(),
                title: self.todo.new_task.trim().to_string(),
                due: super::today().format("%Y-%m-%d").to_string(),
                ..Default::default()
            };
            self.todo.new_task.clear();
            self.spawn(None, move |st, _| async move {
                providers::save_task(&st, task).await.map(|_| ())
            });
        }
        ui.add_space(4.0);
        let mut open: Vec<Task> = self
            .data
            .tasks
            .iter()
            .filter(|t| !t.completed)
            .cloned()
            .collect();
        open.sort_by(|a, b| (a.due.is_empty(), &a.due).cmp(&(b.due.is_empty(), &b.due)));
        if open.is_empty() {
            ui.label(
                RichText::new("There are no items to show in this view.")
                    .size(12.0)
                    .color(t.text_weak),
            );
        }
        let mut done = None;
        for task in open.iter().take(20) {
            ui.horizontal(|ui| {
                let mut c = false;
                if ui
                    .checkbox(&mut c, "")
                    .on_hover_text("Mark Complete")
                    .changed()
                {
                    done = Some(task.clone());
                }
                ui.vertical(|ui| {
                    ui.label(RichText::new(&task.title).size(13.0));
                    if let Ok(d) = NaiveDate::parse_from_str(&task.due, "%Y-%m-%d") {
                        let overdue = d < super::today();
                        ui.label(
                            RichText::new(format!("Due: {}", d.format("%d/%m/%Y")))
                                .size(11.0)
                                .color(if overdue { t.danger } else { t.text_weak }),
                        );
                    }
                });
            });
        }
        if let Some(mut task) = done {
            task.completed = true;
            self.spawn(None, move |st, _| async move {
                providers::save_task(&st, task).await.map(|_| ())
            });
        }
    }
}

fn prev_month(d: NaiveDate) -> NaiveDate {
    if d.month() == 1 {
        NaiveDate::from_ymd_opt(d.year() - 1, 12, 1).unwrap()
    } else {
        NaiveDate::from_ymd_opt(d.year(), d.month() - 1, 1).unwrap()
    }
}

fn next_month(d: NaiveDate) -> NaiveDate {
    if d.month() == 12 {
        NaiveDate::from_ymd_opt(d.year() + 1, 1, 1).unwrap()
    } else {
        NaiveDate::from_ymd_opt(d.year(), d.month() + 1, 1).unwrap()
    }
}
