//! Native GUI modelled on the classic Outlook desktop client: own title bar
//! with quick access toolbar, ribbon, folder pane with module switcher,
//! content panes, status bar, File backstage, and real OS windows for
//! messages, appointments, contacts, tasks, options and reminders.
//!
//! The UI thread only reads from SQLite (fast, local). Everything that may
//! touch the network runs on the Tokio runtime via [`App::spawn`]; results
//! come back through a channel as [`Done`] and trigger a reload.

mod backstage;
mod calendar;
mod chrome;
mod compose;
mod html;
mod mail;
mod notes;
mod people;
mod reminders;
mod rules_ui;
mod tasks;
mod theme;

use std::{
    future::Future,
    sync::{mpsc, Arc},
    time::{Duration, Instant},
};

use chrono::{DateTime, Datelike, Local, NaiveDate, NaiveDateTime, TimeZone, Utc};
use eframe::egui::{self, Color32, RichText, Sense};
use egui_phosphor::regular as ph;

use crate::{db, models::*, providers, state::AppState};
use chrome::{IC_BLUE, IC_GREEN, IC_PURPLE, IC_YELLOW};
use theme::{Design, Theme};

pub const WEEKDAYS: [&str; 7] = ["Mo", "Di", "Mi", "Do", "Fr", "Sa", "So"];
pub const WEEKDAYS_LONG: [&str; 7] = [
    "Montag",
    "Dienstag",
    "Mittwoch",
    "Donnerstag",
    "Freitag",
    "Samstag",
    "Sonntag",
];
pub const MONTHS: [&str; 12] = [
    "Januar",
    "Februar",
    "März",
    "April",
    "Mai",
    "Juni",
    "Juli",
    "August",
    "September",
    "Oktober",
    "November",
    "Dezember",
];
/// Outlook's default colour categories.
pub const CATEGORIES: [(&str, Color32); 6] = [
    ("Rote Kategorie", Color32::from_rgb(232, 17, 35)),
    ("Orange Kategorie", Color32::from_rgb(247, 99, 12)),
    ("Gelbe Kategorie", Color32::from_rgb(255, 185, 0)),
    ("Grüne Kategorie", Color32::from_rgb(16, 124, 16)),
    ("Blaue Kategorie", Color32::from_rgb(0, 120, 212)),
    ("Lila Kategorie", Color32::from_rgb(136, 23, 152)),
];

pub fn category_color(name: &str) -> Color32 {
    CATEGORIES
        .iter()
        .find(|c| c.0 == name || c.0.starts_with(&name.chars().take(3).collect::<String>()))
        .map(|c| c.1)
        .unwrap_or(IC_BLUE)
}

/// Result of a background job.
pub enum Done {
    /// Success; optional status text. Always triggers a reload.
    Ok(Option<String>),
    Err(String),
    /// A message window's mail was sent; close that window.
    Sent(u64),
    /// Sending failed; keep the message window open.
    SendFailed(u64, String),
    /// Microsoft sign-in: show this code to the user (empty = finished).
    DeviceCode {
        user_code: String,
        uri: String,
    },
    /// Current automatic-reply settings of an account were loaded.
    AutoReply(i64, providers::graph::AutoReply),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Module {
    Mail,
    Calendar,
    People,
    Tasks,
    Notes,
    Journal,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ReadingPane {
    Right,
    Bottom,
    Off,
}

/// User preferences (stored in the `settings` table).
pub struct Prefs {
    pub design: Design,
    pub signature: String,
    pub reading_pane: ReadingPane,
    pub preview: bool,
    pub mark_read: bool,
    pub work_start: u32,
    pub work_end: u32,
    pub zoom: f32,
    pub conversations: bool,
}

impl Prefs {
    fn load(c: &rusqlite::Connection) -> Self {
        let s = db::get_settings(c).unwrap_or_default();
        let str_ = |k: &str| s.get(k).and_then(|v| v.as_str()).map(String::from);
        let num = |k: &str, d: f64| s.get(k).and_then(|v| v.as_f64()).unwrap_or(d);
        let flag = |k: &str, d: bool| s.get(k).and_then(|v| v.as_bool()).unwrap_or(d);
        Prefs {
            design: Design::parse(&str_("design").unwrap_or_default()),
            signature: str_("signature").unwrap_or_default(),
            reading_pane: match str_("reading_pane").as_deref() {
                Some("bottom") => ReadingPane::Bottom,
                Some("off") => ReadingPane::Off,
                _ => ReadingPane::Right,
            },
            preview: flag("preview", true),
            mark_read: flag("mark_read", true),
            work_start: num("work_start", 8.0) as u32,
            work_end: num("work_end", 17.0) as u32,
            zoom: num("zoom", 1.0) as f32,
            conversations: flag("conversations", false),
        }
    }
}

/// Data shared by all modules, reloaded from the database when `dirty`.
#[derive(Default)]
pub struct Data {
    pub accounts: Vec<Account>,
    pub folders: Vec<Folder>,
    pub contacts: Vec<Contact>,
    pub tasks: Vec<Task>,
}

impl Data {
    pub fn account(&self, id: i64) -> Option<&Account> {
        self.accounts.iter().find(|a| a.id == id)
    }

    pub fn local_account_id(&self) -> i64 {
        self.accounts
            .iter()
            .find(|a| a.kind == AccountKind::Local)
            .map(|a| a.id)
            .unwrap_or(1)
    }

    /// The account used for new calendar items, contacts and tasks:
    /// the first Microsoft account, else the local one.
    pub fn default_pim_account(&self) -> i64 {
        self.accounts
            .iter()
            .find(|a| a.kind == AccountKind::Graph)
            .map(|a| a.id)
            .unwrap_or_else(|| self.local_account_id())
    }
}

/// A separate OS window.
pub enum Win {
    Compose(compose::Composer),
    Event(calendar::EventEditor),
    Contact(people::ContactEditor),
    Task(tasks::TaskEditor),
    Options(backstage::OptionsWin),
    AddAccount(backstage::AddAccountWin),
    Rules(rules_ui::RulesWin),
    AutoReply(backstage::AutoReplyWin),
    Note(notes::NoteEditor),
    Journal(notes::JournalEditor),
}

pub struct WinEntry {
    pub id: u64,
    pub win: Win,
}

/// Commands reachable from "Was möchten Sie tun?".
#[derive(Clone, Copy)]
enum Cmd {
    NewMail,
    NewEvent,
    NewContact,
    NewTask,
    SyncAll,
    Options,
    AddAccount,
    Go(Module),
    Design,
}

const COMMANDS: [(&str, Cmd); 11] = [
    ("Neue E-Mail", Cmd::NewMail),
    ("Neuer Termin", Cmd::NewEvent),
    ("Neuer Kontakt", Cmd::NewContact),
    ("Neue Aufgabe", Cmd::NewTask),
    ("Senden/Empfangen – alle Ordner", Cmd::SyncAll),
    ("Optionen", Cmd::Options),
    ("Konto hinzufügen", Cmd::AddAccount),
    ("E-Mail", Cmd::Go(Module::Mail)),
    ("Kalender", Cmd::Go(Module::Calendar)),
    ("Personen", Cmd::Go(Module::People)),
    ("Design wechseln", Cmd::Design),
];

pub struct App {
    st: Arc<AppState>,
    rt: tokio::runtime::Runtime,
    ctx: egui::Context,
    tx: mpsc::Sender<Done>,
    rx: mpsc::Receiver<Done>,
    busy: usize,
    dirty: bool,
    pub t: Theme,
    pub prefs: Prefs,
    module: Module,
    ribbon_tab: usize,
    ribbon_collapsed: bool,
    folder_pane_min: bool,
    backstage: Option<backstage::Page>,
    tell_me: String,
    search: String,
    status: (String, bool, Instant),
    data: Data,
    mail: mail::MailView,
    cal: calendar::CalendarView,
    people: people::PeopleView,
    tasks: tasks::TasksView,
    notes: notes::NotesView,
    reminders: reminders::Reminders,
    windows: Vec<WinEntry>,
    next_window: u64,
    device_code: Option<(String, String)>,
    /// "Freigegebenes Postfach hinzufügen": (owner account, address).
    shared_dialog: Option<(i64, String)>,
}

impl App {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        st: Arc<AppState>,
        rt: tokio::runtime::Runtime,
    ) -> Self {
        let (tx, rx) = mpsc::channel();
        let prefs = Prefs::load(&st.db());
        chrome::setup_fonts(&cc.egui_ctx);
        let mut app = Self {
            st,
            rt,
            ctx: cc.egui_ctx.clone(),
            tx,
            rx,
            busy: 0,
            dirty: true,
            t: Theme::new(prefs.design),
            prefs,
            module: Module::Mail,
            ribbon_tab: 0,
            ribbon_collapsed: false,
            folder_pane_min: false,
            backstage: None,
            tell_me: String::new(),
            search: String::new(),
            status: (
                "Alle Ordner sind auf dem neuesten Stand.".into(),
                false,
                Instant::now(),
            ),
            data: Data::default(),
            mail: Default::default(),
            cal: calendar::CalendarView::new(),
            people: Default::default(),
            tasks: Default::default(),
            notes: Default::default(),
            reminders: Default::default(),
            windows: vec![],
            next_window: 1,
            device_code: None,
            shared_dialog: None,
        };
        app.apply_style();
        app.start_background_sync();
        // Move account secrets of older installations into the OS credential store.
        let st = app.st.clone();
        app.rt
            .spawn_blocking(move || crate::secrets::secure_all(&st));
        app
    }

    fn apply_style(&mut self) {
        self.t = Theme::new(self.prefs.design);
        self.ctx.set_visuals(self.t.visuals());
        self.ctx.set_zoom_factor(self.prefs.zoom);
        self.ctx.style_mut(|s| {
            use egui::{FontFamily::Proportional, FontId, TextStyle};
            s.text_styles
                .insert(TextStyle::Small, FontId::new(11.0, Proportional));
            s.text_styles
                .insert(TextStyle::Body, FontId::new(13.0, Proportional));
            s.text_styles
                .insert(TextStyle::Button, FontId::new(13.0, Proportional));
            s.text_styles
                .insert(TextStyle::Heading, FontId::new(15.0, Proportional));
            s.spacing.item_spacing = egui::vec2(6.0, 5.0);
            s.spacing.button_padding = egui::vec2(8.0, 3.0);
            s.spacing.interact_size.y = 22.0;
        });
    }

    /// Background "Senden/Empfangen" every `MAILCRAFT_SYNC_MINUTES` (default 5).
    fn start_background_sync(&mut self) {
        let minutes: u64 = std::env::var("MAILCRAFT_SYNC_MINUTES")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(5);
        let (st, tx, ctx) = (self.st.clone(), self.tx.clone(), self.ctx.clone());
        self.rt.spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(minutes.max(1) * 60));
            loop {
                tick.tick().await;
                providers::sync_all(&st).await;
                let _ = tx.send(Done::Ok(None));
                ctx.request_repaint();
            }
        });
    }

    /// Run a job on the background runtime. `ok` is shown in the status bar.
    pub fn spawn<F, Fut>(&mut self, ok: Option<&str>, f: F)
    where
        F: FnOnce(Arc<AppState>, mpsc::Sender<Done>) -> Fut,
        Fut: Future<Output = anyhow::Result<()>> + Send + 'static,
    {
        self.busy += 1;
        let fut = f(self.st.clone(), self.tx.clone());
        let (tx, ctx, ok) = (self.tx.clone(), self.ctx.clone(), ok.map(String::from));
        self.rt.spawn(async move {
            let msg = match fut.await {
                Ok(()) => Done::Ok(ok),
                Err(e) => Done::Err(format!("{e:#}")),
            };
            let _ = tx.send(msg);
            ctx.request_repaint();
        });
    }

    /// Message in the status bar; errors stay longer and are shown in red.
    pub fn toast(&mut self, text: impl Into<String>, error: bool) {
        self.status = (text.into(), error, Instant::now());
    }

    pub fn open_window(&mut self, win: Win) -> u64 {
        let id = self.next_window;
        self.next_window += 1;
        self.windows.push(WinEntry { id, win });
        id
    }

    fn handle_done(&mut self) {
        while let Ok(d) = self.rx.try_recv() {
            match d {
                // The periodic sync also sends `Ok(None)`, hence `saturating_sub`.
                Done::Ok(msg) => {
                    self.busy = self.busy.saturating_sub(1);
                    if let Some(m) = msg {
                        self.toast(m, false);
                    }
                    self.dirty = true;
                }
                Done::Err(e) => {
                    self.busy = self.busy.saturating_sub(1);
                    self.toast(e, true);
                    self.dirty = true;
                }
                Done::Sent(id) => {
                    self.busy = self.busy.saturating_sub(1);
                    self.windows.retain(|w| w.id != id);
                    self.toast("Nachricht gesendet.", false);
                    self.dirty = true;
                }
                Done::SendFailed(id, e) => {
                    self.busy = self.busy.saturating_sub(1);
                    for w in &mut self.windows {
                        if let (true, Win::Compose(c)) = (w.id == id, &mut w.win) {
                            c.sending = false;
                        }
                    }
                    self.toast(format!("Senden fehlgeschlagen: {e}"), true);
                }
                Done::DeviceCode { user_code, uri } => {
                    self.device_code = (!user_code.is_empty()).then_some((user_code, uri));
                }
                Done::AutoReply(account, data) => {
                    for w in &mut self.windows {
                        if let Win::AutoReply(a) = &mut w.win {
                            if a.account_id == account {
                                a.loaded(data.clone());
                            }
                        }
                    }
                }
            }
        }
    }

    fn reload(&mut self) {
        let c = self.st.db();
        let res: anyhow::Result<()> = (|| {
            self.data.accounts = db::list_accounts(&c)?;
            self.data.folders = db::list_folders(&c)?;
            self.data.contacts = db::list_contacts(&c)?;
            self.data.tasks = db::list_tasks(&c)?;
            Ok(())
        })();
        drop(c);
        if let Err(e) = res {
            self.toast(format!("Datenbankfehler: {e:#}"), true);
        }
        self.reload_messages();
        self.reload_events();
        self.reload_notes();
        self.dirty = false;
    }

    pub fn save_setting(&mut self, key: &str, value: serde_json::Value) {
        let res = db::set_setting(&self.st.db(), key, &value);
        if let Err(e) = res {
            self.toast(format!("{e:#}"), true);
        }
    }

    pub fn sync_all(&mut self) {
        self.toast("Senden/Empfangen wird ausgeführt …", false);
        self.spawn(
            Some("Alle Ordner sind auf dem neuesten Stand."),
            |st, _| async move {
                providers::sync_all(&st).await;
                Ok(())
            },
        );
    }

    fn run(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::NewMail => self.open_composer(compose::ComposeKind::New),
            Cmd::NewEvent => self.new_event_now(),
            Cmd::NewContact => self.new_contact(),
            Cmd::NewTask => self.new_task(),
            Cmd::SyncAll => self.sync_all(),
            Cmd::Options => self.open_options(),
            Cmd::AddAccount => self.open_add_account(),
            Cmd::Go(m) => self.switch_module(m),
            Cmd::Design => {
                self.prefs.design = if self.prefs.design == Design::Bunt {
                    Design::Schwarz
                } else {
                    Design::Bunt
                };
                self.save_setting("design", serde_json::json!(self.prefs.design.key()));
                self.apply_style();
            }
        }
    }

    pub fn switch_module(&mut self, m: Module) {
        if self.module != m {
            self.module = m;
            self.ribbon_tab = 0;
            self.search.clear();
            self.dirty = true;
        }
    }

    fn window_title(&self) -> String {
        let place = match self.module {
            Module::Mail => {
                let f = self
                    .mail
                    .folder_id
                    .and_then(|id| self.data.folders.iter().find(|f| f.id == id));
                match f {
                    Some(f) => format!(
                        "{} - {}",
                        f.name,
                        self.data
                            .account(f.account_id)
                            .map(|a| a.email.as_str())
                            .unwrap_or_default()
                    ),
                    None => "E-Mail".into(),
                }
            }
            Module::Calendar => "Kalender".into(),
            Module::People => "Kontakte".into(),
            Module::Tasks => "Aufgaben".into(),
            Module::Notes => "Notizen".into(),
            Module::Journal => "Journal".into(),
        };
        format!("{place} - MailCraft")
    }

    // ------------------------------------------------------------ chrome

    fn chrome(&mut self, ctx: &egui::Context) {
        let t = self.t;
        let title = self.window_title();
        egui::TopBottomPanel::top("chrome")
            .frame(egui::Frame::new().fill(t.band))
            .show_separator_line(false)
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                let mut sync = false;
                let mut undo_ribbon = false;
                if chrome::title_bar(ui, &t, &title, |ui| {
                    ui.label(RichText::new(ph::ENVELOPE_SIMPLE).size(16.0));
                    ui.add_space(6.0);
                    sync = chrome::qat_button(
                        ui,
                        &t,
                        ph::ARROWS_CLOCKWISE,
                        "Senden/Empfangen – alle Ordner (F9)",
                    );
                    undo_ribbon = chrome::qat_button(
                        ui,
                        &t,
                        ph::CARET_LINE_DOWN,
                        "Menüband anzeigen/ausblenden (Strg+F1)",
                    );
                }) {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                if sync || ctx.input(|i| i.key_pressed(egui::Key::F9)) {
                    self.sync_all();
                }
                if undo_ribbon || ctx.input(|i| pressed(i, egui::Key::F1, true, false)) {
                    self.ribbon_collapsed = !self.ribbon_collapsed;
                }
                if self.backstage.is_some() {
                    return;
                }
                let tabs: &[&str] = match self.module {
                    Module::Mail => &["Start", "Senden/Empfangen", "Ordner", "Ansicht", "Hilfe"],
                    _ => &["Start", "Senden/Empfangen", "Ordner", "Ansicht", "Hilfe"],
                };
                let mut tab = self.ribbon_tab;
                let mut tell = String::new();
                std::mem::swap(&mut tell, &mut self.tell_me);
                let mut chosen = None;
                let file = chrome::ribbon_tabs(ui, &t, tabs, &mut tab, |ui| {
                    let r = ui.add(
                        egui::TextEdit::singleline(&mut tell)
                            .hint_text(
                                RichText::new(format!("{}  Was möchten Sie tun?", ph::LIGHTBULB))
                                    .color(t.band_text),
                            )
                            .desired_width(220.0)
                            .frame(false)
                            .text_color(t.band_text),
                    );
                    let needle = tell.trim().to_lowercase();
                    if r.has_focus() && !needle.is_empty() {
                        egui::Popup::from_response(&r).open(true).show(|ui| {
                            ui.set_min_width(260.0);
                            for (label, cmd) in COMMANDS
                                .iter()
                                .filter(|(l, _)| l.to_lowercase().contains(&needle))
                            {
                                if ui.add(egui::Button::new(*label).frame(false)).clicked()
                                    || ui.input(|i| i.key_pressed(egui::Key::Enter))
                                {
                                    chosen = Some(*cmd);
                                }
                            }
                        });
                    }
                });
                self.tell_me = tell;
                if let Some(c) = chosen {
                    self.tell_me.clear();
                    self.run(c);
                }
                if file {
                    self.backstage = Some(backstage::Page::Info);
                }
                if tab != self.ribbon_tab {
                    self.ribbon_tab = tab;
                    self.ribbon_collapsed = false;
                }
                if !self.ribbon_collapsed {
                    chrome::ribbon_body(ui, &t, |ui| match self.module {
                        Module::Mail => self.mail_ribbon(ui),
                        Module::Calendar => self.calendar_ribbon(ui),
                        Module::People => self.people_ribbon(ui),
                        Module::Tasks => self.tasks_ribbon(ui),
                        Module::Notes => self.notes_ribbon(ui),
                        Module::Journal => self.journal_ribbon(ui),
                    });
                }
            });
    }

    /// Shared ribbon tabs: Senden/Empfangen, Ordner, Ansicht, Hilfe.
    pub fn common_ribbon(&mut self, ui: &mut egui::Ui) -> bool {
        let t = self.t;
        match self.ribbon_tab {
            1 => {
                chrome::group(ui, &t, "Senden und Empfangen", |ui| {
                    if chrome::large(
                        ui,
                        &t,
                        ph::ARROWS_CLOCKWISE,
                        IC_GREEN,
                        "Senden/Empfangen\nalle Ordner",
                        true,
                    )
                    .clicked()
                    {
                        self.sync_all();
                    }
                    let acc = self.current_account();
                    if chrome::large(
                        ui,
                        &t,
                        ph::FOLDER_SIMPLE,
                        IC_YELLOW,
                        "Ordner\naktualisieren",
                        acc.is_some(),
                    )
                    .clicked()
                    {
                        if let Some(id) = acc {
                            self.spawn(Some("Ordner aktualisiert."), move |st, _| async move {
                                providers::sync(&st, id).await
                            });
                        }
                    }
                });
                true
            }
            2 => {
                chrome::group(ui, &t, "Konten", |ui| {
                    if chrome::large(ui, &t, ph::USER_PLUS, IC_BLUE, "Konto\nhinzufügen", true)
                        .clicked()
                    {
                        self.open_add_account();
                    }
                    if chrome::large(
                        ui,
                        &t,
                        ph::GEAR,
                        chrome::IC_GREY,
                        "Konto-\neinstellungen",
                        true,
                    )
                    .clicked()
                    {
                        self.backstage = Some(backstage::Page::Info);
                    }
                });
                true
            }
            3 => {
                chrome::group(ui, &t, "Design", |ui| {
                    let label = if self.prefs.design == Design::Bunt {
                        "Design:\nSchwarz"
                    } else {
                        "Design:\nBunt"
                    };
                    if chrome::large(ui, &t, ph::PALETTE, IC_PURPLE, label, true).clicked() {
                        self.run(Cmd::Design);
                    }
                });
                chrome::group(ui, &t, "Layout", |ui| {
                    let r = chrome::large(
                        ui,
                        &t,
                        ph::SIDEBAR,
                        IC_BLUE,
                        "Ordnerbereich\n\u{E136}",
                        true,
                    );
                    chrome::menu(&r, |ui| {
                        if ui.radio(!self.folder_pane_min, "Normal").clicked() {
                            self.folder_pane_min = false;
                            ui.close();
                        }
                        if ui.radio(self.folder_pane_min, "Minimiert").clicked() {
                            self.folder_pane_min = true;
                            ui.close();
                        }
                    });
                });
                chrome::group(ui, &t, "Zoom", |ui| {
                    chrome::column(ui, |ui| {
                        for (label, z) in [("100 %", 1.0), ("125 %", 1.25), ("150 %", 1.5)] {
                            if chrome::small(
                                ui,
                                &t,
                                ph::MAGNIFYING_GLASS_PLUS,
                                IC_BLUE,
                                label,
                                true,
                            )
                            .clicked()
                            {
                                self.set_zoom(z);
                            }
                        }
                    });
                });
                false
            }
            4 => {
                chrome::group(ui, &t, "Hilfe", |ui| {
                    let r = chrome::large(
                        ui,
                        &t,
                        ph::KEYBOARD,
                        chrome::IC_GREY,
                        "Tasten-\nkombinationen",
                        true,
                    );
                    chrome::menu(&r, |ui| {
                        egui::Grid::new("keys")
                            .num_columns(2)
                            .spacing([16.0, 4.0])
                            .show(ui, |ui| {
                                for (k, d) in [
                                    ("Strg+N", "Neues Element"),
                                    ("Strg+R / Strg+Umschalt+R", "Antworten / Allen antworten"),
                                    ("Strg+F", "Weiterleiten"),
                                    ("Entf", "Löschen"),
                                    ("Strg+Q / Strg+U", "Als gelesen / ungelesen markieren"),
                                    ("Einfg", "Zur Nachverfolgung kennzeichnen"),
                                    ("Strg+E", "Suchen"),
                                    ("Alt+S", "Senden (im Nachrichtenfenster)"),
                                    ("Strg+1 … 4", "E-Mail, Kalender, Personen, Aufgaben"),
                                    ("F9", "Senden/Empfangen – alle Ordner"),
                                    ("Strg+F1", "Menüband ein-/ausblenden"),
                                ] {
                                    ui.label(RichText::new(k).strong());
                                    ui.label(d);
                                    ui.end_row();
                                }
                            });
                    });
                    let r = chrome::large(ui, &t, ph::INFO, IC_BLUE, "Info", true);
                    chrome::menu(&r, |ui| {
                        ui.label(RichText::new("MailCraft").size(18.0).strong());
                        ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
                        ui.label("E-Mail, Kalender, Personen und Aufgaben –\nIMAP/SMTP und Microsoft 365.");
                    });
                });
                true
            }
            _ => false,
        }
    }

    fn set_zoom(&mut self, z: f32) {
        self.prefs.zoom = z.clamp(0.5, 2.0);
        self.ctx.set_zoom_factor(self.prefs.zoom);
        self.save_setting("zoom", serde_json::json!(self.prefs.zoom));
    }

    /// Account of the selected mail folder.
    fn current_account(&self) -> Option<i64> {
        self.mail
            .folder_id
            .and_then(|id| self.data.folders.iter().find(|f| f.id == id))
            .map(|f| f.account_id)
    }

    fn status_bar(&mut self, ctx: &egui::Context) {
        let t = self.t;
        egui::TopBottomPanel::bottom("status")
            .exact_height(24.0)
            .frame(
                egui::Frame::new()
                    .fill(t.band)
                    .inner_margin(egui::Margin::symmetric(10, 0)),
            )
            .show_separator_line(false)
            .show(ctx, |ui| {
                ui.style_mut().visuals.override_text_color = Some(t.band_text);
                ui.horizontal_centered(|ui| {
                    let small = |s: String| RichText::new(s).size(12.0);
                    match self.module {
                        Module::Mail => {
                            let (n, u) = (self.mail.count(), self.mail.unread());
                            ui.label(small(format!("Elemente: {n}")));
                            ui.add_space(14.0);
                            if u > 0 {
                                ui.label(small(format!("Ungelesen: {u}")));
                            }
                        }
                        Module::Calendar => {
                            ui.label(small(format!("Elemente: {}", self.cal.count())));
                        }
                        Module::People => {
                            ui.label(small(format!("Elemente: {}", self.data.contacts.len())));
                        }
                        Module::Tasks => {
                            let open = self.data.tasks.iter().filter(|t| !t.completed).count();
                            ui.label(small(format!("Elemente: {}", self.data.tasks.len())));
                            ui.add_space(14.0);
                            ui.label(small(format!("Nicht erledigt: {open}")));
                        }
                        Module::Notes => {
                            ui.label(small(format!("Elemente: {}", self.notes.notes.len())));
                        }
                        Module::Journal => {
                            ui.label(small(format!("Elemente: {}", self.notes.journal.len())));
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // Zoom slider like Office.
                        let mut z = (self.prefs.zoom * 100.0).round();
                        ui.label(small(format!("{z:.0} %")));
                        if ui
                            .add(
                                egui::Slider::new(&mut z, 50.0..=200.0)
                                    .show_value(false)
                                    .step_by(10.0),
                            )
                            .changed()
                        {
                            self.set_zoom(z / 100.0);
                        }
                        ui.add_space(12.0);
                        let connected = self
                            .data
                            .accounts
                            .iter()
                            .any(|a| a.kind != AccountKind::Local);
                        let err = self.data.accounts.iter().find_map(|a| a.last_error.clone());
                        if let Some(e) = err {
                            ui.label(small(format!("{}  Getrennt", ph::WARNING)))
                                .on_hover_text(e);
                        } else if connected {
                            ui.label(small(format!("{}  Verbunden", ph::PLUG)));
                        } else {
                            ui.label(small(format!("{}  Nur lokal", ph::DESKTOP)));
                        }
                        ui.add_space(16.0);
                        let (text, error, at) = &self.status;
                        let shown = if self.busy > 0 {
                            "Senden/Empfangen wird ausgeführt …".to_string()
                        } else if at.elapsed() < Duration::from_secs(if *error { 30 } else { 6 }) {
                            text.clone()
                        } else {
                            "Alle Ordner sind auf dem neuesten Stand.".into()
                        };
                        if self.busy > 0 {
                            ui.add(egui::Spinner::new().size(12.0).color(t.band_text));
                        }
                        let rt = RichText::new(&shown).size(12.0);
                        let r = ui.add(
                            egui::Label::new(if *error && shown == *text {
                                rt.background_color(t.danger)
                            } else {
                                rt
                            })
                            .truncate(),
                        );
                        if *error {
                            r.on_hover_text(text);
                        }
                    });
                });
            });
        if self.busy > 0 || self.status.2.elapsed() < Duration::from_secs(31) {
            ctx.request_repaint_after(Duration::from_millis(500));
        }
    }

    /// Folder pane with the module switcher at the bottom (Outlook 2016 style).
    fn navigation(&mut self, ctx: &egui::Context) {
        let t = self.t;
        let width = if self.folder_pane_min { 44.0 } else { 260.0 };
        egui::SidePanel::left("nav")
            .resizable(!self.folder_pane_min)
            .default_width(width)
            .width_range(if self.folder_pane_min {
                44.0..=44.0
            } else {
                180.0..=420.0
            })
            .frame(
                egui::Frame::new()
                    .fill(t.pane)
                    .stroke(egui::Stroke::new(1.0_f32, t.border)),
            )
            .show(ctx, |ui| {
                egui::TopBottomPanel::bottom("modules")
                    .frame(
                        egui::Frame::new()
                            .fill(t.pane)
                            .inner_margin(egui::Margin::symmetric(6, 6)),
                    )
                    .show_inside(ui, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing.x = 4.0;
                            for (m, icon, label) in [
                                (Module::Mail, ph::ENVELOPE_SIMPLE, "E-Mail"),
                                (Module::Calendar, ph::CALENDAR_BLANK, "Kalender"),
                                (Module::People, ph::USERS, "Personen"),
                                (Module::Tasks, ph::CHECK_SQUARE_OFFSET, "Aufgaben"),
                            ] {
                                let active = self.module == m;
                                let color = if active { t.accent } else { t.text };
                                let text = if self.folder_pane_min || ui.available_width() < 236.0 {
                                    RichText::new(icon).size(20.0)
                                } else {
                                    RichText::new(label).size(13.0)
                                };
                                let r = ui.add(
                                    egui::Button::new(text.color(color).strong_if(active))
                                        .frame(false),
                                );
                                if r.on_hover_text(label).clicked() {
                                    self.switch_module(m);
                                }
                            }
                            // Outlook keeps Notizen and Journal behind "···".
                            let more_active =
                                matches!(self.module, Module::Notes | Module::Journal);
                            let r = ui.add(
                                egui::Button::new(
                                    RichText::new(ph::DOTS_THREE)
                                        .size(18.0)
                                        .color(if more_active { t.accent } else { t.text }),
                                )
                                .frame(false),
                            );
                            chrome::menu(&r, |ui| {
                                if chrome::menu_item(ui, ph::NOTE, IC_YELLOW, "Notizen (Strg+5)") {
                                    self.switch_module(Module::Notes);
                                }
                                if chrome::menu_item(ui, ph::BOOK_OPEN, IC_BLUE, "Journal (Strg+8)")
                                {
                                    self.switch_module(Module::Journal);
                                }
                            });
                        });
                    });
                if self.folder_pane_min {
                    ui.vertical_centered(|ui| {
                        if ui
                            .add(
                                egui::Button::new(RichText::new(ph::CARET_RIGHT).size(16.0))
                                    .frame(false),
                            )
                            .clicked()
                        {
                            self.folder_pane_min = false;
                        }
                    });
                    return;
                }
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add(
                                egui::Button::new(RichText::new(ph::CARET_LEFT).size(14.0))
                                    .frame(false),
                            )
                            .on_hover_text("Ordnerbereich minimieren")
                            .clicked()
                        {
                            self.folder_pane_min = true;
                        }
                    });
                });
                match self.module {
                    Module::Mail => self.mail_folders(ui),
                    Module::Calendar => self.calendar_nav(ui),
                    Module::People => self.people_nav(ui),
                    Module::Tasks => self.tasks_nav(ui),
                    Module::Notes => self.notes_nav(ui),
                    Module::Journal => self.journal_nav(ui),
                }
            });
        for (k, m) in [
            (egui::Key::Num1, Module::Mail),
            (egui::Key::Num2, Module::Calendar),
            (egui::Key::Num3, Module::People),
            (egui::Key::Num4, Module::Tasks),
            (egui::Key::Num5, Module::Notes),
            (egui::Key::Num8, Module::Journal),
        ] {
            if ctx.input(|i| pressed(i, k, true, false)) {
                self.switch_module(m);
            }
        }
        if ctx.input(|i| pressed(i, egui::Key::N, true, false)) {
            match self.module {
                Module::Mail => self.open_composer(compose::ComposeKind::New),
                Module::Calendar => self.new_event_now(),
                Module::People => self.new_contact(),
                Module::Tasks => self.new_task(),
                Module::Notes => self.new_note(),
                Module::Journal => self.new_journal_entry("Telefonanruf"),
            }
        }
    }

    fn windows_ui(&mut self, ctx: &egui::Context) {
        let mut windows = std::mem::take(&mut self.windows);
        let mut closed = vec![];
        for w in &mut windows {
            let vid = egui::ViewportId::from_hash_of(("mailcraft-window", w.id));
            let close = match &mut w.win {
                Win::Compose(c) => self.compose_window(ctx, vid, w.id, c),
                Win::Event(e) => self.event_window(ctx, vid, e),
                Win::Contact(c) => self.contact_window(ctx, vid, c),
                Win::Task(t) => self.task_window(ctx, vid, t),
                Win::Options(o) => self.options_window(ctx, vid, o),
                Win::AddAccount(a) => self.add_account_window(ctx, vid, a),
                Win::Rules(r) => self.rules_window(ctx, vid, r),
                Win::AutoReply(a) => self.auto_reply_window(ctx, vid, a),
                Win::Note(n) => self.note_window(ctx, vid, n),
                Win::Journal(j) => self.journal_window(ctx, vid, j),
            };
            if close {
                closed.push(w.id);
            }
        }
        // Windows opened while drawing (e.g. "Element öffnen") were pushed to `self.windows`.
        windows.append(&mut self.windows);
        windows.retain(|w| !closed.contains(&w.id));
        self.windows = windows;
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.handle_done();
        if self.dirty {
            self.reload();
        }
        chrome::resize_edges(ctx);
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(self.window_title()));
        self.chrome(ctx);
        self.status_bar(ctx);
        if self.backstage.is_some() {
            self.backstage_ui(ctx);
        } else {
            self.navigation(ctx);
            egui::CentralPanel::default()
                .frame(egui::Frame::new().fill(self.t.pane))
                .show(ctx, |ui| match self.module {
                    Module::Mail => self.mail_ui(ui),
                    Module::Calendar => self.calendar_ui(ui),
                    Module::People => self.people_ui(ui),
                    Module::Tasks => self.tasks_ui(ui),
                    Module::Notes => self.notes_ui(ui),
                    Module::Journal => self.journal_ui(ui),
                });
        }
        self.windows_ui(ctx);
        self.reminders_ui(ctx);
    }
}

// ---------------------------------------------------------------- shared widgets & helpers

trait StrongIf {
    fn strong_if(self, on: bool) -> Self;
}

impl StrongIf for RichText {
    fn strong_if(self, on: bool) -> Self {
        if on {
            self.strong()
        } else {
            self
        }
    }
}

/// Keyboard shortcut check that uses the modifiers of the key event itself:
/// with a quick Ctrl+X the Ctrl key can already be released before the end
/// of the frame, so the current modifier state is not reliable.
pub fn pressed(i: &egui::InputState, key: egui::Key, cmd: bool, shift: bool) -> bool {
    i.events.iter().any(|e| {
        matches!(e, egui::Event::Key { key: k, pressed: true, modifiers, .. }
            if *k == key && modifiers.command == cmd && modifiers.shift == shift && !modifiers.alt)
    })
}

/// Like [`pressed`] for Alt+key.
pub fn pressed_alt(i: &egui::InputState, key: egui::Key) -> bool {
    i.events.iter().any(|e| matches!(e, egui::Event::Key { key: k, pressed: true, modifiers, .. } if *k == key && modifiers.alt))
}

/// Lay out `text` on a single line, cut off with "…" at `width`.
pub fn one_line(
    painter: &egui::Painter,
    text: &str,
    font: egui::FontId,
    color: Color32,
    width: f32,
) -> Arc<egui::Galley> {
    let flat: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let mut job = egui::text::LayoutJob::simple_singleline(flat, font, color);
    job.wrap = egui::text::TextWrapping {
        max_width: width.max(10.0),
        max_rows: 1,
        break_anywhere: true,
        overflow_character: Some('…'),
    };
    painter.layout_job(job)
}

/// Search box above lists ("Aktuelles Postfach durchsuchen").
pub fn search_box(ui: &mut egui::Ui, t: &Theme, value: &mut String, hint: &str) -> egui::Response {
    let mut resp = None;
    egui::Frame::new()
        .fill(t.pane)
        .stroke(egui::Stroke::new(1.0_f32, t.border))
        .inner_margin(egui::Margin::symmetric(6, 3))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let r = ui.add(
                    egui::TextEdit::singleline(value)
                        .hint_text(hint)
                        .frame(false)
                        .desired_width(ui.available_width() - 24.0),
                );
                if ui.input(|i| pressed(i, egui::Key::E, true, false)) {
                    r.request_focus();
                }
                if !value.is_empty() {
                    if ui.add(egui::Button::new(ph::X).frame(false)).clicked() {
                        value.clear();
                    }
                } else {
                    ui.label(RichText::new(ph::MAGNIFYING_GLASS).color(t.accent));
                }
                resp = Some(r);
            });
        });
    resp.unwrap()
}

pub fn initials(name: &str) -> String {
    let parts: Vec<&str> = name
        .split_whitespace()
        .filter(|p| p.chars().next().is_some_and(|c| c.is_alphanumeric()))
        .collect();
    match parts.as_slice() {
        [] => "?".into(),
        [one] => one.chars().take(2).collect::<String>().to_uppercase(),
        [first, .., last] => format!(
            "{}{}",
            first.chars().next().unwrap(),
            last.chars().next().unwrap()
        )
        .to_uppercase(),
    }
}

pub fn avatar_color(name: &str) -> Color32 {
    const PALETTE: [Color32; 8] = [
        Color32::from_rgb(164, 38, 44),
        Color32::from_rgb(0, 120, 212),
        Color32::from_rgb(16, 124, 16),
        Color32::from_rgb(135, 100, 184),
        Color32::from_rgb(202, 80, 16),
        Color32::from_rgb(3, 131, 135),
        Color32::from_rgb(92, 46, 145),
        Color32::from_rgb(142, 86, 46),
    ];
    let h = name
        .bytes()
        .fold(0u32, |a, b| a.wrapping_mul(31).wrapping_add(b as u32));
    PALETTE[(h as usize) % PALETTE.len()]
}

pub fn paint_avatar(painter: &egui::Painter, center: egui::Pos2, radius: f32, name: &str) {
    painter.circle_filled(center, radius, avatar_color(name));
    painter.text(
        center,
        egui::Align2::CENTER_CENTER,
        initials(name),
        egui::FontId::proportional(radius * 0.8),
        Color32::WHITE,
    );
}

pub fn avatar(ui: &mut egui::Ui, size: f32, name: &str) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), Sense::hover());
    paint_avatar(ui.painter(), rect.center(), size / 2.0, name);
}

pub fn parse_utc(s: &str) -> Option<DateTime<Local>> {
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|d| d.with_timezone(&Local))
}

pub fn local_to_utc_string(dt: NaiveDateTime) -> String {
    Local
        .from_local_datetime(&dt)
        .earliest()
        .unwrap_or_else(|| Utc.from_utc_datetime(&dt).with_timezone(&Local))
        .with_timezone(&Utc)
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string()
}

pub fn today() -> NaiveDate {
    Local::now().date_naive()
}

/// Outlook list date: time for today, "Mi 14:02" this week, else "29.10.2026".
pub fn short_date(s: &str) -> String {
    let Some(d) = parse_utc(s) else {
        return String::new();
    };
    let days = (today() - d.date_naive()).num_days();
    if days == 0 {
        d.format("%H:%M").to_string()
    } else if (1..7).contains(&days) {
        format!(
            "{} {}",
            WEEKDAYS[d.weekday().num_days_from_monday() as usize],
            d.format("%H:%M")
        )
    } else {
        d.format("%d.%m.%Y").to_string()
    }
}

/// "Do 29.10.2026 14:02" like Outlook's reading pane.
pub fn long_date(s: &str) -> String {
    let Some(d) = parse_utc(s) else {
        return s.to_string();
    };
    format!(
        "{} {}",
        WEEKDAYS[d.weekday().num_days_from_monday() as usize],
        d.format("%d.%m.%Y %H:%M")
    )
}

pub fn date_long(d: NaiveDate) -> String {
    format!(
        "{}, {}. {} {}",
        WEEKDAYS_LONG[d.weekday().num_days_from_monday() as usize],
        d.day(),
        MONTHS[d.month0() as usize],
        d.year()
    )
}

/// Open a URL or file with the system's default handler.
pub fn open_external(target: &str) {
    #[cfg(target_os = "windows")]
    let r = std::process::Command::new("cmd")
        .args(["/C", "start", "", target])
        .spawn();
    #[cfg(target_os = "macos")]
    let r = std::process::Command::new("open").arg(target).spawn();
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let r = std::process::Command::new("xdg-open").arg(target).spawn();
    if let Err(e) = r {
        tracing::warn!("Konnte {target} nicht öffnen: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn makes_initials() {
        assert_eq!(initials("Anna Schmidt"), "AS");
        assert_eq!(initials("Max von der Heide"), "MH");
        assert_eq!(initials("team"), "TE");
        assert_eq!(initials(""), "?");
    }

    #[test]
    fn local_utc_roundtrip() {
        let dt = NaiveDate::from_ymd_opt(2026, 7, 1)
            .unwrap()
            .and_hms_opt(9, 30, 0)
            .unwrap();
        let s = local_to_utc_string(dt);
        assert_eq!(parse_utc(&s).unwrap().naive_local(), dt);
    }

    #[test]
    fn category_colors_accept_short_names() {
        assert_eq!(category_color("Rote Kategorie"), CATEGORIES[0].1);
        assert_eq!(category_color("Grün"), CATEGORIES[3].1);
    }
}
