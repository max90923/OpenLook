//! Microsoft 365 / Outlook.com provider via Microsoft Graph.
//!
//! Sign-in uses the OAuth 2.0 device code flow, so no redirect URI or client
//! secret is needed: register a "public client" app in Entra ID (Azure AD)
//! and pass its client id via `OPENLOOK_GRAPH_CLIENT_ID`.

use anyhow::{anyhow, bail, Context, Result};
use base64::Engine;
use reqwest::{Method, StatusCode};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{db, mail, models::*, providers::OutgoingMail, state::AppState};

const GRAPH: &str = "https://graph.microsoft.com/v1.0";
/// Delegated permissions; the ".Shared" ones allow opening mailboxes that
/// colleagues shared with the signed-in user.
pub const SCOPES: &str = "offline_access User.Read Mail.ReadWrite Mail.Send Calendars.ReadWrite Contacts.ReadWrite Tasks.ReadWrite \
                          Mail.ReadWrite.Shared Mail.Send.Shared Calendars.ReadWrite.Shared Contacts.ReadWrite.Shared MailboxSettings.ReadWrite";

/// Graph endpoint, overridable with `OPENLOOK_GRAPH_URL` (national clouds, tests).
fn graph_url() -> String {
    std::env::var("OPENLOOK_GRAPH_URL").unwrap_or_else(|_| GRAPH.to_string())
}

/// Account whose tokens are used, and the shared mailbox address if the
/// account is a mailbox shared with that user.
fn target(state: &AppState, account_id: i64) -> Result<(i64, Option<String>)> {
    let cfg: Value = serde_json::from_str(&db::account_config(&state.db(), account_id)?)?;
    Ok(match (cfg["shared_of"].as_i64(), cfg["mailbox"].as_str()) {
        (Some(owner), Some(mailbox)) => (owner, Some(mailbox.to_string())),
        _ => (account_id, None),
    })
}

/// `/me/...` addresses the signed-in user; a shared mailbox is `/users/{address}/...`.
fn rewrite_path(path: &str, shared: Option<&str>) -> String {
    match shared {
        Some(m) if path == "/me" || path.starts_with("/me/") || path.starts_with("/me?") => {
            format!("/users/{m}{}", &path[3..])
        }
        _ => path.to_string(),
    }
}

/// Add a mailbox that a colleague shared with the signed-in Microsoft 365 user.
pub async fn add_shared_mailbox(state: &AppState, owner: i64, address: &str) -> Result<i64> {
    let address = address.trim();
    if !address.contains('@') {
        bail!("Please enter the email address of the shared mailbox");
    }
    // Probe access first so a missing permission gives a clear error.
    let probe = call(
        state,
        owner,
        Method::GET,
        &format!("/users/{address}/mailFolders/inbox?$select=id"),
        None,
    )
    .await;
    if let Err(e) = probe {
        bail!("No access to the mailbox {address}: {e:#}");
    }
    let cfg = json!({ "shared_of": owner, "mailbox": address });
    let id = db::insert_account(
        &state.db(),
        AccountKind::Graph,
        address,
        address,
        &cfg.to_string(),
    )?;
    Ok(id)
}
/// Upper bound of pages followed per collection during a sync.
const MAX_PAGES: usize = 10;

fn login_url(state: &AppState, path: &str) -> String {
    format!(
        "https://login.microsoftonline.com/{}/oauth2/v2.0/{path}",
        state.graph_tenant
    )
}

fn client_id(state: &AppState) -> Result<&str> {
    state
        .graph_client_id
        .as_deref()
        .ok_or_else(|| anyhow!("OPENLOOK_GRAPH_CLIENT_ID is not set (see README)"))
}

// ---------------------------------------------------------------- device code flow

#[derive(Debug, Deserialize)]
pub struct DeviceCode {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub interval: u64,
    pub expires_in: u64,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: i64,
}

#[derive(Debug, Deserialize)]
struct TokenError {
    error: String,
    error_description: Option<String>,
}

pub async fn start_device_flow(state: &AppState) -> Result<DeviceCode> {
    let res = state
        .http
        .post(login_url(state, "devicecode"))
        .form(&[("client_id", client_id(state)?), ("scope", SCOPES)])
        .send()
        .await?;
    if !res.status().is_success() {
        bail!(
            "Device code request failed: {}",
            res.text().await.unwrap_or_default()
        );
    }
    Ok(res.json().await?)
}

/// Poll the token endpoint until the user has signed in. Returns the new account id.
pub async fn finish_device_flow(state: &AppState, dc: &DeviceCode) -> Result<i64> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(dc.expires_in);
    let mut interval = dc.interval.max(1);
    let tokens = loop {
        tokio::time::sleep(std::time::Duration::from_secs(interval)).await;
        if std::time::Instant::now() > deadline {
            bail!("Sign-in expired");
        }
        let res = state
            .http
            .post(login_url(state, "token"))
            .form(&[
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                ("client_id", client_id(state)?),
                ("device_code", dc.device_code.as_str()),
            ])
            .send()
            .await?;
        if res.status().is_success() {
            break res.json::<TokenResponse>().await?;
        }
        let err: TokenError = res.json().await?;
        match err.error.as_str() {
            "authorization_pending" => continue,
            "slow_down" => interval += 5,
            _ => bail!(
                "Sign-in failed: {}",
                err.error_description.unwrap_or(err.error)
            ),
        }
    };
    let cfg = GraphConfig {
        access_token: tokens.access_token,
        refresh_token: tokens.refresh_token.unwrap_or_default(),
        expires_at: chrono::Utc::now().timestamp() + tokens.expires_in,
    };
    let me: Value = state
        .http
        .get(format!("{}/me", graph_url()))
        .bearer_auth(&cfg.access_token)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let email = me["mail"]
        .as_str()
        .or(me["userPrincipalName"].as_str())
        .unwrap_or_default()
        .to_string();
    let name = me["displayName"].as_str().unwrap_or(&email).to_string();
    let id = db::insert_account(
        &state.db(),
        AccountKind::Graph,
        &name,
        &email,
        &serde_json::to_string(&cfg)?,
    )?;
    crate::secrets::secure(state, id)?;
    Ok(id)
}

async fn access_token(state: &AppState, account_id: i64) -> Result<String> {
    let raw = crate::secrets::config(state, account_id)?;
    let mut cfg: GraphConfig = serde_json::from_str(&raw)?;
    if cfg.expires_at - 120 > chrono::Utc::now().timestamp() {
        return Ok(cfg.access_token);
    }
    let res = state
        .http
        .post(login_url(state, "token"))
        .form(&[
            ("grant_type", "refresh_token"),
            ("client_id", client_id(state)?),
            ("refresh_token", cfg.refresh_token.as_str()),
            ("scope", SCOPES),
        ])
        .send()
        .await?;
    if !res.status().is_success() {
        bail!(
            "Token refresh failed, please reconnect the account: {}",
            res.text().await.unwrap_or_default()
        );
    }
    let t: TokenResponse = res.json().await?;
    cfg.access_token = t.access_token;
    if let Some(r) = t.refresh_token {
        cfg.refresh_token = r;
    }
    cfg.expires_at = chrono::Utc::now().timestamp() + t.expires_in;
    db::set_account_config(&state.db(), account_id, &serde_json::to_string(&cfg)?)?;
    crate::secrets::secure(state, account_id)?;
    Ok(cfg.access_token)
}

// ---------------------------------------------------------------- HTTP helpers

async fn call(
    state: &AppState,
    account_id: i64,
    method: Method,
    path: &str,
    body: Option<Value>,
) -> Result<Value> {
    let (token_account, shared) = target(state, account_id)?;
    let token = access_token(state, token_account).await?;
    let url = if path.starts_with("http://") || path.starts_with("https://") {
        path.to_string()
    } else {
        format!("{}{}", graph_url(), rewrite_path(path, shared.as_deref()))
    };
    let mut req = state.http.request(method, &url).bearer_auth(token).header(
        "Prefer",
        "outlook.timezone=\"UTC\", outlook.body-content-type=\"html\"",
    );
    if let Some(b) = body {
        req = req.json(&b);
    }
    let res = req.send().await?;
    let status = res.status();
    if status == StatusCode::NO_CONTENT || status == StatusCode::ACCEPTED {
        return Ok(Value::Null);
    }
    let text = res.text().await?;
    if !status.is_success() {
        bail!("Graph {status}: {text}");
    }
    if text.is_empty() {
        return Ok(Value::Null);
    }
    Ok(serde_json::from_str(&text)?)
}

async fn get_all(state: &AppState, account_id: i64, path: &str) -> Result<Vec<Value>> {
    let mut out = vec![];
    let mut next = Some(path.to_string());
    for _ in 0..MAX_PAGES {
        let Some(url) = next.take() else { break };
        let page = call(state, account_id, Method::GET, &url, None).await?;
        if let Some(items) = page["value"].as_array() {
            out.extend(items.iter().cloned());
        }
        next = page["@odata.nextLink"].as_str().map(String::from);
    }
    Ok(out)
}

fn s(v: &Value) -> String {
    v.as_str().unwrap_or_default().to_string()
}

/// Graph returns `dateTime` without offset when `outlook.timezone="UTC"` is requested.
fn graph_dt(v: &Value) -> String {
    let dt = v["dateTime"].as_str().unwrap_or_default();
    let trimmed = dt.split('.').next().unwrap_or(dt);
    format!("{trimmed}Z")
}

fn recipients(v: &Value) -> String {
    v.as_array()
        .map(|a| {
            a.iter()
                .map(|r| {
                    let name = s(&r["emailAddress"]["name"]);
                    let addr = s(&r["emailAddress"]["address"]);
                    if name.is_empty() || name == addr {
                        addr
                    } else {
                        format!("{name} <{addr}>")
                    }
                })
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default()
}

fn to_recipients(list: &str) -> Value {
    Value::Array(
        crate::providers::split_addresses(list)
            .into_iter()
            .map(|a| json!({ "emailAddress": { "address": a } }))
            .collect(),
    )
}

// ---------------------------------------------------------------- sync

pub async fn sync(state: &AppState, account_id: i64) -> Result<()> {
    sync_mail(state, account_id).await.context("Mail")?;
    sync_events(state, account_id).await.context("Calendar")?;
    sync_contacts(state, account_id).await.context("Contacts")?;
    // Microsoft To Do lists cannot be opened for other users' mailboxes.
    if target(state, account_id)?.1.is_none() {
        sync_tasks(state, account_id).await.context("Tasks")?;
    }
    Ok(())
}

async fn sync_mail(state: &AppState, account_id: i64) -> Result<()> {
    let mut roles = std::collections::HashMap::new();
    for (wk, role) in [
        ("inbox", "inbox"),
        ("sentitems", "sent"),
        ("drafts", "drafts"),
        ("deleteditems", "trash"),
        ("junkemail", "junk"),
        ("archive", "archive"),
    ] {
        if let Ok(f) = call(
            state,
            account_id,
            Method::GET,
            &format!("/me/mailFolders/{wk}?$select=id"),
            None,
        )
        .await
        {
            roles.insert(s(&f["id"]), role);
        }
    }
    // Top-level folders plus their subfolders (breadth first, a few levels deep).
    let mut folders: Vec<(Value, String)> = get_all(
        state,
        account_id,
        "/me/mailFolders?$top=100&includeHiddenFolders=false",
    )
    .await?
    .into_iter()
    .map(|f| (f, String::new()))
    .collect();
    let mut i = 0;
    while i < folders.len() && folders.len() < 500 {
        let (f, _) = &folders[i];
        if f["childFolderCount"].as_i64().unwrap_or(0) > 0 {
            let id = s(&f["id"]);
            let children = get_all(
                state,
                account_id,
                &format!("/me/mailFolders/{id}/childFolders?$top=100"),
            )
            .await?;
            folders.extend(children.into_iter().map(|c| (c, id.clone())));
        }
        i += 1;
    }
    let mut keep_folders = vec![];
    for (f, parent) in folders {
        let rid = s(&f["id"]);
        let role = roles.get(&rid).copied().unwrap_or("other");
        let fid = db::upsert_folder(&state.db(), account_id, &rid, &s(&f["displayName"]), role)?;
        db::set_folder_parent(&state.db(), fid, &parent)?;
        keep_folders.push(fid);

        let msgs = call(
            state,
            account_id,
            Method::GET,
            &format!(
                "/me/mailFolders/{rid}/messages?$top=100&$orderby=receivedDateTime desc&$select=id,subject,from,toRecipients,ccRecipients,receivedDateTime,bodyPreview,body,isRead,flag,hasAttachments,importance,categories"
            ),
            None,
        )
        .await?;
        let mut keep = vec![];
        for m in msgs["value"].as_array().cloned().unwrap_or_default() {
            let html = s(&m["body"]["content"]);
            let nm = NewMessage {
                remote_id: s(&m["id"]),
                subject: m["subject"]
                    .as_str()
                    .filter(|x| !x.is_empty())
                    .unwrap_or("(no subject)")
                    .to_string(),
                from_name: s(&m["from"]["emailAddress"]["name"]),
                from_addr: s(&m["from"]["emailAddress"]["address"]),
                to: recipients(&m["toRecipients"]),
                cc: recipients(&m["ccRecipients"]),
                date: mail::to_utc(&s(&m["receivedDateTime"])),
                preview: mail::make_preview(&s(&m["bodyPreview"])),
                body_text: mail::strip_html(&html),
                body_html: html,
                is_read: m["isRead"].as_bool().unwrap_or(false),
                is_flagged: m["flag"]["flagStatus"].as_str() == Some("flagged"),
                has_attachments: m["hasAttachments"].as_bool().unwrap_or(false),
                importance: s(&m["importance"]),
                attachments: vec![],
            };
            let id = db::upsert_message(&state.db(), account_id, fid, &nm)?;
            let cats: Vec<String> = m["categories"]
                .as_array()
                .map(|a| a.iter().map(s).collect())
                .unwrap_or_default();
            db::update_message_flags(&state.db(), id, None, None, None, Some(&cats))?;
            keep.push(nm.remote_id);
        }
        db::prune_messages(&state.db(), fid, &keep)?;
    }
    db::prune_folders(&state.db(), account_id, &keep_folders)?;
    Ok(())
}

async fn sync_events(state: &AppState, account_id: i64) -> Result<()> {
    let now = chrono::Utc::now();
    let from = (now - chrono::Duration::days(90)).format("%Y-%m-%dT00:00:00Z");
    let to = (now + chrono::Duration::days(365)).format("%Y-%m-%dT00:00:00Z");
    let items = get_all(
        state,
        account_id,
        &format!("/me/calendarView?startDateTime={from}&endDateTime={to}&$top=200&$select=id,subject,start,end,isAllDay,location,body,attendees,showAs,categories,reminderMinutesBeforeStart,isReminderOn,responseStatus,organizer,isOrganizer"),
    )
    .await?;
    let c = state.db();
    db::clear_remote_items(&c, "events", account_id)?;
    for e in items {
        db::save_event(&c, &event_from_graph(account_id, &e))?;
    }
    Ok(())
}

fn event_from_graph(account_id: i64, e: &Value) -> Event {
    Event {
        id: 0,
        account_id,
        remote_id: s(&e["id"]),
        title: s(&e["subject"]),
        start: graph_dt(&e["start"]),
        end: graph_dt(&e["end"]),
        all_day: e["isAllDay"].as_bool().unwrap_or(false),
        location: s(&e["location"]["displayName"]),
        body: mail::strip_html(&s(&e["body"]["content"]))
            .trim()
            .to_string(),
        attendees: e["attendees"]
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|x| s(&x["emailAddress"]["address"]))
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default(),
        show_as: match e["showAs"].as_str() {
            Some("free") => "free",
            Some("tentative") => "tentative",
            Some("oof") => "oof",
            _ => "busy",
        }
        .into(),
        category: e["categories"][0].as_str().unwrap_or_default().to_string(),
        reminder: if e["isReminderOn"].as_bool().unwrap_or(false) {
            e["reminderMinutesBeforeStart"].as_i64().unwrap_or(15)
        } else {
            -1
        },
        // calendarView returns single occurrences, so no pattern is kept locally.
        recurrence: String::new(),
        response: if e["isOrganizer"].as_bool().unwrap_or(true) {
            String::new()
        } else {
            s(&e["responseStatus"]["response"])
        },
        organizer: s(&e["organizer"]["emailAddress"]["address"]),
    }
}

async fn sync_contacts(state: &AppState, account_id: i64) -> Result<()> {
    let items = get_all(state, account_id, "/me/contacts?$top=500").await?;
    let c = state.db();
    db::clear_remote_items(&c, "contacts", account_id)?;
    for x in items {
        let addr = &x["businessAddress"];
        let address = [s(&addr["street"]), s(&addr["postalCode"]), s(&addr["city"])]
            .into_iter()
            .filter(|p| !p.is_empty())
            .collect::<Vec<_>>()
            .join(", ");
        db::save_contact(
            &c,
            &Contact {
                id: 0,
                account_id,
                remote_id: s(&x["id"]),
                display_name: s(&x["displayName"]),
                email: s(&x["emailAddresses"][0]["address"]),
                phone: x["mobilePhone"]
                    .as_str()
                    .map(String::from)
                    .unwrap_or_else(|| s(&x["businessPhones"][0])),
                company: s(&x["companyName"]),
                job_title: s(&x["jobTitle"]),
                address,
                notes: s(&x["personalNotes"]),
                favorite: false,
            },
        )?;
    }
    Ok(())
}

async fn sync_tasks(state: &AppState, account_id: i64) -> Result<()> {
    let lists = get_all(state, account_id, "/me/todo/lists").await?;
    let mut all = vec![];
    for l in &lists {
        let lid = s(&l["id"]);
        let tasks = get_all(
            state,
            account_id,
            &format!("/me/todo/lists/{lid}/tasks?$top=100"),
        )
        .await?;
        for t in tasks {
            all.push(Task {
                id: 0,
                account_id,
                remote_id: s(&t["id"]),
                list_name: s(&l["displayName"]),
                list_remote_id: lid.clone(),
                title: s(&t["title"]),
                due: t["dueDateTime"]["dateTime"]
                    .as_str()
                    .map(|d| d.chars().take(10).collect())
                    .unwrap_or_default(),
                completed: t["status"].as_str() == Some("completed"),
                important: t["importance"].as_str() == Some("high"),
                my_day: false,
                notes: s(&t["body"]["content"]),
            });
        }
    }
    let c = state.db();
    db::clear_remote_items(&c, "tasks", account_id)?;
    for t in all {
        db::save_task(&c, &t)?;
    }
    Ok(())
}

// ---------------------------------------------------------------- write-through

pub async fn patch_message(
    state: &AppState,
    account_id: i64,
    remote_id: &str,
    read: Option<bool>,
    flagged: Option<bool>,
    categories: Option<&[String]>,
) -> Result<()> {
    let mut body = serde_json::Map::new();
    if let Some(r) = read {
        body.insert("isRead".into(), json!(r));
    }
    if let Some(f) = flagged {
        body.insert(
            "flag".into(),
            json!({ "flagStatus": if f { "flagged" } else { "notFlagged" } }),
        );
    }
    if let Some(c) = categories {
        body.insert("categories".into(), json!(c));
    }
    if body.is_empty() {
        return Ok(());
    }
    call(
        state,
        account_id,
        Method::PATCH,
        &format!("/me/messages/{remote_id}"),
        Some(Value::Object(body)),
    )
    .await?;
    Ok(())
}

/// Move a message; returns its new remote id.
pub async fn move_message(
    state: &AppState,
    account_id: i64,
    remote_id: &str,
    dest_remote: &str,
) -> Result<String> {
    let res = call(
        state,
        account_id,
        Method::POST,
        &format!("/me/messages/{remote_id}/move"),
        Some(json!({ "destinationId": dest_remote })),
    )
    .await?;
    Ok(s(&res["id"]))
}

pub async fn delete_message(state: &AppState, account_id: i64, remote_id: &str) -> Result<()> {
    call(
        state,
        account_id,
        Method::DELETE,
        &format!("/me/messages/{remote_id}"),
        None,
    )
    .await?;
    Ok(())
}

pub async fn send(state: &AppState, account_id: i64, m: &OutgoingMail) -> Result<()> {
    let mut body = json!({
        "message": {
            "subject": m.subject,
            "body": { "contentType": "HTML", "content": m.body_html },
            "toRecipients": to_recipients(&m.to),
            "ccRecipients": to_recipients(&m.cc),
            "bccRecipients": to_recipients(&m.bcc),
            "importance": if m.importance.is_empty() { "normal" } else { &m.importance },
            "attachments": m.attachments.iter().map(|a| json!({
                "@odata.type": "#microsoft.graph.fileAttachment",
                "name": a.name,
                "contentType": a.mime,
                "contentBytes": base64::engine::general_purpose::STANDARD.encode(&a.data),
            })).collect::<Vec<_>>(),
        },
        "saveToSentItems": true,
    });
    let msg = &mut body["message"];
    msg["isReadReceiptRequested"] = json!(m.read_receipt);
    msg["isDeliveryReceiptRequested"] = json!(m.delivery_receipt);
    let reply_to = to_recipients(&m.reply_to);
    if reply_to.as_array().is_some_and(|a| !a.is_empty()) {
        msg["replyTo"] = reply_to;
    }
    if let Some(at) = &m.send_at {
        // PidTagDeferredSendTime: Exchange sends the mail at that time.
        msg["singleValueExtendedProperties"] = json!([{ "id": "SystemTime 0x3FEF", "value": at }]);
    }
    call(state, account_id, Method::POST, "/me/sendMail", Some(body)).await?;
    Ok(())
}

/// Create a mail folder; returns its id.
pub async fn create_folder(
    state: &AppState,
    account_id: i64,
    parent: Option<&str>,
    name: &str,
) -> Result<String> {
    let path = match parent {
        Some(p) => format!("/me/mailFolders/{p}/childFolders"),
        None => "/me/mailFolders".to_string(),
    };
    let r = call(
        state,
        account_id,
        Method::POST,
        &path,
        Some(json!({ "displayName": name })),
    )
    .await?;
    Ok(s(&r["id"]))
}

pub async fn rename_folder(state: &AppState, account_id: i64, id: &str, name: &str) -> Result<()> {
    call(
        state,
        account_id,
        Method::PATCH,
        &format!("/me/mailFolders/{id}"),
        Some(json!({ "displayName": name })),
    )
    .await?;
    Ok(())
}

pub async fn delete_folder(state: &AppState, account_id: i64, id: &str) -> Result<()> {
    call(
        state,
        account_id,
        Method::DELETE,
        &format!("/me/mailFolders/{id}"),
        None,
    )
    .await?;
    Ok(())
}

/// Download the file attachments of a message.
pub async fn load_attachments(
    state: &AppState,
    account_id: i64,
    remote_id: &str,
) -> Result<Vec<NewAttachment>> {
    let items = get_all(
        state,
        account_id,
        &format!("/me/messages/{remote_id}/attachments"),
    )
    .await?;
    Ok(items
        .iter()
        .filter(|a| a["@odata.type"] == "#microsoft.graph.fileAttachment")
        .map(|a| NewAttachment {
            name: s(&a["name"]),
            mime: s(&a["contentType"]),
            data: base64::engine::general_purpose::STANDARD
                .decode(s(&a["contentBytes"]))
                .unwrap_or_default(),
        })
        .collect())
}

fn event_to_graph(e: &Event) -> Value {
    let dt = |v: &str| {
        let v = v.trim_end_matches('Z');
        let v = v.split('+').next().unwrap_or(v);
        json!({ "dateTime": v, "timeZone": "UTC" })
    };
    let mut v = json!({
        "subject": e.title,
        "start": dt(&e.start),
        "end": dt(&e.end),
        "isAllDay": e.all_day,
        "location": { "displayName": e.location },
        "body": { "contentType": "Text", "content": e.body },
        "attendees": crate::providers::split_addresses(&e.attendees).into_iter()
            .map(|a| json!({ "emailAddress": { "address": a }, "type": "required" })).collect::<Vec<_>>(),
        "showAs": e.show_as,
        "categories": if e.category.is_empty() { vec![] } else { vec![e.category.clone()] },
        "isReminderOn": e.reminder >= 0,
        "reminderMinutesBeforeStart": e.reminder.max(0),
    });
    if let Some(r) = crate::recur::Recurrence::parse(&e.recurrence) {
        let start = chrono::DateTime::parse_from_rfc3339(&e.start)
            .map(|d| d.with_timezone(&chrono::Local).date_naive())
            .unwrap_or_default();
        v["recurrence"] = r.to_graph(start);
    }
    v
}

/// Answer a meeting request: "accept", "tentativelyAccept" or "decline".
pub async fn respond_event(
    state: &AppState,
    account_id: i64,
    remote_id: &str,
    answer: &str,
) -> Result<()> {
    call(
        state,
        account_id,
        Method::POST,
        &format!("/me/events/{remote_id}/{answer}"),
        Some(json!({ "sendResponse": true, "comment": "" })),
    )
    .await?;
    Ok(())
}

/// Busy times of people for the Scheduling Assistant (`getSchedule`).
/// `from`/`to` are UTC "YYYY-MM-DDTHH:MM:SSZ".
pub async fn get_schedule(
    state: &AppState,
    account_id: i64,
    emails: &[String],
    from: &str,
    to: &str,
) -> Result<Vec<super::FreeBusy>> {
    let utc = |s: &str| json!({ "dateTime": s.trim_end_matches('Z'), "timeZone": "UTC" });
    let v = call(
        state,
        account_id,
        Method::POST,
        "/me/calendar/getSchedule",
        Some(json!({
            "schedules": emails,
            "startTime": utc(from),
            "endTime": utc(to),
            "availabilityViewInterval": 30,
        })),
    )
    .await?;
    Ok(v["value"]
        .as_array()
        .map(|a| a.as_slice())
        .unwrap_or_default()
        .iter()
        .map(|x| super::FreeBusy {
            email: s(&x["scheduleId"]),
            busy: x["error"].is_null().then(|| {
                x["scheduleItems"]
                    .as_array()
                    .map(|a| a.as_slice())
                    .unwrap_or_default()
                    .iter()
                    .map(|i| super::Busy {
                        start: graph_dt(&i["start"]),
                        end: graph_dt(&i["end"]),
                        show_as: s(&i["status"]),
                    })
                    .collect()
            }),
        })
        .collect())
}

/// Automatic replies ("out of office") of a Microsoft 365 mailbox.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AutoReply {
    pub enabled: bool,
    /// Only between `start` and `end` (UTC "YYYY-MM-DDTHH:MM:SS").
    pub scheduled: bool,
    pub start: String,
    pub end: String,
    pub internal: String,
    pub external: String,
}

pub async fn get_auto_reply(state: &AppState, account_id: i64) -> Result<AutoReply> {
    let v = call(
        state,
        account_id,
        Method::GET,
        "/me/mailboxSettings/automaticRepliesSetting",
        None,
    )
    .await?;
    let status = s(&v["status"]);
    Ok(AutoReply {
        enabled: status != "disabled",
        scheduled: status == "scheduled",
        start: s(&v["scheduledStartDateTime"]["dateTime"])
            .chars()
            .take(19)
            .collect(),
        end: s(&v["scheduledEndDateTime"]["dateTime"])
            .chars()
            .take(19)
            .collect(),
        internal: mail::strip_html(&s(&v["internalReplyMessage"])),
        external: mail::strip_html(&s(&v["externalReplyMessage"])),
    })
}

pub async fn set_auto_reply(state: &AppState, account_id: i64, a: &AutoReply) -> Result<()> {
    let status = match (a.enabled, a.scheduled) {
        (false, _) => "disabled",
        (true, true) => "scheduled",
        (true, false) => "alwaysEnabled",
    };
    let mut setting = json!({
        "status": status,
        "internalReplyMessage": mail::text_to_html(&a.internal),
        "externalReplyMessage": mail::text_to_html(&a.external),
        "externalAudience": if a.external.trim().is_empty() { "none" } else { "all" },
    });
    if a.scheduled {
        setting["scheduledStartDateTime"] = json!({ "dateTime": a.start, "timeZone": "UTC" });
        setting["scheduledEndDateTime"] = json!({ "dateTime": a.end, "timeZone": "UTC" });
    }
    call(
        state,
        account_id,
        Method::PATCH,
        "/me/mailboxSettings",
        Some(json!({ "automaticRepliesSetting": setting })),
    )
    .await?;
    Ok(())
}

/// Create or update an event remotely; returns the remote id.
pub async fn save_event(state: &AppState, e: &Event) -> Result<String> {
    if e.remote_id.is_empty() {
        let r = call(
            state,
            e.account_id,
            Method::POST,
            "/me/events",
            Some(event_to_graph(e)),
        )
        .await?;
        Ok(s(&r["id"]))
    } else {
        call(
            state,
            e.account_id,
            Method::PATCH,
            &format!("/me/events/{}", e.remote_id),
            Some(event_to_graph(e)),
        )
        .await?;
        Ok(e.remote_id.clone())
    }
}

pub async fn delete_event(state: &AppState, account_id: i64, remote_id: &str) -> Result<()> {
    call(
        state,
        account_id,
        Method::DELETE,
        &format!("/me/events/{remote_id}"),
        None,
    )
    .await?;
    Ok(())
}

fn contact_to_graph(c: &Contact) -> Value {
    let mut v = json!({
        "displayName": c.display_name,
        "companyName": c.company,
        "jobTitle": c.job_title,
        "mobilePhone": c.phone,
        "personalNotes": c.notes,
        "emailAddresses": if c.email.is_empty() { vec![] } else { vec![json!({ "address": c.email, "name": c.display_name })] },
    });
    // Graph requires givenName or displayName parts for some clients.
    if let Some((first, last)) = c.display_name.split_once(' ') {
        v["givenName"] = json!(first);
        v["surname"] = json!(last);
    } else {
        v["givenName"] = json!(c.display_name);
    }
    v
}

pub async fn save_contact(state: &AppState, c: &Contact) -> Result<String> {
    if c.remote_id.is_empty() {
        let r = call(
            state,
            c.account_id,
            Method::POST,
            "/me/contacts",
            Some(contact_to_graph(c)),
        )
        .await?;
        Ok(s(&r["id"]))
    } else {
        call(
            state,
            c.account_id,
            Method::PATCH,
            &format!("/me/contacts/{}", c.remote_id),
            Some(contact_to_graph(c)),
        )
        .await?;
        Ok(c.remote_id.clone())
    }
}

pub async fn delete_contact(state: &AppState, account_id: i64, remote_id: &str) -> Result<()> {
    call(
        state,
        account_id,
        Method::DELETE,
        &format!("/me/contacts/{remote_id}"),
        None,
    )
    .await?;
    Ok(())
}

fn task_to_graph(t: &Task) -> Value {
    let mut v = json!({
        "title": t.title,
        "status": if t.completed { "completed" } else { "notStarted" },
        "importance": if t.important { "high" } else { "normal" },
        "body": { "contentType": "text", "content": t.notes },
    });
    v["dueDateTime"] = if t.due.is_empty() {
        Value::Null
    } else {
        json!({ "dateTime": format!("{}T00:00:00", t.due), "timeZone": "UTC" })
    };
    v
}

/// Create or update a To Do task; returns (remote id, list id, list name).
pub async fn save_task(state: &AppState, t: &Task) -> Result<(String, String, String)> {
    let (list_id, list_name) = if t.list_remote_id.is_empty() {
        let lists = get_all(state, t.account_id, "/me/todo/lists").await?;
        let l = lists
            .iter()
            .find(|l| l["wellknownListName"] == "defaultList")
            .or(lists.first())
            .ok_or_else(|| anyhow!("No To Do list found"))?;
        (s(&l["id"]), s(&l["displayName"]))
    } else {
        (t.list_remote_id.clone(), t.list_name.clone())
    };
    let id = if t.remote_id.is_empty() {
        let r = call(
            state,
            t.account_id,
            Method::POST,
            &format!("/me/todo/lists/{list_id}/tasks"),
            Some(task_to_graph(t)),
        )
        .await?;
        s(&r["id"])
    } else {
        call(
            state,
            t.account_id,
            Method::PATCH,
            &format!("/me/todo/lists/{list_id}/tasks/{}", t.remote_id),
            Some(task_to_graph(t)),
        )
        .await?;
        t.remote_id.clone()
    };
    Ok((id, list_id, list_name))
}

pub async fn delete_task(state: &AppState, t: &Task) -> Result<()> {
    call(
        state,
        t.account_id,
        Method::DELETE,
        &format!("/me/todo/lists/{}/tasks/{}", t.list_remote_id, t.remote_id),
        None,
    )
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewrites_paths_for_shared_mailboxes() {
        assert_eq!(
            rewrite_path("/me/messages/1", Some("team@x.de")),
            "/users/team@x.de/messages/1"
        );
        assert_eq!(rewrite_path("/me", Some("team@x.de")), "/users/team@x.de");
        assert_eq!(
            rewrite_path("/me/mailFolders/inbox", None),
            "/me/mailFolders/inbox"
        );
        assert_eq!(
            rewrite_path("/users/a@x.de/mailFolders", Some("team@x.de")),
            "/users/a@x.de/mailFolders"
        );
    }

    #[test]
    fn parses_graph_datetime() {
        assert_eq!(
            graph_dt(&json!({ "dateTime": "2026-03-10T09:00:00.0000000" })),
            "2026-03-10T09:00:00Z"
        );
    }

    #[test]
    fn formats_recipients() {
        let v = json!([{ "emailAddress": { "name": "Bob", "address": "bob@x.de" } }, { "emailAddress": { "address": "a@x.de" } }]);
        assert_eq!(recipients(&v), "Bob <bob@x.de>, a@x.de");
    }

    #[test]
    fn maps_event_fields() {
        let e = event_from_graph(
            1,
            &json!({
                "id": "abc", "subject": "Daily", "isAllDay": false, "showAs": "tentative",
                "start": { "dateTime": "2026-03-10T09:00:00.0000000" }, "end": { "dateTime": "2026-03-10T09:15:00.0000000" },
                "isReminderOn": true, "reminderMinutesBeforeStart": 5, "categories": ["Blue Category"]
            }),
        );
        assert_eq!(
            (
                e.title.as_str(),
                e.show_as.as_str(),
                e.reminder,
                e.category.as_str()
            ),
            ("Daily", "tentative", 5, "Blue Category")
        );
        assert_eq!(
            event_to_graph(&e)["start"]["dateTime"],
            "2026-03-10T09:00:00"
        );
    }
}
