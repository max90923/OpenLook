//! Settings window: accounts (IMAP/SMTP, Microsoft 365) and general options.

use eframe::egui::{self, RichText};

use super::{App, Done, DANGER};
use crate::{db, models::*, providers};

#[derive(Default, PartialEq, Eq, Clone, Copy)]
enum Tab {
    #[default]
    Accounts,
    General,
}

#[derive(Default)]
pub struct SettingsView {
    pub open: bool,
    tab: Tab,
    imap: Option<ImapForm>,
    /// (user code, verification URL) of a running Microsoft sign-in.
    pub device_code: Option<(String, String)>,
    signature: Option<String>,
}

#[derive(Clone)]
struct ImapForm {
    name: String,
    email: String,
    cfg: ImapConfig,
}

impl Default for ImapForm {
    fn default() -> Self {
        Self {
            name: String::new(),
            email: String::new(),
            cfg: ImapConfig {
                imap_host: String::new(),
                imap_port: 993,
                smtp_host: String::new(),
                smtp_port: 587,
                username: String::new(),
                password: String::new(),
            },
        }
    }
}

/// (label, imap host, imap port, smtp host, smtp port)
const PRESETS: [(&str, &str, u16, &str, u16); 7] = [
    ("Gmail", "imap.gmail.com", 993, "smtp.gmail.com", 587),
    ("GMX", "imap.gmx.net", 993, "mail.gmx.net", 587),
    ("WEB.DE", "imap.web.de", 993, "smtp.web.de", 587),
    (
        "T-Online",
        "secureimap.t-online.de",
        993,
        "securesmtp.t-online.de",
        465,
    ),
    ("iCloud", "imap.mail.me.com", 993, "smtp.mail.me.com", 587),
    ("Posteo", "posteo.de", 993, "posteo.de", 587),
    (
        "Yahoo",
        "imap.mail.yahoo.com",
        993,
        "smtp.mail.yahoo.com",
        465,
    ),
];

impl App {
    pub fn settings_ui(&mut self, ctx: &egui::Context) {
        if !self.settings.open {
            return;
        }
        if self.settings.signature.is_none() {
            self.settings.signature = Some(self.data.signature.clone());
        }
        let mut open = true;
        let mut sync = None;
        let mut remove = None;
        let mut add_imap = None;
        let mut cancel_imap = false;
        let mut start_graph = false;
        let mut save_signature = None;
        let graph_enabled = self.st.graph_client_id.is_some();
        let accounts = self.data.accounts.clone();
        let s = &mut self.settings;
        egui::Window::new("⚙ Einstellungen")
            .open(&mut open)
            .collapsible(false)
            .default_size([640.0, 520.0])
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut s.tab, Tab::Accounts, "Konten");
                    ui.selectable_value(&mut s.tab, Tab::General, "Allgemein");
                });
                ui.separator();
                match s.tab {
                    Tab::Accounts => {
                        for a in &accounts {
                            ui.horizontal(|ui| {
                                super::avatar(ui, 36.0, &a.name);
                                ui.vertical(|ui| {
                                    ui.label(RichText::new(&a.name).strong());
                                    let kind = match a.kind {
                                        AccountKind::Local => "Lokales Postfach (nur auf diesem Gerät)",
                                        AccountKind::Imap => "IMAP/SMTP",
                                        AccountKind::Graph => "Microsoft 365 / Outlook.com",
                                    };
                                    ui.label(RichText::new(format!("{} · {kind}", a.email)).small().weak());
                                    if let Some(t) = &a.last_sync {
                                        ui.label(RichText::new(format!("Zuletzt synchronisiert: {}", super::long_date(t))).small().weak());
                                    }
                                    if let Some(e) = &a.last_error {
                                        ui.label(RichText::new(format!("⚠ {e}")).small().color(DANGER));
                                    }
                                });
                                if a.kind != AccountKind::Local {
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        if ui.button(RichText::new("Entfernen").color(DANGER)).clicked() {
                                            remove = Some(a.id);
                                        }
                                        if ui.button("🔄 Sync").clicked() {
                                            sync = Some(a.id);
                                        }
                                    });
                                }
                            });
                            ui.separator();
                        }
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            let r = ui.add_enabled(graph_enabled, egui::Button::new("Ⓜ  Microsoft 365 / Outlook.com verbinden"));
                            if !graph_enabled {
                                r.on_disabled_hover_text("Setze die Umgebungsvariable MAILCRAFT_GRAPH_CLIENT_ID (siehe README).");
                            } else if r.clicked() {
                                start_graph = true;
                            }
                            if ui.button("📨  IMAP/SMTP-Konto hinzufügen").clicked() {
                                s.imap = Some(ImapForm::default());
                            }
                        });
                        if !graph_enabled {
                            ui.label(RichText::new("Microsoft-Anmeldung: MAILCRAFT_GRAPH_CLIENT_ID setzen (App-Registrierung in Entra ID, siehe README).").small().weak());
                        }
                        if let Some((code, uri)) = s.device_code.clone() {
                            ui.add_space(10.0);
                            egui::Frame::group(ui.style()).show(ui, |ui| {
                                ui.label("Öffne die Microsoft-Anmeldeseite und gib diesen Code ein:");
                                ui.label(RichText::new(&code).size(28.0).strong().monospace());
                                ui.horizontal(|ui| {
                                    if ui.button("📋 Code kopieren").clicked() {
                                        ui.ctx().copy_text(code.clone());
                                    }
                                    if ui.button("🌐 Anmeldeseite öffnen").clicked() {
                                        super::open_external(&uri);
                                    }
                                    ui.label(RichText::new(&uri).small().weak());
                                });
                                ui.label(RichText::new("MailCraft wartet auf die Anmeldung …").small().weak());
                            });
                        }
                        if let Some(f) = &mut s.imap {
                            ui.add_space(10.0);
                            egui::Frame::group(ui.style()).show(ui, |ui| {
                                ui.label(RichText::new("IMAP/SMTP-Konto").strong());
                                ui.horizontal_wrapped(|ui| {
                                    ui.label("Vorlage:");
                                    for (label, ih, ip, sh, sp) in PRESETS {
                                        if ui.small_button(label).clicked() {
                                            f.cfg.imap_host = ih.into();
                                            f.cfg.imap_port = ip;
                                            f.cfg.smtp_host = sh.into();
                                            f.cfg.smtp_port = sp;
                                        }
                                    }
                                });
                                egui::Grid::new("imap_form").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                                    ui.label("Anzeigename");
                                    ui.text_edit_singleline(&mut f.name);
                                    ui.end_row();
                                    ui.label("E-Mail-Adresse");
                                    if ui.text_edit_singleline(&mut f.email).changed() && f.cfg.username.is_empty() {
                                        f.cfg.username = f.email.clone();
                                    }
                                    ui.end_row();
                                    ui.label("Benutzername");
                                    ui.text_edit_singleline(&mut f.cfg.username);
                                    ui.end_row();
                                    ui.label("Passwort / App-Passwort");
                                    ui.add(egui::TextEdit::singleline(&mut f.cfg.password).password(true));
                                    ui.end_row();
                                    ui.label("IMAP-Server (SSL/TLS)");
                                    ui.horizontal(|ui| {
                                        ui.text_edit_singleline(&mut f.cfg.imap_host);
                                        ui.add(egui::DragValue::new(&mut f.cfg.imap_port).range(1..=65535));
                                    });
                                    ui.end_row();
                                    ui.label("SMTP-Server");
                                    ui.horizontal(|ui| {
                                        ui.text_edit_singleline(&mut f.cfg.smtp_host);
                                        ui.add(egui::DragValue::new(&mut f.cfg.smtp_port).range(1..=65535));
                                    });
                                    ui.end_row();
                                });
                                ui.label(RichText::new("Port 465 = SSL/TLS, sonst STARTTLS. Bei Gmail/iCloud/Yahoo ein App-Passwort verwenden.").small().weak());
                                ui.horizontal(|ui| {
                                    if super::primary_button(ui, "Anmelden & hinzufügen").clicked() {
                                        add_imap = Some(f.clone());
                                    }
                                    if ui.button("Abbrechen").clicked() {
                                        cancel_imap = true;
                                    }
                                });
                            });
                        }
                    }
                    Tab::General => {
                        ui.label(RichText::new("E-Mail-Signatur").strong());
                        ui.label(RichText::new("Wird an neue Nachrichten, Antworten und Weiterleitungen angehängt.").small().weak());
                        if let Some(sig) = &mut s.signature {
                            ui.add(egui::TextEdit::multiline(sig).desired_rows(5).desired_width(f32::INFINITY));
                            if ui.button("💾 Signatur speichern").clicked() {
                                save_signature = Some(sig.clone());
                            }
                        }
                        ui.add_space(14.0);
                        ui.label(RichText::new("Tastenkombinationen").strong());
                        egui::Grid::new("keys").num_columns(2).spacing([20.0, 4.0]).show(ui, |ui| {
                            for (k, d) in [
                                ("N", "Neue E-Mail"),
                                ("R / Umschalt+R", "Antworten / Allen antworten"),
                                ("F", "Weiterleiten"),
                                ("Entf", "Löschen"),
                                ("Q", "Gelesen / ungelesen"),
                                ("Einfg", "Kennzeichnen"),
                                ("↑ / ↓", "Vorherige / nächste Nachricht"),
                                ("/", "Suche"),
                                ("Strg+Enter", "Senden (im Verfassen-Fenster)"),
                                ("Strg+1…4", "E-Mail, Kalender, Personen, To Do"),
                                ("F9", "Alle Konten synchronisieren"),
                            ] {
                                ui.label(RichText::new(k).monospace());
                                ui.label(d);
                                ui.end_row();
                            }
                        });
                    }
                }
            });
        if cancel_imap {
            self.settings.imap = None;
        }
        if !open {
            self.settings.open = false;
            self.settings.signature = None;
        }
        if let Some(sig) = save_signature {
            self.save_setting("signature", serde_json::json!(sig));
            self.data.signature = sig;
            self.toast("Signatur gespeichert", false);
        }
        if let Some(id) = sync {
            self.spawn(
                Some("Synchronisierung abgeschlossen"),
                move |st, _| async move { providers::sync(&st, id).await },
            );
        }
        if let Some(id) = remove {
            let res = db::delete_account(&self.st.db(), id);
            if let Err(e) = res {
                self.toast(format!("{e:#}"), true);
            }
            self.dirty = true;
        }
        if let Some(f) = add_imap {
            self.settings.imap = None;
            self.spawn(Some("Konto hinzugefügt"), move |st, _| async move {
                providers::imap::test_login(&f.cfg).await?;
                let id = db::insert_account(
                    &st.db(),
                    AccountKind::Imap,
                    if f.name.trim().is_empty() {
                        &f.email
                    } else {
                        &f.name
                    },
                    &f.email,
                    &serde_json::to_string(&f.cfg)?,
                )?;
                providers::sync(&st, id).await
            });
        }
        if start_graph {
            self.settings.device_code = None;
            self.spawn(
                Some("Microsoft-Konto verbunden"),
                move |st, tx| async move {
                    let dc = providers::graph::start_device_flow(&st).await?;
                    let _ = tx.send(Done::DeviceCode {
                        user_code: dc.user_code.clone(),
                        uri: dc.verification_uri.clone(),
                    });
                    let res = providers::graph::finish_device_flow(&st, &dc).await;
                    let _ = tx.send(Done::DeviceCode {
                        user_code: String::new(),
                        uri: String::new(),
                    });
                    providers::sync(&st, res?).await
                },
            );
        }
        if self
            .settings
            .device_code
            .as_ref()
            .is_some_and(|(c, _)| c.is_empty())
        {
            self.settings.device_code = None;
        }
    }
}
