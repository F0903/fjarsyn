use std::{collections::BTreeMap, sync::Arc};

use fjarsyn_core::{
    peer_session::{PeerSessionEvent, SessionCloseReason},
    services::messaging_service::{MessagingEvent, MessagingSnapshot},
};
use iced::Task;

use crate::ui::{
    message::{Message, RuntimeMessage},
    runtime::RuntimeEvent,
    shell::{AppLifecycle, Fjarsyn, MessagingState},
};

pub fn handle_runtime_msg(app: &mut Fjarsyn, message: RuntimeMessage) -> Task<Message> {
    match message {
        RuntimeMessage::Initialized(result) => {
            match result {
                Ok(slot) => {
                    let Some(runtime) = slot.take() else {
                        return Task::none();
                    };
                    app.ctx.config = runtime.active_config.clone();
                    app.ctx.local_peer_id = Some(runtime.local_peer_id.clone());
                    app.ctx.local_public_key = Some(runtime.local_public_key.clone());
                    let contacts = runtime.handles.contacts.projection();
                    app.ctx.contacts = contacts.contacts;
                    app.ctx.contacts_source_id = contacts.source_id;
                    app.ctx.contacts_revision = contacts.revision;
                    app.ctx.presence = runtime.handles.presence.snapshot();
                    app.ctx.sessions = runtime.handles.sessions.snapshot();
                    app.ctx.messaging = messaging_state(runtime.handles.messaging.snapshot(), 0);
                    app.runtime.application = Some(runtime);
                    app.ctx.lifecycle = AppLifecycle::Ready;
                }
                Err(error) => {
                    app.ctx.lifecycle = AppLifecycle::Failed(error.to_string());
                }
            }
            Task::none()
        }
        RuntimeMessage::Event(event) => {
            apply_runtime_event(app, event);
            Task::none()
        }
        RuntimeMessage::ShutdownFinished(result) => {
            if let Err(error) = result {
                tracing::warn!("application shutdown completed with errors: {error}");
            }
            iced::exit()
        }
        RuntimeMessage::RestartFinished { shutdown_warning, launch_result } => {
            let effect = apply_restart_finished(&mut app.ctx.lifecycle, &launch_result);
            if effect == RestartFinishedEffect::Ignored {
                return Task::none();
            }
            if let Some(error) = shutdown_warning {
                tracing::warn!(
                    "application shutdown completed with errors before restart: {error}"
                );
            }
            match effect {
                RestartFinishedEffect::Exit => iced::exit(),
                RestartFinishedEffect::RetryableFailure | RestartFinishedEffect::Ignored => {
                    Task::none()
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RestartFinishedEffect {
    Ignored,
    Exit,
    RetryableFailure,
}

fn apply_restart_finished(
    lifecycle: &mut AppLifecycle,
    launch_result: &Result<(), Arc<String>>,
) -> RestartFinishedEffect {
    if !matches!(lifecycle, AppLifecycle::Restarting) {
        return RestartFinishedEffect::Ignored;
    }
    match launch_result {
        Ok(()) => RestartFinishedEffect::Exit,
        Err(error) => {
            *lifecycle = AppLifecycle::RestartFailed(error.to_string());
            RestartFinishedEffect::RetryableFailure
        }
    }
}

fn apply_runtime_event(app: &mut Fjarsyn, event: RuntimeEvent) {
    match event {
        RuntimeEvent::Presence(snapshot) => app.ctx.presence = snapshot,
        RuntimeEvent::Sessions(snapshot) => {
            app.ctx.media.reconcile_shares(&snapshot);
            app.ctx.sessions = snapshot;
        }
        RuntimeEvent::SessionEvent(event) => match event {
            PeerSessionEvent::IncomingRequest { peer_id, .. } => {
                app.ctx.notify_info(format!(
                    "Connection request from {}.",
                    app.ctx.display_name(&peer_id)
                ));
            }
            PeerSessionEvent::Connected { peer_id, .. } => {
                app.ctx.notify_success(format!("Connected to {}.", app.ctx.display_name(&peer_id)));
            }
            PeerSessionEvent::Closed { peer_id, reason, .. }
                if !matches!(reason, SessionCloseReason::LocalDisconnect) =>
            {
                app.ctx.notify_info(format!(
                    "Session with {} closed: {}",
                    app.ctx.display_name(&peer_id),
                    close_reason(&reason),
                ));
            }
            _ => {}
        },
        RuntimeEvent::Messaging { revision, summaries, conversations } => {
            app.ctx.messaging = MessagingState { summaries, conversations, revision };
        }
        RuntimeEvent::MessagingEvent(event) => {
            if let MessagingEvent::IncomingMessage { peer_id, .. } = event {
                app.ctx
                    .notify_info(format!("New message from {}.", app.ctx.display_name(&peer_id)));
            }
        }
        RuntimeEvent::Media(event) => {
            let failure = match &event {
                crate::ui::runtime::MediaEvent::LocalState {
                    state: crate::ui::runtime::LocalMediaState::Failed(reason),
                    ..
                }
                | crate::ui::runtime::MediaEvent::RemoteState {
                    state: crate::ui::runtime::RemoteMediaState::Failed(reason),
                    ..
                } => Some(reason.clone()),
                _ => None,
            };
            app.ctx.media.apply(event);
            app.ctx.media.reconcile_shares(&app.ctx.sessions);
            if let Some(reason) = failure {
                app.ctx.notify_error(reason);
            }
        }
    }
}

fn messaging_state(snapshot: MessagingSnapshot, revision: u64) -> MessagingState {
    let conversations = snapshot
        .conversations
        .iter()
        .map(|(peer_id, messages)| (peer_id.clone(), messages.clone()))
        .collect::<BTreeMap<_, _>>();
    MessagingState {
        summaries: snapshot.summaries,
        conversations: Arc::new(conversations),
        revision,
    }
}

fn close_reason(reason: &SessionCloseReason) -> String {
    match reason {
        SessionCloseReason::LocalDisconnect => "disconnected".into(),
        SessionCloseReason::RemoteDisconnect => "the contact disconnected".into(),
        SessionCloseReason::Rejected { reason } => format!("rejected ({reason})"),
        SessionCloseReason::Cancelled => "cancelled".into(),
        SessionCloseReason::SignalingLost => "signaling was lost".into(),
        SessionCloseReason::ConnectionFailed { reason } => reason.clone(),
        SessionCloseReason::ProtocolViolation { reason } => {
            format!("protocol violation ({reason})")
        }
        SessionCloseReason::TrustRevoked => "the contact's trusted identity changed".into(),
        SessionCloseReason::ServiceShutdown => "Fjarsyn is shutting down".into(),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::{AppLifecycle, RestartFinishedEffect, apply_restart_finished};

    #[test]
    fn stale_restart_completion_is_ignored() {
        let mut lifecycle = AppLifecycle::Ready;

        let effect = apply_restart_finished(&mut lifecycle, &Err(Arc::new("stale failure".into())));

        assert_eq!(effect, RestartFinishedEffect::Ignored);
        assert_eq!(lifecycle, AppLifecycle::Ready);
    }

    #[test]
    fn successful_replacement_requests_exit_only_after_launch_completion() {
        let mut lifecycle = AppLifecycle::Restarting;

        let effect = apply_restart_finished(&mut lifecycle, &Ok(()));

        assert_eq!(effect, RestartFinishedEffect::Exit);
        assert_eq!(lifecycle, AppLifecycle::Restarting);
    }

    #[test]
    fn launch_failure_enters_an_inert_retryable_state() {
        let mut lifecycle = AppLifecycle::Restarting;

        let effect = apply_restart_finished(&mut lifecycle, &Err(Arc::new("launch failed".into())));

        assert_eq!(effect, RestartFinishedEffect::RetryableFailure);
        assert_eq!(lifecycle, AppLifecycle::RestartFailed("launch failed".into()));
    }
}
