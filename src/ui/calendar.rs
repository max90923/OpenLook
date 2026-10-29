//! Calendar module: day / work week / week / month views and the event editor.

use chrono::{Datelike, Duration, NaiveDate, NaiveTime, Timelike};
use eframe::egui::{self, Color32, RichText, Sense};

use super::{App, BRAND, CATEGORIES, DANGER, MONTHS, WEEKDAYS};
use crate::{db, models::*, providers};

const HOUR_H: f32 = 48.0;

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
    mini_month: NaiveDate,
    events: Vec<Event>,
    editor: Option<EventEditor>,
    scrolled: bool,
}

/// Event being edited; dates/times kept in local time for the form.
struct EventEditor {
    ev: Event,
    start_date: NaiveDate,
    start_time: NaiveTime,
    end_date: NaiveDate,
    end_time: NaiveTime,
}

impl CalendarView {
    pub fn new() -> Self {
        let t = super::today();
        Self {
            view: CalView::WorkWeek,
            date: t,
            mini_month: t,
            events: vec![],
            editor: None,
            scrolled: false,
        }
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
        self.mini_month = self.date;
    }
}

fn add_months(d: NaiveDate, n: i64) -> NaiveDate {
    let total = d.year() as i64 * 12 + d.month0() as i64 + n;
    NaiveDate::from_ymd_opt((total / 12) as i32, (total % 12) as u32 + 1, 1).unwrap()
}

fn local(e: &str) -> chrono::NaiveDateTime {
    super::parse_utc(e)
        .map(|d| d.naive_local())
        .unwrap_or_default()
}

fn time_slots() -> Vec<NaiveTime> {
    (0..48)
        .map(|i| NaiveTime::from_hms_opt(i / 2, (i % 2) * 30, 0).unwrap())
        .collect()
}

impl App {
    pub fn reload_events(&mut self) {
        let (from, to) = self.cal.range();
        let from = super::local_to_utc_string(from.and_hms_opt(0, 0, 0).unwrap());
        let to = super::local_to_utc_string((to + Duration::days(1)).and_hms_opt(0, 0, 0).unwrap());
        self.cal.events = db::list_events(&self.st.db(), &from, &to).unwrap_or_default();
    }

    fn new_event_at(&mut self, start: chrono::NaiveDateTime, all_day: bool) {
        let end = if all_day {
            start + Duration::days(1)
        } else {
            start + Duration::minutes(30)
        };
        self.cal.editor = Some(EventEditor {
            ev: Event {
                account_id: self.data.local_account_id(),
                show_as: "busy".into(),
                reminder: 15,
                all_day,
                ..Default::default()
            },
            start_date: start.date(),
            start_time: start.time(),
            end_date: if all_day { start.date() } else { end.date() },
            end_time: end.time(),
        });
    }

    fn edit_event(&mut self, e: &Event) {
        let (s, en) = (local(&e.start), local(&e.end));
        // All-day events end at midnight of the following day; show the last day.
        let end_date = if e.all_day {
            (en - Duration::seconds(1)).date()
        } else {
            en.date()
        };
        self.cal.editor = Some(EventEditor {
            ev: e.clone(),
            start_date: s.date(),
            start_time: s.time(),
            end_date,
            end_time: en.time(),
        });
    }

    pub fn calendar_ui(&mut self, ui: &mut egui::Ui) {
        let mut reload = false;
        egui::TopBottomPanel::top("cal_ribbon")
            .frame(super::pane_frame(ui).inner_margin(egui::Margin::symmetric(8, 6)))
            .show_inside(ui, |ui| {
                ui.horizontal(|ui| {
                    if super::primary_button(ui, "📅  Neues Ereignis").clicked() {
                        let start = super::today()
                            .and_hms_opt((chrono::Local::now().hour() + 1).min(23), 0, 0)
                            .unwrap();
                        self.new_event_at(start, false);
                    }
                    ui.separator();
                    for (v, label) in [
                        (CalView::Day, "Tag"),
                        (CalView::WorkWeek, "Arbeitswoche"),
                        (CalView::Week, "Woche"),
                        (CalView::Month, "Monat"),
                    ] {
                        if ui.selectable_label(self.cal.view == v, label).clicked() {
                            self.cal.view = v;
                            self.cal.scrolled = false;
                            reload = true;
                        }
                    }
                });
            });
        ui.add_space(8.0);

        egui::SidePanel::left("cal_side")
            .exact_width(250.0)
            .resizable(false)
            .frame(egui::Frame::new())
            .show_inside(ui, |ui| {
                if self.mini_calendar(ui) {
                    reload = true;
                }
                ui.add_space(16.0);
                ui.label(RichText::new("Meine Kalender").strong());
                for a in &self.data.accounts {
                    ui.label(format!(
                        "■ {}",
                        if a.kind == AccountKind::Local {
                            "Kalender (lokal)".to_string()
                        } else {
                            a.email.clone()
                        }
                    ));
                }
                ui.add_space(16.0);
                ui.label(RichText::new("Kategorien").strong());
                for (name, c) in CATEGORIES {
                    ui.label(RichText::new(format!("■ {name}")).color(c));
                }
            });

        egui::CentralPanel::default()
            .frame(super::pane_frame(ui).inner_margin(egui::Margin::same(10)))
            .show_inside(ui, |ui| {
                ui.horizontal(|ui| {
                    if ui.button("Heute").clicked() {
                        self.cal.date = super::today();
                        self.cal.mini_month = self.cal.date;
                        reload = true;
                    }
                    if ui.button("◀").clicked() {
                        self.cal.step(-1);
                        reload = true;
                    }
                    if ui.button("▶").clicked() {
                        self.cal.step(1);
                        reload = true;
                    }
                    let (a, b) = self.cal.range();
                    let title = match self.cal.view {
                        CalView::Day => format!(
                            "{}, {}. {} {}",
                            super::WEEKDAYS_LONG[a.weekday().num_days_from_monday() as usize],
                            a.day(),
                            MONTHS[a.month0() as usize],
                            a.year()
                        ),
                        CalView::Month => format!(
                            "{} {}",
                            MONTHS[self.cal.date.month0() as usize],
                            self.cal.date.year()
                        ),
                        _ if a.month() == b.month() => format!(
                            "{}.–{}. {} {}",
                            a.day(),
                            b.day(),
                            MONTHS[a.month0() as usize],
                            a.year()
                        ),
                        _ => format!(
                            "{}. {} – {}. {} {}",
                            a.day(),
                            MONTHS[a.month0() as usize],
                            b.day(),
                            MONTHS[b.month0() as usize],
                            b.year()
                        ),
                    };
                    ui.label(RichText::new(title).size(18.0).strong());
                });
                ui.add_space(6.0);
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

    /// Returns true when the selected date changed.
    fn mini_calendar(&mut self, ui: &mut egui::Ui) -> bool {
        let mut changed = false;
        let m = self.cal.mini_month.with_day(1).unwrap();
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!("{} {}", MONTHS[m.month0() as usize], m.year())).strong(),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("▶").clicked() {
                    self.cal.mini_month = add_months(m, 1);
                }
                if ui.small_button("◀").clicked() {
                    self.cal.mini_month = add_months(m, -1);
                }
            });
        });
        let start = m - Duration::days(m.weekday().num_days_from_monday() as i64);
        let (sel_a, sel_b) = self.cal.range();
        egui::Grid::new("mini").spacing([2.0, 2.0]).show(ui, |ui| {
            for w in WEEKDAYS {
                ui.add_sized(
                    [28.0, 18.0],
                    egui::Label::new(RichText::new(w).small().weak()),
                );
            }
            ui.end_row();
            for week in 0..6 {
                for d in 0..7 {
                    let day = start + Duration::days(week * 7 + d);
                    let is_today = day == super::today();
                    let mut text = RichText::new(day.day().to_string()).size(12.0);
                    if day.month() != m.month() {
                        text = text.weak();
                    }
                    if is_today {
                        text = text.color(Color32::WHITE).strong();
                    }
                    let in_sel = self.cal.view != CalView::Month && day >= sel_a && day <= sel_b;
                    let btn = egui::Button::new(text)
                        .corner_radius(15)
                        .fill(if is_today {
                            BRAND
                        } else if in_sel {
                            ui.visuals().selection.bg_fill
                        } else {
                            Color32::TRANSPARENT
                        })
                        .stroke(egui::Stroke::NONE);
                    if ui.add_sized([28.0, 26.0], btn).clicked() {
                        self.cal.date = day;
                        changed = true;
                    }
                }
                ui.end_row();
            }
        });
        changed
    }

    fn time_grid(&mut self, ui: &mut egui::Ui) {
        let (a, b) = self.cal.range();
        let days: Vec<NaiveDate> = (0..=(b - a).num_days())
            .map(|i| a + Duration::days(i))
            .collect();
        let gutter = 52.0;
        let col_w = ((ui.available_width() - gutter - 14.0) / days.len() as f32).max(60.0);
        let mut open: Option<Event> = None;
        let mut create: Option<(chrono::NaiveDateTime, bool)> = None;

        // Day headers + all-day row.
        let header_h = 46.0;
        let allday: Vec<&Event> = self.cal.events.iter().filter(|e| e.all_day).collect();
        let allday_rows = days
            .iter()
            .map(|d| {
                allday
                    .iter()
                    .filter(|e| local(&e.start).date() <= *d && local(&e.end).date() > *d)
                    .count()
            })
            .max()
            .unwrap_or(0);
        let allday_h = 22.0 * allday_rows.max(1) as f32 + 4.0;
        let (head, _) = ui.allocate_exact_size(
            egui::vec2(gutter + col_w * days.len() as f32, header_h + allday_h),
            Sense::hover(),
        );
        let v = ui.visuals().clone();
        let stroke = v.widgets.noninteractive.bg_stroke;
        for (i, d) in days.iter().enumerate() {
            let x = head.left() + gutter + col_w * i as f32;
            let is_today = *d == super::today();
            let color = if is_today { BRAND } else { v.text_color() };
            let p = ui.painter();
            p.text(
                egui::pos2(x + 8.0, head.top() + 4.0),
                egui::Align2::LEFT_TOP,
                WEEKDAYS[d.weekday().num_days_from_monday() as usize],
                egui::FontId::proportional(12.0),
                v.weak_text_color(),
            );
            p.text(
                egui::pos2(x + 8.0, head.top() + 18.0),
                egui::Align2::LEFT_TOP,
                d.day().to_string(),
                egui::FontId::proportional(20.0),
                color,
            );
            let cell = egui::Rect::from_min_size(
                egui::pos2(x, head.top() + header_h),
                egui::vec2(col_w, allday_h),
            );
            p.line_segment([cell.left_top(), cell.left_bottom()], stroke);
            let r = ui.interact(cell, ui.id().with(("allday", i)), Sense::click());
            if r.double_clicked() {
                create = Some((d.and_hms_opt(0, 0, 0).unwrap(), true));
            }
            let mut row = 0.0;
            for e in allday
                .iter()
                .filter(|e| local(&e.start).date() <= *d && local(&e.end).date() > *d)
            {
                let er = egui::Rect::from_min_size(
                    cell.min + egui::vec2(2.0, 2.0 + row),
                    egui::vec2(col_w - 4.0, 20.0),
                );
                if self.paint_event(ui, er, e, ("ad", i)) {
                    open = Some((*e).clone());
                }
                row += 22.0;
            }
        }
        ui.painter().line_segment(
            [
                egui::pos2(head.left(), head.bottom()),
                egui::pos2(head.right(), head.bottom()),
            ],
            stroke,
        );
        ui.add_space(-6.0);
        ui.label(RichText::new("ganztägig").small().weak());

        let mut scroll = egui::ScrollArea::vertical()
            .id_salt("cal_scroll")
            .auto_shrink([false, false]);
        if !self.cal.scrolled {
            scroll = scroll.vertical_scroll_offset(HOUR_H * 7.5);
            self.cal.scrolled = true;
        }
        scroll.show(ui, |ui| {
            let (grid, _) = ui.allocate_exact_size(
                egui::vec2(gutter + col_w * days.len() as f32, HOUR_H * 24.0),
                Sense::hover(),
            );
            let p = ui.painter();
            for h in 0..24 {
                let y = grid.top() + h as f32 * HOUR_H;
                if h > 0 {
                    p.text(
                        egui::pos2(grid.left() + gutter - 8.0, y),
                        egui::Align2::RIGHT_CENTER,
                        format!("{h:02}:00"),
                        egui::FontId::proportional(11.0),
                        v.weak_text_color(),
                    );
                }
                p.line_segment(
                    [
                        egui::pos2(grid.left() + gutter, y),
                        egui::pos2(grid.right(), y),
                    ],
                    stroke,
                );
                let half = egui::Stroke::new(stroke.width, stroke.color.gamma_multiply(0.5));
                p.line_segment(
                    [
                        egui::pos2(grid.left() + gutter, y + HOUR_H / 2.0),
                        egui::pos2(grid.right(), y + HOUR_H / 2.0),
                    ],
                    half,
                );
            }
            for (i, d) in days.iter().enumerate() {
                let x = grid.left() + gutter + col_w * i as f32;
                let col = egui::Rect::from_min_size(
                    egui::pos2(x, grid.top()),
                    egui::vec2(col_w, grid.height()),
                );
                if *d == super::today() {
                    ui.painter().rect_filled(col, 0, BRAND.gamma_multiply(0.06));
                }
                ui.painter()
                    .line_segment([col.left_top(), col.left_bottom()], stroke);
                let r = ui.interact(col, ui.id().with(("col", i)), Sense::click());
                if r.double_clicked() {
                    if let Some(pos) = r.interact_pointer_pos() {
                        let slot = ((pos.y - grid.top()) / (HOUR_H / 2.0)).floor() as i64;
                        create = Some((
                            d.and_hms_opt(0, 0, 0).unwrap() + Duration::minutes(slot * 30),
                            false,
                        ));
                    }
                }
                if r.hovered() {
                    if let Some(pos) = r.hover_pos() {
                        let slot = ((pos.y - grid.top()) / (HOUR_H / 2.0)).floor();
                        let sr = egui::Rect::from_min_size(
                            egui::pos2(x, grid.top() + slot * HOUR_H / 2.0),
                            egui::vec2(col_w, HOUR_H / 2.0),
                        );
                        ui.painter()
                            .rect_filled(sr, 0, v.widgets.hovered.weak_bg_fill);
                    }
                }

                // Lay out overlapping events in lanes.
                let mut evs: Vec<&Event> = self
                    .cal
                    .events
                    .iter()
                    .filter(|e| {
                        !e.all_day
                            && local(&e.start).date() <= *d
                            && local(&e.end) > d.and_hms_opt(0, 0, 0).unwrap()
                    })
                    .collect();
                evs.sort_by(|a, b| a.start.cmp(&b.start));
                let mut lanes: Vec<chrono::NaiveDateTime> = vec![];
                let mut placed = vec![];
                for e in &evs {
                    let s = local(&e.start);
                    let lane = lanes.iter().position(|end| *end <= s).unwrap_or_else(|| {
                        lanes.push(s);
                        lanes.len() - 1
                    });
                    lanes[lane] = local(&e.end);
                    placed.push((lane, *e));
                }
                let n = lanes.len().max(1) as f32;
                let day_start = d.and_hms_opt(0, 0, 0).unwrap();
                for (lane, e) in placed {
                    let s = (local(&e.start) - day_start).num_minutes().max(0) as f32;
                    let en = (local(&e.end) - day_start).num_minutes().min(24 * 60) as f32;
                    let lane_w = (col_w - 6.0) / n;
                    let er = egui::Rect::from_min_size(
                        egui::pos2(
                            x + 2.0 + lane as f32 * lane_w,
                            grid.top() + s / 60.0 * HOUR_H + 1.0,
                        ),
                        egui::vec2(lane_w - 2.0, ((en - s) / 60.0 * HOUR_H - 2.0).max(18.0)),
                    );
                    if self.paint_event(ui, er, e, ("ev", i)) {
                        open = Some(e.clone());
                    }
                }
            }
            // Current time indicator.
            let now = chrono::Local::now().naive_local();
            if let Some(i) = days.iter().position(|d| *d == now.date()) {
                let y = grid.top() + (now.hour() * 60 + now.minute()) as f32 / 60.0 * HOUR_H;
                let x = grid.left() + gutter + col_w * i as f32;
                ui.painter().line_segment(
                    [egui::pos2(x, y), egui::pos2(x + col_w, y)],
                    egui::Stroke::new(2.0_f32, DANGER),
                );
                ui.painter().circle_filled(egui::pos2(x, y), 4.0, DANGER);
            }
        });
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_secs(60));

        if let Some(e) = open {
            self.edit_event(&e);
        }
        if let Some((start, all_day)) = create {
            self.new_event_at(start, all_day);
        }
    }

    /// Paint an event block; returns true when clicked.
    fn paint_event(
        &self,
        ui: &mut egui::Ui,
        rect: egui::Rect,
        e: &Event,
        salt: impl std::hash::Hash,
    ) -> bool {
        let color = if e.category.is_empty() {
            BRAND
        } else {
            super::category_color(&e.category)
        };
        let resp = ui.interact(
            rect,
            ui.id().with(("event", e.id)).with(salt),
            Sense::click(),
        );
        let fill = if e.show_as == "free" {
            ui.visuals().window_fill
        } else {
            color.gamma_multiply(if resp.hovered() { 0.35 } else { 0.22 })
        };
        let p = ui.painter_at(rect);
        p.rect_filled(rect, 4, fill);
        p.rect_filled(
            egui::Rect::from_min_size(rect.min, egui::vec2(3.0, rect.height())),
            2,
            color,
        );
        if e.show_as == "tentative" {
            p.rect_stroke(
                rect,
                4,
                egui::Stroke::new(1.0_f32, color),
                egui::StrokeKind::Inside,
            );
        }
        let text = ui.visuals().text_color();
        let mut label = e.title.clone();
        if rect.height() >= 34.0 {
            let (s, en) = (local(&e.start), local(&e.end));
            if !e.all_day {
                label.push_str(&format!("\n{}–{}", s.format("%H:%M"), en.format("%H:%M")));
            }
            if !e.location.is_empty() {
                label.push_str(&format!("\n{}", e.location));
            }
        }
        let g = p.layout(
            label,
            egui::FontId::proportional(12.0),
            text,
            rect.width() - 10.0,
        );
        p.galley(rect.min + egui::vec2(7.0, 2.0), g, text);
        resp.on_hover_text(format!(
            "{}\n{} – {}{}",
            e.title,
            local(&e.start).format("%d.%m. %H:%M"),
            local(&e.end).format("%d.%m. %H:%M"),
            if e.location.is_empty() {
                String::new()
            } else {
                format!("\n📍 {}", e.location)
            }
        ))
        .clicked()
    }

    fn month_grid(&mut self, ui: &mut egui::Ui) {
        let (start, _) = self.cal.range();
        let w = ui.available_width() / 7.0;
        ui.horizontal(|ui| {
            for d in super::WEEKDAYS_LONG {
                ui.add_sized(
                    [w - 8.0, 18.0],
                    egui::Label::new(RichText::new(d).small().weak()),
                );
            }
        });
        let h = ((ui.available_height() - 4.0) / 6.0).max(70.0);
        let (grid, _) = ui.allocate_exact_size(egui::vec2(w * 7.0, h * 6.0), Sense::hover());
        let v = ui.visuals().clone();
        let stroke = v.widgets.noninteractive.bg_stroke;
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
                p.rect_filled(cell, 0, v.faint_bg_color);
            } else if r.hovered() {
                p.rect_filled(cell, 0, v.widgets.hovered.weak_bg_fill);
            }
            p.rect_stroke(cell, 0, stroke, egui::StrokeKind::Inside);
            let num = if day.day() == 1 {
                format!("{}. {}", day.day(), &MONTHS[day.month0() as usize][..3])
            } else {
                day.day().to_string()
            };
            if day == super::today() {
                p.circle_filled(cell.min + egui::vec2(16.0, 14.0), 11.0, BRAND);
                p.text(
                    cell.min + egui::vec2(16.0, 14.0),
                    egui::Align2::CENTER_CENTER,
                    num,
                    egui::FontId::proportional(12.0),
                    Color32::WHITE,
                );
            } else {
                p.text(
                    cell.min + egui::vec2(8.0, 6.0),
                    egui::Align2::LEFT_TOP,
                    num,
                    egui::FontId::proportional(12.0),
                    v.text_color(),
                );
            }
            if r.double_clicked() {
                create = Some(day);
            }
            let evs: Vec<&Event> = self
                .cal
                .events
                .iter()
                .filter(|e| {
                    let (s, en) = (local(&e.start), local(&e.end));
                    s.date() <= day && (en.date() > day || en.date() == day && !e.all_day)
                })
                .collect();
            let max = ((h - 30.0) / 20.0).floor().max(0.0) as usize;
            for (k, e) in evs.iter().take(max).enumerate() {
                let er = egui::Rect::from_min_size(
                    cell.min + egui::vec2(3.0, 26.0 + k as f32 * 20.0),
                    egui::vec2(w - 6.0, 18.0),
                );
                let mut ev = (*e).clone();
                if !e.all_day {
                    ev.title = format!("{} {}", local(&e.start).format("%H:%M"), e.title);
                }
                if self.paint_event(ui, er, &ev, ("m", i)) {
                    open = Some((*e).clone());
                }
            }
            if evs.len() > max {
                let more = ui.put(
                    egui::Rect::from_min_size(
                        cell.left_bottom() - egui::vec2(-4.0, 18.0),
                        egui::vec2(w - 8.0, 16.0),
                    ),
                    egui::Button::new(
                        RichText::new(format!("+{} weitere", evs.len() - max)).small(),
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
            self.new_event_at(d.and_hms_opt(9, 0, 0).unwrap(), false);
        }
        if let Some(d) = goto {
            self.cal.date = d;
            self.cal.view = CalView::Day;
            self.reload_events();
        }
    }

    pub fn event_editor_ui(&mut self, ctx: &egui::Context) {
        let Some(ed) = &mut self.cal.editor else {
            return;
        };
        let mut open = true;
        let mut save = false;
        let mut delete = false;
        let is_new = ed.ev.id == 0;
        let accounts = self.data.accounts.clone();
        egui::Window::new(if is_new {
            "Neues Ereignis"
        } else {
            "Ereignis bearbeiten"
        })
        .open(&mut open)
        .collapsible(false)
        .default_width(560.0)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .show(ctx, |ui| {
            ui.add(
                egui::TextEdit::singleline(&mut ed.ev.title)
                    .hint_text("Titel hinzufügen")
                    .font(egui::TextStyle::Heading)
                    .desired_width(f32::INFINITY),
            );
            ui.add_space(8.0);
            egui::Grid::new("ev_grid")
                .num_columns(2)
                .spacing([12.0, 8.0])
                .show(ui, |ui| {
                    ui.label("Kalender");
                    ui.add_enabled_ui(is_new, |ui| {
                        egui::ComboBox::from_id_salt("ev_acc")
                            .selected_text(
                                accounts
                                    .iter()
                                    .find(|a| a.id == ed.ev.account_id)
                                    .map(|a| a.email.clone())
                                    .unwrap_or_default(),
                            )
                            .show_ui(ui, |ui| {
                                for a in accounts.iter().filter(|a| a.kind != AccountKind::Imap) {
                                    ui.selectable_value(&mut ed.ev.account_id, a.id, &a.email);
                                }
                            });
                    });
                    ui.end_row();
                    ui.label("Beginn");
                    ui.horizontal(|ui| {
                        ui.add(
                            egui_extras::DatePickerButton::new(&mut ed.start_date)
                                .id_salt("ev_sd")
                                .format("%d.%m.%Y"),
                        );
                        if !ed.ev.all_day {
                            time_combo(ui, "ev_st", &mut ed.start_time);
                        }
                        ui.checkbox(&mut ed.ev.all_day, "Ganztägig");
                    });
                    ui.end_row();
                    ui.label("Ende");
                    ui.horizontal(|ui| {
                        ui.add(
                            egui_extras::DatePickerButton::new(&mut ed.end_date)
                                .id_salt("ev_ed")
                                .format("%d.%m.%Y"),
                        );
                        if !ed.ev.all_day {
                            time_combo(ui, "ev_et", &mut ed.end_time);
                        }
                    });
                    ui.end_row();
                    ui.label("Ort");
                    ui.add(
                        egui::TextEdit::singleline(&mut ed.ev.location)
                            .hint_text("Ort oder Besprechungslink")
                            .desired_width(f32::INFINITY),
                    );
                    ui.end_row();
                    ui.label("Teilnehmer");
                    ui.add(
                        egui::TextEdit::singleline(&mut ed.ev.attendees)
                            .hint_text("E-Mail-Adressen, mit ; getrennt")
                            .desired_width(f32::INFINITY),
                    );
                    ui.end_row();
                    ui.label("Anzeigen als");
                    egui::ComboBox::from_id_salt("ev_show")
                        .selected_text(show_as_label(&ed.ev.show_as))
                        .show_ui(ui, |ui| {
                            for s in ["free", "tentative", "busy", "oof"] {
                                ui.selectable_value(&mut ed.ev.show_as, s.into(), show_as_label(s));
                            }
                        });
                    ui.end_row();
                    ui.label("Erinnerung");
                    egui::ComboBox::from_id_salt("ev_rem")
                        .selected_text(reminder_label(ed.ev.reminder))
                        .show_ui(ui, |ui| {
                            for m in [-1, 0, 5, 15, 30, 60, 120, 1440] {
                                ui.selectable_value(&mut ed.ev.reminder, m, reminder_label(m));
                            }
                        });
                    ui.end_row();
                    ui.label("Kategorie");
                    egui::ComboBox::from_id_salt("ev_cat")
                        .selected_text(if ed.ev.category.is_empty() {
                            "Keine".to_string()
                        } else {
                            ed.ev.category.clone()
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut ed.ev.category, String::new(), "Keine");
                            for (c, col) in CATEGORIES {
                                ui.selectable_value(
                                    &mut ed.ev.category,
                                    c.into(),
                                    RichText::new(format!("■ {c}")).color(col),
                                );
                            }
                        });
                    ui.end_row();
                });
            ui.add_space(6.0);
            ui.add(
                egui::TextEdit::multiline(&mut ed.ev.body)
                    .hint_text("Beschreibung hinzufügen")
                    .desired_rows(5)
                    .desired_width(f32::INFINITY),
            );
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if super::primary_button(ui, "💾  Speichern").clicked() {
                    save = true;
                }
                if !is_new
                    && ui
                        .button(RichText::new("🗑  Löschen").color(DANGER))
                        .clicked()
                {
                    delete = true;
                }
            });
        });
        if save {
            let mut e = ed.ev.clone();
            if e.title.trim().is_empty() {
                e.title = "(Kein Titel)".into();
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
                self.toast("Das Ende liegt vor dem Beginn.", true);
                return;
            }
            e.start = super::local_to_utc_string(s);
            e.end = super::local_to_utc_string(en);
            self.cal.editor = None;
            self.spawn(Some("Ereignis gespeichert"), move |st, _| async move {
                providers::save_event(&st, e).await.map(|_| ())
            });
        } else if delete {
            let id = ed.ev.id;
            self.cal.editor = None;
            self.spawn(Some("Ereignis gelöscht"), move |st, _| async move {
                providers::delete_event(&st, id).await
            });
        } else if !open {
            self.cal.editor = None;
        }
    }
}

fn time_combo(ui: &mut egui::Ui, id: &str, t: &mut NaiveTime) {
    egui::ComboBox::from_id_salt(id)
        .width(70.0)
        .selected_text(t.format("%H:%M").to_string())
        .height(300.0)
        .show_ui(ui, |ui| {
            for s in time_slots() {
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

fn reminder_label(m: i64) -> String {
    match m {
        -1 => "Keine".into(),
        0 => "Zum Beginn".into(),
        m if m < 60 => format!("{m} Minuten vorher"),
        60 => "1 Stunde vorher".into(),
        m if m < 1440 => format!("{} Stunden vorher", m / 60),
        _ => "1 Tag vorher".into(),
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
}
