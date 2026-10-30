//! OpenLook – a native, Outlook-style desktop client for mail, calendar,
//! contacts and tasks. A single program: the GUI runs on the main thread,
//! network I/O (IMAP/SMTP, Microsoft Graph) on a background Tokio runtime,
//! and everything is cached in a local SQLite file.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod conversation;
mod db;
mod mail;
mod models;
mod providers;
mod recur;
mod rules;
mod secrets;
mod seed;
mod state;
mod ui;

use std::{path::PathBuf, sync::Arc};

use anyhow::Result;

/// `OPENLOOK_DB` or `<data dir>/OpenLook/openlook.db`.
fn db_path() -> PathBuf {
    if let Ok(p) = std::env::var("OPENLOOK_DB") {
        return p.into();
    }
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("XDG_DATA_HOME").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .unwrap_or_else(|| ".".into());
    let dir = base.join("OpenLook");
    let _ = std::fs::create_dir_all(&dir);
    dir.join("openlook.db")
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "openlook=info".into()),
        )
        .init();
    let _ = rustls::crypto::ring::default_provider().install_default();

    let client_id = std::env::var("OPENLOOK_GRAPH_CLIENT_ID")
        .ok()
        .filter(|s| !s.is_empty());
    let tenant = std::env::var("OPENLOOK_GRAPH_TENANT").unwrap_or_else(|_| "common".into());

    let path = db_path();
    let conn = db::open(&path.to_string_lossy())?;
    seed::ensure_local_account(&conn)?;
    let state = Arc::new(state::AppState::new(conn, client_id, tenant));
    let runtime = tokio::runtime::Runtime::new()?;

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("OpenLook")
            .with_inner_size([1440.0, 900.0])
            .with_min_inner_size([960.0, 600.0])
            // The Outlook-style title bar is drawn by OpenLook itself.
            .with_decorations(false),
        ..Default::default()
    };
    eframe::run_native(
        "OpenLook",
        options,
        Box::new(move |cc| Ok(Box::new(ui::App::new(cc, state, runtime)))),
    )
    .map_err(|e| anyhow::anyhow!("GUI konnte nicht gestartet werden: {e}"))
}
