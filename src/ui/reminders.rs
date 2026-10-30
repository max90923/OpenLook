//! Reminder window ("Erinnerungen") for appointments, as in Outlook.

use std::{
    collections::{HashMap, HashSet},
    time::{Duration, Instant},
};

use chrono::Local;
use eframe::egui::{self, RichText, Sense};
use egui_phosphor::regular as ph;

use super::{calendar::local, chrome, App};
use crate::{db, models::Event};

pub struct Reminders {
    last_check: Instant,
    due: Vec<Event>,
    dismissed: HashSet<(i64, String)>,
    snoozed: HashMap<(i64, String), Instant>,
    selected: usize,
    snooze_minutes: i64,
}

impl Default for Reminders {
    fn default() -> Self {
        Self {
            last_check: Instant::now() - Duration::from_secs(3600),
            due: vec![],
            dismissed: HashSet::new(),
            snoozed: HashMap::new(),
            selected: 0,
            snooze_minutes: 5,
        }
    }
}

fn key(e: &Event) -> (i64, String) {
    (e.id, e.start.clone())
}

/// Events whose reminder time has passed and that have not ended yet.
pub fn due_events(events: &[Event], now: chrono::NaiveDateTime) -> Vec<Event> {
    events
        .iter()
        .filter(|e| e.reminder >= 0 && !e.all_day)
        .filter(|e| {
            let start = local(&e.start);
            now >= start - chrono::Duration::minutes(e.reminder) && now < local(&e.end)
        })
        .cloned()
        .collect()
}

fn due_in(e: &Event) -> String {
    let mins = (local(&e.start) - Local::now().naive_local()).num_minutes();
    match mins {
        m if m > 0 => format!("{m} Minuten"),
        0 => "Jetzt".into(),
        m => format!("vor {} Minuten überfällig", -m),
    }
}

impl App {
    pub(super) fn reminders_ui(&mut self, ctx: &egui::Context) {
        if self.reminders.last_check.elapsed() > Duration::from_secs(15) {
            self.reminders.last_check = Instant::now();
            let now = Local::now().naive_local();
            let from = super::local_to_utc_string(now - chrono::Duration::days(1));
            let to = super::local_to_utc_string(now + chrono::Duration::days(2));
            let events = db::list_events(&self.st.db(), &from, &to).unwrap_or_default();
            let r = &mut self.reminders;
            r.snoozed.retain(|_, until| *until > Instant::now());
            r.due = due_events(&events, now)
                .into_iter()
                .filter(|e| !r.dismissed.contains(&key(e)) && !r.snoozed.contains_key(&key(e)))
                .collect();
            r.selected = r.selected.min(r.due.len().saturating_sub(1));
        }
        ctx.request_repaint_after(Duration::from_secs(15));
        if self.reminders.due.is_empty() {
            return;
        }
        let t = self.t;
        let n = self.reminders.due.len();
        let title = if n == 1 {
            "1 Erinnerung".to_string()
        } else {
            format!("{n} Erinnerungen")
        };
        let mut open = None;
        let mut dismiss: Vec<(i64, String)> = vec![];
        let mut snooze: Vec<(i64, String)> = vec![];
        let r = &mut self.reminders;
        let closed = chrome::native_window(
            ctx,
            egui::ViewportId::from_hash_of("openlook-reminders"),
            &title,
            [520.0, 380.0],
            &t,
            |ui| {
                egui::Frame::new()
                    .inner_margin(egui::Margin::same(12))
                    .show(ui, |ui| {
                        let sel = &r.due[r.selected.min(n - 1)];
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(ph::BELL).size(26.0).color(chrome::IC_YELLOW));
                            ui.vertical(|ui| {
                                ui.label(RichText::new(&sel.title).strong());
                                let s = local(&sel.start);
                                ui.label(format!(
                                    "Beginn: {} {}",
                                    super::date_long(s.date()),
                                    s.format("%H:%M")
                                ));
                                if !sel.location.is_empty() {
                                    ui.label(format!("Ort: {}", sel.location));
                                }
                            });
                        });
                        ui.add_space(8.0);
                        let (head, _) = ui.allocate_exact_size(
                            egui::vec2(ui.available_width(), 22.0),
                            Sense::hover(),
                        );
                        ui.painter().rect_filled(head, 0, t.ribbon);
                        ui.painter().text(
                            head.left_center() + egui::vec2(8.0, 0.0),
                            egui::Align2::LEFT_CENTER,
                            "Betreff",
                            egui::FontId::proportional(12.5),
                            t.text,
                        );
                        ui.painter().text(
                            head.right_center() - egui::vec2(150.0, 0.0),
                            egui::Align2::LEFT_CENTER,
                            "Fällig in",
                            egui::FontId::proportional(12.5),
                            t.text,
                        );
                        egui::ScrollArea::vertical()
                            .max_height(140.0)
                            .show(ui, |ui| {
                                for (i, e) in r.due.iter().enumerate() {
                                    let (row, resp) = ui.allocate_exact_size(
                                        egui::vec2(ui.available_width(), 24.0),
                                        Sense::click(),
                                    );
                                    if i == r.selected {
                                        ui.painter().rect_filled(row, 0, t.selected);
                                    }
                                    ui.painter().text(
                                        row.left_center() + egui::vec2(8.0, 0.0),
                                        egui::Align2::LEFT_CENTER,
                                        format!("{}  {}", ph::CALENDAR_BLANK, e.title),
                                        egui::FontId::proportional(13.0),
                                        t.text,
                                    );
                                    ui.painter().text(
                                        row.right_center() - egui::vec2(150.0, 0.0),
                                        egui::Align2::LEFT_CENTER,
                                        due_in(e),
                                        egui::FontId::proportional(13.0),
                                        t.text,
                                    );
                                    if resp.clicked() {
                                        r.selected = i;
                                    }
                                    if resp.double_clicked() {
                                        open = Some(e.clone());
                                    }
                                }
                            });
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            if ui.button("Alle schließen").clicked() {
                                dismiss.extend(r.due.iter().map(key));
                            }
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui.button("Schließen").clicked() {
                                        dismiss.push(key(&r.due[r.selected]));
                                    }
                                    if ui.button("Element öffnen").clicked() {
                                        open = Some(r.due[r.selected].clone());
                                    }
                                },
                            );
                        });
                        ui.add_space(6.0);
                        ui.label(
                            "Klicken Sie auf \"Erneut erinnern\", um erneut erinnert zu werden in:",
                        );
                        ui.horizontal(|ui| {
                            egui::ComboBox::from_id_salt("snooze")
                                .width(160.0)
                                .selected_text(super::calendar::reminder_label(r.snooze_minutes))
                                .show_ui(ui, |ui| {
                                    for m in [5, 10, 15, 30, 60, 120, 1440] {
                                        ui.selectable_value(
                                            &mut r.snooze_minutes,
                                            m,
                                            super::calendar::reminder_label(m),
                                        );
                                    }
                                });
                            if ui.button("Erneut erinnern").clicked() {
                                snooze.push(key(&r.due[r.selected]));
                            }
                        });
                    });
            },
        );
        let r = &mut self.reminders;
        if closed {
            // Closing the window snoozes everything, so nothing gets lost.
            snooze.extend(r.due.iter().map(key));
        }
        let until = Instant::now() + Duration::from_secs(r.snooze_minutes as u64 * 60);
        for k in snooze {
            r.snoozed.insert(k, until);
        }
        for k in dismiss {
            r.dismissed.insert(k);
        }
        r.due
            .retain(|e| !r.dismissed.contains(&key(e)) && !r.snoozed.contains_key(&key(e)));
        r.selected = 0;
        if let Some(e) = open {
            self.edit_event(&e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reminder_due_window() {
        let now = Local::now().naive_local();
        let mk = |start_min: i64, len: i64, reminder: i64| Event {
            id: 1,
            start: super::super::local_to_utc_string(now + chrono::Duration::minutes(start_min)),
            end: super::super::local_to_utc_string(
                now + chrono::Duration::minutes(start_min + len),
            ),
            reminder,
            ..Default::default()
        };
        assert_eq!(due_events(&[mk(10, 30, 15)], now).len(), 1); // reminder time passed
        assert_eq!(due_events(&[mk(30, 30, 15)], now).len(), 0); // not yet
        assert_eq!(due_events(&[mk(-60, 30, 15)], now).len(), 0); // already over
        assert_eq!(due_events(&[mk(10, 30, -1)], now).len(), 0); // no reminder
    }
}
