use fjarsyn_core::{
    peer_session::{LocalShareState, PeerSessionError, SessionId, ShareId},
    services::messaging_service::MessagingError,
};
use iced::Task;

use crate::ui::{
    message::{Message, MessageSendOutcome, PeerActionMessage},
    shell::Fjarsyn,
};

pub fn handle_peer_action(app: &mut Fjarsyn, message: PeerActionMessage) -> Task<Message> {
    match message {
        PeerActionMessage::Connect(peer_id) => {
            let Some(sessions) = sessions(app) else {
                return unavailable(app);
            };
            Task::future(async move {
                Message::PeerAction(PeerActionMessage::ConnectCompleted(
                    sessions.connect(peer_id).await.map_err(|error| error.to_string()),
                ))
            })
        }
        PeerActionMessage::ConnectCompleted(result) => {
            if let Err(error) = result {
                app.ctx.notify_error(format!("Connection failed: {error}"));
            }
            Task::none()
        }
        PeerActionMessage::Accept { session_id } => {
            session_command(app, move |sessions| async move {
                sessions.accept(session_id).await.map_err(|error| error.to_string())
            })
        }
        PeerActionMessage::Reject { session_id } => {
            session_command(app, move |sessions| async move {
                sessions
                    .reject(session_id, "rejected by user")
                    .await
                    .map_err(|error| error.to_string())
            })
        }
        PeerActionMessage::Disconnect { session_id } => {
            session_command(app, move |sessions| async move {
                sessions.disconnect(session_id).await.map_err(|error| error.to_string())
            })
        }
        PeerActionMessage::SessionCommandCompleted(result) => {
            if let Err(error) = result {
                app.ctx.notify_error(error);
            }
            Task::none()
        }
        PeerActionMessage::SendMessage { session_id, peer_id, body } => {
            let Some(messaging) =
                app.runtime.application.as_ref().map(|runtime| runtime.handles.messaging.clone())
            else {
                return unavailable(app);
            };
            Task::future(async move {
                let outcome = match messaging.send_message(session_id, peer_id, body).await {
                    Ok(_) => MessageSendOutcome::Sent,
                    Err(MessagingError::Session(PeerSessionError::OutcomeUnknown)) => {
                        MessageSendOutcome::DeliveryUncertain
                    }
                    Err(error) => MessageSendOutcome::Failed(error.to_string()),
                };
                Message::PeerAction(PeerActionMessage::MessageSent(outcome))
            })
        }
        PeerActionMessage::MessageSent(outcome) => {
            match outcome {
                MessageSendOutcome::Sent => {}
                MessageSendOutcome::DeliveryUncertain => app.ctx.notify_info(
                    "Delivery could not be confirmed; the message is marked uncertain.",
                ),
                MessageSendOutcome::Failed(error) => {
                    app.ctx.notify_error(format!("Message failed: {error}"));
                }
            }
            Task::none()
        }
        PeerActionMessage::BeginScreenShare { session_id } => begin_screen_share(app, session_id),
        PeerActionMessage::CaptureSourceSelected { session_id, result } => match result {
            Ok(Some(item)) => start_screen_share(app, session_id, item),
            Ok(None) => {
                if let Some(media) = media(app) {
                    Task::future(async move {
                        media.lock().await.cancel_local(session_id).await;
                        Message::NoOp
                    })
                } else {
                    Task::none()
                }
            }
            Err(error) => {
                app.ctx.notify_error(format!("Failed to select a capture source: {error}"));
                if let Some(media) = media(app) {
                    Task::future(async move {
                        media.lock().await.fail_local(session_id, error).await;
                        Message::NoOp
                    })
                } else {
                    Task::none()
                }
            }
        },
        PeerActionMessage::StopScreenShare { session_id } => stop_screen_share(app, session_id),
        PeerActionMessage::ScreenShareCompleted(result) => {
            if let Err(error) = result {
                app.ctx.notify_error(format!("Screen sharing failed: {error}"));
            }
            Task::none()
        }
    }
}

fn begin_screen_share(
    app: &mut Fjarsyn,
    session_id: fjarsyn_core::peer_session::SessionId,
) -> Task<Message> {
    let Some(raw_window_id) = app.ctx.ui.main_window.as_ref().and_then(|window| window.raw_id)
    else {
        app.ctx.notify_error("The application window is not ready for capture selection.");
        return Task::none();
    };
    let Some(media) = media(app) else {
        return unavailable(app);
    };
    let picker =
        match fjarsyn_core::capture_providers::user_pick_platform_capture_item(raw_window_id) {
            Ok(picker) => picker,
            Err(error) => {
                app.ctx.notify_error(format!("Failed to open capture picker: {error}"));
                return Task::none();
            }
        };

    Task::future(async move {
        media.lock().await.mark_selecting(session_id).await;
        let result = picker.await.map_err(|error| error.to_string());
        Message::PeerAction(PeerActionMessage::CaptureSourceSelected { session_id, result })
    })
}

fn start_screen_share(
    app: &mut Fjarsyn,
    session_id: fjarsyn_core::peer_session::SessionId,
    item: fjarsyn_core::capture_providers::PlatformCaptureItem,
) -> Task<Message> {
    let Some(sessions) = sessions(app) else {
        return unavailable(app);
    };
    let Some(media) = media(app) else {
        return unavailable(app);
    };
    let config = app.ctx.config.clone();

    Task::future(async move {
        media.lock().await.begin_local_start(session_id).await;
        let result = async {
            let share_id = resolve_started_share(&sessions, session_id)
                .await
                .map_err(|error| error.to_string())?;
            let sink = match sessions.encoded_video_sink(session_id).await {
                Ok(sink) => sink,
                Err(error) => {
                    let _ = sessions.stop_screen_share(session_id, share_id).await;
                    return Err(error.to_string());
                }
            };
            if let Err(error) =
                media.lock().await.start_local(session_id, share_id, item, sink, config).await
            {
                let _ = sessions.stop_screen_share(session_id, share_id).await;
                return Err(error);
            }
            Ok(())
        }
        .await;
        if let Err(error) = &result {
            media.lock().await.fail_local(session_id, error.clone()).await;
        }
        Message::PeerAction(PeerActionMessage::ScreenShareCompleted(result))
    })
}

fn stop_screen_share(
    app: &mut Fjarsyn,
    session_id: fjarsyn_core::peer_session::SessionId,
) -> Task<Message> {
    let Some(sessions) = sessions(app) else {
        return unavailable(app);
    };
    let Some(media) = media(app) else {
        return unavailable(app);
    };
    Task::future(async move {
        let binding = media.lock().await.request_local_stop(session_id).await;
        if let Some(binding) = binding
            && let Err(error) =
                sessions.stop_screen_share(binding.session_id, binding.share_id).await
        {
            // The application owns a durable stop intent. Snapshot
            // reconciliation retries this exact ShareId until it is inactive
            // or the session disappears, so an ambiguous/busy response is not
            // presented as a definitive user-facing failure.
            tracing::debug!(
                session_id = %binding.session_id,
                share_id = %binding.share_id,
                %error,
                "screen-share stop is pending reconciliation"
            );
        }
        Message::PeerAction(PeerActionMessage::ScreenShareCompleted(Ok(())))
    })
}

async fn resolve_started_share(
    sessions: &fjarsyn_core::peer_session::PeerSessionServiceHandle,
    session_id: SessionId,
) -> Result<ShareId, PeerSessionError> {
    let mut snapshots = sessions.subscribe();
    match sessions.start_screen_share(session_id).await {
        Ok(share_id) => Ok(share_id),
        Err(error @ (PeerSessionError::OutcomeUnknown | PeerSessionError::ResponseDropped)) => {
            if let Some(share_id) = active_local_share(&snapshots.borrow(), session_id) {
                return Ok(share_id);
            }
            let observed = tokio::time::timeout(std::time::Duration::from_secs(5), async {
                loop {
                    snapshots.changed().await.ok()?;
                    if let Some(share_id) = active_local_share(&snapshots.borrow(), session_id) {
                        return Some(share_id);
                    }
                    snapshots.borrow().session(session_id)?;
                }
            })
            .await
            .ok()
            .flatten();
            observed.ok_or(error)
        }
        Err(error) => Err(error),
    }
}

fn active_local_share(
    snapshot: &fjarsyn_core::peer_session::PeerSessionServiceSnapshot,
    session_id: SessionId,
) -> Option<ShareId> {
    snapshot.session(session_id).and_then(|session| match session.local_share {
        LocalShareState::Active { share_id } => Some(share_id),
        LocalShareState::Inactive => None,
    })
}

fn sessions(app: &Fjarsyn) -> Option<fjarsyn_core::peer_session::PeerSessionServiceHandle> {
    app.runtime.application.as_ref().map(|runtime| runtime.handles.sessions.clone())
}

fn media(
    app: &Fjarsyn,
) -> Option<std::sync::Arc<tokio::sync::Mutex<crate::ui::runtime::SessionMediaService>>> {
    app.runtime.application.as_ref().map(|runtime| runtime.handles.media.clone())
}

fn unavailable(app: &mut Fjarsyn) -> Task<Message> {
    app.ctx.notify_error("Peer sessions are unavailable while Fjarsyn is starting.");
    Task::none()
}

fn session_command<F, Fut>(app: &mut Fjarsyn, command: F) -> Task<Message>
where
    F: FnOnce(fjarsyn_core::peer_session::PeerSessionServiceHandle) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Result<(), String>> + Send + 'static,
{
    let Some(sessions) = sessions(app) else {
        return unavailable(app);
    };
    Task::future(async move {
        Message::PeerAction(PeerActionMessage::SessionCommandCompleted(command(sessions).await))
    })
}
