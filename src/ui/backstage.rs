//! "Datei" backstage (Kontoinformationen), the options dialog and the
//! "Konto hinzufügen" wizard.

use std::time::Instant;

use eframe::egui::{self, Color32, RichText, Sense};
use egui_phosphor::regular as ph;

use super::{
    chrome::{self, IC_BLUE},
    theme::Design,
    App, ReadingPane, Win,
};
use crate::{db, models::*, providers};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Info,
    About,
}

impl App {
    pub fn open_options(&mut self) {
        let o = OptionsWin {
            cat: 0,
            design: self.prefs.design,
            signature: self.prefs.signature.clone(),
            mark_read: self.prefs.mark_read,
            preview: self.prefs.preview,
            reading_pane: self.prefs.reading_pane,
            work_start: self.prefs.work_start,
            work_end: self.prefs.work_end,
        };
        self.open_window(Win::Options(o));
    }

    pub fn open_add_account(&mut self) {
        self.device_code = None;
        self.open_window(Win::AddAccount(AddAccountWin::default()));
    }

    pub fn backstage_ui(&mut self, ctx: &egui::Context) {
        let t = self.t;
        let page = self.backstage.unwrap_or(Page::Info);
        egui::SidePanel::left("backstage_nav")
            .exact_width(190.0)
            .resizable(false)
            .frame(egui::Frame::new().fill(t.band))
            .show(ctx, |ui| {
                ui.style_mut().visuals.override_text_color = Some(t.band_text);
                ui.add_space(14.0);
                ui.horizontal(|ui| {
                    ui.add_space(14.0);
                    let (r, resp) = ui.allocate_exact_size(egui::vec2(36.0, 36.0), Sense::click());
                    ui.painter().circle_stroke(
                        r.center(),
                        16.0,
                        egui::Stroke::new(1.5_f32, t.band_text),
                    );
                    if resp.hovered() {
                        ui.painter().circle_filled(r.center(), 15.0, t.band_hover);
                    }
                    ui.painter().text(
                        r.center(),
                        egui::Align2::CENTER_CENTER,
                        ph::ARROW_LEFT,
                        egui::FontId::proportional(18.0),
                        t.band_text,
                    );
                    if resp.on_hover_text("Zurück").clicked() {
                        self.backstage = None;
                    }
                });
                ui.add_space(18.0);
                let item = |ui: &mut egui::Ui, label: &str, active: bool| -> bool {
                    let (r, resp) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width(), 40.0),
                        Sense::click(),
                    );
                    if active {
                        ui.painter()
                            .rect_filled(r, 0, Color32::from_black_alpha(60));
                    } else if resp.hovered() {
                        ui.painter().rect_filled(r, 0, t.band_hover);
                    }
                    ui.painter().text(
                        r.left_center() + egui::vec2(26.0, 0.0),
                        egui::Align2::LEFT_CENTER,
                        label,
                        egui::FontId::proportional(15.0),
                        t.band_text,
                    );
                    resp.clicked()
                };
                if item(ui, "Informationen", page == Page::Info) {
                    self.backstage = Some(Page::Info);
                }
                if item(ui, "Drucken", false) {
                    match self.mail.selected_id() {
                        Some(id) if self.module == super::Module::Mail => {
                            self.backstage = None;
                            self.print_message(id);
                        }
                        _ => self.toast("Wählen Sie zum Drucken eine E-Mail aus.", true),
                    }
                }
                if item(ui, "Optionen", false) {
                    self.open_options();
                }
                if item(ui, "Info zu MailCraft", page == Page::About) {
                    self.backstage = Some(Page::About);
                }
                if item(ui, "Beenden", false) {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
        egui::CentralPanel::default().frame(egui::Frame::new().fill(t.pane).inner_margin(egui::Margin { left: 40, right: 40, top: 24, bottom: 24 })).show(ctx, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| match page {
                Page::Info => self.account_info(ui),
                Page::About => {
                    ui.label(RichText::new("Info zu MailCraft").size(34.0).color(t.text));
                    ui.add_space(16.0);
                    ui.label(format!("MailCraft {}", env!("CARGO_PKG_VERSION")));
                    ui.label("E-Mail, Kalender, Personen und Aufgaben für IMAP/SMTP und Microsoft 365.");
                    ui.add_space(8.0);
                    ui.label(RichText::new(format!("Datenbank: {}", std::env::var("MAILCRAFT_DB").unwrap_or_else(|_| "Standard-Datenverzeichnis".into()))).color(t.text_weak));
                }
            });
        });
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.backstage = None;
        }
    }

    fn account_info(&mut self, ui: &mut egui::Ui) {
        let t = self.t;
        ui.label(RichText::new("Kontoinformationen").size(34.0).color(t.text));
        ui.add_space(18.0);
        let accounts = self.data.accounts.clone();
        let sel_id = ui.ctx().data_mut(|d| {
            *d.get_temp_mut_or(
                egui::Id::new("backstage_acc"),
                accounts.first().map(|a| a.id).unwrap_or(0),
            )
        });
        let sel = accounts
            .iter()
            .find(|a| a.id == sel_id)
            .or(accounts.first())
            .cloned();
        let kind = |a: &Account| match a.kind {
            AccountKind::Local => "Lokales Postfach",
            AccountKind::Imap => "IMAP/SMTP",
            AccountKind::Graph => "Microsoft 365",
        };
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("acc_pick")
                .width(420.0)
                .height(300.0)
                .selected_text(
                    sel.as_ref()
                        .map(|a| format!("{}   {}", a.email, kind(a)))
                        .unwrap_or_default(),
                )
                .show_ui(ui, |ui| {
                    for a in &accounts {
                        if ui
                            .selectable_label(
                                Some(a.id) == sel.as_ref().map(|s| s.id),
                                format!("{}   {}", a.email, kind(a)),
                            )
                            .clicked()
                        {
                            ui.ctx()
                                .data_mut(|d| d.insert_temp(egui::Id::new("backstage_acc"), a.id));
                        }
                    }
                });
        });
        ui.add_space(6.0);
        if ui
            .add(
                egui::Button::new((RichText::new(ph::PLUS).color(IC_BLUE), "Konto hinzufügen"))
                    .frame(false),
            )
            .clicked()
        {
            self.open_add_account();
        }
        ui.add_space(16.0);
        let Some(acc) = sel else { return };

        let section = |ui: &mut egui::Ui,
                       icon: &str,
                       label: &str,
                       title: &str,
                       text: &str|
         -> egui::Response {
            ui.separator();
            ui.add_space(8.0);
            let mut resp = None;
            ui.horizontal(|ui| {
                let (r, b) = ui.allocate_exact_size(egui::vec2(96.0, 92.0), Sense::click());
                ui.painter().rect(
                    r,
                    0,
                    if b.hovered() { t.hover } else { t.pane },
                    egui::Stroke::new(1.0_f32, t.border),
                    egui::StrokeKind::Inside,
                );
                ui.painter().text(
                    r.center() - egui::vec2(0.0, 14.0),
                    egui::Align2::CENTER_CENTER,
                    icon,
                    chrome::fill_font(30.0),
                    IC_BLUE,
                );
                let g = ui.painter().layout(
                    label.to_string(),
                    egui::FontId::proportional(12.0),
                    t.text,
                    90.0,
                );
                ui.painter().galley(
                    egui::pos2(r.center().x - g.size().x / 2.0, r.bottom() - 34.0),
                    g,
                    t.text,
                );
                ui.add_space(14.0);
                ui.vertical(|ui| {
                    ui.add_space(8.0);
                    ui.label(RichText::new(title).size(16.0));
                    ui.label(RichText::new(text).color(t.text_weak));
                });
                resp = Some(b);
            });
            ui.add_space(8.0);
            resp.unwrap()
        };

        let detail = match acc.kind {
            AccountKind::Local => {
                "Dieses Postfach wird nur auf diesem Computer gespeichert.".to_string()
            }
            _ => format!(
                "Zuletzt synchronisiert: {}{}",
                acc.last_sync
                    .as_deref()
                    .map(super::long_date)
                    .unwrap_or_else(|| "noch nie".into()),
                acc.last_error
                    .as_ref()
                    .map(|e| format!("\nFehler: {e}"))
                    .unwrap_or_default()
            ),
        };
        let r = section(
            ui,
            ph::GEAR,
            "Konto-\neinstellungen \u{E136}",
            "Kontoeinstellungen",
            &format!("Ändern Sie die Einstellungen für dieses Konto.\n{detail}"),
        );
        let acc_id = acc.id;
        let is_local = acc.kind == AccountKind::Local;
        chrome::menu(&r, |ui| {
            if chrome::menu_item(ui, ph::PLUS, IC_BLUE, "Konto hinzufügen…") {
                self.open_add_account();
            }
            if !is_local
                && chrome::menu_item(ui, ph::ARROWS_CLOCKWISE, IC_BLUE, "Jetzt synchronisieren")
            {
                self.spawn(Some("Konto synchronisiert."), move |st, _| async move {
                    providers::sync(&st, acc_id).await
                });
            }
            if !is_local && chrome::menu_item(ui, ph::X, t.danger, "Konto entfernen") {
                let res = db::delete_account(&self.st.db(), acc_id);
                match res {
                    Ok(()) => self.toast("Konto entfernt.", false),
                    Err(e) => self.toast(format!("{e:#}"), true),
                }
                self.dirty = true;
            }
        });

        let trash = self.folder_for_role(acc.id, "trash");
        let trash_count = trash
            .and_then(|id| self.data.folders.iter().find(|f| f.id == id))
            .map(|f| f.total)
            .unwrap_or(0);
        let r = section(
            ui,
            ph::BROOM,
            "Tools \u{E136}",
            "Postfacheinstellungen",
            &format!("Verwalten Sie die Größe Ihres Postfachs, indem Sie gelöschte Elemente leeren.\nGelöschte Elemente: {trash_count}"),
        );
        chrome::menu(&r, |ui| {
            if chrome::menu_item(
                ui,
                ph::TRASH,
                t.danger,
                "Ordner \"Gelöschte Elemente\" leeren",
            ) {
                if let Some(folder) = trash {
                    let ids: Vec<i64> = db::list_messages(
                        &self.st.db(),
                        &db::MessageQuery {
                            folder_id: Some(folder),
                            limit: 100_000,
                            ..Default::default()
                        },
                    )
                    .unwrap_or_default()
                    .into_iter()
                    .map(|m| m.id)
                    .collect();
                    self.spawn(
                        Some("Gelöschte Elemente wurden geleert."),
                        move |st, _| async move {
                            for id in ids {
                                providers::delete_message(&st, id).await?;
                            }
                            Ok(())
                        },
                    );
                }
            }
        });
    }

    // ------------------------------------------------------------ options dialog

    pub(super) fn options_window(
        &mut self,
        ctx: &egui::Context,
        vid: egui::ViewportId,
        o: &mut OptionsWin,
    ) -> bool {
        let t = self.t;
        let mut ok = false;
        let mut cancel = false;
        let closed = chrome::native_window(
            ctx,
            vid,
            "MailCraft-Optionen",
            [860.0, 600.0],
            &t,
            |ui| {
                egui::TopBottomPanel::bottom("opt_buttons")
                    .frame(egui::Frame::new().inner_margin(egui::Margin::same(12)))
                    .show_inside(ui, |ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .add_sized([90.0, 26.0], egui::Button::new("Abbrechen"))
                                .clicked()
                            {
                                cancel = true;
                            }
                            if ui
                                .add_sized([90.0, 26.0], egui::Button::new("OK"))
                                .clicked()
                            {
                                ok = true;
                            }
                        });
                    });
                egui::SidePanel::left("opt_cats")
                    .exact_width(170.0)
                    .resizable(false)
                    .frame(
                        egui::Frame::new()
                            .fill(t.pane)
                            .stroke(egui::Stroke::new(1.0_f32, t.border)),
                    )
                    .show_inside(ui, |ui| {
                        ui.add_space(6.0);
                        for (i, c) in ["Allgemein", "E-Mail", "Kalender", "Erweitert"]
                            .into_iter()
                            .enumerate()
                        {
                            let (r, resp) = ui.allocate_exact_size(
                                egui::vec2(ui.available_width(), 28.0),
                                Sense::click(),
                            );
                            if o.cat == i {
                                ui.painter().rect_filled(r, 0, t.selected);
                            } else if resp.hovered() {
                                ui.painter().rect_filled(r, 0, t.hover);
                            }
                            ui.painter().text(
                                r.left_center() + egui::vec2(12.0, 0.0),
                                egui::Align2::LEFT_CENTER,
                                c,
                                egui::FontId::proportional(13.0),
                                t.text,
                            );
                            if resp.clicked() {
                                o.cat = i;
                            }
                        }
                    });
                egui::CentralPanel::default().frame(egui::Frame::new().inner_margin(egui::Margin::same(18))).show_inside(ui, |ui| {
                let heading = |ui: &mut egui::Ui, s: &str| {
                    ui.add_space(6.0);
                    ui.label(RichText::new(s).strong());
                    ui.separator();
                };
                match o.cat {
                    0 => {
                        heading(ui, "MailCraft personalisieren");
                        ui.horizontal(|ui| {
                            ui.label("Design:");
                            egui::ComboBox::from_id_salt("opt_design").selected_text(if o.design == Design::Bunt { "Bunt" } else { "Schwarz" }).show_ui(ui, |ui| {
                                ui.selectable_value(&mut o.design, Design::Bunt, "Bunt");
                                ui.selectable_value(&mut o.design, Design::Schwarz, "Schwarz");
                            });
                        });
                    }
                    1 => {
                        heading(ui, "Nachrichten verfassen");
                        ui.label("Signatur für neue Nachrichten, Antworten und Weiterleitungen:");
                        ui.add(egui::TextEdit::multiline(&mut o.signature).desired_rows(6).desired_width(f32::INFINITY));
                        heading(ui, "Outlook-Bereiche");
                        ui.horizontal(|ui| {
                            ui.label("Lesebereich:");
                            egui::ComboBox::from_id_salt("opt_rp")
                                .selected_text(match o.reading_pane {
                                    ReadingPane::Right => "Rechts",
                                    ReadingPane::Bottom => "Unten",
                                    ReadingPane::Off => "Aus",
                                })
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(&mut o.reading_pane, ReadingPane::Right, "Rechts");
                                    ui.selectable_value(&mut o.reading_pane, ReadingPane::Bottom, "Unten");
                                    ui.selectable_value(&mut o.reading_pane, ReadingPane::Off, "Aus");
                                });
                        });
                        ui.checkbox(&mut o.mark_read, "Elemente als gelesen markieren, wenn sie im Lesebereich angezeigt werden");
                        ui.checkbox(&mut o.preview, "Nachrichtenvorschau in der Nachrichtenliste anzeigen");
                    }
                    2 => {
                        heading(ui, "Arbeitszeit");
                        ui.horizontal(|ui| {
                            ui.label("Beginnt um:");
                            hour_combo(ui, "ws", &mut o.work_start);
                            ui.add_space(20.0);
                            ui.label("Endet um:");
                            hour_combo(ui, "we", &mut o.work_end);
                        });
                    }
                    _ => {
                        heading(ui, "Senden und Empfangen");
                        let minutes = std::env::var("MAILCRAFT_SYNC_MINUTES").unwrap_or_else(|_| "5".into());
                        ui.label(format!("Automatisches Senden/Empfangen alle {minutes} Minuten (Umgebungsvariable MAILCRAFT_SYNC_MINUTES)."));
                        heading(ui, "Microsoft 365");
                        match &self.st.graph_client_id {
                            Some(id) => ui.label(format!("Anwendungs-ID: {id}")),
                            None => ui.label("Für die Microsoft-Anmeldung die Umgebungsvariable MAILCRAFT_GRAPH_CLIENT_ID setzen (siehe README)."),
                        };
                    }
                }
            });
            },
        );
        if ok {
            if o.work_end <= o.work_start {
                self.toast(
                    "Das Ende der Arbeitszeit muss nach dem Beginn liegen.",
                    true,
                );
                return false;
            }
            self.prefs.design = o.design;
            self.prefs.signature = o.signature.clone();
            self.prefs.mark_read = o.mark_read;
            self.prefs.preview = o.preview;
            self.prefs.reading_pane = o.reading_pane;
            self.prefs.work_start = o.work_start;
            self.prefs.work_end = o.work_end;
            for (k, v) in [
                ("design", serde_json::json!(o.design.key())),
                ("signature", serde_json::json!(o.signature)),
                ("mark_read", serde_json::json!(o.mark_read)),
                ("preview", serde_json::json!(o.preview)),
                (
                    "reading_pane",
                    serde_json::json!(match o.reading_pane {
                        ReadingPane::Right => "right",
                        ReadingPane::Bottom => "bottom",
                        ReadingPane::Off => "off",
                    }),
                ),
                ("work_start", serde_json::json!(o.work_start)),
                ("work_end", serde_json::json!(o.work_end)),
            ] {
                self.save_setting(k, v);
            }
            self.apply_style();
            return true;
        }
        cancel || closed
    }

    // ------------------------------------------------------------ add account wizard

    pub(super) fn add_account_window(
        &mut self,
        ctx: &egui::Context,
        vid: egui::ViewportId,
        a: &mut AddAccountWin,
    ) -> bool {
        let t = self.t;
        let graph_enabled = self.st.graph_client_id.is_some();
        let mut close = false;
        let mut start_imap = false;
        let mut start_graph = false;
        // Finished when the new account shows up in the account list.
        if a.started.is_some()
            && !a.done
            && self.data.accounts.iter().any(|x| {
                x.email.eq_ignore_ascii_case(a.email.trim())
                    || (a.graph && x.kind == AccountKind::Graph && x.id > a.max_id)
            })
        {
            a.done = true;
        }
        let error = match (a.started, &self.status) {
            (Some(started), (msg, true, at)) if *at > started && !a.done => Some(msg.clone()),
            _ => None,
        };
        let device = self.device_code.clone();
        let closed = chrome::native_window(
            ctx,
            vid,
            "Konto hinzufügen",
            [560.0, 600.0],
            &t,
            |ui| {
                egui::Frame::new().inner_margin(egui::Margin { left: 40, right: 40, top: 30, bottom: 20 }).show(ui, |ui| {
                ui.vertical_centered(|ui| {
                    ui.label(RichText::new(ph::ENVELOPE_SIMPLE).size(44.0).color(IC_BLUE));
                    ui.label(RichText::new("MailCraft").size(26.0));
                });
                ui.add_space(20.0);
                if a.done {
                    ui.vertical_centered(|ui| {
                        ui.label(RichText::new(ph::CHECK_CIRCLE).size(40.0).color(chrome::IC_GREEN));
                        ui.label(RichText::new("Das Konto wurde erfolgreich hinzugefügt.").size(16.0));
                        ui.label(a.email.trim());
                        ui.add_space(20.0);
                        if ui.add_sized([120.0, 30.0], egui::Button::new("Fertig")).clicked() {
                            close = true;
                        }
                    });
                    return;
                }
                if a.started.is_some() {
                    ui.vertical_centered(|ui| {
                        if let Some(e) = &error {
                            ui.label(RichText::new(ph::WARNING).size(36.0).color(t.danger));
                            ui.label(RichText::new("Fehler").size(18.0));
                            ui.add(egui::Label::new(RichText::new(e).color(t.danger)).wrap());
                            ui.add_space(12.0);
                            if ui.button("Zurück").clicked() {
                                a.started = None;
                            }
                        } else if let (true, Some((code, uri))) = (a.graph, &device) {
                            ui.label("Melden Sie sich bei Microsoft an. Öffnen Sie die Anmeldeseite und geben Sie diesen Code ein:");
                            ui.add_space(8.0);
                            ui.label(RichText::new(code).size(32.0).strong());
                            ui.add_space(8.0);
                            ui.horizontal(|ui| {
                                if ui.button(format!("{}  Code kopieren", ph::COPY)).clicked() {
                                    ui.ctx().copy_text(code.clone());
                                }
                                if ui.button(format!("{}  Anmeldeseite öffnen", ph::GLOBE)).clicked() {
                                    super::open_external(uri);
                                }
                            });
                            ui.label(RichText::new(uri).color(t.text_weak));
                            ui.add_space(10.0);
                            ui.add(egui::Spinner::new());
                        } else {
                            ui.add(egui::Spinner::new().size(28.0));
                            ui.label("Konto wird hinzugefügt …");
                        }
                    });
                    return;
                }
                match a.step {
                    0 => {
                        ui.label("E-Mail-Adresse");
                        ui.add(egui::TextEdit::singleline(&mut a.email).desired_width(f32::INFINITY));
                        ui.add_space(6.0);
                        ui.label("Name (wie er Empfängern angezeigt wird)");
                        ui.add(egui::TextEdit::singleline(&mut a.name).desired_width(f32::INFINITY));
                        ui.add_space(12.0);
                        ui.collapsing("Erweiterte Optionen", |ui| {
                            ui.checkbox(&mut a.manual, "Ich möchte mein Konto manuell einrichten");
                        });
                        ui.add_space(12.0);
                        if ui.add_sized([120.0, 30.0], egui::Button::new("Verbinden")).clicked() && a.email.contains('@') {
                            let domain = a.email.rsplit('@').next().unwrap_or("").to_lowercase();
                            let ms = ["outlook.com", "hotmail.com", "live.com", "outlook.de", "hotmail.de", "msn.com"].contains(&domain.as_str());
                            if let Some(p) = preset_for(&domain) {
                                a.cfg.imap_host = p.0.into();
                                a.cfg.imap_port = p.1;
                                a.cfg.smtp_host = p.2.into();
                                a.cfg.smtp_port = p.3;
                            }
                            a.cfg.username = a.email.trim().to_string();
                            a.step = if !a.manual && !ms && preset_for(&domain).is_some() { 2 } else { 1 };
                        }
                    }
                    1 => {
                        ui.label(RichText::new("Erweiterte Einrichtung").size(18.0));
                        ui.add_space(10.0);
                        ui.horizontal_wrapped(|ui| {
                            let tile = |ui: &mut egui::Ui, icon: &str, label: &str, enabled: bool| {
                                let (r, resp) = ui.allocate_exact_size(egui::vec2(110.0, 96.0), if enabled { Sense::click() } else { Sense::hover() });
                                ui.painter().rect(r, 0, if resp.hovered() { t.hover } else { t.pane }, egui::Stroke::new(1.0_f32, t.border), egui::StrokeKind::Inside);
                                let c = if enabled { IC_BLUE } else { t.text_weak };
                                ui.painter().text(r.center() - egui::vec2(0.0, 14.0), egui::Align2::CENTER_CENTER, icon, chrome::fill_font(30.0), c);
                                ui.painter().text(r.center() + egui::vec2(0.0, 26.0), egui::Align2::CENTER_CENTER, label, egui::FontId::proportional(12.5), if enabled { t.text } else { t.text_weak });
                                resp.clicked()
                            };
                            if tile(ui, ph::MICROSOFT_OUTLOOK_LOGO, "Microsoft 365", graph_enabled) {
                                start_graph = true;
                            }
                            if tile(ui, ph::ENVELOPE_SIMPLE, "Outlook.com", graph_enabled) {
                                start_graph = true;
                            }
                            if tile(ui, ph::TRAY, "IMAP", true) {
                                a.step = 2;
                            }
                        });
                        if !graph_enabled {
                            ui.add_space(8.0);
                            ui.label(RichText::new("Microsoft 365 / Outlook.com: zuerst MAILCRAFT_GRAPH_CLIENT_ID setzen (siehe README).").color(t.text_weak));
                        }
                        ui.add_space(14.0);
                        if ui.button("Zurück").clicked() {
                            a.step = 0;
                        }
                    }
                    _ => {
                        ui.label(RichText::new("IMAP-Kontoeinstellungen").size(18.0));
                        ui.label(RichText::new(a.email.trim()).color(t.text_weak));
                        ui.add_space(10.0);
                        ui.label(RichText::new("Eingehende E-Mail").strong());
                        egui::Grid::new("in").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                            ui.label("Server");
                            ui.add(egui::TextEdit::singleline(&mut a.cfg.imap_host).desired_width(260.0));
                            ui.end_row();
                            ui.label("Port");
                            ui.add(egui::DragValue::new(&mut a.cfg.imap_port).range(1..=65535));
                            ui.end_row();
                            ui.label("Verschlüsselungsmethode");
                            ui.label("SSL/TLS");
                            ui.end_row();
                        });
                        ui.add_space(6.0);
                        ui.label(RichText::new("Ausgehende E-Mail").strong());
                        egui::Grid::new("out").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                            ui.label("Server");
                            ui.add(egui::TextEdit::singleline(&mut a.cfg.smtp_host).desired_width(260.0));
                            ui.end_row();
                            ui.label("Port");
                            ui.add(egui::DragValue::new(&mut a.cfg.smtp_port).range(1..=65535));
                            ui.end_row();
                            ui.label("Verschlüsselungsmethode");
                            ui.label(if a.cfg.smtp_port == 465 { "SSL/TLS" } else { "STARTTLS" });
                            ui.end_row();
                        });
                        ui.add_space(6.0);
                        egui::Grid::new("cred").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                            ui.label("Benutzername");
                            ui.add(egui::TextEdit::singleline(&mut a.cfg.username).desired_width(260.0));
                            ui.end_row();
                            ui.label("Kennwort");
                            ui.add(egui::TextEdit::singleline(&mut a.cfg.password).password(true).desired_width(260.0));
                            ui.end_row();
                        });
                        ui.label(RichText::new("Gmail, iCloud und Yahoo verlangen ein App-Kennwort.").size(12.0).color(t.text_weak));
                        ui.add_space(12.0);
                        ui.horizontal(|ui| {
                            if ui.button("Zurück").clicked() {
                                a.step = 1;
                            }
                            if ui.add_sized([120.0, 28.0], egui::Button::new("Verbinden")).clicked() {
                                start_imap = true;
                            }
                        });
                    }
                }
            });
            },
        );
        if start_imap {
            a.started = Some(Instant::now());
            a.graph = false;
            let (cfg, email, name) = (
                a.cfg.clone(),
                a.email.trim().to_string(),
                a.name.trim().to_string(),
            );
            self.spawn(Some("Konto hinzugefügt."), move |st, _| async move {
                providers::imap::test_login(&cfg).await?;
                let id = db::insert_account(
                    &st.db(),
                    AccountKind::Imap,
                    if name.is_empty() { &email } else { &name },
                    &email,
                    &serde_json::to_string(&cfg)?,
                )?;
                providers::sync(&st, id).await
            });
        }
        if start_graph {
            a.started = Some(Instant::now());
            a.graph = true;
            a.max_id = self.data.accounts.iter().map(|x| x.id).max().unwrap_or(0);
            self.device_code = None;
            self.spawn(
                Some("Microsoft-Konto verbunden."),
                move |st, tx| async move {
                    let dc = providers::graph::start_device_flow(&st).await?;
                    let _ = tx.send(super::Done::DeviceCode {
                        user_code: dc.user_code.clone(),
                        uri: dc.verification_uri.clone(),
                    });
                    let res = providers::graph::finish_device_flow(&st, &dc).await;
                    let _ = tx.send(super::Done::DeviceCode {
                        user_code: String::new(),
                        uri: String::new(),
                    });
                    providers::sync(&st, res?).await
                },
            );
        }
        close || closed
    }
}

fn hour_combo(ui: &mut egui::Ui, id: &str, h: &mut u32) {
    egui::ComboBox::from_id_salt(id)
        .width(70.0)
        .selected_text(format!("{h:02}:00"))
        .show_ui(ui, |ui| {
            for x in 0..24 {
                ui.selectable_value(h, x, format!("{x:02}:00"));
            }
        });
}

/// Known IMAP/SMTP settings by mail domain: (imap host, port, smtp host, port).
fn preset_for(domain: &str) -> Option<(&'static str, u16, &'static str, u16)> {
    Some(match domain {
        "gmail.com" | "googlemail.com" => ("imap.gmail.com", 993, "smtp.gmail.com", 587),
        "gmx.de" | "gmx.net" | "gmx.at" | "gmx.ch" => ("imap.gmx.net", 993, "mail.gmx.net", 587),
        "web.de" => ("imap.web.de", 993, "smtp.web.de", 587),
        "t-online.de" => ("secureimap.t-online.de", 993, "securesmtp.t-online.de", 465),
        "icloud.com" | "me.com" | "mac.com" => ("imap.mail.me.com", 993, "smtp.mail.me.com", 587),
        "posteo.de" | "posteo.net" => ("posteo.de", 993, "posteo.de", 587),
        "yahoo.com" | "yahoo.de" => ("imap.mail.yahoo.com", 993, "smtp.mail.yahoo.com", 465),
        "mailbox.org" => ("imap.mailbox.org", 993, "smtp.mailbox.org", 587),
        "freenet.de" => ("mx.freenet.de", 993, "mx.freenet.de", 587),
        "aol.com" => ("imap.aol.com", 993, "smtp.aol.com", 465),
        _ => return None,
    })
}

pub struct OptionsWin {
    cat: usize,
    design: Design,
    signature: String,
    mark_read: bool,
    preview: bool,
    reading_pane: ReadingPane,
    work_start: u32,
    work_end: u32,
}

pub struct AddAccountWin {
    step: u8,
    email: String,
    name: String,
    manual: bool,
    cfg: ImapConfig,
    started: Option<Instant>,
    graph: bool,
    max_id: i64,
    done: bool,
}

impl Default for AddAccountWin {
    fn default() -> Self {
        Self {
            step: 0,
            email: String::new(),
            name: String::new(),
            manual: false,
            cfg: ImapConfig {
                imap_host: String::new(),
                imap_port: 993,
                smtp_host: String::new(),
                smtp_port: 587,
                username: String::new(),
                password: String::new(),
            },
            started: None,
            graph: false,
            max_id: 0,
            done: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_by_domain() {
        assert_eq!(preset_for("gmx.de").unwrap().0, "imap.gmx.net");
        assert!(preset_for("example.org").is_none());
    }
}
