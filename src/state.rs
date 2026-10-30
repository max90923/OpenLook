use std::sync::{Mutex, MutexGuard};

use rusqlite::Connection;

/// Shared between the UI thread and background (network) tasks.
pub struct AppState {
    db: Mutex<Connection>,
    pub http: reqwest::Client,
    /// Entra ID application (client) id for Microsoft Graph sign-in.
    pub graph_client_id: Option<String>,
    /// Entra ID tenant: "common", "organizations", "consumers" or a tenant id.
    pub graph_tenant: String,
}

impl AppState {
    pub fn new(db: Connection, graph_client_id: Option<String>, graph_tenant: String) -> Self {
        Self {
            db: Mutex::new(db),
            http: reqwest::Client::builder()
                .user_agent(concat!("OpenLook/", env!("CARGO_PKG_VERSION")))
                .build()
                .expect("HTTP-Client"),
            graph_client_id,
            graph_tenant,
        }
    }

    /// Lock the database. Never hold the guard across an `.await`.
    pub fn db(&self) -> MutexGuard<'_, Connection> {
        self.db.lock().unwrap_or_else(|e| e.into_inner())
    }

    #[cfg(test)]
    pub fn in_memory() -> Self {
        Self::new(crate::db::open(":memory:").unwrap(), None, "common".into())
    }
}
