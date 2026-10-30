//! Scheduling Assistant: free/busy grid of the organizer and the
//! attendees in the meeting window, with AutoPick of the next free slot.

use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime, NaiveTime, Weekday};
use eframe::egui::{self, Color32, RichText, Sense};

use super::{calendar::local, App};
use crate::providers::{self, FreeBusy};

/// Days of free/busy information loaded from the meeting day on.
const DAYS: i64 = 14;
const SLOT_W: f32 = 22.0;
const ROW_H: f32 = 24.0;
const NAME_W: f32 = 210.0;

#[derive(Default)]
pub struct Planner {
    /// What `data` was loaded for (account, people, first day).
    key: String,
    /// None while loading.
    data: Option<Result<Vec<FreeBusy>, String>>,
    add: String,
}

/// Address part of "Name <address>".
fn address(p: &str) -> String {
    match p.rsplit_once('<') {
        Some((_, a)) => a.trim_end_matches('>').trim().to_string(),
        None => p.trim().to_string(),
    }
}

/// Organizer first, then the attendees.
pub(super) fn people(organizer: &str, attendees: &str) -> Vec<String> {
    let mut out = vec![organizer.to_string()];
    for a in attendees
        .split([';', ','])
        .map(address)
        .filter(|a| !a.is_empty())
    {
        if !out.iter().any(|o| o.eq_ignore_ascii_case(&a)) {
            out.push(a);
        }
    }
    out
}

/// First start (searching forward or backward from `from`, in 30-minute
/// steps) at which a meeting of `dur` fits on a weekday within working hours
/// without touching any busy block.
pub(super) fn auto_pick(
    busy: &[(NaiveDateTime, NaiveDateTime)],
    from: NaiveDateTime,
    dur: Duration,
    work: (u32, u32),
    forward: bool,
    limit: Duration,
) -> Option<NaiveDateTime> {
    let step = Duration::minutes(30);
    let mut t = from;
    for _ in 0..(limit.num_minutes() / 30) {
        t = if forward { t + step } else { t - step };
        let end = t + dur;
        let weekday = !matches!(t.weekday(), Weekday::Sat | Weekday::Sun);
        let midnight = t.date().and_time(NaiveTime::MIN);
        let in_hours = t >= midnight + Duration::hours(work.0 as i64)
            && end <= midnight + Duration::hours(work.1 as i64);
        if weekday && in_hours && busy.iter().all(|(s, e)| *e <= t || *s >= end) {
            return Some(t);
        }
    }
    None
}

fn busy_color(show_as: &str) -> Color32 {
    match show_as {
        "tentative" => Color32::from_rgb(140, 190, 235),
        "oof" => Color32::from_rgb(135, 100, 184),
        "workingElsewhere" => Color32::from_rgb(190, 170, 220),
        "free" => Color32::TRANSPARENT,
        _ => Color32::from_rgb(0, 95, 184),
    }
}

/// The meeting fields the assistant edits.
pub(super) struct Meeting<'a> {
    pub account_id: i64,
    pub organizer: String,
    pub attendees: &'a mut String,
    pub start_date: &'a mut NaiveDate,
    pub start_time: &'a mut NaiveTime,
    pub end_date: &'a mut NaiveDate,
    pub end_time: &'a mut NaiveTime,
}

impl App {
    /// Free/busy results arrived for planner `key`.
    pub(super) fn free_busy_loaded(&mut self, key: &str, data: Result<Vec<FreeBusy>, String>) {
        for w in &mut self.windows {
            if let super::Win::Event(e) = &mut w.win {
                if let Some(p) = e.planner.as_mut().filter(|p| p.key == key) {
                    p.data = Some(data.clone());
                }
            }
        }
    }

    pub(super) fn scheduling_ui(&mut self, ui: &mut egui::Ui, p: &mut Planner, mut m: Meeting) {
        let t = self.t;
        let names = people(&m.organizer, m.attendees);
        let key = format!("{}|{}|{}", m.account_id, names.join(";"), m.start_date);
        if p.key != key {
            p.key = key.clone();
            p.data = None;
            let (acc, emails) = (m.account_id, names.clone());
            let from = super::local_to_utc_string(m.start_date.and_hms_opt(0, 0, 0).unwrap());
            let to = super::local_to_utc_string(
                (*m.start_date + Duration::days(DAYS))
                    .and_hms_opt(0, 0, 0)
                    .unwrap(),
            );
            self.spawn(None, move |st, tx| async move {
                let res = providers::free_busy(&st, acc, &emails, &from, &to)
                    .await
                    .map_err(|e| format!("{e:#}"));
                let _ = tx.send(super::Done::FreeBusy(key, res));
                Ok(())
            });
        }
        let known: Vec<Option<Vec<(NaiveDateTime, NaiveDateTime, String)>>> = names
            .iter()
            .map(|n| match &p.data {
                Some(Ok(list)) => list
                    .iter()
                    .find(|f| f.email.eq_ignore_ascii_case(n))
                    .and_then(|f| f.busy.as_ref())
                    .map(|b| {
                        b.iter()
                            .map(|x| (local(&x.start), local(&x.end), x.show_as.clone()))
                            .collect()
                    }),
                _ => None,
            })
            .collect();

        let work = (
            self.prefs.work_start,
            self.prefs.work_end.max(self.prefs.work_start + 1),
        );
        let day = *m.start_date;
        let first = day.and_hms_opt(work.0, 0, 0).unwrap();
        let slots = ((work.1 - work.0) * 2) as usize;
        let start = m.start_date.and_time(*m.start_time);
        let end = m.end_date.and_time(*m.end_time);
        let dur = (end - start).max(Duration::minutes(30));

        // Header with the day and the hours.
        let grid_w = NAME_W + SLOT_W * slots as f32;
        egui::ScrollArea::horizontal().id_salt("planner").show(ui, |ui| {
            let rows = names.len() + 1;
            let (rect, resp) = ui.allocate_exact_size(
                egui::vec2(grid_w, ROW_H * (rows as f32 + 2.0)),
                Sense::click(),
            );
            let p_ = ui.painter_at(rect);
            let x0 = rect.left() + NAME_W;
            let x_of = |dt: NaiveDateTime| {
                x0 + ((dt - first).num_minutes() as f32 / 30.0) * SLOT_W
            };
            p_.rect_filled(
                egui::Rect::from_min_size(rect.min, egui::vec2(grid_w, ROW_H)),
                0,
                t.pane_alt,
            );
            p_.text(
                egui::pos2(x0 + 4.0, rect.top() + ROW_H / 2.0),
                egui::Align2::LEFT_CENTER,
                format!(
                    "{}, {}",
                    super::WEEKDAYS_LONG[day.weekday().num_days_from_monday() as usize],
                    day.format("%d/%m/%Y")
                ),
                egui::FontId::proportional(12.5),
                t.text,
            );
            for i in 0..=slots {
                let x = x0 + i as f32 * SLOT_W;
                let full = i % 2 == 0;
                let top = rect.top() + if full { ROW_H } else { ROW_H * 1.6 };
                p_.line_segment(
                    [egui::pos2(x, top), egui::pos2(x, rect.bottom())],
                    egui::Stroke::new(1.0_f32, if full { t.border } else { t.border.gamma_multiply(0.45) }),
                );
                if full && i < slots {
                    p_.text(
                        egui::pos2(x + 3.0, rect.top() + ROW_H * 1.5),
                        egui::Align2::LEFT_CENTER,
                        format!("{:02}:00", work.0 + i as u32 / 2),
                        egui::FontId::proportional(11.5),
                        t.text_weak,
                    );
                }
            }
            let row_rect = |r: usize| {
                egui::Rect::from_min_size(
                    egui::pos2(rect.left(), rect.top() + ROW_H * (r as f32 + 2.0)),
                    egui::vec2(grid_w, ROW_H),
                )
            };
            let mut all: Vec<(NaiveDateTime, NaiveDateTime, String)> = vec![];
            let mut unknown = false;
            for (i, name) in names.iter().enumerate() {
                let r = row_rect(i + 1);
                let label = if i == 0 {
                    format!("{name} (Organizer)")
                } else {
                    name.clone()
                };
                let g = super::one_line(&p_, &label, egui::FontId::proportional(12.5), t.text, NAME_W - 12.0);
                p_.galley(egui::pos2(r.left() + 6.0, r.center().y - g.size().y / 2.0), g, t.text);
                p_.line_segment(
                    [r.left_bottom(), r.right_bottom()],
                    egui::Stroke::new(1.0_f32, t.border.gamma_multiply(0.6)),
                );
                match &known[i] {
                    None => {
                        unknown = true;
                        // "No Information": hatched grey.
                        let area = egui::Rect::from_x_y_ranges(x0..=r.right(), r.y_range()).shrink2(egui::vec2(0.0, 4.0));
                        let mut x = area.left();
                        while x < area.right() {
                            p_.line_segment(
                                [egui::pos2(x, area.bottom()), egui::pos2((x + area.height()).min(area.right()), area.top())],
                                egui::Stroke::new(1.0_f32, t.text_weak.gamma_multiply(0.5)),
                            );
                            x += 6.0;
                        }
                    }
                    Some(blocks) => {
                        for (s, e, show) in blocks {
                            let a = x_of(*s).max(x0);
                            let b = x_of(*e).min(r.right());
                            if b > a {
                                p_.rect_filled(
                                    egui::Rect::from_x_y_ranges(a..=b, r.y_range()).shrink2(egui::vec2(0.0, 4.0)),
                                    0,
                                    busy_color(show),
                                );
                            }
                            if show != "free" {
                                all.push((*s, *e, show.clone()));
                            }
                        }
                    }
                }
            }
            // "All Attendees": combined busy times.
            let r = row_rect(0);
            p_.text(
                egui::pos2(r.left() + 6.0, r.center().y),
                egui::Align2::LEFT_CENTER,
                "All Attendees",
                super::chrome::bold_font(12.5),
                t.text,
            );
            p_.line_segment(
                [r.left_bottom(), r.right_bottom()],
                egui::Stroke::new(1.0_f32, t.border),
            );
            for (s, e, show) in &all {
                let a = x_of(*s).max(x0);
                let b = x_of(*e).min(r.right());
                if b > a {
                    p_.rect_filled(
                        egui::Rect::from_x_y_ranges(a..=b, r.y_range()).shrink2(egui::vec2(0.0, 4.0)),
                        0,
                        busy_color(show),
                    );
                }
            }
            // The meeting: light band with green start and red end line.
            if start.date() == day {
                let (a, b) = (x_of(start), x_of(end));
                let ys = rect.top() + ROW_H * 2.0..=rect.bottom();
                p_.rect_filled(
                    egui::Rect::from_x_y_ranges(a..=b, ys.clone()),
                    0,
                    t.accent.gamma_multiply(0.12),
                );
                p_.line_segment(
                    [egui::pos2(a, *ys.start()), egui::pos2(a, *ys.end())],
                    egui::Stroke::new(2.0_f32, Color32::from_rgb(16, 124, 16)),
                );
                p_.line_segment(
                    [egui::pos2(b, *ys.start()), egui::pos2(b, *ys.end())],
                    egui::Stroke::new(2.0_f32, Color32::from_rgb(196, 49, 75)),
                );
            }
            // Click in the grid moves the meeting there (same length).
            if let Some(pos) = resp.interact_pointer_pos().filter(|_| resp.clicked()) {
                if pos.x >= x0 && pos.y > rect.top() + ROW_H * 2.0 {
                    let slot = ((pos.x - x0) / SLOT_W).floor() as i64;
                    let s = first + Duration::minutes(30 * slot);
                    set_times(&mut m, s, s + dur);
                }
            }
            if p.data.is_none() {
                ui.label(RichText::new("Retrieving free/busy information…").size(12.0).color(t.text_weak));
            } else if let Some(Err(e)) = &p.data {
                ui.label(RichText::new(format!("Free/busy information not available: {e}")).size(12.0).color(t.danger));
            } else if unknown {
                ui.label(
                    RichText::new("Free/busy information is not available for some attendees (only within Microsoft 365).")
                        .size(12.0)
                        .color(t.text_weak),
                );
            }
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.add_sized([NAME_W - 10.0, 24.0], egui::Label::new("Add Attendees:"));
            let contacts = self.data.contacts.clone();
            let groups = self.data.groups.clone();
            super::compose::address_field(ui, "planner_add", &mut p.add, &contacts, &groups, 300.0);
            if ui.button("Add").clicked() && !p.add.trim().is_empty() {
                let add = super::compose::expand_groups(&p.add, &groups);
                let sep = if m.attendees.trim().is_empty() {
                    ""
                } else {
                    "; "
                };
                *m.attendees = format!("{}{sep}{add}", m.attendees.trim().trim_end_matches(';'));
                p.add.clear();
            }
        });
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.add_sized([NAME_W - 10.0, 24.0], egui::Label::new("Start time:"));
            ui.add(
                egui_extras::DatePickerButton::new(m.start_date)
                    .id_salt("pl_sd")
                    .format("%d/%m/%Y"),
            );
            super::calendar::time_combo(ui, "pl_st", m.start_time);
            ui.add_space(16.0);
            ui.label("End time:");
            ui.add(
                egui_extras::DatePickerButton::new(m.end_date)
                    .id_salt("pl_ed")
                    .format("%d/%m/%Y"),
            );
            super::calendar::time_combo(ui, "pl_et", m.end_time);
        });
        ui.add_space(4.0);
        let busy: Vec<(NaiveDateTime, NaiveDateTime)> = match &p.data {
            Some(Ok(list)) => list
                .iter()
                .filter_map(|f| f.busy.as_ref())
                .flatten()
                .filter(|b| b.show_as != "free")
                .map(|b| (local(&b.start), local(&b.end)))
                .collect(),
            _ => vec![],
        };
        ui.horizontal(|ui| {
            let mut pick = None;
            if ui
                .button("<<")
                .on_hover_text("Previous free time slot")
                .clicked()
            {
                pick = Some(false);
            }
            if ui
                .button("AutoPick >>")
                .on_hover_text("Next time slot when all attendees are free")
                .clicked()
            {
                pick = Some(true);
            }
            if let Some(forward) = pick {
                match auto_pick(&busy, start, dur, work, forward, Duration::days(DAYS - 1)) {
                    Some(s) => set_times(&mut m, s, s + dur),
                    None => self.toast("No free time slot found for all attendees.", true),
                }
            }
            ui.add_space(16.0);
            for (label, c, hatched) in [
                ("Busy", busy_color("busy"), false),
                ("Tentative", busy_color("tentative"), false),
                ("Out of Office", busy_color("oof"), false),
                ("Working Elsewhere", busy_color("workingElsewhere"), false),
                ("No Information", t.text_weak.gamma_multiply(0.5), true),
            ] {
                let (r, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), Sense::hover());
                if hatched {
                    ui.painter().rect_stroke(
                        r,
                        0,
                        egui::Stroke::new(1.0_f32, c),
                        egui::StrokeKind::Inside,
                    );
                    ui.painter().line_segment(
                        [r.left_bottom(), r.right_top()],
                        egui::Stroke::new(1.0_f32, c),
                    );
                } else {
                    ui.painter().rect_filled(r, 0, c);
                }
                ui.label(RichText::new(label).size(12.0));
                ui.add_space(6.0);
            }
        });
    }
}

fn set_times(m: &mut Meeting, s: NaiveDateTime, e: NaiveDateTime) {
    *m.start_date = s.date();
    *m.start_time = s.time();
    *m.end_date = e.date();
    *m.end_time = e.time();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dt(d: u32, h: u32, min: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 11, d)
            .unwrap()
            .and_hms_opt(h, min, 0)
            .unwrap()
    }

    #[test]
    fn lists_organizer_and_attendees_once() {
        assert_eq!(
            people("me@x.de", "Anna <anna@x.de>; bob@x.de, ME@x.de"),
            vec!["me@x.de", "anna@x.de", "bob@x.de"]
        );
    }

    #[test]
    fn auto_pick_skips_busy_times_evenings_and_weekends() {
        // Monday 2/11/2026; busy 9:00-10:30 and 11:00-17:00.
        let busy = vec![(dt(2, 9, 0), dt(2, 10, 30)), (dt(2, 11, 0), dt(2, 17, 0))];
        let hour = Duration::hours(1);
        let lim = Duration::days(10);
        // From 8:30 the next free hour is Tuesday 8:00 (10:30-11:00 is too short).
        assert_eq!(
            auto_pick(&busy, dt(2, 8, 30), hour, (8, 17), true, lim),
            Some(dt(3, 8, 0))
        );
        // Half an hour fits between the blocks.
        assert_eq!(
            auto_pick(
                &busy,
                dt(2, 8, 30),
                Duration::minutes(30),
                (8, 17),
                true,
                lim
            ),
            Some(dt(2, 10, 30))
        );
        // Friday 16:30 -> next Monday 8:00.
        assert_eq!(
            auto_pick(&[], dt(6, 16, 0), hour, (8, 17), true, lim),
            Some(dt(9, 8, 0))
        );
        // Backwards from Tuesday 8:00: Monday 8:00 is free (8-9).
        assert_eq!(
            auto_pick(&busy, dt(3, 8, 0), hour, (8, 17), false, lim),
            Some(dt(2, 8, 0))
        );
    }
}
