use tokio::io::AsyncWriteExt;

use roux_runtime::host::RuntimeHost;
use roux_runtime::pty_service::{PtyOutputEvent, PTY_OUTPUT_DEFAULT_POLL_BYTES};
use roux_runtime::watch_runner::WatchRunner;

use super::identity::{request_authorized, DaemonIdentity};
use super::protocol::{
    AliasEventFrame, MailboxEventFrame, PtyAttachFrame, Request, SubscriptionEventFrame,
    WatchEventFrame,
};

pub(super) async fn handle_daemon_pty_attach_stream<W>(
    req: Request,
    writer: &mut W,
    host: &RuntimeHost,
    identity: &DaemonIdentity,
) -> bool
where
    W: tokio::io::AsyncWrite + Unpin,
{
    let result = handle_daemon_pty_attach_stream_inner(req, writer, host, identity).await;
    let _ = writer.shutdown().await;
    result
}

async fn handle_daemon_pty_attach_stream_inner<W>(
    req: Request,
    writer: &mut W,
    host: &RuntimeHost,
    identity: &DaemonIdentity,
) -> bool
where
    W: tokio::io::AsyncWrite + Unpin,
{
    if !request_authorized(&req, identity) {
        let _ = write_attach_frame(writer, &PtyAttachFrame::Error { error: "unauthorized".into() })
            .await;
        return false;
    }
    let Some(id) = req.args.get("id").and_then(|id| id.as_str()) else {
        let _ = write_attach_frame(writer, &PtyAttachFrame::Error { error: "id required".into() })
            .await;
        return false;
    };
    let max_replay_bytes = req
        .args
        .get("maxBytes")
        .or_else(|| req.args.get("max_bytes"))
        .and_then(|max_bytes| max_bytes.as_u64())
        .map(|max_bytes| max_bytes as usize)
        .unwrap_or(PTY_OUTPUT_DEFAULT_POLL_BYTES);

    let mut attach = match host.pty_handle.attach(id, max_replay_bytes).await {
        Ok(Some(attach)) => attach,
        Ok(None) => {
            let _ = write_attach_frame(
                writer,
                &PtyAttachFrame::Error { error: "daemon pty not found".into() },
            )
            .await;
            return false;
        }
        Err(err) => {
            let _ =
                write_attach_frame(writer, &PtyAttachFrame::Error { error: err.to_string() }).await;
            return false;
        }
    };

    let record = attach.record.clone();
    let replay_bytes = std::mem::take(&mut attach.replay_bytes);
    if !write_attach_frame(
        writer,
        &PtyAttachFrame::Ready {
            id: record.id.clone(),
            record: Box::new(record.clone()),
            replay_offset: attach.replay_offset,
            replay_bytes,
        },
    )
    .await
    {
        return false;
    }

    if !record.running {
        let _ = write_attach_frame(
            writer,
            &PtyAttachFrame::Exit { code: record.exit_code, generation: record.generation },
        )
        .await;
        return true;
    }

    loop {
        match attach.events.recv().await {
            Ok(PtyOutputEvent::Output(frame)) => {
                if !write_attach_frame(
                    writer,
                    &PtyAttachFrame::Output { offset: frame.offset, bytes: frame.bytes },
                )
                .await
                {
                    return false;
                }
            }
            Ok(PtyOutputEvent::Exit { code, generation }) => {
                let _ =
                    write_attach_frame(writer, &PtyAttachFrame::Exit { code, generation }).await;
                return true;
            }
            Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                let _ = write_attach_frame(
                    writer,
                    &PtyAttachFrame::Error {
                        error: format!("daemon pty output stream lagged by {skipped} frame(s)"),
                    },
                )
                .await;
                return false;
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => return true,
        }
    }
}

async fn write_attach_frame<W>(writer: &mut W, frame: &PtyAttachFrame) -> bool
where
    W: tokio::io::AsyncWrite + Unpin,
{
    let Ok(json) = serde_json::to_string(frame) else {
        return false;
    };
    writer.write_all(json.as_bytes()).await.is_ok() && writer.write_all(b"\n").await.is_ok()
}

pub(super) async fn handle_watch_events_stream<W>(
    req: Request,
    writer: &mut W,
    host: &RuntimeHost,
    watch_runner: &WatchRunner,
    identity: &DaemonIdentity,
) -> bool
where
    W: tokio::io::AsyncWrite + Unpin,
{
    let result = handle_watch_events_stream_inner(req, writer, host, watch_runner, identity).await;
    let _ = writer.shutdown().await;
    result
}

async fn handle_watch_events_stream_inner<W>(
    req: Request,
    writer: &mut W,
    host: &RuntimeHost,
    watch_runner: &WatchRunner,
    identity: &DaemonIdentity,
) -> bool
where
    W: tokio::io::AsyncWrite + Unpin,
{
    if !request_authorized(&req, identity) {
        let _ = write_watch_event_frame(
            writer,
            &WatchEventFrame::Error { error: "unauthorized".into() },
        )
        .await;
        return false;
    }

    let send_backlog = req.args.get("backlog").and_then(|value| value.as_bool()).unwrap_or(true);
    let mut rx = watch_runner.subscribe();

    if !write_watch_event_frame(writer, &WatchEventFrame::Ready).await {
        return false;
    }

    if send_backlog {
        let watches = match host.watch_handle.list().await {
            Ok(watches) => watches,
            Err(err) => {
                let _ = write_watch_event_frame(
                    writer,
                    &WatchEventFrame::Error { error: err.to_string() },
                )
                .await;
                return false;
            }
        };
        for watch in watches {
            let event =
                roux_core::WatchUpdateEvent { watch, changed: false, previous_outcome: None };
            if !write_watch_event_frame(writer, &WatchEventFrame::Update { event: Box::new(event) })
                .await
            {
                return false;
            }
        }
    }

    loop {
        match rx.recv().await {
            Ok(event) => {
                if !write_watch_event_frame(
                    writer,
                    &WatchEventFrame::Update { event: Box::new(event) },
                )
                .await
                {
                    return false;
                }
            }
            Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                let warning = WatchEventFrame::Warning {
                    message: format!("dropped {skipped} buffered watch event(s)"),
                };
                if !write_watch_event_frame(writer, &warning).await {
                    return false;
                }
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => return true,
        }
    }
}

async fn write_watch_event_frame<W>(writer: &mut W, frame: &WatchEventFrame) -> bool
where
    W: tokio::io::AsyncWrite + Unpin,
{
    let Ok(json) = serde_json::to_string(frame) else {
        return false;
    };
    writer.write_all(json.as_bytes()).await.is_ok() && writer.write_all(b"\n").await.is_ok()
}

pub(super) async fn handle_alias_events_stream<W>(
    req: Request,
    writer: &mut W,
    identity: &DaemonIdentity,
) -> bool
where
    W: tokio::io::AsyncWrite + Unpin,
{
    let result = handle_alias_events_stream_inner(req, writer, identity).await;
    let _ = writer.shutdown().await;
    result
}

async fn handle_alias_events_stream_inner<W>(
    req: Request,
    writer: &mut W,
    identity: &DaemonIdentity,
) -> bool
where
    W: tokio::io::AsyncWrite + Unpin,
{
    if !request_authorized(&req, identity) {
        let _ = write_alias_event_frame(
            writer,
            &AliasEventFrame::Error { error: "unauthorized".into() },
        )
        .await;
        return false;
    }

    let mut rx = identity.alias_manager.subscribe_events();
    if !write_alias_event_frame(writer, &AliasEventFrame::Ready).await {
        return false;
    }

    loop {
        match rx.recv().await {
            Ok(event) => {
                if !write_alias_event_frame(writer, &AliasEventFrame::Event { event }).await {
                    return false;
                }
            }
            Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                let warning = AliasEventFrame::Warning {
                    message: format!("dropped {skipped} buffered alias event(s)"),
                };
                if !write_alias_event_frame(writer, &warning).await {
                    return false;
                }
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => return true,
        }
    }
}

async fn write_alias_event_frame<W>(writer: &mut W, frame: &AliasEventFrame) -> bool
where
    W: tokio::io::AsyncWrite + Unpin,
{
    let Ok(json) = serde_json::to_string(frame) else {
        return false;
    };
    writer.write_all(json.as_bytes()).await.is_ok() && writer.write_all(b"\n").await.is_ok()
}

pub(super) async fn handle_mailbox_events_stream<W>(
    req: Request,
    writer: &mut W,
    identity: &DaemonIdentity,
) -> bool
where
    W: tokio::io::AsyncWrite + Unpin,
{
    let result = handle_mailbox_events_stream_inner(req, writer, identity).await;
    let _ = writer.shutdown().await;
    result
}

async fn handle_mailbox_events_stream_inner<W>(
    req: Request,
    writer: &mut W,
    identity: &DaemonIdentity,
) -> bool
where
    W: tokio::io::AsyncWrite + Unpin,
{
    if !request_authorized(&req, identity) {
        let _ = write_mailbox_event_frame(
            writer,
            &MailboxEventFrame::Error { error: "unauthorized".into() },
        )
        .await;
        return false;
    }

    let mut rx = identity.mailbox_manager.subscribe_events();
    if !write_mailbox_event_frame(writer, &MailboxEventFrame::Ready).await {
        return false;
    }

    loop {
        match rx.recv().await {
            Ok(event) => {
                if !write_mailbox_event_frame(
                    writer,
                    &MailboxEventFrame::Event { event: Box::new(event) },
                )
                .await
                {
                    return false;
                }
            }
            Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                let warning = MailboxEventFrame::Warning {
                    message: format!("dropped {skipped} buffered mailbox event(s)"),
                };
                if !write_mailbox_event_frame(writer, &warning).await {
                    return false;
                }
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => return true,
        }
    }
}

async fn write_mailbox_event_frame<W>(writer: &mut W, frame: &MailboxEventFrame) -> bool
where
    W: tokio::io::AsyncWrite + Unpin,
{
    let Ok(json) = serde_json::to_string(frame) else {
        return false;
    };
    writer.write_all(json.as_bytes()).await.is_ok() && writer.write_all(b"\n").await.is_ok()
}

pub(super) async fn handle_subscription_events_stream<W>(
    req: Request,
    writer: &mut W,
    identity: &DaemonIdentity,
) -> bool
where
    W: tokio::io::AsyncWrite + Unpin,
{
    let result = handle_subscription_events_stream_inner(req, writer, identity).await;
    let _ = writer.shutdown().await;
    result
}

async fn handle_subscription_events_stream_inner<W>(
    req: Request,
    writer: &mut W,
    identity: &DaemonIdentity,
) -> bool
where
    W: tokio::io::AsyncWrite + Unpin,
{
    if !request_authorized(&req, identity) {
        let _ = write_subscription_event_frame(
            writer,
            &SubscriptionEventFrame::Error { error: "unauthorized".into() },
        )
        .await;
        return false;
    }

    let mut rx = identity.subscription_manager.subscribe_events();
    if !write_subscription_event_frame(writer, &SubscriptionEventFrame::Ready).await {
        return false;
    }

    loop {
        match rx.recv().await {
            Ok(event) => {
                if !write_subscription_event_frame(writer, &SubscriptionEventFrame::Event { event })
                    .await
                {
                    return false;
                }
            }
            Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                let warning = SubscriptionEventFrame::Warning {
                    message: format!("dropped {skipped} buffered subscription event(s)"),
                };
                if !write_subscription_event_frame(writer, &warning).await {
                    return false;
                }
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => return true,
        }
    }
}

async fn write_subscription_event_frame<W>(writer: &mut W, frame: &SubscriptionEventFrame) -> bool
where
    W: tokio::io::AsyncWrite + Unpin,
{
    let Ok(json) = serde_json::to_string(frame) else {
        return false;
    };
    writer.write_all(json.as_bytes()).await.is_ok() && writer.write_all(b"\n").await.is_ok()
}
