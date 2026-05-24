use serde::Serialize;
use serde_json::Value;

use roux_core::{ConsumptionMode, EventBuilder, EventKind};
use roux_runtime::alias_store::{BindRequest, ProjectFilter};

use super::identity::DaemonIdentity;
use super::protocol::{Request, Response};

fn serialize_response<T: Serialize>(value: T, label: &str) -> Response {
    match serde_json::to_value(value) {
        Ok(value) => Response::success(value),
        Err(err) => Response::err(format!("failed to serialize {label}: {err}")),
    }
}

pub(super) async fn handle_alias_set(req: Request, identity: &DaemonIdentity) -> Response {
    let raw_alias = match request_arg_str(&req, "alias") {
        Some(alias) => alias,
        None => return Response::err("alias required"),
    };
    let canonical = match roux_core::validate_user_alias_name(raw_alias) {
        Ok(alias) => alias,
        Err(err) => return Response::err(err.to_string()),
    };
    let session_id = match request_arg_str(&req, "session_id")
        .map(String::from)
        .or_else(|| req.session_id.clone())
    {
        Some(session_id) => session_id,
        None => {
            return Response::err(
                "session_id required (call from a session, or pass args.session_id)",
            )
        }
    };
    let bind_req = BindRequest {
        project_id: request_arg_str(&req, "project_id").map(String::from),
        session_id: Some(session_id),
        pane_id: request_arg_str(&req, "pane_id").map(String::from).or_else(|| req.pane_id.clone()),
        auto_claimed: false,
        force: request_arg_bool(&req, "force").unwrap_or(false),
    };

    match identity.alias_manager.bind(&canonical, bind_req) {
        Ok(alias) => serialize_response(alias, "alias"),
        Err(err) => Response::err(err.to_string()),
    }
}

pub(super) async fn handle_alias_unset(req: Request, identity: &DaemonIdentity) -> Response {
    let raw_alias = match request_arg_str(&req, "alias") {
        Some(alias) => alias,
        None => return Response::err("alias required"),
    };
    let canonical = match roux_core::validate_user_alias_name(raw_alias) {
        Ok(alias) => alias,
        Err(err) => return Response::err(err.to_string()),
    };
    let changed = identity.alias_manager.unbind(&canonical, request_arg_str(&req, "project_id"));
    Response::success(serde_json::json!({ "changed": changed }))
}

pub(super) async fn handle_alias_claim(req: Request, identity: &DaemonIdentity) -> Response {
    let raw_alias = match request_arg_str(&req, "alias") {
        Some(alias) => alias,
        None => return Response::err("alias required"),
    };
    let canonical = match roux_core::validate_user_alias_name(raw_alias) {
        Ok(alias) => alias,
        Err(err) => return Response::err(err.to_string()),
    };
    let session_id = match req.session_id.clone() {
        Some(session_id) => session_id,
        None => return Response::err("alias-claim must be invoked from inside a session"),
    };
    let bind_req = BindRequest {
        project_id: request_arg_str(&req, "project_id").map(String::from),
        session_id: Some(session_id),
        pane_id: request_arg_str(&req, "pane_id").map(String::from).or_else(|| req.pane_id.clone()),
        auto_claimed: false,
        force: request_arg_bool(&req, "steal").unwrap_or(false),
    };

    match identity.alias_manager.bind(&canonical, bind_req) {
        Ok(alias) => serialize_response(alias, "alias"),
        Err(err) => Response::err(err.to_string()),
    }
}

pub(super) async fn handle_alias_list(req: Request, identity: &DaemonIdentity) -> Response {
    let aliases = identity.alias_manager.list(
        alias_project_filter(request_arg_str(&req, "project_id"), request_arg_bool(&req, "global")),
        request_arg_bool(&req, "only_unbound").unwrap_or(false),
    );
    serialize_response(aliases, "aliases")
}

pub(super) async fn handle_alias_get(req: Request, identity: &DaemonIdentity) -> Response {
    let raw_alias = match request_arg_str(&req, "alias") {
        Some(alias) => alias,
        None => return Response::err("alias required"),
    };
    let canonical = match roux_core::validate_alias_name(raw_alias) {
        Ok(alias) => alias,
        Err(err) => return Response::err(err.to_string()),
    };
    let project_id = request_arg_str(&req, "project_id");

    if let Some(alias) = identity.alias_manager.get(&canonical, project_id) {
        serialize_response(alias, "alias")
    } else if project_id.is_none() {
        let matches = identity.alias_manager.find_all_by_name(&canonical);
        match matches.len() {
            0 => Response::err(format!("alias '{canonical}' not found")),
            1 => serialize_response(&matches[0], "alias"),
            _ => {
                let projects: Vec<_> =
                    matches.iter().map(|alias| alias.project_id.clone()).collect();
                Response::err(format!(
                    "alias '{canonical}' is ambiguous across projects {projects:?}; pass project_id"
                ))
            }
        }
    } else {
        Response::err(format!("alias '{canonical}' not found"))
    }
}

pub(super) async fn handle_alias_whoami(req: Request, identity: &DaemonIdentity) -> Response {
    let session_id = match request_arg_str(&req, "session_id")
        .map(String::from)
        .or_else(|| req.session_id.clone())
    {
        Some(session_id) => session_id,
        None => {
            return Response::err(
                "session_id required (call from a session, or pass args.session_id)",
            )
        }
    };
    serialize_response(identity.alias_manager.whoami(&session_id), "aliases")
}

pub(super) async fn handle_alias_add_member(req: Request, identity: &DaemonIdentity) -> Response {
    let alias = match canonical_user_alias_arg(&req) {
        Ok(alias) => alias,
        Err(response) => return response,
    };
    let pane_id = match request_arg_str(&req, "pane_id")
        .map(String::from)
        .or_else(|| req.pane_id.clone())
    {
        Some(pane_id) => pane_id,
        None => return Response::err("pane_id required (call from a pane, or pass args.pane_id)"),
    };
    match identity.alias_manager.add_member(&alias, request_arg_str(&req, "project_id"), &pane_id) {
        Ok(alias) => serialize_response(alias, "alias"),
        Err(err) => Response::err(err.to_string()),
    }
}

pub(super) async fn handle_alias_remove_member(
    req: Request,
    identity: &DaemonIdentity,
) -> Response {
    let alias = match canonical_user_alias_arg(&req) {
        Ok(alias) => alias,
        Err(response) => return response,
    };
    let pane_id =
        match request_arg_str(&req, "pane_id").map(String::from).or_else(|| req.pane_id.clone()) {
            Some(pane_id) => pane_id,
            None => return Response::err("pane_id required"),
        };
    match identity.alias_manager.remove_member(
        &alias,
        request_arg_str(&req, "project_id"),
        &pane_id,
    ) {
        Ok(removed) => Response::success(serde_json::json!({ "removed": removed })),
        Err(err) => Response::err(err.to_string()),
    }
}

pub(super) async fn handle_alias_mode(req: Request, identity: &DaemonIdentity) -> Response {
    let alias = match canonical_user_alias_arg(&req) {
        Ok(alias) => alias,
        Err(response) => return response,
    };
    let mode = match request_arg_str(&req, "mode") {
        Some("competing") | Some("competingConsumer") | Some("competing-consumer") => {
            ConsumptionMode::CompetingConsumer
        }
        Some("broadcast") => ConsumptionMode::Broadcast,
        Some(other) => {
            return Response::err(format!(
                "invalid mode '{other}'; expected 'competing' or 'broadcast'"
            ))
        }
        None => return Response::err("mode required"),
    };

    match identity.alias_manager.set_consumption_mode(
        &alias,
        request_arg_str(&req, "project_id"),
        mode,
    ) {
        Ok(alias) => serialize_response(alias, "alias"),
        Err(err) => Response::err(err.to_string()),
    }
}

fn canonical_user_alias_arg(req: &Request) -> Result<String, Response> {
    let raw_alias = request_arg_str(req, "alias").ok_or_else(|| Response::err("alias required"))?;
    roux_core::validate_user_alias_name(raw_alias).map_err(|err| Response::err(err.to_string()))
}

fn alias_project_filter<'a>(project: Option<&'a str>, global: Option<bool>) -> ProjectFilter<'a> {
    match (project, global) {
        (Some(project), _) => ProjectFilter::Exact(Some(project)),
        (None, Some(true)) => ProjectFilter::Exact(None),
        (None, _) => ProjectFilter::Any,
    }
}

fn request_arg_str<'a>(req: &'a Request, key: &str) -> Option<&'a str> {
    req.args.get(key).and_then(|value| value.as_str())
}

fn request_arg_bool(req: &Request, key: &str) -> Option<bool> {
    req.args.get(key).and_then(|value| value.as_bool())
}

fn parse_event_kind(value: &str) -> Result<EventKind, String> {
    match value {
        "task" => Ok(EventKind::Task),
        "result" => Ok(EventKind::Result),
        "question" => Ok(EventKind::Question),
        "fyi" => Ok(EventKind::Fyi),
        "signal" => Ok(EventKind::Signal),
        other => Err(format!("invalid kind: {other}; expected task|result|question|fyi|signal")),
    }
}

fn resolve_recipient_alias(
    identity: &DaemonIdentity,
    req: &Request,
    explicit: Option<&str>,
) -> Result<String, String> {
    if let Some(alias) = explicit {
        return roux_core::validate_alias_name(alias).map_err(|err| err.to_string());
    }

    let mut candidates = Vec::new();
    if let Some(pane_id) = req.pane_id.as_deref() {
        candidates.extend(identity.alias_manager.find_for_pane(pane_id));
    }
    if candidates.is_empty() {
        if let Some(session_id) = req.session_id.as_deref() {
            candidates.extend(identity.alias_manager.whoami(session_id));
        }
    }

    match candidates.len() {
        0 => Err(format!(
            "no alias bound to {context}; claim one with `roux alias claim <name>` or pass args.alias",
            context = req
                .pane_id
                .as_deref()
                .map(|pane_id| format!("pane {pane_id}"))
                .or_else(|| req.session_id.as_deref().map(|session_id| format!("session {session_id}")))
                .unwrap_or_else(|| "this caller".to_string())
        )),
        1 => Ok(candidates[0].alias.clone()),
        _ => {
            let names: Vec<_> = candidates.iter().map(|alias| alias.alias.clone()).collect();
            Err(format!("caller holds multiple aliases ({names:?}); pass args.alias"))
        }
    }
}

fn default_mailbox_from(identity: &DaemonIdentity, req: &Request) -> Option<String> {
    if let Some(pane_id) = req.pane_id.as_deref() {
        let pane_aliases = identity.alias_manager.find_for_pane(pane_id);
        if pane_aliases.len() == 1 {
            return Some(pane_aliases[0].alias.clone());
        }
    }
    let session_id = req.session_id.as_deref()?;
    let mine = identity.alias_manager.whoami(session_id);
    if mine.len() == 1 {
        Some(mine[0].alias.clone())
    } else {
        Some(session_id.to_string())
    }
}

pub(super) async fn handle_mailbox_post(req: Request, identity: &DaemonIdentity) -> Response {
    let body = match request_arg_str(&req, "body") {
        Some(body) => body.to_string(),
        None => return Response::err("body required"),
    };
    let to_raw = request_arg_str(&req, "to");
    let topic = request_arg_str(&req, "topic").map(String::from);
    if to_raw.is_none() && topic.is_none() {
        return Response::err("at least one of `to` or `topic` required");
    }
    let canonical_to = match to_raw {
        Some(raw) => match roux_core::validate_alias_name(raw) {
            Ok(alias) => Some(alias),
            Err(err) => return Response::err(err.to_string()),
        },
        None => None,
    };
    let kind = match request_arg_str(&req, "kind") {
        Some(kind) => match parse_event_kind(kind) {
            Ok(kind) => kind,
            Err(err) => return Response::err(err),
        },
        None => EventKind::Task,
    };
    let from = request_arg_str(&req, "from")
        .map(String::from)
        .or_else(|| default_mailbox_from(identity, &req));
    let project_id = request_arg_str(&req, "project_id").map(String::from);

    if let Some(alias) = &canonical_to {
        identity.alias_manager.ensure(alias, project_id.clone());
    }

    let mut builder = EventBuilder::new(body).kind(kind);
    if let Some(alias) = canonical_to {
        builder = builder.to(alias);
    }
    if let Some(topic) = topic {
        builder = builder.topic(topic);
    }
    if let Some(from) = from {
        builder = builder.from(from);
    }
    if let Some(project_id) = project_id {
        builder = builder.project_id(project_id);
    }
    if let Some(subject) = request_arg_str(&req, "subject") {
        builder = builder.subject(subject.to_string());
    }
    if let Some(correlation_id) = request_arg_str(&req, "correlation_id") {
        builder = builder.correlation_id(correlation_id.to_string());
    }
    if let Some(structured) = req.args.get("structured").cloned().filter(|value| !value.is_null()) {
        builder = builder.structured(structured);
    }

    match identity.mailbox_manager.post(builder) {
        Ok(event) => serialize_response(event, "event"),
        Err(err) => Response::err(err.to_string()),
    }
}

pub(super) async fn handle_mailbox_peek(req: Request, identity: &DaemonIdentity) -> Response {
    let alias = match resolve_recipient_alias(identity, &req, request_arg_str(&req, "alias")) {
        Ok(alias) => alias,
        Err(err) => return Response::err(err),
    };
    let mut events = identity.mailbox_manager.list_for_recipient(
        &alias,
        request_arg_bool(&req, "unread").unwrap_or(false),
        alias_project_filter(request_arg_str(&req, "project_id"), request_arg_bool(&req, "global")),
    );
    if let Some(limit) = req.args.get("limit").and_then(|value| value.as_u64()) {
        events.truncate(limit as usize);
    }
    serialize_response(events, "events")
}

pub(super) async fn handle_mailbox_read(req: Request, identity: &DaemonIdentity) -> Response {
    let alias = match resolve_recipient_alias(identity, &req, request_arg_str(&req, "alias")) {
        Ok(alias) => alias,
        Err(err) => return Response::err(err),
    };
    let mut events = identity.mailbox_manager.list_for_recipient(
        &alias,
        true,
        alias_project_filter(request_arg_str(&req, "project_id"), request_arg_bool(&req, "global")),
    );
    if let Some(limit) = req.args.get("limit").and_then(|value| value.as_u64()) {
        events.truncate(limit as usize);
    }
    for event in &events {
        identity.mailbox_manager.mark_read(&event.id, &alias);
        if request_arg_bool(&req, "ack").unwrap_or(false) {
            identity.mailbox_manager.ack(&event.id, &alias, None);
        }
    }
    serialize_response(events, "events")
}

pub(super) async fn handle_mailbox_get(req: Request, identity: &DaemonIdentity) -> Response {
    let event_id = match request_arg_str(&req, "event_id") {
        Some(event_id) => event_id,
        None => return Response::err("event_id required"),
    };
    serialize_response(identity.mailbox_manager.get(event_id), "mailbox event")
}

pub(super) async fn handle_mailbox_read_state(req: Request, identity: &DaemonIdentity) -> Response {
    let event_id = match request_arg_str(&req, "event_id") {
        Some(event_id) => event_id,
        None => return Response::err("event_id required"),
    };
    let recipient = match request_arg_str(&req, "recipient") {
        Some(recipient) => recipient,
        None => return Response::err("recipient required"),
    };
    serialize_response(
        identity.mailbox_manager.read_state(event_id, recipient),
        "mailbox read state",
    )
}

pub(super) async fn handle_mailbox_mark_read(req: Request, identity: &DaemonIdentity) -> Response {
    let event_id = match request_arg_str(&req, "event_id") {
        Some(event_id) => event_id.to_string(),
        None => return Response::err("event_id required"),
    };
    let recipient = match request_arg_str(&req, "recipient") {
        Some(recipient) => recipient.to_string(),
        None => return Response::err("recipient required"),
    };
    let changed = identity.mailbox_manager.mark_read(&event_id, &recipient);
    Response::success(serde_json::json!({ "changed": changed }))
}

pub(super) async fn handle_mailbox_ack(req: Request, identity: &DaemonIdentity) -> Response {
    let event_id = match request_arg_str(&req, "event_id") {
        Some(event_id) => event_id.to_string(),
        None => return Response::err("event_id required"),
    };
    let alias = match resolve_recipient_alias(identity, &req, request_arg_str(&req, "alias")) {
        Ok(alias) => alias,
        Err(err) => return Response::err(err),
    };
    let changed = identity.mailbox_manager.ack(
        &event_id,
        &alias,
        request_arg_str(&req, "result").map(String::from),
    );
    Response::success(serde_json::json!({ "changed": changed }))
}

pub(super) async fn handle_mailbox_retract(req: Request, identity: &DaemonIdentity) -> Response {
    let event_id = match request_arg_str(&req, "event_id") {
        Some(event_id) => event_id.to_string(),
        None => return Response::err("event_id required"),
    };
    let alias = match resolve_recipient_alias(identity, &req, request_arg_str(&req, "alias")) {
        Ok(alias) => alias,
        Err(err) => return Response::err(err),
    };
    match identity.mailbox_manager.retract(&event_id, &alias) {
        Ok(event) => serialize_response(event, "event"),
        Err(err) => Response::err(err.to_string()),
    }
}

pub(super) async fn handle_mailbox_dismiss(req: Request, identity: &DaemonIdentity) -> Response {
    let event_id = match request_arg_str(&req, "event_id") {
        Some(event_id) => event_id.to_string(),
        None => return Response::err("event_id required"),
    };
    let alias = match resolve_recipient_alias(identity, &req, request_arg_str(&req, "alias")) {
        Ok(alias) => alias,
        Err(err) => return Response::err(err),
    };
    let changed = identity.mailbox_manager.dismiss(&event_id, &alias);
    Response::success(serde_json::json!({ "changed": changed }))
}

pub(super) async fn handle_mailbox_count(req: Request, identity: &DaemonIdentity) -> Response {
    let alias = match resolve_recipient_alias(identity, &req, request_arg_str(&req, "alias")) {
        Ok(alias) => alias,
        Err(err) => return Response::err(err),
    };
    let count = identity.mailbox_manager.unread_count(
        &alias,
        alias_project_filter(request_arg_str(&req, "project_id"), request_arg_bool(&req, "global")),
    );
    Response::success(serde_json::json!({ "unread": count }))
}

pub(super) async fn handle_mailbox_clear(req: Request, identity: &DaemonIdentity) -> Response {
    let alias = match resolve_recipient_alias(identity, &req, request_arg_str(&req, "alias")) {
        Ok(alias) => alias,
        Err(err) => return Response::err(err),
    };
    let cleared = identity.mailbox_manager.clear_read(
        &alias,
        alias_project_filter(request_arg_str(&req, "project_id"), request_arg_bool(&req, "global")),
    );
    Response::success(serde_json::json!({ "cleared": cleared }))
}

pub(super) async fn handle_mailbox_reply(req: Request, identity: &DaemonIdentity) -> Response {
    let event_id = match request_arg_str(&req, "event_id") {
        Some(event_id) => event_id.to_string(),
        None => return Response::err("event_id required"),
    };
    let body = match request_arg_str(&req, "body") {
        Some(body) => body.to_string(),
        None => return Response::err("body required"),
    };
    let original = match identity.mailbox_manager.get(&event_id) {
        Some(event) => event,
        None => return Response::err(format!("event_id not found: {event_id}")),
    };
    let recipient = match original.from.as_deref() {
        Some(sender) => sender.to_string(),
        None => {
            return Response::err("cannot reply: original event has no `from` (anonymous sender)")
        }
    };
    let canonical_to = roux_core::validate_alias_name(&recipient).ok().unwrap_or(recipient);
    let correlation_id = original.correlation_id.clone().unwrap_or_else(|| original.id.clone());
    let kind = match request_arg_str(&req, "kind") {
        Some(kind) => match parse_event_kind(kind) {
            Ok(kind) => kind,
            Err(err) => return Response::err(err),
        },
        None => EventKind::Result,
    };

    let mut builder =
        EventBuilder::new(body).to(canonical_to.clone()).kind(kind).correlation_id(correlation_id);
    if let Some(from) = request_arg_str(&req, "from")
        .map(String::from)
        .or_else(|| default_mailbox_from(identity, &req))
    {
        builder = builder.from(from);
    }
    if let Some(subject) = request_arg_str(&req, "subject") {
        builder = builder.subject(subject.to_string());
    }
    if let Some(project_id) = original.project_id {
        builder = builder.project_id(project_id);
    }
    if let Some(structured) = req.args.get("structured").cloned().filter(|value| !value.is_null()) {
        builder = builder.structured(structured);
    }

    identity.alias_manager.ensure(&canonical_to, builder.project_id.clone());
    match identity.mailbox_manager.post(builder) {
        Ok(event) => serialize_response(event, "event"),
        Err(err) => Response::err(err.to_string()),
    }
}

pub(super) async fn handle_mailbox_sent(req: Request, identity: &DaemonIdentity) -> Response {
    let sender = match request_arg_str(&req, "sender")
        .map(String::from)
        .or_else(|| default_mailbox_from(identity, &req))
    {
        Some(sender) => sender,
        None => return Response::err("sender required (call from a session, or pass args.sender)"),
    };
    let limit = req.args.get("limit").and_then(|value| value.as_u64()).map(|value| value as usize);
    let payload: Vec<Value> = identity
        .mailbox_manager
        .list_sent_by(&sender, request_arg_str(&req, "to"), limit)
        .into_iter()
        .map(|(event, state)| serde_json::json!({ "event": event, "state": state }))
        .collect();
    Response::success(Value::Array(payload))
}

pub(super) async fn handle_bus_publish(req: Request, identity: &DaemonIdentity) -> Response {
    let topic = match request_arg_str(&req, "topic") {
        Some(topic) => topic.to_string(),
        None => return Response::err("topic required"),
    };
    let body = request_arg_str(&req, "body").unwrap_or("").to_string();
    let structured = req.args.get("structured").cloned();
    if body.trim().is_empty() && structured.as_ref().map(|value| value.is_null()).unwrap_or(true) {
        return Response::err("body or structured payload required");
    }
    let kind = match request_arg_str(&req, "kind") {
        Some(kind) => match parse_event_kind(kind) {
            Ok(kind) => kind,
            Err(err) => return Response::err(err),
        },
        None => EventKind::Signal,
    };
    let mut builder = EventBuilder::new(body).topic(topic).kind(kind);
    if let Some(from) = request_arg_str(&req, "from")
        .map(String::from)
        .or_else(|| default_mailbox_from(identity, &req))
    {
        builder = builder.from(from);
    }
    if let Some(project_id) = request_arg_str(&req, "project_id") {
        builder = builder.project_id(project_id.to_string());
    }
    if let Some(subject) = request_arg_str(&req, "subject") {
        builder = builder.subject(subject.to_string());
    }
    if let Some(structured) = structured.filter(|value| !value.is_null()) {
        builder = builder.structured(structured);
    }
    match identity.mailbox_manager.post(builder) {
        Ok(event) => serialize_response(event, "event"),
        Err(err) => Response::err(err.to_string()),
    }
}

pub(super) async fn handle_bus_tail(req: Request, identity: &DaemonIdentity) -> Response {
    let limit = req.args.get("limit").and_then(|value| value.as_u64()).map(|value| value as usize);
    let filter =
        alias_project_filter(request_arg_str(&req, "project_id"), request_arg_bool(&req, "global"));
    let events = match request_arg_str(&req, "topic") {
        Some(topic) => {
            let mut events = identity.mailbox_manager.list_for_topic(topic, filter);
            if let Some(limit) = limit {
                events.truncate(limit);
            }
            events
        }
        None => identity.mailbox_manager.list_all(filter, limit),
    };
    serialize_response(events, "events")
}

fn resolve_subscriber_alias(identity: &DaemonIdentity, req: &Request) -> Result<String, String> {
    if let Some(alias) = request_arg_str(req, "alias") {
        return Ok(alias.to_string());
    }
    let pane_id = req.pane_id.as_deref().ok_or_else(|| {
        "no --alias given and no pane context available; pass --alias <name>".to_string()
    })?;
    let held = identity.alias_manager.find_for_pane(pane_id);
    match held.len() {
        0 => {
            Err("no --alias given and the calling pane holds no alias; pass --alias <name>"
                .to_string())
        }
        1 => Ok(held[0].alias.clone()),
        _ => {
            let names: Vec<_> = held.iter().map(|alias| alias.alias.as_str()).collect();
            Err(format!(
                "no --alias given and the calling pane holds multiple aliases ({names:?}); pass --alias <name>"
            ))
        }
    }
}

pub(super) async fn handle_bus_subscribe(req: Request, identity: &DaemonIdentity) -> Response {
    let pattern = match request_arg_str(&req, "pattern") {
        Some(pattern) if !pattern.trim().is_empty() => pattern.to_string(),
        _ => return Response::err("pattern required"),
    };
    let alias = match resolve_subscriber_alias(identity, &req) {
        Ok(alias) => alias,
        Err(err) => return Response::err(err),
    };
    match identity.subscription_manager.subscribe(
        &alias,
        &pattern,
        request_arg_str(&req, "project_id").map(String::from),
    ) {
        Ok(subscription) => serialize_response(subscription, "subscription"),
        Err(err) => Response::err(err.to_string()),
    }
}

pub(super) async fn handle_bus_unsubscribe(req: Request, identity: &DaemonIdentity) -> Response {
    let id = match request_arg_str(&req, "id") {
        Some(id) if !id.is_empty() => id.to_string(),
        _ => return Response::err("subscription id required"),
    };
    match identity.subscription_manager.unsubscribe(&id) {
        Ok(removed) => Response::success(serde_json::json!({ "removed": removed })),
        Err(err) => Response::err(err.to_string()),
    }
}

pub(super) async fn handle_bus_subscriptions(req: Request, identity: &DaemonIdentity) -> Response {
    let filter =
        alias_project_filter(request_arg_str(&req, "project_id"), request_arg_bool(&req, "global"));
    let subscriptions = match request_arg_str(&req, "alias") {
        Some(alias) => identity.subscription_manager.for_alias(alias, filter),
        None => identity.subscription_manager.list(filter),
    };
    serialize_response(subscriptions, "subscriptions")
}
