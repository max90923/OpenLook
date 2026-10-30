//! Tests of the Microsoft Graph provider against a local mock of the Graph
//! REST API. Responses follow the shapes documented by Microsoft; every
//! request is recorded so the payloads OpenLook sends can be checked.

use std::sync::{Arc, Mutex};

use axum::{
    body::Bytes,
    extract::State,
    http::{Method, StatusCode, Uri},
    response::{IntoResponse, Response},
    Json, Router,
};
use base64::Engine;
use serde_json::{json, Value};

use crate::{
    db,
    models::*,
    providers::{self, MessagePatch, OutgoingMail},
    state::AppState,
};

type Log = Arc<Mutex<Vec<(String, String, Value)>>>;

const SHARED: &str = "team@contoso.com";

fn folder_id(wk: &str) -> &'static str {
    match wk {
        "inbox" => "F-INBOX",
        "sentitems" => "F-SENT",
        "drafts" => "F-DRAFTS",
        "deleteditems" => "F-TRASH",
        "junkemail" => "F-JUNK",
        "archive" => "F-ARCHIVE",
        _ => "",
    }
}

async fn handler(State(log): State<Log>, method: Method, uri: Uri, body: Bytes) -> Response {
    let full = uri
        .path_and_query()
        .map(|p| p.as_str().to_string())
        .unwrap_or_default();
    let body_json: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    log.lock()
        .unwrap()
        .push((method.to_string(), full.clone(), body_json));
    // Shared mailbox requests look like the user's own, just under /users/{address}.
    let path = uri.path().replacen(&format!("/users/{SHARED}"), "/me", 1);
    let ok = |v: Value| Json(v).into_response();
    match (method.as_str(), path.as_str()) {
        ("GET", p) if p.starts_with("/me/mailFolders/") && p.matches('/').count() == 3 => {
            let wk = p.rsplit('/').next().unwrap();
            ok(json!({ "id": folder_id(wk) }))
        }
        ("GET", "/me/mailFolders") => ok(json!({ "value": [
            { "id": "F-INBOX", "displayName": "Inbox" },
            { "id": "F-SENT", "displayName": "Sent Items" },
            { "id": "F-DRAFTS", "displayName": "Drafts" },
            { "id": "F-TRASH", "displayName": "Deleted Items" },
            { "id": "F-JUNK", "displayName": "Junk Email" },
            { "id": "F-ARCHIVE", "displayName": "Archive" },
            { "id": "F-PROJ", "displayName": "Projects", "childFolderCount": 1 }
        ]})),
        ("GET", "/me/mailFolders/F-PROJ/childFolders") => ok(json!({ "value": [
            { "id": "F-OFFERS", "displayName": "Quotes", "childFolderCount": 0 }
        ]})),
        ("GET", "/me/mailFolders/F-INBOX/messages") => ok(json!({ "value": [{
            "id": "M1",
            "subject": "Quote Q4",
            "from": { "emailAddress": { "name": "Lea Fischer", "address": "lea@fabrikam.com" } },
            "toRecipients": [{ "emailAddress": { "name": "Me", "address": "me@contoso.com" } }],
            "ccRecipients": [],
            "receivedDateTime": "2026-10-28T08:15:00Z",
            "bodyPreview": "Please find the quote attached",
            "body": { "contentType": "html", "content": "<p>Please find the <b>quote</b> attached</p>" },
            "isRead": false,
            "flag": { "flagStatus": "flagged" },
            "hasAttachments": true,
            "importance": "high",
            "categories": ["Red Category"]
        }]})),
        ("GET", p) if p.starts_with("/me/mailFolders/") && p.ends_with("/messages") => {
            ok(json!({ "value": [] }))
        }
        ("GET", "/me/messages/M1/attachments") => ok(json!({ "value": [{
            "@odata.type": "#microsoft.graph.fileAttachment",
            "name": "quote.pdf",
            "contentType": "application/pdf",
            "contentBytes": base64::engine::general_purpose::STANDARD.encode(b"%PDF-1.4 Quote")
        }]})),
        ("PATCH", "/me/messages/M1") => ok(json!({ "id": "M1" })),
        ("POST", "/me/messages/M1/move") => ok(json!({ "id": "M1-MOVED" })),
        ("POST", "/me/sendMail") => StatusCode::ACCEPTED.into_response(),
        ("GET", "/me/calendarView") => ok(json!({ "value": [{
            "id": "E1",
            "subject": "Project meeting",
            "start": { "dateTime": "2026-11-02T09:00:00.0000000", "timeZone": "UTC" },
            "end": { "dateTime": "2026-11-02T10:00:00.0000000", "timeZone": "UTC" },
            "isAllDay": false,
            "location": { "displayName": "Room 4" },
            "body": { "contentType": "html", "content": "<p>Agenda</p>" },
            "attendees": [{ "emailAddress": { "address": "me@contoso.com" } }],
            "showAs": "tentative",
            "categories": [],
            "isReminderOn": true,
            "reminderMinutesBeforeStart": 15,
            "isOrganizer": false,
            "organizer": { "emailAddress": { "address": "boss@contoso.com" } },
            "responseStatus": { "response": "notResponded" }
        }]})),
        ("POST", "/me/events") => ok(json!({ "id": "E-NEW" })),
        ("POST", "/me/events/E1/accept") => StatusCode::ACCEPTED.into_response(),
        ("POST", "/me/calendar/getSchedule") => ok(json!({ "value": [
            {
                "scheduleId": "lea@fabrikam.com",
                "scheduleItems": [{
                    "status": "busy",
                    "start": { "dateTime": "2026-11-02T11:00:00.0000000", "timeZone": "UTC" },
                    "end": { "dateTime": "2026-11-02T12:00:00.0000000", "timeZone": "UTC" }
                }]
            },
            {
                "scheduleId": "external@example.org",
                "error": { "message": "Free/busy information is not available", "responseCode": "ErrorMailRecipientNotFound" }
            }
        ]})),
        ("GET", "/me/contacts") => ok(json!({ "value": [{
            "id": "C1",
            "displayName": "Lea Fischer",
            "emailAddresses": [{ "address": "lea@fabrikam.com" }],
            "mobilePhone": "+49 170 1234567",
            "businessPhones": [],
            "companyName": "Fabrikam AG",
            "jobTitle": "Purchasing",
            "businessAddress": { "street": "1 High Street", "postalCode": "10115", "city": "Berlin" },
            "personalNotes": ""
        }]})),
        ("GET", "/me/todo/lists") => ok(
            json!({ "value": [{ "id": "L1", "displayName": "Tasks", "wellknownListName": "defaultList" }] }),
        ),
        ("GET", "/me/todo/lists/L1/tasks") => ok(json!({ "value": [{
            "id": "T1",
            "title": "Review quote",
            "status": "notStarted",
            "importance": "high",
            "dueDateTime": { "dateTime": "2026-11-03T00:00:00.0000000", "timeZone": "UTC" },
            "body": { "content": "", "contentType": "text" }
        }]})),
        ("GET", "/me/mailboxSettings/automaticRepliesSetting") => ok(json!({
            "status": "scheduled",
            "externalAudience": "all",
            "scheduledStartDateTime": { "dateTime": "2026-12-20T08:00:00.0000000", "timeZone": "UTC" },
            "scheduledEndDateTime": { "dateTime": "2027-01-06T08:00:00.0000000", "timeZone": "UTC" },
            "internalReplyMessage": "<p>I am on vacation</p>",
            "externalReplyMessage": "<p>Out of office</p>"
        })),
        ("PATCH", "/me/mailboxSettings") => ok(json!({})),
        _ => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": { "code": "NotFound", "message": full } })),
        )
            .into_response(),
    }
}

fn requests(log: &Log, method: &str, path_prefix: &str) -> Vec<(String, Value)> {
    log.lock()
        .unwrap()
        .iter()
        .filter(|(m, p, _)| m == method && p.starts_with(path_prefix))
        .map(|(_, p, b)| (p.clone(), b.clone()))
        .collect()
}

#[tokio::test]
async fn microsoft_365_against_mock_graph() {
    let log: Log = Default::default();
    let app = Router::new().fallback(handler).with_state(log.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    std::env::set_var("OPENLOOK_GRAPH_URL", format!("http://{addr}"));

    let state = AppState::in_memory();
    let cfg = GraphConfig {
        access_token: "token".into(),
        refresh_token: "refresh".into(),
        expires_at: i64::MAX / 2,
    };
    let acc = db::insert_account(
        &state.db(),
        AccountKind::Graph,
        "Me",
        "me@contoso.com",
        &serde_json::to_string(&cfg).unwrap(),
    )
    .unwrap();

    // --- sync: folders, mail, calendar, contacts, To Do
    providers::sync(&state, acc).await.unwrap();
    let folders = db::list_folders(&state.db()).unwrap();
    for role in ["inbox", "sent", "drafts", "trash", "junk", "archive"] {
        assert!(
            folders.iter().any(|f| f.role == role),
            "folder role {role} missing"
        );
    }
    assert!(folders
        .iter()
        .any(|f| f.name == "Projects" && f.role == "other"));
    let pos = |name: &str| folders.iter().position(|f| f.name == name).unwrap();
    assert_eq!(
        pos("Quotes"),
        pos("Projects") + 1,
        "subfolder follows its parent folder"
    );
    assert_eq!(folders[pos("Quotes")].depth, 1);
    let inbox = db::folder_by_role(&state.db(), acc, "inbox")
        .unwrap()
        .unwrap();
    let msgs = db::list_messages(
        &state.db(),
        &db::MessageQuery {
            folder_id: Some(inbox.id),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(msgs.len(), 1);
    let m = &msgs[0];
    assert_eq!(
        (
            m.subject.as_str(),
            m.from_name.as_str(),
            m.importance.as_str()
        ),
        ("Quote Q4", "Lea Fischer", "high")
    );
    assert!(!m.is_read && m.is_flagged && m.has_attachments);
    assert_eq!(m.categories, vec!["Red Category"]);
    assert_eq!(m.date, "2026-10-28T08:15:00Z");

    let events =
        db::list_events(&state.db(), "2026-11-01T00:00:00Z", "2026-11-30T00:00:00Z").unwrap();
    assert_eq!(events.len(), 1);
    let e = &events[0];
    assert_eq!(
        (
            e.title.as_str(),
            e.start.as_str(),
            e.show_as.as_str(),
            e.reminder
        ),
        ("Project meeting", "2026-11-02T09:00:00Z", "tentative", 15)
    );
    assert_eq!(
        (e.response.as_str(), e.organizer.as_str()),
        ("notResponded", "boss@contoso.com")
    );
    let contacts = db::list_contacts(&state.db()).unwrap();
    assert_eq!(contacts[0].address, "1 High Street, 10115, Berlin");
    let tasks = db::list_tasks(&state.db()).unwrap();
    assert_eq!(
        (
            tasks[0].title.as_str(),
            tasks[0].due.as_str(),
            tasks[0].important
        ),
        ("Review quote", "2026-11-03", true)
    );

    // --- attachments on demand
    providers::ensure_attachments(&state, m.id).await.unwrap();
    let atts = db::attachments_with_data(&state.db(), m.id).unwrap();
    assert_eq!(
        (atts[0].name.as_str(), atts[0].data.as_slice()),
        ("quote.pdf", b"%PDF-1.4 Quote".as_slice())
    );

    // --- flags and categories go to the server
    providers::patch_message(
        &state,
        m.id,
        &MessagePatch {
            is_read: Some(true),
            is_flagged: Some(false),
            categories: Some(vec!["Blue Category".into()]),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let patch = &requests(&log, "PATCH", "/me/messages/M1")[0].1;
    assert_eq!(patch["isRead"], true);
    assert_eq!(patch["flag"]["flagStatus"], "notFlagged");
    assert_eq!(patch["categories"][0], "Blue Category");

    // --- move keeps the new server id
    let projects = folders.iter().find(|f| f.name == "Projects").unwrap();
    providers::move_message(&state, m.id, projects.id)
        .await
        .unwrap();
    assert_eq!(
        requests(&log, "POST", "/me/messages/M1/move")[0].1["destinationId"],
        "F-PROJ"
    );
    assert_eq!(db::message_remote(&state.db(), m.id).unwrap().0, "M1-MOVED");

    // --- send with attachment
    providers::send(
        &state,
        &OutgoingMail {
            account_id: acc,
            to: "Lea Fischer <lea@fabrikam.com>".into(),
            cc: "boss@contoso.com".into(),
            subject: "RE: Quote Q4".into(),
            body_html: "<p>Thanks</p>".into(),
            importance: "high".into(),
            attachments: vec![NewAttachment {
                name: "reply.txt".into(),
                mime: "text/plain".into(),
                data: b"ok".to_vec(),
            }],
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let sent = &requests(&log, "POST", "/me/sendMail")[0].1;
    assert_eq!(
        sent["message"]["toRecipients"][0]["emailAddress"]["address"],
        "lea@fabrikam.com"
    );
    assert_eq!(
        sent["message"]["ccRecipients"][0]["emailAddress"]["address"],
        "boss@contoso.com"
    );
    assert_eq!(sent["message"]["importance"], "high");
    assert_eq!(
        sent["message"]["attachments"][0]["contentBytes"],
        base64::engine::general_purpose::STANDARD.encode(b"ok")
    );
    assert_eq!(sent["saveToSentItems"], true);

    // --- recurring appointment is sent as a Graph recurrence pattern
    providers::save_event(
        &state,
        Event {
            account_id: acc,
            title: "Weekly sync".into(),
            start: "2026-11-02T08:00:00Z".into(),
            end: "2026-11-02T08:30:00Z".into(),
            recurrence: "weekly;1;1;2027-01-31".into(),
            show_as: "busy".into(),
            reminder: 15,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let created = &requests(&log, "POST", "/me/events")[0].1;
    assert_eq!(created["recurrence"]["pattern"]["type"], "weekly");
    assert_eq!(created["recurrence"]["pattern"]["daysOfWeek"][0], "monday");
    assert_eq!(created["recurrence"]["range"]["endDate"], "2027-01-31");

    // --- meeting request answered
    let invite = db::list_events(&state.db(), "2026-11-01T00:00:00Z", "2026-11-30T00:00:00Z")
        .unwrap()
        .into_iter()
        .find(|e| e.remote_id == "E1")
        .unwrap();
    providers::respond_event(&state, invite.id, "accept")
        .await
        .unwrap();
    assert_eq!(
        requests(&log, "POST", "/me/events/E1/accept")[0].1["sendResponse"],
        true
    );
    assert_eq!(
        db::get_event(&state.db(), invite.id)
            .unwrap()
            .unwrap()
            .response,
        "accepted"
    );

    // --- Scheduling Assistant: own calendar locally, others via getSchedule
    let people: Vec<String> = ["me@contoso.com", "lea@fabrikam.com", "external@example.org"]
        .map(String::from)
        .to_vec();
    let fb = providers::free_busy(
        &state,
        acc,
        &people,
        "2026-11-02T00:00:00Z",
        "2026-11-03T00:00:00Z",
    )
    .await
    .unwrap();
    let busy = |i: usize| fb[i].busy.clone().unwrap();
    assert_eq!(busy(0)[0].start, "2026-11-02T09:00:00Z", "accepted meeting");
    assert_eq!(busy(1)[0].end, "2026-11-02T12:00:00Z");
    assert_eq!(busy(1)[0].show_as, "busy");
    assert_eq!(fb[2].busy, None, "no information");
    let req = &requests(&log, "POST", "/me/calendar/getSchedule")[0].1;
    assert_eq!(
        req["schedules"],
        json!(["lea@fabrikam.com", "external@example.org"])
    );
    assert_eq!(req["startTime"]["dateTime"], "2026-11-02T00:00:00");

    // --- automatic replies
    let mut ar = providers::graph::get_auto_reply(&state, acc).await.unwrap();
    assert!(ar.enabled && ar.scheduled);
    assert_eq!(ar.internal, "I am on vacation");
    assert_eq!(ar.start, "2026-12-20T08:00:00");
    ar.enabled = false;
    providers::graph::set_auto_reply(&state, acc, &ar)
        .await
        .unwrap();
    assert_eq!(
        requests(&log, "PATCH", "/me/mailboxSettings")[0].1["automaticRepliesSetting"]["status"],
        "disabled"
    );

    // --- shared mailbox: same token, /users/{address} paths, no To Do
    let before = log.lock().unwrap().len();
    let shared = providers::graph::add_shared_mailbox(&state, acc, SHARED)
        .await
        .unwrap();
    providers::sync(&state, shared).await.unwrap();
    let shared_requests: Vec<String> = log.lock().unwrap()[before..]
        .iter()
        .map(|(_, p, _)| p.clone())
        .collect();
    assert!(
        shared_requests
            .iter()
            .all(|p| p.starts_with(&format!("/users/{SHARED}/"))),
        "{shared_requests:?}"
    );
    assert!(!shared_requests.iter().any(|p| p.contains("/todo/")));
    assert!(db::folder_by_role(&state.db(), shared, "inbox")
        .unwrap()
        .is_some());
}
