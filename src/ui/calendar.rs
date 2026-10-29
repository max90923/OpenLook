//! Calendar module: date navigator, day / work week / week / month views
//! and the appointment window ("Termin").

use std::collections::HashSet;

use chrono::{Datelike, Duration, NaiveDate, NaiveTime, Timelike};
use eframe::egui::{self, Color32, RichText, Sense};
use egui_phosphor::regular as ph;

use super::{
    chrome::{self, IC_BLUE, IC_GREEN, IC_GREY, IC_PURPLE, IC_RED},
    App, Win, CATEGORIES, MONTHS, WEEKDAYS, WEEKDAYS_LONG,
};
use crate::{
    db,
    models::*,
    providers,
    recur::{Freq, Recurrence},
};

const HOUR_H: f32 = 44.0;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CalView {
    Day,
    WorkWeek,
    Week,
    Month,
}

pub struct CalendarView {
    view: CalView,
    date: NaiveDate,
    nav_month: NaiveDate,
    events: Vec<Event>,
    hidden: HashSet<i64>,
    scrolled: bool,
}

/// Appointment being edited; dates/times kept in local time for the form.
pub struct EventEditor {
    ev: Event,
    meeting: bool,
    start_date: NaiveDate,
    start_time: NaiveTime,
    end_date: NaiveDate,
    end_time: NaiveTime,
    /// Series pattern being edited ("Serie" dialog).
    rec: Option<Recurrence>,
    show_series: bool,
}

impl EventEditor {
    /// Turn the appointment into a meeting with this attendee.
    pub fn set_attendee(&mut self, email: &str) {
        self.meeting = true;
        self.ev.attendees = email.to_string();
    }
}

impl CalendarView {
    pub fn new() -> Self {
        let t = super::today();
        Self {
            view: CalView::WorkWeek,
            date: t,
            nav_month: t,
            events: vec![],
            hidden: HashSet::new(),
            scrolled: false,
        }
    }

    pub fn count(&self) -> usize {
        self.events
            .iter()
            .filter(|e| !self.hidden.contains(&e.account_id))
            .count()
    }

    fn range(&self) -> (NaiveDate, NaiveDate) {
        let monday = self.date - Duration::days(self.date.weekday().num_days_from_monday() as i64);
        match self.view {
            CalView::Day => (self.date, self.date),
            CalView::WorkWeek => (monday, monday + Duration::days(4)),
            CalView::Week => (monday, monday + Duration::days(6)),
            CalView::Month => {
                let first = self.date.with_day(1).unwrap();
                let start = first - Duration::days(first.weekday().num_days_from_monday() as i64);
                (start, start + Duration::days(41))
            }
        }
    }

    fn step(&mut self, dir: i64) {
        self.date = match self.view {
            CalView::Day => self.date + Duration::days(dir),
            CalView::WorkWeek | CalView::Week => self.date + Duration::days(7 * dir),
            CalView::Month => add_months(self.date.with_day(1).unwrap(), dir),
        };
        self.nav_month = self.date;
    }

    fn visible(&self) -> impl Iterator<Item = &Event> {
        self.events
            .iter()
            .filter(|e| !self.hidden.contains(&e.account_id))
    }
}

fn add_months(d: NaiveDate, n: i64) -> NaiveDate {
    let total = d.year() as i64 * 12 + d.month0() as i64 + n;
    NaiveDate::from_ymd_opt((total / 12) as i32, (total % 12) as u32 + 1, 1).unwrap()
}

pub(super) fn local(e: &str) -> chrono::NaiveDateTime {
    super::parse_utc(e)
        .map(|d| d.naive_local())
        .unwrap_or_default()
}

impl App {
    pub fn reload_events(&mut self) {
        let (from, to) = self.cal.range();
        let from = super::local_to_utc_string(from.and_hms_opt(0, 0, 0).unwrap());
        let to = super::local_to_utc_string((to + Duration::days(1)).and_hms_opt(0, 0, 0).unwrap());
        self.cal.events = db::list_events(&self.st.db(), &from, &to).unwrap_or_default();
    }

    pub fn new_event_now(&mut self) {
        let h = (chrono::Local::now().hour() + 1).min(23);
        let start = if self.module == super::Module::Calendar {
            self.cal.date
        } else {
            super::today()
        }
        .and_hms_opt(h, 0, 0)
        .unwrap();
        self.new_event_at(start, false, false);
    }

    fn new_event_at(&mut self, start: chrono::NaiveDateTime, all_day: bool, meeting: bool) {
        let end = start + Duration::minutes(30);
        self.open_window(Win::Event(EventEditor {
            ev: Event {
                account_id: self.data.default_pim_account(),
                show_as: "busy".into(),
                reminder: 15,
                all_day,
                ..Default::default()
            },
            meeting,
            rec: None,
            show_series: false,
            start_date: start.date(),
            start_time: start.time(),
            end_date: if all_day { start.date() } else { end.date() },
            end_time: end.time(),
        }));
    }

    pub(super) fn edit_event(&mut self, e: &Event) {
        // An occurrence of a local series opens the whole series, like Outlook's "Serie öffnen".
        let master = (!e.recurrence.is_empty())
            .then(|| db::get_event(&self.st.db(), e.id).ok().flatten())
            .flatten();
        let e = master.as_ref().unwrap_or(e);
        let (s, en) = (local(&e.start), local(&e.end));
        // All-day events end at midnight of the following day; show the last day.
        let end_date = if e.all_day {
            (en - Duration::seconds(1)).date()
        } else {
            en.date()
        };
        self.open_window(Win::Event(EventEditor {
            ev: e.clone(),
            meeting: !e.attendees.is_empty(),
            rec: Recurrence::parse(&e.recurrence),
            show_series: false,
            start_date: s.date(),
            start_time: s.time(),
            end_date,
            end_time: en.time(),
        }));
    }

    // ------------------------------------------------------------ ribbon & navigation

    pub fn calendar_ribbon(&mut self, ui: &mut egui::Ui) {
        if self.ribbon_tab != 0 {
            self.common_ribbon(ui);
            return;
        }
        let t = self.t;
        let mut reload = false;
        chrome::group(ui, &t, "Neu", |ui| {
            if chrome::large(ui, &t, ph::CALENDAR_PLUS, IC_BLUE, "Neuer\nTermin", true).clicked() {
                self.new_event_now();
            }
            if chrome::large(ui, &t, ph::USERS_THREE, IC_BLUE, "Neue\nBesprechung", true).clicked()
            {
                let h = (chrono::Local::now().hour() + 1).min(23);
                self.new_event_at(self.cal.date.and_hms_opt(h, 0, 0).unwrap(), false, true);
            }
            let r = chrome::large(
                ui,
                &t,
                ph::FILE_PLUS,
                chrome::IC_YELLOW,
                "Neue\nElemente \u{E136}",
                true,
            );
            chrome::menu(&r, |ui| {
                if chrome::menu_item(ui, ph::ENVELOPE_SIMPLE, IC_BLUE, "E-Mail-Nachricht") {
                    self.open_composer(super::compose::ComposeKind::New);
                }
                if chrome::menu_item(ui, ph::CALENDAR_BLANK, IC_BLUE, "Ganztägiges Ereignis") {
                    self.new_event_at(self.cal.date.and_hms_opt(0, 0, 0).unwrap(), true, false);
                }
                if chrome::menu_item(ui, ph::USER, IC_BLUE, "Kontakt") {
                    self.new_contact();
                }
                if chrome::menu_item(ui, ph::CHECK_SQUARE_OFFSET, IC_GREEN, "Aufgabe") {
                    self.new_task();
                }
            });
        });
        chrome::group(ui, &t, "Gehe zu", |ui| {
            if chrome::large(ui, &t, ph::CALENDAR_CHECK, IC_BLUE, "Heute", true).clicked() {
                self.cal.date = super::today();
                self.cal.nav_month = self.cal.date;
                if self.cal.view == CalView::Month {
                    self.cal.view = CalView::Day;
                }
                reload = true;
            }
            if chrome::large(ui, &t, ph::CALENDAR_DOTS, IC_BLUE, "Nächste\n7 Tage", true).clicked()
            {
                self.cal.date = super::today();
                self.cal.view = CalView::Week;
                reload = true;
            }
        });
        chrome::group(ui, &t, "Anordnen", |ui| {
            for (v, icon, label) in [
                (CalView::Day, ph::SQUARE, "Tag"),
                (CalView::WorkWeek, ph::COLUMNS, "Arbeits-\nwoche"),
                (CalView::Week, ph::SQUARES_FOUR, "Woche"),
                (CalView::Month, ph::CALENDAR_BLANK, "Monat"),
            ] {
                let selected = self.cal.view == v;
                let r = chrome::large(
                    ui,
                    &t,
                    icon,
                    if selected { IC_BLUE } else { IC_GREY },
                    label,
                    true,
                );
                if selected {
                    ui.painter().rect_stroke(
                        r.rect,
                        0,
                        egui::Stroke::new(1.0_f32, t.button_down),
                        egui::StrokeKind::Inside,
                    );
                }
                if r.clicked() {
                    self.cal.view = v;
                    self.cal.scrolled = false;
                    reload = true;
                }
            }
        });
        if reload {
            self.reload_events();
        }
    }

    pub fn calendar_nav(&mut self, ui: &mut egui::Ui) {
        let mut reload = false;
        egui::ScrollArea::vertical()
            .id_salt("cal_nav")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for i in 0..2 {
                    if self.date_navigator(ui, i) {
                        reload = true;
                    }
                    ui.add_space(8.0);
                }
                ui.add_space(6.0);
                ui.label(RichText::new(format!("{}  Meine Kalender", ph::CARET_DOWN)).strong());
                for a in self.data.accounts.clone() {
                    let mut on = !self.cal.hidden.contains(&a.id);
                    let name = match a.kind {
                        AccountKind::Local => "Kalender".to_string(),
                        AccountKind::Imap => {
                            format!("Kalender (Nur dieser Computer) – {}", a.email)
                        }
                        AccountKind::Graph => format!("Kalender – {}", a.email),
                    };
                    ui.horizontal(|ui| {
                        ui.add_space(14.0);
                        if ui.checkbox(&mut on, name).changed() {
                            if on {
                                self.cal.hidden.remove(&a.id);
                            } else {
                                self.cal.hidden.insert(a.id);
                            }
                        }
                    });
                }
            });
        if reload {
            self.reload_events();
        }
    }

    /// Outlook's date navigator; `offset` months after the navigator month.
    fn date_navigator(&mut self, ui: &mut egui::Ui, offset: i64) -> bool {
        let t = self.t;
        let mut changed = false;
        let m = add_months(self.cal.nav_month.with_day(1).unwrap(), offset);
        let w = ui.available_width().min(230.0);
        let cell = (w - 8.0) / 7.0;
        ui.horizontal(|ui| {
            ui.set_width(w);
            if offset == 0
                && ui
                    .add(egui::Button::new(ph::CARET_LEFT).frame(false))
                    .clicked()
            {
                self.cal.nav_month = add_months(self.cal.nav_month, -1);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if offset == 1
                    && ui
                        .add(egui::Button::new(ph::CARET_RIGHT).frame(false))
                        .clicked()
                {
                    self.cal.nav_month = add_months(self.cal.nav_month, 1);
                }
                ui.centered_and_justified(|ui| {
                    ui.label(
                        RichText::new(format!("{} {}", MONTHS[m.month0() as usize], m.year()))
                            .strong(),
                    );
                });
            });
        });
        let (rect, _) =
            ui.allocate_exact_size(egui::vec2(w, cell * 0.8 + 6.0 * cell * 0.8), Sense::hover());
        let ch = cell * 0.8;
        let p = ui.painter();
        for (i, d) in WEEKDAYS.iter().enumerate() {
            p.text(
                egui::pos2(
                    rect.left() + 4.0 + cell * (i as f32 + 0.5),
                    rect.top() + ch / 2.0,
                ),
                egui::Align2::CENTER_CENTER,
                &d[..1],
                egui::FontId::proportional(11.5),
                t.text_weak,
            );
        }
        let start = m - Duration::days(m.weekday().num_days_from_monday() as i64);
        let (sa, sb) = self.cal.range();
        for i in 0..42 {
            let day = start + Duration::days(i);
            let r = egui::Rect::from_min_size(
                egui::pos2(
                    rect.left() + 4.0 + cell * (i % 7) as f32,
                    rect.top() + ch * (1 + i / 7) as f32,
                ),
                egui::vec2(cell, ch),
            );
            let resp = ui.interact(r, ui.id().with(("nav", offset, i)), Sense::click());
            let in_sel = self.cal.view != CalView::Month
                && day >= sa
                && day <= sb
                && day.month() == m.month();
            if in_sel {
                ui.painter().rect_filled(r, 0, t.selected);
            } else if resp.hovered() {
                ui.painter().rect_filled(r, 0, t.hover);
            }
            if day == super::today() {
                ui.painter().rect_stroke(
                    r.shrink(1.0),
                    0,
                    egui::Stroke::new(1.5_f32, t.today),
                    egui::StrokeKind::Inside,
                );
            }
            let color = if day.month() != m.month() {
                t.text_weak.gamma_multiply(0.6)
            } else {
                t.text
            };
            let has_events = self.cal.visible().any(|e| local(&e.start).date() == day);
            let font = egui::FontId::proportional(12.0);
            ui.painter().text(
                r.center(),
                egui::Align2::CENTER_CENTER,
                day.day().to_string(),
                font.clone(),
                color,
            );
            if has_events && day.month() == m.month() {
                ui.painter().text(
                    r.center() + egui::vec2(0.4, 0.0),
                    egui::Align2::CENTER_CENTER,
                    day.day().to_string(),
                    font,
                    color,
                );
            }
            if resp.clicked() {
                self.cal.date = day;
                if self.cal.view == CalView::Month {
                    self.cal.view = CalView::Day;
                }
                changed = true;
            }
        }
        changed
    }

    // ------------------------------------------------------------ views

    pub fn calendar_ui(&mut self, ui: &mut egui::Ui) {
        let mut reload = false;
        egui::Frame::new()
            .inner_margin(egui::Margin {
                left: 10,
                right: 10,
                top: 8,
                bottom: 4,
            })
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if ui
                        .add(
                            egui::Button::new(RichText::new(ph::CARET_LEFT).size(16.0))
                                .frame(false),
                        )
                        .clicked()
                    {
                        self.cal.step(-1);
                        reload = true;
                    }
                    if ui
                        .add(
                            egui::Button::new(RichText::new(ph::CARET_RIGHT).size(16.0))
                                .frame(false),
                        )
                        .clicked()
                    {
                        self.cal.step(1);
                        reload = true;
                    }
                    let (a, b) = self.cal.range();
                    let title = match self.cal.view {
                        CalView::Day => {
                            format!("{}. {} {}", a.day(), MONTHS[a.month0() as usize], a.year())
                        }
                        CalView::Month => format!(
                            "{} {}",
                            MONTHS[self.cal.date.month0() as usize],
                            self.cal.date.year()
                        ),
                        _ if a.month() == b.month() => format!(
                            "{}. - {}. {} {}",
                            a.day(),
                            b.day(),
                            MONTHS[a.month0() as usize],
                            a.year()
                        ),
                        _ => format!(
                            "{}. {} - {}. {} {}",
                            a.day(),
                            MONTHS[a.month0() as usize],
                            b.day(),
                            MONTHS[b.month0() as usize],
                            b.year()
                        ),
                    };
                    ui.label(RichText::new(title).size(20.0));
                });
            });
        egui::Frame::new()
            .inner_margin(egui::Margin {
                left: 4,
                right: 4,
                top: 0,
                bottom: 4,
            })
            .show(ui, |ui| {
                if self.cal.view == CalView::Month {
                    self.month_grid(ui);
                } else {
                    self.time_grid(ui);
                }
            });
        if reload {
            self.reload_events();
        }
    }

    fn time_grid(&mut self, ui: &mut egui::Ui) {
        let t = self.t;
        let (a, b) = self.cal.range();
        let days: Vec<NaiveDate> = (0..=(b - a).num_days())
            .map(|i| a + Duration::days(i))
            .collect();
        let gutter = 52.0;
        let col_w = ((ui.available_width() - gutter - 14.0) / days.len() as f32).max(60.0);
        let mut open: Option<Event> = None;
        let mut create: Option<(chrono::NaiveDateTime, bool)> = None;
        let events: Vec<Event> = self.cal.visible().cloned().collect();
        let stroke = egui::Stroke::new(1.0_f32, t.border);

        // Day headers + all-day row.
        let header_h = 40.0;
        let allday: Vec<&Event> = events.iter().filter(|e| e.all_day).collect();
        let spans =
            |e: &Event, d: NaiveDate| local(&e.start).date() <= d && local(&e.end).date() > d;
        let rows = days
            .iter()
            .map(|d| allday.iter().filter(|e| spans(e, *d)).count())
            .max()
            .unwrap_or(0);
        let allday_h = 22.0 * rows.max(1) as f32 + 4.0;
        let (head, _) = ui.allocate_exact_size(
            egui::vec2(gutter + col_w * days.len() as f32, header_h + allday_h),
            Sense::hover(),
        );
        for (i, d) in days.iter().enumerate() {
            let x = head.left() + gutter + col_w * i as f32;
            let cell =
                egui::Rect::from_min_size(egui::pos2(x, head.top()), egui::vec2(col_w, header_h));
            let is_today = *d == super::today();
            if is_today {
                ui.painter().rect_filled(cell, 0, t.selected);
                ui.painter().hline(
                    cell.x_range(),
                    cell.top() + 1.0,
                    egui::Stroke::new(3.0_f32, t.today),
                );
            }
            let p = ui.painter();
            p.text(
                cell.left_top() + egui::vec2(8.0, 4.0),
                egui::Align2::LEFT_TOP,
                d.day().to_string(),
                egui::FontId::proportional(18.0),
                if is_today { t.today } else { t.text },
            );
            p.text(
                cell.left_top() + egui::vec2(36.0, 9.0),
                egui::Align2::LEFT_TOP,
                WEEKDAYS_LONG[d.weekday().num_days_from_monday() as usize],
                egui::FontId::proportional(12.5),
                t.text_weak,
            );
            p.line_segment([cell.left_top(), egui::pos2(x, head.bottom())], stroke);
            let ad = egui::Rect::from_min_size(
                egui::pos2(x, head.top() + header_h),
                egui::vec2(col_w, allday_h),
            );
            let r = ui.interact(ad, ui.id().with(("allday", i)), Sense::click());
            if r.double_clicked() {
                create = Some((d.and_hms_opt(0, 0, 0).unwrap(), true));
            }
            for (row, e) in allday.iter().filter(|e| spans(e, *d)).enumerate() {
                let er = egui::Rect::from_min_size(
                    ad.min + egui::vec2(2.0, 2.0 + 22.0 * row as f32),
                    egui::vec2(col_w - 4.0, 20.0),
                );
                if paint_event(ui, &t, er, e, ("ad", i)) {
                    open = Some((*e).clone());
                }
            }
        }
        ui.painter()
            .hline(head.x_range(), head.top() + header_h, stroke);
        ui.painter().hline(head.x_range(), head.bottom(), stroke);

        let mut scroll = egui::ScrollArea::vertical()
            .id_salt("cal_scroll")
            .auto_shrink([false, false]);
        if !self.cal.scrolled {
            scroll = scroll
                .vertical_scroll_offset(HOUR_H * self.prefs.work_start.saturating_sub(1) as f32);
            self.cal.scrolled = true;
        }
        let (ws, we) = (self.prefs.work_start as f32, self.prefs.work_end as f32);
        scroll.show(ui, |ui| {
            let (grid, _) = ui.allocate_exact_size(
                egui::vec2(gutter + col_w * days.len() as f32, HOUR_H * 24.0),
                Sense::hover(),
            );
            // Working hours are lighter, like Outlook.
            for (i, d) in days.iter().enumerate() {
                let x = grid.left() + gutter + col_w * i as f32;
                let work_day = d.weekday().num_days_from_monday() < 5;
                let col = egui::Rect::from_min_size(
                    egui::pos2(x, grid.top()),
                    egui::vec2(col_w, grid.height()),
                );
                ui.painter().rect_filled(col, 0, t.pane_alt);
                if work_day {
                    let wr = egui::Rect::from_x_y_ranges(
                        col.x_range(),
                        (grid.top() + ws * HOUR_H)..=(grid.top() + we * HOUR_H),
                    );
                    ui.painter().rect_filled(wr, 0, t.pane);
                }
            }
            for h in 0..24 {
                let y = grid.top() + h as f32 * HOUR_H;
                let p = ui.painter();
                p.text(
                    egui::pos2(grid.left() + gutter - 20.0, y + 2.0),
                    egui::Align2::RIGHT_TOP,
                    format!("{h:02}"),
                    egui::FontId::proportional(15.0),
                    t.text_weak,
                );
                p.text(
                    egui::pos2(grid.left() + gutter - 19.0, y + 3.0),
                    egui::Align2::LEFT_TOP,
                    "00",
                    egui::FontId::proportional(9.0),
                    t.text_weak,
                );
                p.hline((grid.left() + gutter)..=grid.right(), y, stroke);
                let half = egui::Stroke::new(1.0_f32, t.border.gamma_multiply(0.45));
                p.hline(
                    (grid.left() + gutter)..=grid.right(),
                    y + HOUR_H / 2.0,
                    half,
                );
            }
            for (i, d) in days.iter().enumerate() {
                let x = grid.left() + gutter + col_w * i as f32;
                let col = egui::Rect::from_min_size(
                    egui::pos2(x, grid.top()),
                    egui::vec2(col_w, grid.height()),
                );
                ui.painter().vline(x, col.y_range(), stroke);
                let r = ui.interact(col, ui.id().with(("col", i)), Sense::click());
                let slot_at = |pos: egui::Pos2| ((pos.y - grid.top()) / (HOUR_H / 2.0)).floor();
                if r.hovered() {
                    if let Some(pos) = r.hover_pos() {
                        let s = slot_at(pos);
                        let sr = egui::Rect::from_min_size(
                            egui::pos2(x + 1.0, grid.top() + s * HOUR_H / 2.0),
                            egui::vec2(col_w - 1.0, HOUR_H / 2.0),
                        );
                        ui.painter()
                            .rect_filled(sr, 0, t.selected.gamma_multiply(0.6));
                    }
                }
                if r.double_clicked() {
                    if let Some(pos) = r.interact_pointer_pos() {
                        create = Some((
                            d.and_hms_opt(0, 0, 0).unwrap()
                                + Duration::minutes(slot_at(pos) as i64 * 30),
                            false,
                        ));
                    }
                }

                // Lay out overlapping appointments side by side.
                let day_start = d.and_hms_opt(0, 0, 0).unwrap();
                let mut evs: Vec<&Event> = events
                    .iter()
                    .filter(|e| {
                        !e.all_day && local(&e.start).date() <= *d && local(&e.end) > day_start
                    })
                    .collect();
                evs.sort_by(|a, b| a.start.cmp(&b.start));
                let mut lanes: Vec<chrono::NaiveDateTime> = vec![];
                let mut placed = vec![];
                for e in &evs {
                    let s = local(&e.start);
                    let lane = match lanes.iter().position(|end| *end <= s) {
                        Some(l) => l,
                        None => {
                            lanes.push(s);
                            lanes.len() - 1
                        }
                    };
                    lanes[lane] = local(&e.end);
                    placed.push((lane, *e));
                }
                let n = lanes.len().max(1) as f32;
                for (lane, e) in placed {
                    let s = (local(&e.start) - day_start).num_minutes().max(0) as f32;
                    let en = (local(&e.end) - day_start).num_minutes().min(24 * 60) as f32;
                    let lane_w = (col_w - 8.0) / n;
                    let er = egui::Rect::from_min_size(
                        egui::pos2(
                            x + 2.0 + lane as f32 * lane_w,
                            grid.top() + s / 60.0 * HOUR_H + 1.0,
                        ),
                        egui::vec2(lane_w - 1.0, ((en - s) / 60.0 * HOUR_H - 2.0).max(18.0)),
                    );
                    if paint_event(ui, &t, er, e, ("ev", i)) {
                        open = Some(e.clone());
                    }
                }
            }
            // Current time line.
            let now = chrono::Local::now().naive_local();
            if let Some(i) = days.iter().position(|d| *d == now.date()) {
                let y = grid.top() + (now.hour() * 60 + now.minute()) as f32 / 60.0 * HOUR_H;
                let x = grid.left() + gutter + col_w * i as f32;
                ui.painter()
                    .hline(x..=(x + col_w), y, egui::Stroke::new(2.0_f32, t.today));
                ui.painter().circle_filled(egui::pos2(x, y), 4.0, t.today);
            }
        });
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_secs(60));

        if let Some(e) = open {
            self.edit_event(&e);
        }
        if let Some((start, all_day)) = create {
            self.new_event_at(start, all_day, false);
        }
    }

    fn month_grid(&mut self, ui: &mut egui::Ui) {
        let t = self.t;
        let (start, _) = self.cal.range();
        let w = (ui.available_width() - 4.0) / 7.0;
        let (head, _) = ui.allocate_exact_size(egui::vec2(w * 7.0, 22.0), Sense::hover());
        for (i, d) in WEEKDAYS_LONG.iter().enumerate() {
            ui.painter().text(
                head.left_center() + egui::vec2(w * i as f32 + 6.0, 0.0),
                egui::Align2::LEFT_CENTER,
                *d,
                egui::FontId::proportional(12.5),
                t.text_weak,
            );
        }
        let h = ((ui.available_height() - 4.0) / 6.0).max(70.0);
        let (grid, _) = ui.allocate_exact_size(egui::vec2(w * 7.0, h * 6.0), Sense::hover());
        let stroke = egui::Stroke::new(1.0_f32, t.border);
        let events: Vec<Event> = self.cal.visible().cloned().collect();
        let mut open = None;
        let mut create = None;
        let mut goto = None;
        for i in 0..42 {
            let day = start + Duration::days(i);
            let cell = egui::Rect::from_min_size(
                grid.min + egui::vec2((i % 7) as f32 * w, (i / 7) as f32 * h),
                egui::vec2(w, h),
            );
            let r = ui.interact(cell, ui.id().with(("mcell", i)), Sense::click());
            let p = ui.painter();
            if day.month() != self.cal.date.month() {
                p.rect_filled(cell, 0, t.pane_alt);
            } else if r.hovered() {
                p.rect_filled(cell, 0, t.hover);
            }
            p.rect_stroke(cell, 0, stroke, egui::StrokeKind::Inside);
            let num = if day.day() == 1 {
                format!("{}. {}", day.day(), &MONTHS[day.month0() as usize][..3])
            } else {
                day.day().to_string()
            };
            if day == super::today() {
                let band = egui::Rect::from_min_size(cell.min, egui::vec2(w, 20.0));
                p.rect_filled(band, 0, t.today);
                p.text(
                    cell.min + egui::vec2(6.0, 3.0),
                    egui::Align2::LEFT_TOP,
                    num,
                    egui::FontId::proportional(12.5),
                    Color32::WHITE,
                );
            } else {
                p.text(
                    cell.min + egui::vec2(6.0, 3.0),
                    egui::Align2::LEFT_TOP,
                    num,
                    egui::FontId::proportional(12.5),
                    t.text,
                );
            }
            if r.double_clicked() {
                create = Some(day);
            }
            let evs: Vec<&Event> = events
                .iter()
                .filter(|e| {
                    let (s, en) = (local(&e.start), local(&e.end));
                    s.date() <= day && (en.date() > day || en.date() == day && !e.all_day)
                })
                .collect();
            let max = ((h - 26.0) / 19.0).floor().max(0.0) as usize;
            let shown = if evs.len() > max {
                max.saturating_sub(1)
            } else {
                evs.len()
            };
            for (k, e) in evs.iter().take(shown).enumerate() {
                let er = egui::Rect::from_min_size(
                    cell.min + egui::vec2(3.0, 23.0 + k as f32 * 19.0),
                    egui::vec2(w - 6.0, 17.0),
                );
                let mut ev = (*e).clone();
                if !e.all_day {
                    ev.title = format!("{} {}", local(&e.start).format("%H:%M"), e.title);
                }
                if paint_event(ui, &t, er, &ev, ("m", i)) {
                    open = Some((*e).clone());
                }
            }
            if evs.len() > shown {
                let more = ui.put(
                    egui::Rect::from_min_size(
                        cell.min + egui::vec2(3.0, 23.0 + shown as f32 * 19.0),
                        egui::vec2(w - 6.0, 17.0),
                    ),
                    egui::Button::new(
                        RichText::new(format!("{} weitere …", evs.len() - shown)).size(11.5),
                    )
                    .frame(false),
                );
                if more.clicked() {
                    goto = Some(day);
                }
            }
        }
        if let Some(e) = open {
            self.edit_event(&e);
        }
        if let Some(d) = create {
            self.new_event_at(d.and_hms_opt(9, 0, 0).unwrap(), false, false);
        }
        if let Some(d) = goto {
            self.cal.date = d;
            self.cal.view = CalView::Day;
            self.reload_events();
        }
    }

    // ------------------------------------------------------------ appointment window

    pub(super) fn event_window(
        &mut self,
        ctx: &egui::Context,
        vid: egui::ViewportId,
        ed: &mut EventEditor,
    ) -> bool {
        let t = self.t;
        let kind = if ed.meeting { "Besprechung" } else { "Termin" };
        let title = format!(
            "{} - {kind}",
            if ed.ev.title.trim().is_empty() {
                "Unbenannt"
            } else {
                ed.ev.title.trim()
            }
        );
        let accounts = self.data.accounts.clone();
        let is_new = ed.ev.id == 0;
        let mut save = false;
        let mut delete = false;
        let mut invite = false;
        let mut answer: Option<&'static str> = None;
        let invited = !ed.ev.response.is_empty() && ed.ev.response != "organizer";
        let closed = chrome::native_window(ctx, vid, &title, [820.0, 640.0], &t, |ui| {
            chrome::item_ribbon(ui, &t, kind, |ui| {
                if invited {
                    chrome::group(ui, &t, "Antworten", |ui| {
                        if chrome::large(ui, &t, ph::CHECK, IC_GREEN, "Annehmen", true).clicked() {
                            answer = Some("accept");
                        }
                        if chrome::large(ui, &t, ph::QUESTION, IC_PURPLE, "Mit\nVorbehalt", true)
                            .clicked()
                        {
                            answer = Some("tentativelyAccept");
                        }
                        if chrome::large(ui, &t, ph::X, IC_RED, "Ablehnen", true).clicked() {
                            answer = Some("decline");
                        }
                    });
                }
                chrome::group(ui, &t, "Aktionen", |ui| {
                    if chrome::large(
                        ui,
                        &t,
                        ph::FLOPPY_DISK,
                        IC_PURPLE,
                        "Speichern &\nschließen",
                        true,
                    )
                    .clicked()
                    {
                        save = true;
                    }
                    if chrome::large(ui, &t, ph::X, IC_RED, "Löschen", !is_new).clicked() {
                        delete = true;
                    }
                });
                chrome::group(ui, &t, "Teilnehmer", |ui| {
                    if chrome::large(
                        ui,
                        &t,
                        ph::USERS_THREE,
                        IC_BLUE,
                        if ed.meeting {
                            "Einladung\nabbrechen"
                        } else {
                            "Teilnehmer\neinladen"
                        },
                        true,
                    )
                    .clicked()
                    {
                        invite = true;
                    }
                });
                chrome::group(ui, &t, "Serie", |ui| {
                    let label = if ed.rec.is_some() {
                        "Serie \u{E182}"
                    } else {
                        "Serie"
                    };
                    if chrome::large(ui, &t, ph::REPEAT, IC_BLUE, label, true).clicked() {
                        ed.show_series = !ed.show_series;
                        if ed.rec.is_none() {
                            let wd = 1u8 << ed.start_date.weekday().num_days_from_monday();
                            ed.rec = Some(Recurrence {
                                freq: Freq::Weekly,
                                interval: 1,
                                weekdays: wd,
                                until: None,
                            });
                        }
                    }
                });
                chrome::group(ui, &t, "Optionen", |ui| {
                    chrome::column(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(format!("{}  Anzeigen als:", ph::CLOCK)).size(12.0),
                            );
                            egui::ComboBox::from_id_salt("ev_show")
                                .width(110.0)
                                .selected_text(show_as_label(&ed.ev.show_as))
                                .show_ui(ui, |ui| {
                                    for s in ["free", "tentative", "busy", "oof"] {
                                        ui.selectable_value(
                                            &mut ed.ev.show_as,
                                            s.into(),
                                            show_as_label(s),
                                        );
                                    }
                                });
                        });
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(format!("{}  Erinnerung:", ph::BELL)).size(12.0),
                            );
                            egui::ComboBox::from_id_salt("ev_rem")
                                .width(118.0)
                                .selected_text(reminder_label(ed.ev.reminder))
                                .show_ui(ui, |ui| {
                                    for m in [-1, 0, 5, 15, 30, 60, 120, 1440] {
                                        ui.selectable_value(
                                            &mut ed.ev.reminder,
                                            m,
                                            reminder_label(m),
                                        );
                                    }
                                });
                        });
                    });
                });
                chrome::group(ui, &t, "Kategorien", |ui| {
                    let r = chrome::large(
                        ui,
                        &t,
                        ph::TAG,
                        IC_PURPLE,
                        "Kategori-\nsieren \u{E136}",
                        true,
                    );
                    chrome::menu(&r, |ui| {
                        if ui
                            .radio(ed.ev.category.is_empty(), "Keine Kategorie")
                            .clicked()
                        {
                            ed.ev.category.clear();
                            ui.close();
                        }
                        for (c, col) in CATEGORIES {
                            if ui
                                .add(
                                    egui::Button::new((
                                        RichText::new("\u{E45E}")
                                            .family(egui::FontFamily::Name("fill".into()))
                                            .color(col)
                                            .size(16.0),
                                        c,
                                    ))
                                    .frame(false)
                                    .selected(ed.ev.category == c),
                                )
                                .clicked()
                            {
                                ed.ev.category = c.into();
                                ui.close();
                            }
                        }
                    });
                });
            });
            egui::Frame::new()
                .inner_margin(egui::Margin::symmetric(14, 10))
                .show(ui, |ui| {
                    let label_w = 90.0;
                    let row =
                        |ui: &mut egui::Ui, label: &str, add: &mut dyn FnMut(&mut egui::Ui)| {
                            ui.horizontal(|ui| {
                                ui.add_sized([label_w, 24.0], egui::Label::new(label));
                                add(ui);
                            });
                        };
                    if ed.meeting {
                        row(ui, "Erforderlich", &mut |ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut ed.ev.attendees)
                                    .hint_text("E-Mail-Adressen, mit ; getrennt")
                                    .desired_width(ui.available_width() - 4.0),
                            );
                        });
                    }
                    row(ui, "Betreff", &mut |ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut ed.ev.title)
                                .desired_width(ui.available_width() - 4.0),
                        );
                    });
                    row(ui, "Ort", &mut |ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut ed.ev.location)
                                .desired_width(ui.available_width() - 4.0),
                        );
                    });
                    ui.add_space(4.0);
                    row(ui, "Beginnt um", &mut |ui| {
                        ui.add(
                            egui_extras::DatePickerButton::new(&mut ed.start_date)
                                .id_salt("ev_sd")
                                .format("%d.%m.%Y"),
                        );
                        if !ed.ev.all_day {
                            time_combo(ui, "ev_st", &mut ed.start_time);
                        }
                        ui.checkbox(&mut ed.ev.all_day, "Ganztägiges Ereignis");
                    });
                    row(ui, "Endet um", &mut |ui| {
                        ui.add(
                            egui_extras::DatePickerButton::new(&mut ed.end_date)
                                .id_salt("ev_ed")
                                .format("%d.%m.%Y"),
                        );
                        if !ed.ev.all_day {
                            time_combo(ui, "ev_et", &mut ed.end_time);
                        }
                    });
                    if let Some(r) = &ed.rec {
                        if !ed.show_series {
                            row(ui, "Serie", &mut |ui| {
                                ui.label(RichText::new(format!("{}  {}", ph::REPEAT, r.describe())).color(t.text_weak));
                            });
                        }
                    }
                    if ed.show_series {
                        series_editor(ui, &t, ed);
                    }
                    if invited {
                        let who = if ed.ev.organizer.is_empty() { "Der Organisator".to_string() } else { ed.ev.organizer.clone() };
                        let state = match ed.ev.response.as_str() {
                            "accepted" => "Sie haben zugesagt.",
                            "tentativelyAccepted" => "Sie haben mit Vorbehalt zugesagt.",
                            "declined" => "Sie haben abgesagt.",
                            _ => "Bitte antworten Sie.",
                        };
                        egui::Frame::new().fill(t.pane_alt).inner_margin(egui::Margin::symmetric(8, 4)).show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.label(RichText::new(format!("{}  {who} hat Sie zu dieser Besprechung eingeladen. {state}", ph::INFO)).size(12.0));
                        });
                    }
                    if is_new {
                        row(ui, "Kalender", &mut |ui| {
                            egui::ComboBox::from_id_salt("ev_acc")
                                .selected_text(
                                    accounts
                                        .iter()
                                        .find(|a| a.id == ed.ev.account_id)
                                        .map(|a| a.email.clone())
                                        .unwrap_or_default(),
                                )
                                .show_ui(ui, |ui| {
                                    for a in &accounts {
                                        ui.selectable_value(&mut ed.ev.account_id, a.id, &a.email);
                                    }
                                });
                        });
                    }
                    ui.add_space(6.0);
                    ui.separator();
                    let (w, h) = (ui.available_width(), ui.available_height().max(120.0));
                    ui.add_sized(
                        [w, h - 4.0],
                        egui::TextEdit::multiline(&mut ed.ev.body).frame(false),
                    );
                });
        });
        if invite {
            ed.meeting = !ed.meeting;
            if !ed.meeting {
                ed.ev.attendees.clear();
            }
        }
        if let Some(a) = answer {
            let id = ed.ev.id;
            let msg = match a {
                "accept" => "Zusage gesendet.",
                "decline" => "Absage gesendet.",
                _ => "Zusage mit Vorbehalt gesendet.",
            };
            self.spawn(Some(msg), move |st, _| async move {
                providers::respond_event(&st, id, a).await
            });
            return true;
        }
        if save {
            let mut e = ed.ev.clone();
            e.recurrence = ed.rec.as_ref().map(|r| r.format()).unwrap_or_default();
            if e.title.trim().is_empty() {
                e.title = "(Kein Betreff)".into();
            }
            let (s, en) = if e.all_day {
                (
                    ed.start_date.and_hms_opt(0, 0, 0).unwrap(),
                    (ed.end_date.max(ed.start_date) + Duration::days(1))
                        .and_hms_opt(0, 0, 0)
                        .unwrap(),
                )
            } else {
                (
                    ed.start_date.and_time(ed.start_time),
                    ed.end_date.and_time(ed.end_time),
                )
            };
            if en < s {
                self.toast("Das eingegebene Enddatum liegt vor dem Anfangsdatum.", true);
                return false;
            }
            e.start = super::local_to_utc_string(s);
            e.end = super::local_to_utc_string(en);
            self.spawn(Some("Element gespeichert."), move |st, _| async move {
                providers::save_event(&st, e).await.map(|_| ())
            });
            return true;
        }
        if delete {
            let id = ed.ev.id;
            self.spawn(Some("Element gelöscht."), move |st, _| async move {
                providers::delete_event(&st, id).await
            });
            return true;
        }
        closed
    }
}

/// Outlook's "Terminserie" settings, shown inline in the appointment window.
fn series_editor(ui: &mut egui::Ui, t: &super::theme::Theme, ed: &mut EventEditor) {
    let start = ed.start_date;
    let mut remove = false;
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new("Serienmuster").strong());
        let Some(r) = &mut ed.rec else { return };
        ui.horizontal(|ui| {
            for (f, label) in [
                (Freq::Daily, "Täglich"),
                (Freq::Weekly, "Wöchentlich"),
                (Freq::Monthly, "Monatlich"),
                (Freq::Yearly, "Jährlich"),
            ] {
                if ui.radio(r.freq == f, label).clicked() {
                    r.freq = f;
                }
            }
        });
        ui.horizontal(|ui| {
            let unit = match r.freq {
                Freq::Daily => "Tag(e)".to_string(),
                Freq::Weekly => "Woche(n) am:".to_string(),
                Freq::Monthly => format!("Monat(e) am {}.", start.day()),
                Freq::Yearly => format!(
                    "Jahr(e) am {}. {}",
                    start.day(),
                    MONTHS[start.month0() as usize]
                ),
            };
            ui.label("Jede(n)");
            ui.add(egui::DragValue::new(&mut r.interval).range(1..=99));
            ui.label(unit);
        });
        if r.freq == Freq::Weekly {
            ui.horizontal(|ui| {
                for (i, d) in super::WEEKDAYS_LONG.iter().enumerate() {
                    let mut on = r.weekdays & (1 << i) != 0;
                    if ui.checkbox(&mut on, *d).changed() {
                        r.weekdays ^= 1 << i;
                    }
                }
            });
            if r.weekdays == 0 {
                r.weekdays = 1 << start.weekday().num_days_from_monday();
            }
        }
        ui.add_space(4.0);
        ui.label(RichText::new("Seriendauer").strong());
        ui.horizontal(|ui| {
            if ui.radio(r.until.is_none(), "Kein Enddatum").clicked() {
                r.until = None;
            }
            if ui.radio(r.until.is_some(), "Endet am:").clicked() && r.until.is_none() {
                r.until = Some(start + Duration::days(90));
            }
            if let Some(u) = &mut r.until {
                ui.add(
                    egui_extras::DatePickerButton::new(u)
                        .id_salt("rec_until")
                        .format("%d.%m.%Y"),
                );
            }
        });
        ui.label(RichText::new(r.describe()).color(t.text_weak));
        if ui.button(format!("{}  Serie entfernen", ph::X)).clicked() {
            remove = true;
        }
    });
    if remove {
        ed.rec = None;
        ed.show_series = false;
    }
}

/// Paint an appointment block; returns true when double-clicked (opens it).
fn paint_event(
    ui: &mut egui::Ui,
    t: &super::theme::Theme,
    rect: egui::Rect,
    e: &Event,
    salt: impl std::hash::Hash,
) -> bool {
    let color = if e.category.is_empty() {
        IC_BLUE
    } else {
        super::category_color(&e.category)
    };
    let resp = ui.interact(
        rect,
        ui.id().with(("event", e.id)).with(salt),
        Sense::click(),
    );
    let fill = if e.show_as == "free" {
        t.pane
    } else {
        color
            .gamma_multiply(if t.dark { 0.45 } else { 0.25 })
            .to_opaque_over(t.pane)
    };
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 0, fill);
    p.rect_filled(
        egui::Rect::from_min_size(rect.min, egui::vec2(5.0, rect.height())),
        0,
        if e.show_as == "tentative" {
            t.pane
        } else {
            color
        },
    );
    if e.show_as == "tentative" || e.show_as == "free" {
        p.rect_stroke(
            rect,
            0,
            egui::Stroke::new(1.0_f32, color),
            egui::StrokeKind::Inside,
        );
    }
    if resp.hovered() {
        p.rect_stroke(
            rect,
            0,
            egui::Stroke::new(1.0_f32, t.text_weak),
            egui::StrokeKind::Inside,
        );
    }
    let mut label = e.title.clone();
    if rect.height() >= 34.0 && !e.location.is_empty() {
        label.push_str(&format!("\n{}", e.location));
    }
    let g = p.layout(
        label,
        egui::FontId::proportional(12.0),
        t.text,
        rect.width() - 10.0,
    );
    p.galley(rect.min + egui::vec2(8.0, 2.0), g, t.text);
    let open = resp.double_clicked();
    resp.on_hover_text(format!(
        "{}\n{} – {}{}",
        e.title,
        local(&e.start).format("%d.%m. %H:%M"),
        local(&e.end).format("%d.%m. %H:%M"),
        if e.location.is_empty() {
            String::new()
        } else {
            format!("\n{}", e.location)
        }
    ));
    open
}

trait Opaque {
    fn to_opaque_over(self, bg: Color32) -> Color32;
}

impl Opaque for Color32 {
    /// Blend a translucent colour over `bg` so overlapping blocks stay readable.
    fn to_opaque_over(self, bg: Color32) -> Color32 {
        let a = self.a() as f32 / 255.0;
        let mix = |c: u8, b: u8| (c as f32 + b as f32 * (1.0 - a)).min(255.0) as u8;
        Color32::from_rgb(
            mix(self.r(), bg.r()),
            mix(self.g(), bg.g()),
            mix(self.b(), bg.b()),
        )
    }
}

fn time_combo(ui: &mut egui::Ui, id: &str, t: &mut NaiveTime) {
    egui::ComboBox::from_id_salt(id)
        .width(70.0)
        .selected_text(t.format("%H:%M").to_string())
        .height(300.0)
        .show_ui(ui, |ui| {
            for i in 0..48 {
                let s = NaiveTime::from_hms_opt(i / 2, (i % 2) * 30, 0).unwrap();
                ui.selectable_value(t, s, s.format("%H:%M").to_string());
            }
        });
}

fn show_as_label(s: &str) -> &'static str {
    match s {
        "free" => "Frei",
        "tentative" => "Mit Vorbehalt",
        "oof" => "Abwesend",
        _ => "Gebucht",
    }
}

pub(super) fn reminder_label(m: i64) -> String {
    match m {
        -1 => "Keine".into(),
        0 => "0 Minuten".into(),
        m if m < 60 => format!("{m} Minuten"),
        60 => "1 Stunde".into(),
        m if m < 1440 => format!("{} Stunden", m / 60),
        _ => "1 Tag".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn month_arithmetic() {
        let d = NaiveDate::from_ymd_opt(2026, 12, 15).unwrap();
        assert_eq!(
            add_months(d, 1),
            NaiveDate::from_ymd_opt(2027, 1, 1).unwrap()
        );
        assert_eq!(
            add_months(d, -12),
            NaiveDate::from_ymd_opt(2025, 12, 1).unwrap()
        );
    }

    #[test]
    fn work_week_range() {
        let mut v = CalendarView::new();
        v.date = NaiveDate::from_ymd_opt(2026, 10, 29).unwrap(); // Thursday
        assert_eq!(
            v.range(),
            (
                NaiveDate::from_ymd_opt(2026, 10, 26).unwrap(),
                NaiveDate::from_ymd_opt(2026, 10, 30).unwrap()
            )
        );
        v.view = CalView::Month;
        assert_eq!(v.range().0, NaiveDate::from_ymd_opt(2026, 9, 28).unwrap());
    }

    #[test]
    fn blends_colors() {
        let c = Color32::from_rgba_unmultiplied(0, 0, 255, 128).to_opaque_over(Color32::WHITE);
        assert!(c.r() > 100 && c.b() == 255);
    }
}
