//! "Notizen" (sticky notes) and "Journal" modules.

use std::time::Instant;

use chrono::{NaiveDate, NaiveTime, Timelike};
use eframe::egui::{self, Color32, RichText, Sense};
use egui_phosphor::regular as ph;

use super::{
    chrome::{self, IC_BLUE, IC_GREEN, IC_PURPLE, IC_RED, IC_YELLOW},
    App, Win,
};
use crate::{db, models::*};

pub const NOTE_COLORS: [(&str, &str, Color32); 5] = [
    ("yellow", "Gelb", Color32::from_rgb(255, 241, 165)),
    ("blue", "Blau", Color32::from_rgb(201, 228, 250)),
    ("green", "Grün", Color32::from_rgb(201, 238, 197)),
    ("pink", "Rosa", Color32::from_rgb(250, 211, 230)),
    ("white", "Weiß", Color32::from_rgb(250, 250, 250)),
];

pub const JOURNAL_KINDS: [&str; 7] = [
    "Telefonanruf",
    "Besprechung",
    "E-Mail-Nachricht",
    "Aufgabe",
    "Dokument",
    "Gespräch",
    "Notiz",
];

fn note_color(c: &str) -> Color32 {
    NOTE_COLORS
        .iter()
        .find(|x| x.0 == c)
        .map(|x| x.2)
        .unwrap_or(NOTE_COLORS[0].2)
}

fn kind_icon(kind: &str) -> &'static str {
    match kind {
        "Telefonanruf" => ph::PHONE_CALL,
        "Besprechung" => ph::USERS_THREE,
        "E-Mail-Nachricht" => ph::ENVELOPE_SIMPLE,
        "Aufgabe" => ph::CHECK_SQUARE_OFFSET,
        "Dokument" => ph::FILE_TEXT,
        "Gespräch" => ph::CHATS,
        _ => ph::NOTE,
    }
}

#[derive(Default)]
pub struct NotesView {
    pub notes: Vec<Note>,
    pub journal: Vec<JournalEntry>,
    selected_note: Option<i64>,
    selected_entry: Option<i64>,
    list_mode: bool,
}

pub struct NoteEditor {
    note: Note,
    saved_body: String,
}

pub struct JournalEditor {
    e: JournalEntry,
    date: NaiveDate,
    time: NaiveTime,
    /// Running timer (started at, minutes before start).
    timer: Option<(Instant, i64)>,
}

impl App {
    pub fn reload_notes(&mut self) {
        let c = self.st.db();
        self.notes.notes = db::list_notes(&c).unwrap_or_default();
        self.notes.journal = db::list_journal(&c).unwrap_or_default();
    }

    pub fn new_note(&mut self) {
        let note = Note {
            color: "yellow".into(),
            ..Default::default()
        };
        self.open_window(Win::Note(NoteEditor {
            saved_body: String::new(),
            note,
        }));
    }

    pub fn new_journal_entry(&mut self, kind: &str) {
        let now = chrono::Local::now().naive_local();
        self.open_window(Win::Journal(JournalEditor {
            e: JournalEntry {
                kind: kind.into(),
                ..Default::default()
            },
            date: now.date(),
            time: NaiveTime::from_hms_opt(now.hour(), now.minute(), 0).unwrap(),
            timer: None,
        }));
    }

    fn open_entry(&mut self, e: &JournalEntry) {
        let start = super::parse_utc(&e.start)
            .map(|d| d.naive_local())
            .unwrap_or_else(|| chrono::Local::now().naive_local());
        self.open_window(Win::Journal(JournalEditor {
            e: e.clone(),
            date: start.date(),
            time: start.time(),
            timer: None,
        }));
    }

    // ------------------------------------------------------------ notes

    pub fn notes_ribbon(&mut self, ui: &mut egui::Ui) {
        if self.ribbon_tab != 0 {
            self.common_ribbon(ui);
            return;
        }
        let t = self.t;
        let sel = self.notes.selected_note;
        chrome::group(ui, &t, "Neu", |ui| {
            if chrome::large(ui, &t, ph::NOTE, IC_YELLOW, "Neue\nNotiz", true).clicked() {
                self.new_note();
            }
        });
        chrome::group(ui, &t, "Löschen", |ui| {
            if chrome::large(ui, &t, ph::X, IC_RED, "Löschen", sel.is_some()).clicked() {
                let _ = db::delete_note(&self.st.db(), sel.unwrap());
                self.notes.selected_note = None;
                self.reload_notes();
            }
        });
        chrome::group(ui, &t, "Kategorien", |ui| {
            let r = chrome::large(
                ui,
                &t,
                ph::PALETTE,
                IC_PURPLE,
                "Farbe\n\u{E136}",
                sel.is_some(),
            );
            chrome::menu(&r, |ui| {
                for (key, label, color) in NOTE_COLORS {
                    if ui
                        .add(
                            egui::Button::new((
                                RichText::new("\u{E45E}")
                                    .family(egui::FontFamily::Name("fill".into()))
                                    .color(color)
                                    .size(16.0),
                                label,
                            ))
                            .frame(false),
                        )
                        .clicked()
                    {
                        if let Some(mut n) =
                            self.notes.notes.iter().find(|n| Some(n.id) == sel).cloned()
                        {
                            n.color = key.into();
                            let _ = db::save_note(&self.st.db(), &n);
                            self.reload_notes();
                        }
                        ui.close();
                    }
                }
            });
        });
        chrome::group(ui, &t, "Aktuelle Ansicht", |ui| {
            for (list, icon, label) in [
                (false, ph::SQUARES_FOUR, "Symbol"),
                (true, ph::LIST, "Notizenliste"),
            ] {
                let on = self.notes.list_mode == list;
                let r = chrome::large(
                    ui,
                    &t,
                    icon,
                    if on { IC_BLUE } else { chrome::IC_GREY },
                    label,
                    true,
                );
                if on {
                    ui.painter().rect_stroke(
                        r.rect,
                        0,
                        egui::Stroke::new(1.0_f32, t.button_down),
                        egui::StrokeKind::Inside,
                    );
                }
                if r.clicked() {
                    self.notes.list_mode = list;
                }
            }
        });
    }

    pub fn notes_nav(&mut self, ui: &mut egui::Ui) {
        let t = self.t;
        ui.label(RichText::new(format!("{}  Meine Notizen", ph::CARET_DOWN)).strong());
        let (r, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 24.0), Sense::hover());
        ui.painter().rect_filled(r, 0, t.selected);
        ui.painter().text(
            r.left_center() + egui::vec2(26.0, 0.0),
            egui::Align2::LEFT_CENTER,
            format!("Notizen ({})", self.notes.notes.len()),
            egui::FontId::proportional(13.0),
            t.text,
        );
    }

    pub fn notes_ui(&mut self, ui: &mut egui::Ui) {
        let t = self.t;
        let needle = self.search.trim().to_lowercase();
        let notes: Vec<Note> = self
            .notes
            .notes
            .iter()
            .filter(|n| needle.is_empty() || n.body.to_lowercase().contains(&needle))
            .cloned()
            .collect();
        egui::Frame::new()
            .inner_margin(egui::Margin::symmetric(8, 8))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Notizen").size(17.0));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.allocate_ui(egui::vec2(320.0, 26.0), |ui| {
                            super::search_box(
                                ui,
                                &t,
                                &mut self.search,
                                "Notizen durchsuchen (Strg+E)",
                            );
                        });
                    });
                });
            });
        let mut open = None;
        egui::ScrollArea::vertical()
            .id_salt("notes")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if self.notes.list_mode {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for n in &notes {
                        let (r, resp) = ui.allocate_exact_size(
                            egui::vec2(ui.available_width(), 26.0),
                            Sense::click(),
                        );
                        if self.notes.selected_note == Some(n.id) {
                            ui.painter().rect_filled(r, 0, t.selected);
                        } else if resp.hovered() {
                            ui.painter().rect_filled(r, 0, t.hover);
                        }
                        ui.painter().rect_filled(
                            egui::Rect::from_center_size(
                                r.left_center() + egui::vec2(16.0, 0.0),
                                egui::vec2(12.0, 12.0),
                            ),
                            0,
                            note_color(&n.color),
                        );
                        let title = n.body.lines().next().unwrap_or("").to_string();
                        let g = super::one_line(
                            ui.painter(),
                            &title,
                            egui::FontId::proportional(13.0),
                            t.text,
                            r.width() - 200.0,
                        );
                        ui.painter().galley(
                            egui::pos2(r.left() + 34.0, r.center().y - g.size().y / 2.0),
                            g,
                            t.text,
                        );
                        ui.painter().text(
                            r.right_center() - egui::vec2(12.0, 0.0),
                            egui::Align2::RIGHT_CENTER,
                            super::long_date(&n.modified),
                            egui::FontId::proportional(12.0),
                            t.text_weak,
                        );
                        if resp.clicked() {
                            self.notes.selected_note = Some(n.id);
                        }
                        if resp.double_clicked() {
                            open = Some(n.clone());
                        }
                    }
                } else {
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(14.0, 14.0);
                        ui.add_space(10.0);
                        for n in &notes {
                            let (r, resp) =
                                ui.allocate_exact_size(egui::vec2(130.0, 130.0), Sense::click());
                            let selected = self.notes.selected_note == Some(n.id);
                            let paper = egui::Rect::from_min_size(
                                r.min + egui::vec2(25.0, 4.0),
                                egui::vec2(80.0, 80.0),
                            );
                            ui.painter().rect_filled(
                                paper.translate(egui::vec2(3.0, 3.0)),
                                0,
                                Color32::from_black_alpha(40),
                            );
                            ui.painter().rect(
                                paper,
                                0,
                                note_color(&n.color),
                                egui::Stroke::new(
                                    if selected { 2.0_f32 } else { 1.0 },
                                    if selected {
                                        t.accent
                                    } else {
                                        Color32::from_gray(190)
                                    },
                                ),
                                egui::StrokeKind::Inside,
                            );
                            let preview = n.body.chars().take(60).collect::<String>();
                            let g = ui.painter().layout(
                                preview,
                                egui::FontId::proportional(10.0),
                                Color32::from_gray(60),
                                72.0,
                            );
                            ui.painter().with_clip_rect(paper.shrink(4.0)).galley(
                                paper.min + egui::vec2(4.0, 4.0),
                                g,
                                Color32::from_gray(60),
                            );
                            let title = n.body.lines().next().unwrap_or("").to_string();
                            let tg = ui.painter().layout(
                                title,
                                egui::FontId::proportional(12.0),
                                t.text,
                                126.0,
                            );
                            let tw = tg.size().x;
                            ui.painter().with_clip_rect(r).galley(
                                egui::pos2(r.center().x - tw / 2.0, paper.bottom() + 8.0),
                                tg,
                                t.text,
                            );
                            if resp.clicked() {
                                self.notes.selected_note = Some(n.id);
                            }
                            if resp.double_clicked() {
                                open = Some(n.clone());
                            }
                        }
                    });
                }
                if notes.is_empty() {
                    ui.add_space(20.0);
                    ui.vertical_centered(|ui| {
                        ui.label(
                            RichText::new("In dieser Ansicht gibt es keine Elemente.")
                                .color(t.text_weak),
                        )
                    });
                }
            });
        if let Some(n) = open {
            self.open_window(Win::Note(NoteEditor {
                saved_body: n.body.clone(),
                note: n,
            }));
        }
        if ui.input(|i| i.key_pressed(egui::Key::Delete)) && !ui.ctx().wants_keyboard_input() {
            if let Some(id) = self.notes.selected_note.take() {
                let _ = db::delete_note(&self.st.db(), id);
                self.reload_notes();
            }
        }
    }

    /// A note is a small coloured window that saves itself, like Outlook's notes.
    pub(super) fn note_window(
        &mut self,
        ctx: &egui::Context,
        vid: egui::ViewportId,
        ed: &mut NoteEditor,
    ) -> bool {
        let t = self.t;
        let title = ed
            .note
            .body
            .lines()
            .next()
            .filter(|l| !l.trim().is_empty())
            .unwrap_or("Notiz")
            .to_string();
        let color = note_color(&ed.note.color);
        let mut delete = false;
        let closed = chrome::native_window(ctx, vid, &title, [320.0, 300.0], &t, |ui| {
            egui::Frame::new()
                .fill(color)
                .inner_margin(egui::Margin::same(8))
                .show(ui, |ui| {
                    ui.style_mut().visuals.override_text_color = Some(Color32::from_gray(30));
                    ui.horizontal(|ui| {
                        let r = ui.add(
                            egui::Button::new(RichText::new(ph::NOTE).size(16.0)).frame(false),
                        );
                        chrome::menu(&r, |ui| {
                            for (key, label, c) in NOTE_COLORS {
                                if ui
                                    .add(
                                        egui::Button::new((
                                            RichText::new("\u{E45E}")
                                                .family(egui::FontFamily::Name("fill".into()))
                                                .color(c)
                                                .size(16.0),
                                            label,
                                        ))
                                        .frame(false),
                                    )
                                    .clicked()
                                {
                                    ed.note.color = key.into();
                                    ui.close();
                                }
                            }
                            ui.separator();
                            if chrome::menu_item(ui, ph::X, IC_RED, "Löschen") {
                                delete = true;
                            }
                        });
                        ui.label(
                            RichText::new(
                                chrono::Local::now().format("%d.%m.%Y %H:%M").to_string(),
                            )
                            .size(11.0)
                            .color(Color32::from_gray(90)),
                        );
                    });
                    let (w, h) = (ui.available_width(), ui.available_height().max(120.0));
                    let mut te = egui::TextEdit::multiline(&mut ed.note.body)
                        .frame(false)
                        .text_color(Color32::from_gray(30))
                        .background_color(color);
                    te = te.desired_width(w);
                    ui.add_sized([w, h - 4.0], te);
                });
        });
        if delete {
            if ed.note.id > 0 {
                let _ = db::delete_note(&self.st.db(), ed.note.id);
                self.reload_notes();
            }
            return true;
        }
        // Save on every change of text or colour.
        let stored = self.notes.notes.iter().find(|n| n.id == ed.note.id);
        let dirty =
            ed.note.body != ed.saved_body || stored.is_some_and(|n| n.color != ed.note.color);
        if dirty && !ed.note.body.trim().is_empty() {
            let res = db::save_note(&self.st.db(), &ed.note);
            if let Ok(id) = res {
                ed.note.id = id;
                ed.saved_body = ed.note.body.clone();
                self.reload_notes();
            }
        }
        closed
    }

    // ------------------------------------------------------------ journal

    pub fn journal_ribbon(&mut self, ui: &mut egui::Ui) {
        if self.ribbon_tab != 0 {
            self.common_ribbon(ui);
            return;
        }
        let t = self.t;
        let sel = self.notes.selected_entry;
        chrome::group(ui, &t, "Neu", |ui| {
            if chrome::large(ui, &t, ph::BOOK_OPEN, IC_BLUE, "Journal-\neintrag", true).clicked() {
                self.new_journal_entry("Telefonanruf");
            }
            let r = chrome::large(
                ui,
                &t,
                ph::FILE_PLUS,
                IC_YELLOW,
                "Neue\nElemente \u{E136}",
                true,
            );
            chrome::menu(&r, |ui| {
                for k in JOURNAL_KINDS {
                    if chrome::menu_item(ui, kind_icon(k), IC_BLUE, k) {
                        self.new_journal_entry(k);
                    }
                }
            });
        });
        chrome::group(ui, &t, "Löschen", |ui| {
            if chrome::large(ui, &t, ph::X, IC_RED, "Löschen", sel.is_some()).clicked() {
                let _ = db::delete_journal(&self.st.db(), sel.unwrap());
                self.notes.selected_entry = None;
                self.reload_notes();
            }
        });
    }

    pub fn journal_nav(&mut self, ui: &mut egui::Ui) {
        let t = self.t;
        ui.label(RichText::new(format!("{}  Meine Journale", ph::CARET_DOWN)).strong());
        let (r, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 24.0), Sense::hover());
        ui.painter().rect_filled(r, 0, t.selected);
        ui.painter().text(
            r.left_center() + egui::vec2(26.0, 0.0),
            egui::Align2::LEFT_CENTER,
            format!("Journal ({})", self.notes.journal.len()),
            egui::FontId::proportional(13.0),
            t.text,
        );
    }

    /// Journal as a list grouped by entry type, like Outlook's "Eintragsliste".
    pub fn journal_ui(&mut self, ui: &mut egui::Ui) {
        let t = self.t;
        let needle = self.search.trim().to_lowercase();
        let entries: Vec<JournalEntry> = self
            .notes
            .journal
            .iter()
            .filter(|e| {
                needle.is_empty()
                    || e.subject.to_lowercase().contains(&needle)
                    || e.notes.to_lowercase().contains(&needle)
                    || e.company.to_lowercase().contains(&needle)
            })
            .cloned()
            .collect();
        egui::Frame::new()
            .inner_margin(egui::Margin::symmetric(8, 8))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Journal").size(17.0));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.allocate_ui(egui::vec2(320.0, 26.0), |ui| {
                            super::search_box(
                                ui,
                                &t,
                                &mut self.search,
                                "Journal durchsuchen (Strg+E)",
                            );
                        });
                    });
                });
            });
        let w = ui.available_width() - 12.0;
        let cols: [(&str, f32); 5] = [
            ("Eintragstyp", 160.0),
            ("Betreff", w - 160.0 - 170.0 - 90.0 - 160.0),
            ("Beginn", 170.0),
            ("Dauer", 90.0),
            ("Firma", 160.0),
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
        let mut open = None;
        egui::ScrollArea::vertical()
            .id_salt("journal")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for kind in JOURNAL_KINDS {
                    let group: Vec<&JournalEntry> =
                        entries.iter().filter(|e| e.kind == kind).collect();
                    if group.is_empty() {
                        continue;
                    }
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(format!(
                            "  {}  Eintragstyp: {kind} ({})",
                            ph::CARET_DOWN,
                            group.len()
                        ))
                        .strong(),
                    );
                    for e in group {
                        let (r, resp) =
                            ui.allocate_exact_size(egui::vec2(w + 12.0, 26.0), Sense::click());
                        if self.notes.selected_entry == Some(e.id) {
                            ui.painter().rect_filled(r, 0, t.selected);
                        } else if resp.hovered() {
                            ui.painter().rect_filled(r, 0, t.hover);
                        }
                        let cy = r.center().y;
                        let mut x = r.left() + 6.0;
                        let values = [
                            format!("{}  {}", kind_icon(kind), e.kind),
                            e.subject.clone(),
                            super::long_date(&e.start),
                            format!("{} Min.", e.minutes),
                            e.company.clone(),
                        ];
                        for ((_, cw), v) in cols.iter().zip(values) {
                            let g = super::one_line(
                                ui.painter(),
                                &v,
                                egui::FontId::proportional(12.5),
                                t.text,
                                cw - 12.0,
                            );
                            ui.painter().galley(
                                egui::pos2(x + 6.0, cy - g.size().y / 2.0),
                                g,
                                t.text,
                            );
                            x += cw;
                        }
                        if resp.clicked() {
                            self.notes.selected_entry = Some(e.id);
                        }
                        if resp.double_clicked() {
                            open = Some(e.clone());
                        }
                    }
                }
                if entries.is_empty() {
                    ui.add_space(20.0);
                    ui.vertical_centered(|ui| {
                        ui.label(
                            RichText::new("In dieser Ansicht gibt es keine Elemente.")
                                .color(t.text_weak),
                        )
                    });
                }
            });
        if let Some(e) = open {
            self.open_entry(&e);
        }
    }

    pub(super) fn journal_window(
        &mut self,
        ctx: &egui::Context,
        vid: egui::ViewportId,
        ed: &mut JournalEditor,
    ) -> bool {
        let t = self.t;
        let title = format!(
            "{} - Journaleintrag",
            if ed.e.subject.trim().is_empty() {
                "Unbenannt"
            } else {
                ed.e.subject.trim()
            }
        );
        let is_new = ed.e.id == 0;
        let mut save = false;
        let mut delete = false;
        if ed.timer.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_secs(1));
        }
        let closed = chrome::native_window(ctx, vid, &title, [760.0, 560.0], &t, |ui| {
            chrome::item_ribbon(ui, &t, "Journaleintrag", |ui| {
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
                chrome::group(ui, &t, "Zeitgeber", |ui| {
                    let running = ed.timer.is_some();
                    if chrome::large(ui, &t, ph::TIMER, IC_GREEN, "Zeitgeber\nstarten", !running)
                        .clicked()
                    {
                        ed.timer = Some((Instant::now(), ed.e.minutes));
                    }
                    if chrome::large(ui, &t, ph::STOP, IC_RED, "Zeitgeber\nanhalten", running)
                        .clicked()
                    {
                        ed.timer = None;
                    }
                });
            });
            if let Some((started, before)) = ed.timer {
                ed.e.minutes = before + (started.elapsed().as_secs() / 60) as i64;
            }
            egui::Frame::new()
                .inner_margin(egui::Margin::symmetric(16, 12))
                .show(ui, |ui| {
                    egui::Grid::new("journal_form")
                        .num_columns(2)
                        .spacing([12.0, 8.0])
                        .show(ui, |ui| {
                            ui.label("Betreff");
                            ui.add(
                                egui::TextEdit::singleline(&mut ed.e.subject).desired_width(480.0),
                            );
                            ui.end_row();
                            ui.label("Eintragstyp");
                            egui::ComboBox::from_id_salt("jkind")
                                .selected_text(ed.e.kind.clone())
                                .show_ui(ui, |ui| {
                                    for k in JOURNAL_KINDS {
                                        ui.selectable_value(&mut ed.e.kind, k.to_string(), k);
                                    }
                                });
                            ui.end_row();
                            ui.label("Firma");
                            ui.add(
                                egui::TextEdit::singleline(&mut ed.e.company).desired_width(480.0),
                            );
                            ui.end_row();
                            ui.label("Beginn");
                            ui.horizontal(|ui| {
                                ui.add(
                                    egui_extras::DatePickerButton::new(&mut ed.date)
                                        .id_salt("jdate")
                                        .format("%d.%m.%Y"),
                                );
                                let mut text = ed.time.format("%H:%M").to_string();
                                if ui
                                    .add(egui::TextEdit::singleline(&mut text).desired_width(50.0))
                                    .changed()
                                {
                                    if let Ok(tm) = NaiveTime::parse_from_str(&text, "%H:%M") {
                                        ed.time = tm;
                                    }
                                }
                            });
                            ui.end_row();
                            ui.label("Dauer (Minuten)");
                            ui.horizontal(|ui| {
                                ui.add_enabled(
                                    ed.timer.is_none(),
                                    egui::DragValue::new(&mut ed.e.minutes).range(0..=100_000),
                                );
                                if ed.timer.is_some() {
                                    ui.label(
                                        RichText::new(format!("{}  Zeitgeber läuft", ph::TIMER))
                                            .color(IC_GREEN),
                                    );
                                }
                            });
                            ui.end_row();
                        });
                    ui.add_space(6.0);
                    ui.separator();
                    let (w, h) = (ui.available_width(), ui.available_height().max(100.0));
                    ui.add_sized(
                        [w, h - 4.0],
                        egui::TextEdit::multiline(&mut ed.e.notes).frame(false),
                    );
                });
        });
        if save {
            if ed.e.subject.trim().is_empty() {
                self.toast("Bitte einen Betreff eingeben.", true);
                return false;
            }
            ed.e.start = super::local_to_utc_string(ed.date.and_time(ed.time));
            let res = db::save_journal(&self.st.db(), &ed.e);
            match res {
                Ok(_) => {
                    self.toast("Journaleintrag gespeichert.", false);
                    self.reload_notes();
                }
                Err(e) => self.toast(format!("{e:#}"), true),
            }
            return true;
        }
        if delete {
            let _ = db::delete_journal(&self.st.db(), ed.e.id);
            self.reload_notes();
            return true;
        }
        closed
    }
}
