//! Native GUI (egui). Layout follows Outlook: blue title bar with search,
//! app rail on the left (Mail, Kalender, Personen, To Do) and the module's
//! panes next to it.
//!
//! The UI thread only reads from SQLite (fast, local). Everything that may
//! touch the network is spawned on the Tokio runtime via [`App::spawn`]; its
//! result comes back through a channel as a [`Done`] and triggers a reload.

mod calendar;
mod compose;
mod mail;
mod people;
mod settings;
mod tasks;

use std::{
    future::Future,
    sync::{mpsc, Arc},
    time::{Duration, Instant},
};

use chrono::{DateTime, Datelike, Local, NaiveDate, NaiveDateTime, TimeZone, Utc};
use eframe::egui::{self, Color32, RichText};

use crate::{db, models::*, providers, state::AppState};

pub const BRAND: Color32 = Color32::from_rgb(15, 108, 189);
pub const BRAND_DARK: Color32 = Color32::from_rgb(10, 46, 79);
pub const DANGER: Color32 = Color32::from_rgb(197, 15, 31);

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
    ("Rot", Color32::from_rgb(232, 17, 35)),
    ("Orange", Color32::from_rgb(247, 99, 12)),
    ("Gelb", Color32::from_rgb(255, 185, 0)),
    ("Grün", Color32::from_rgb(16, 124, 16)),
    ("Blau", Color32::from_rgb(0, 120, 212)),
    ("Lila", Color32::from_rgb(136, 23, 152)),
];

pub fn category_color(name: &str) -> Color32 {
    CATEGORIES
        .iter()
        .find(|c| c.0 == name)
        .map(|c| c.1)
        .unwrap_or(BRAND)
}

/// Result of a background job.
pub enum Done {
    /// Success; optional toast text. Always triggers a reload.
    Ok(Option<String>),
    Err(String),
    /// A composer's mail was sent; close that window.
    Sent(u64),
    /// Sending failed; keep the composer open.
    SendFailed(u64, String),
    /// Microsoft sign-in: show this code to the user.
    DeviceCode {
        user_code: String,
        uri: String,
    },
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Module {
    Mail,
    Calendar,
    People,
    Tasks,
}

/// Data shared by all modules, reloaded from the database when `dirty`.
#[derive(Default)]
pub struct Data {
    pub accounts: Vec<Account>,
    pub folders: Vec<Folder>,
    pub contacts: Vec<Contact>,
    pub tasks: Vec<Task>,
    pub signature: String,
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
}

pub struct App {
    st: Arc<AppState>,
    rt: tokio::runtime::Runtime,
    ctx: egui::Context,
    tx: mpsc::Sender<Done>,
    rx: mpsc::Receiver<Done>,
    busy: usize,
    dirty: bool,
    dark: bool,
    module: Module,
    search: String,
    toast: Option<(String, bool, Instant)>,
    data: Data,
    mail: mail::MailView,
    cal: calendar::CalendarView,
    people: people::PeopleView,
    tasks: tasks::TasksView,
    settings: settings::SettingsView,
    composers: Vec<compose::Composer>,
    next_composer: u64,
}

impl App {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        st: Arc<AppState>,
        rt: tokio::runtime::Runtime,
    ) -> Self {
        let (tx, rx) = mpsc::channel();
        let dark = db::get_settings(&st.db())
            .ok()
            .and_then(|s| s.get("dark").and_then(|v| v.as_bool()))
            .unwrap_or(false);
        let mut app = Self {
            st,
            rt,
            ctx: cc.egui_ctx.clone(),
            tx,
            rx,
            busy: 0,
            dirty: true,
            dark,
            module: Module::Mail,
            search: String::new(),
            toast: None,
            data: Data::default(),
            mail: Default::default(),
            cal: calendar::CalendarView::new(),
            people: Default::default(),
            tasks: Default::default(),
            settings: Default::default(),
            composers: vec![],
            next_composer: 1,
        };
        app.apply_style();
        app.start_background_sync();
        app
    }

    fn apply_style(&self) {
        let mut v = if self.dark {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        };
        v.selection.bg_fill = if self.dark {
            Color32::from_rgb(12, 59, 94)
        } else {
            Color32::from_rgb(207, 228, 250)
        };
        v.selection.stroke.color = if self.dark {
            Color32::WHITE
        } else {
            Color32::from_rgb(36, 36, 36)
        };
        v.hyperlink_color = BRAND;
        if !self.dark {
            v.panel_fill = Color32::from_rgb(250, 250, 250);
            v.window_fill = Color32::WHITE;
        }
        self.ctx.set_visuals(v);
        self.ctx.style_mut(|s| {
            use egui::{FontFamily::Proportional, FontId, TextStyle};
            s.text_styles
                .insert(TextStyle::Small, FontId::new(12.0, Proportional));
            s.text_styles
                .insert(TextStyle::Body, FontId::new(14.0, Proportional));
            s.text_styles
                .insert(TextStyle::Button, FontId::new(14.0, Proportional));
            s.text_styles
                .insert(TextStyle::Heading, FontId::new(20.0, Proportional));
            s.spacing.item_spacing = egui::vec2(8.0, 6.0);
            s.spacing.button_padding = egui::vec2(8.0, 4.0);
        });
    }

    /// Initial sync and then every `MAILCRAFT_SYNC_MINUTES` (default 5).
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

    /// Run a job on the background runtime. `ok` is shown as toast on success.
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

    pub fn toast(&mut self, text: impl Into<String>, error: bool) {
        self.toast = Some((text.into(), error, Instant::now()));
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
                    self.composers.retain(|c| c.id != id);
                    self.toast("Nachricht gesendet", false);
                    self.dirty = true;
                }
                Done::SendFailed(id, e) => {
                    self.busy = self.busy.saturating_sub(1);
                    if let Some(c) = self.composers.iter_mut().find(|c| c.id == id) {
                        c.sending = false;
                    }
                    self.toast(format!("Senden fehlgeschlagen: {e}"), true);
                }
                Done::DeviceCode { user_code, uri } => {
                    self.settings.device_code = Some((user_code, uri));
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
            self.data.signature = db::get_settings(&c)?
                .get("signature")
                .and_then(|v| v.as_str().map(String::from))
                .unwrap_or_default();
            Ok(())
        })();
        drop(c);
        if let Err(e) = res {
            self.toast(format!("Datenbankfehler: {e:#}"), true);
        }
        self.reload_messages();
        self.reload_events();
        self.dirty = false;
    }

    pub fn save_setting(&mut self, key: &str, value: serde_json::Value) {
        let res = db::set_setting(&self.st.db(), key, &value);
        if let Err(e) = res {
            self.toast(format!("{e:#}"), true);
        }
    }

    fn top_bar(&mut self, ctx: &egui::Context) {
        let fill = if self.dark { BRAND_DARK } else { BRAND };
        egui::TopBottomPanel::top("topbar")
            .exact_height(48.0)
            .frame(
                egui::Frame::new()
                    .fill(fill)
                    .inner_margin(egui::Margin::symmetric(12, 6)),
            )
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.label(
                        RichText::new("MailCraft")
                            .size(17.0)
                            .strong()
                            .color(Color32::WHITE),
                    );
                    ui.add_space(80.0);
                    let hint = match self.module {
                        Module::Mail => "🔍 E-Mails durchsuchen",
                        Module::Calendar => "🔍 Termine durchsuchen",
                        Module::People => "🔍 Personen durchsuchen",
                        Module::Tasks => "🔍 Aufgaben durchsuchen",
                    };
                    let search = ui.add(
                        egui::TextEdit::singleline(&mut self.search)
                            .hint_text(hint)
                            .desired_width(ui.available_width().min(520.0) - 160.0)
                            .margin(egui::Margin::symmetric(8, 6)),
                    );
                    if search.changed() && self.module == Module::Mail {
                        self.reload_messages();
                    }
                    if ctx.input(|i| i.key_pressed(egui::Key::Slash)) && !ctx.wants_keyboard_input()
                    {
                        search.request_focus();
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let white = |s: &str| RichText::new(s).size(18.0).color(Color32::WHITE);
                        if ui
                            .add(egui::Button::new(white("⚙")).frame(false))
                            .on_hover_text("Einstellungen")
                            .clicked()
                        {
                            self.settings.open = true;
                        }
                        let theme = if self.dark { "☀" } else { "🌙" };
                        if ui
                            .add(egui::Button::new(white(theme)).frame(false))
                            .on_hover_text("Design wechseln")
                            .clicked()
                        {
                            self.dark = !self.dark;
                            self.apply_style();
                            self.save_setting("dark", serde_json::json!(self.dark));
                        }
                        if ui
                            .add(egui::Button::new(white("🔄")).frame(false))
                            .on_hover_text("Alle Konten synchronisieren (F9)")
                            .clicked()
                            || ctx.input(|i| i.key_pressed(egui::Key::F9))
                        {
                            self.spawn(
                                Some("Synchronisierung abgeschlossen"),
                                |st, _| async move {
                                    providers::sync_all(&st).await;
                                    Ok(())
                                },
                            );
                        }
                        if self.busy > 0 {
                            ui.add(egui::Spinner::new().color(Color32::WHITE));
                        }
                    });
                });
            });
    }

    fn rail(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("rail")
            .exact_width(56.0)
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .inner_margin(egui::Margin::symmetric(6, 10))
                    .fill(ctx.style().visuals.panel_fill),
            )
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    for (m, icon, tip) in [
                        (Module::Mail, "✉", "E-Mail (Strg+1)"),
                        (Module::Calendar, "📅", "Kalender (Strg+2)"),
                        (Module::People, "👤", "Personen (Strg+3)"),
                        (Module::Tasks, "✔", "To Do (Strg+4)"),
                    ] {
                        let active = self.module == m;
                        let text = RichText::new(icon).size(22.0).color(if active {
                            BRAND
                        } else {
                            ui.visuals().weak_text_color()
                        });
                        let r = ui.add_sized(
                            [44.0, 44.0],
                            egui::Button::new(text).frame(active).selected(active),
                        );
                        if r.on_hover_text(tip).clicked() {
                            self.switch_module(m);
                        }
                        ui.add_space(4.0);
                    }
                });
            });
        let keys = [
            (egui::Key::Num1, Module::Mail),
            (egui::Key::Num2, Module::Calendar),
            (egui::Key::Num3, Module::People),
            (egui::Key::Num4, Module::Tasks),
        ];
        for (k, m) in keys {
            if ctx.input(|i| i.modifiers.command && i.key_pressed(k)) {
                self.switch_module(m);
            }
        }
    }

    fn switch_module(&mut self, m: Module) {
        if self.module != m {
            self.module = m;
            self.search.clear();
            self.dirty = true;
        }
    }

    fn toast_ui(&mut self, ctx: &egui::Context) {
        let Some((text, error, at)) = &self.toast else {
            return;
        };
        let ttl = if *error { 8.0 } else { 3.5 };
        if at.elapsed().as_secs_f32() > ttl {
            self.toast = None;
            return;
        }
        ctx.request_repaint_after(Duration::from_millis(250));
        let (text, error) = (text.clone(), *error);
        egui::Area::new(egui::Id::new("toast"))
            .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -28.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(if error {
                        DANGER
                    } else {
                        Color32::from_gray(50)
                    })
                    .corner_radius(6)
                    .inner_margin(egui::Margin::symmetric(16, 10))
                    .show(ui, |ui| {
                        ui.set_max_width(700.0);
                        ui.label(RichText::new(text).color(Color32::WHITE));
                    });
            });
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.handle_done();
        if self.dirty {
            self.reload();
        }
        self.top_bar(ctx);
        self.rail(ctx);
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(ctx.style().visuals.panel_fill)
                    .inner_margin(egui::Margin {
                        left: 0,
                        right: 8,
                        top: 8,
                        bottom: 8,
                    }),
            )
            .show(ctx, |ui| match self.module {
                Module::Mail => self.mail_ui(ui),
                Module::Calendar => self.calendar_ui(ui),
                Module::People => self.people_ui(ui),
                Module::Tasks => self.tasks_ui(ui),
            });
        self.composers_ui(ctx);
        self.event_editor_ui(ctx);
        self.contact_editor_ui(ctx);
        self.settings_ui(ctx);
        self.toast_ui(ctx);
    }
}

// ---------------------------------------------------------------- shared widgets & helpers

pub fn pane_frame(ui: &egui::Ui) -> egui::Frame {
    egui::Frame::new()
        .fill(ui.visuals().window_fill)
        .corner_radius(6)
        .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
}

pub fn ribbon_button(ui: &mut egui::Ui, icon: &str, label: &str, enabled: bool) -> egui::Response {
    let text = RichText::new(format!("{icon}  {label}"));
    ui.add_enabled(enabled, egui::Button::new(text).frame(false))
}

pub fn primary_button(ui: &mut egui::Ui, label: &str) -> egui::Response {
    ui.add(egui::Button::new(RichText::new(label).color(Color32::WHITE).strong()).fill(BRAND))
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
        Color32::from_rgb(196, 49, 75),
        Color32::from_rgb(0, 120, 212),
        Color32::from_rgb(16, 124, 16),
        Color32::from_rgb(136, 23, 152),
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
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
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

/// Outlook-style list date: time for today, weekday for this week, else date.
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

pub fn long_date(s: &str) -> String {
    let Some(d) = parse_utc(s) else {
        return s.to_string();
    };
    format!(
        "{}, {}. {} {} {}",
        WEEKDAYS_LONG[d.weekday().num_days_from_monday() as usize],
        d.day(),
        MONTHS[d.month0() as usize],
        d.year(),
        d.format("%H:%M")
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
}
