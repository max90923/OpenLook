//! Account secrets (IMAP password, Microsoft tokens) in the operating
//! system's credential store: Windows Credential Manager, macOS Keychain,
//! or the Secret Service (GNOME Keyring / KWallet) on Linux.
//!
//! The keyring calls run on a fresh OS thread with its own small runtime,
//! so they never block or panic inside the app's Tokio executor. When no
//! credential store is available the secrets stay in the local database.

use anyhow::{anyhow, Result};
use serde_json::Value;

use crate::{db, state::AppState};

const SERVICE: &str = "OpenLook";
/// Secret fields inside an account's config JSON.
const FIELDS: [&str; 3] = ["password", "refresh_token", "access_token"];

fn isolated<T: Send>(f: impl FnOnce() -> keyring::Result<T> + Send) -> Option<T> {
    std::thread::scope(|s| {
        s.spawn(|| {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .ok()?;
            let _guard = rt.enter();
            f().ok()
        })
        .join()
        .ok()
        .flatten()
    })
}

fn key(account_id: i64, field: &str) -> String {
    format!("account-{account_id}-{field}")
}

fn store(account_id: i64, field: &str, secret: &str) -> bool {
    let k = key(account_id, field);
    isolated(|| keyring::Entry::new(SERVICE, &k)?.set_password(secret)).is_some()
}

fn load(account_id: i64, field: &str) -> Option<String> {
    let k = key(account_id, field);
    isolated(|| keyring::Entry::new(SERVICE, &k)?.get_password())
}

/// Remove an account's secrets from the credential store.
pub fn forget(account_id: i64) {
    for f in FIELDS {
        let k = key(account_id, f);
        let _ = isolated(|| keyring::Entry::new(SERVICE, &k)?.delete_credential());
    }
}

/// Move the secrets of an account's config into the credential store.
/// Returns true when the config no longer contains secrets.
pub fn secure(state: &AppState, account_id: i64) -> Result<bool> {
    let raw = db::account_config(&state.db(), account_id)?;
    let mut cfg: Value = serde_json::from_str(&raw)?;
    let mut moved = false;
    for f in FIELDS {
        let Some(secret) = cfg[f].as_str().filter(|s| !s.is_empty()).map(String::from) else {
            continue;
        };
        if !store(account_id, f, &secret) {
            return Ok(false);
        }
        cfg[f] = Value::String(String::new());
        moved = true;
    }
    if moved {
        cfg["keyring"] = Value::Bool(true);
        db::set_account_config(&state.db(), account_id, &cfg.to_string())?;
    }
    Ok(true)
}

/// The account config with its secrets filled in from the credential store.
pub fn config(state: &AppState, account_id: i64) -> Result<String> {
    let raw = db::account_config(&state.db(), account_id)?;
    let mut cfg: Value = serde_json::from_str(&raw)?;
    if cfg["keyring"].as_bool() != Some(true) {
        return Ok(raw);
    }
    for f in FIELDS {
        if cfg.get(f).is_some_and(|v| v.as_str() == Some("")) {
            let secret = load(account_id, f).ok_or_else(|| {
                anyhow!("The credentials were not found in the system keychain – please remove the account and add it again")
            })?;
            cfg[f] = Value::String(secret);
        }
    }
    Ok(cfg.to_string())
}

/// Move secrets of all accounts that still have them in the database.
pub fn secure_all(state: &AppState) {
    let Ok(accounts) = db::list_accounts(&state.db()) else {
        return;
    };
    for a in accounts {
        match secure(state, a.id) {
            Ok(true) => {}
            Ok(false) => tracing::info!(
                "No keychain available – credentials of {} stay in the local database",
                a.email
            ),
            Err(e) => tracing::warn!("Keychain for {}: {e:#}", a.email),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::AccountKind;

    #[test]
    fn plain_config_is_returned_unchanged_without_keyring_flag() {
        let state = AppState::in_memory();
        let id = db::insert_account(
            &state.db(),
            AccountKind::Imap,
            "T",
            "t@x.de",
            r#"{"password":"secret123"}"#,
        )
        .unwrap();
        assert!(config(&state, id).unwrap().contains("secret123"));
    }

    #[test]
    fn secure_never_loses_the_secret() {
        // Whether or not a credential store exists here, the password must
        // stay retrievable through `config`.
        let state = AppState::in_memory();
        let id = db::insert_account(
            &state.db(),
            AccountKind::Imap,
            "T",
            "t@x.de",
            r#"{"password":"secret123","username":"u"}"#,
        )
        .unwrap();
        let moved = secure(&state, id).unwrap();
        let raw = db::account_config(&state.db(), id).unwrap();
        assert_eq!(moved, !raw.contains("secret123"));
        if moved {
            assert!(config(&state, id).unwrap().contains("secret123"));
            forget(id);
        }
    }

    /// Needs a running credential store (e.g. gnome-keyring in a D-Bus
    /// session). Run with `cargo test -- --ignored keyring_roundtrip`.
    #[test]
    #[ignore]
    fn keyring_roundtrip() {
        let state = AppState::in_memory();
        let id = db::insert_account(
            &state.db(),
            AccountKind::Graph,
            "T",
            "t@x.de",
            r#"{"access_token":"a1","refresh_token":"r1","expires_at":0}"#,
        )
        .unwrap();
        assert!(secure(&state, id).unwrap(), "Keychain not available");
        let raw = db::account_config(&state.db(), id).unwrap();
        assert!(
            !raw.contains("r1") && !raw.contains("a1"),
            "Secrets still in the database: {raw}"
        );
        let full = config(&state, id).unwrap();
        assert!(full.contains("r1") && full.contains("a1"));
        forget(id);
        assert!(
            config(&state, id).is_err(),
            "no data may remain after removal"
        );
    }
}
