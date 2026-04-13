//! Non-consuming OSC parser for PTY output.
//!
//! Runs every byte that leaves a PTY through `vte::Parser`. Matches on OSC 9
//! (iTerm2 growl body), OSC 777 (`notify;title;body`), and OSC 99 (kitty
//! desktop notification protocol with chunking and id-based update). The
//! parser is non-consuming: bytes are forwarded to xterm unchanged, we just
//! observe them to extract notification intent.
//!
//! OSC 99 id (`i=`) values are namespaced per-PTY — each `OscSniffer` has its
//! own map, so two PTYs can both use `i=1` without collision.

use std::collections::HashMap;

use tauri::{AppHandle, Manager};
use vte::{Parser, Perform};

use roux_core::{
    ActionKind, NotificationAction, NotificationLevel, NotificationRequest, NotificationSource,
};

use crate::state::AppState;

/// Partially-accumulated OSC 99 notification, keyed by the sender's `i=` id.
#[derive(Default, Debug)]
struct Osc99Buffer {
    title: Option<String>,
    subtitle: Option<String>,
    body: Option<String>,
    /// Existing notification id in the store if this buffer has been flushed
    /// at least once — subsequent packets update in place.
    notification_id: Option<String>,
}

pub struct OscSniffer {
    parser: Parser,
    state: OscState,
}

struct OscState {
    app: AppHandle,
    session_id: Option<String>,
    osc99_buffers: HashMap<String, Osc99Buffer>,
}

impl OscSniffer {
    pub fn new(app: AppHandle, session_id: Option<String>) -> Self {
        Self {
            parser: Parser::new(),
            state: OscState { app, session_id, osc99_buffers: HashMap::new() },
        }
    }

    /// Feed a chunk of bytes through the parser. Does not modify the bytes.
    pub fn feed(&mut self, bytes: &[u8]) {
        let Self { parser, state } = self;
        parser.advance(state, bytes);
    }
}

impl Perform for OscState {
    fn osc_dispatch(&mut self, params: &[&[u8]], _bell_terminated: bool) {
        // First param is the OSC code as ASCII bytes. Everything after is the
        // semicolon-delimited payload.
        let Some(code_bytes) = params.first() else {
            return;
        };
        let Ok(code_str) = std::str::from_utf8(code_bytes) else {
            return;
        };
        let Ok(code) = code_str.parse::<u16>() else {
            return;
        };

        match code {
            9 => self.handle_osc_9(&params[1..]),
            777 => self.handle_osc_777(&params[1..]),
            99 => self.handle_osc_99(&params[1..]),
            _ => {}
        }
    }
}

impl OscState {
    fn handle_osc_9(&mut self, rest: &[&[u8]]) {
        // OSC 9 is iTerm2 growl-style: a single body string, no title.
        // Some senders stuff JSON into that field (e.g. {"message":"..."}),
        // so we attempt a tiny normalization pass before falling back.
        // `rest` is everything after the "9;" prefix; join with ';' to
        // preserve any literal semicolons in the user's message.
        let raw = join_params(rest);
        if raw.trim().is_empty() {
            return;
        }

        if let Some(parsed) = parse_osc9_payload(&raw) {
            self.push(
                NotificationSource::Osc { code: 9, sender_id: None },
                parsed.title,
                parsed.subtitle,
                parsed.body,
                None,
            );
            return;
        }

        self.push(NotificationSource::Osc { code: 9, sender_id: None }, raw, None, None, None);
    }

    fn handle_osc_777(&mut self, rest: &[&[u8]]) {
        // OSC 777 is `notify;<title>;<body>`.
        if rest.is_empty() {
            return;
        }
        let kind = std::str::from_utf8(rest[0]).unwrap_or("");
        if kind != "notify" {
            return;
        }
        let title = rest.get(1).and_then(|b| std::str::from_utf8(b).ok()).unwrap_or("").to_string();
        if title.is_empty() {
            return;
        }
        let body = rest.get(2).and_then(|b| std::str::from_utf8(b).ok()).map(String::from);
        self.push(NotificationSource::Osc { code: 777, sender_id: None }, title, None, body, None);
    }

    fn handle_osc_99(&mut self, rest: &[&[u8]]) {
        // OSC 99 (kitty): `<params>;<payload>` where params is a ':'-separated
        // list of k=v pairs.
        //   i=<id>  notification id (for dedup / update)
        //   e=<0|1> encoding: 0=utf8, 1=base64
        //   d=<0|1> done flag: 0=more chunks, 1=last
        //   p=<part> part: title | subtitle | body | icon | ? | ...
        if rest.is_empty() {
            return;
        }
        let Some(params_raw) = rest.first().and_then(|b| std::str::from_utf8(b).ok()) else {
            return;
        };
        let payload_raw = rest.get(1).and_then(|b| std::str::from_utf8(b).ok()).unwrap_or("");

        let mut id: Option<String> = None;
        let mut encoding = 0u8;
        let mut done = true;
        let mut part = "body".to_string();
        for kv in params_raw.split(':') {
            if let Some((k, v)) = kv.split_once('=') {
                match k {
                    "i" => id = Some(v.to_string()),
                    "e" => encoding = v.parse().unwrap_or(0),
                    "d" => done = v != "0",
                    "p" => part = v.to_string(),
                    _ => {}
                }
            }
        }

        let payload_decoded = if encoding == 1 {
            use base64::Engine;
            match base64::engine::general_purpose::STANDARD.decode(payload_raw) {
                Ok(bytes) => String::from_utf8(bytes).unwrap_or_default(),
                Err(_) => return,
            }
        } else {
            payload_raw.to_string()
        };

        let key = id.clone().unwrap_or_else(|| "__anon__".to_string());
        let buf = self.osc99_buffers.entry(key.clone()).or_default();

        match part.as_str() {
            "title" => buf.title = Some(payload_decoded),
            "subtitle" => buf.subtitle = Some(payload_decoded),
            "body" => buf.body = Some(payload_decoded),
            // icon and other parts are ignored for v1
            _ => {}
        }

        if !done {
            return;
        }

        // Commit: if title absent, use body as title; if both absent, skip.
        let title = buf.title.clone().or_else(|| buf.body.clone()).unwrap_or_default();
        if title.is_empty() {
            // Nothing useful — reset the buffer slot and bail.
            self.osc99_buffers.remove(&key);
            return;
        }
        let subtitle = buf.subtitle.clone();
        let body = if buf.title.is_some() { buf.body.clone() } else { None };
        let existing_id = buf.notification_id.clone();

        // If we've already pushed an earlier version of this notification,
        // remove it so the update appears fresh (Phase 2 pane doesn't yet
        // support in-place replacement via Updated, but Removed + Added is
        // semantically correct and already wired).
        if let Some(ref old_id) = existing_id {
            let state = self.app.state::<AppState>();
            state.notification_manager.remove(old_id, Some(&self.app));
        }

        let pushed = self.push_returning(
            NotificationSource::Osc { code: 99, sender_id: id.clone() },
            title,
            subtitle,
            body,
            id.clone(),
        );

        // Re-fetch the entry (we held a &mut borrow that dropped at self.push).
        if let Some(buf) = self.osc99_buffers.get_mut(&key) {
            buf.notification_id = Some(pushed);
            // Clear field accumulators so the next OSC 99 with the same id
            // starts fresh (the sender always includes the fields it wants).
            buf.title = None;
            buf.subtitle = None;
            buf.body = None;
        }
    }

    fn push(
        &self,
        source: NotificationSource,
        title: String,
        subtitle: Option<String>,
        body: Option<String>,
        _sender_id: Option<String>,
    ) {
        let _ = self.push_returning(source, title, subtitle, body, _sender_id);
    }

    fn push_returning(
        &self,
        source: NotificationSource,
        title: String,
        subtitle: Option<String>,
        body: Option<String>,
        _sender_id: Option<String>,
    ) -> String {
        let state = self.app.state::<AppState>();
        let mut actions = Vec::new();
        if let Some(ref sid) = self.session_id {
            actions.push(NotificationAction {
                id: "focus".into(),
                label: "Focus session".into(),
                kind: ActionKind::FocusSession { session_id: sid.clone() },
                primary: true,
            });
        }
        actions.push(NotificationAction {
            id: "dismiss".into(),
            label: "Dismiss".into(),
            kind: ActionKind::Dismiss,
            primary: actions.is_empty(),
        });

        let notification = state.notification_manager.push(
            NotificationRequest {
                level: NotificationLevel::Info,
                source,
                title,
                subtitle,
                body,
                session_id: self.session_id.clone(),
                actions,
            },
            Some(&self.app),
        );
        notification.id
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedOsc9Payload {
    title: String,
    subtitle: Option<String>,
    body: Option<String>,
}

fn parse_osc9_payload(raw: &str) -> Option<ParsedOsc9Payload> {
    use serde_json::{Map, Value};

    fn first_string(obj: &Map<String, Value>, keys: &[&str]) -> Option<String> {
        keys.iter().find_map(|k| {
            obj.get(*k)
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
        })
    }

    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    match serde_json::from_str::<Value>(trimmed).ok()? {
        Value::String(s) => {
            let s = s.trim();
            if s.is_empty() {
                return None;
            }
            Some(ParsedOsc9Payload { title: s.to_string(), subtitle: None, body: None })
        }
        Value::Object(obj) => {
            let subtitle = first_string(&obj, &["subtitle", "subTitle"]);
            let title = first_string(&obj, &["title", "summary", "subject"]);
            let message = first_string(&obj, &["body", "message", "text", "detail", "details"]);

            let Some(final_title) = title.clone().or_else(|| message.clone()) else {
                return None;
            };
            let body = match (title.is_some(), message) {
                (true, Some(m)) if m != final_title => Some(m),
                _ => None,
            };

            Some(ParsedOsc9Payload { title: final_title, subtitle, body })
        }
        _ => None,
    }
}

fn join_params(parts: &[&[u8]]) -> String {
    let strs: Vec<String> = parts.iter().map(|p| String::from_utf8_lossy(p).into_owned()).collect();
    strs.join(";")
}

#[cfg(test)]
mod tests {
    use super::*;
    use vte::Perform;

    /// A recording `Perform` that captures all OSC dispatches, so tests can
    /// verify the parser correctly segments sequences without spinning up a
    /// full Tauri app.
    #[derive(Default)]
    struct Recorder {
        osc: Vec<Vec<Vec<u8>>>,
    }

    impl Perform for Recorder {
        fn osc_dispatch(&mut self, params: &[&[u8]], _bell: bool) {
            self.osc.push(params.iter().map(|p| p.to_vec()).collect());
        }
    }

    fn parse(bytes: &[u8]) -> Vec<Vec<Vec<u8>>> {
        let mut parser = Parser::new();
        let mut rec = Recorder::default();
        parser.advance(&mut rec, bytes);
        rec.osc
    }

    #[test]
    fn osc_9_body_only_bel() {
        let seq = b"\x1b]9;hello\x07";
        let captured = parse(seq);
        assert_eq!(captured.len(), 1);
        assert_eq!(captured[0][0], b"9");
        assert_eq!(captured[0][1], b"hello");
    }

    #[test]
    fn osc_777_notify_bel() {
        let seq = b"\x1b]777;notify;my title;my body\x07";
        let captured = parse(seq);
        assert_eq!(captured.len(), 1);
        assert_eq!(captured[0][0], b"777");
        assert_eq!(captured[0][1], b"notify");
        assert_eq!(captured[0][2], b"my title");
        assert_eq!(captured[0][3], b"my body");
    }

    #[test]
    fn osc_777_st_terminator() {
        let seq = b"\x1b]777;notify;hello;body\x1b\\";
        let captured = parse(seq);
        assert_eq!(captured.len(), 1);
        assert_eq!(captured[0][2], b"hello");
    }

    #[test]
    fn osc_99_single_packet() {
        let seq = b"\x1b]99;i=1:d=1:p=title;Build done\x1b\\";
        let captured = parse(seq);
        assert_eq!(captured.len(), 1);
        assert_eq!(captured[0][0], b"99");
        assert_eq!(captured[0][1], b"i=1:d=1:p=title");
        assert_eq!(captured[0][2], b"Build done");
    }

    #[test]
    fn osc_9_json_payload_extracts_message() {
        let payload = r#"{"message":"Build failed","subtitle":"repo-a"}"#;
        let parsed = parse_osc9_payload(payload).expect("expected JSON payload to parse");
        assert_eq!(parsed.title, "Build failed");
        assert_eq!(parsed.subtitle.as_deref(), Some("repo-a"));
        assert_eq!(parsed.body, None);
    }

    #[test]
    fn osc_parser_splits_across_buffer_boundaries() {
        // Byte-at-a-time feeding must still produce exactly one dispatch.
        let seq = b"\x1b]777;notify;hello;body\x07";
        let mut parser = Parser::new();
        let mut rec = Recorder::default();
        for b in seq {
            parser.advance(&mut rec, std::slice::from_ref(b));
        }
        assert_eq!(rec.osc.len(), 1);
    }
}
