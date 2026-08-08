use fjarsyn_engine::{
    media::capture::{self, PlatformItem},
    peer_session::SessionId,
    screen_share,
};
use iced::Task;

use super::runtime_unavailable;
use crate::ui::{
    message::{self, Message},
    shell::Fjarsyn,
};

const ENCODER_RESTART_REQUIRED_MESSAGE: &str =
    "Screen sharing is unavailable until Fjarsyn restarts.";

pub(super) fn begin(app: &mut Fjarsyn, session_id: SessionId) -> Task<Message> {
    if app.state.screen_share.encoder_restart_required() {
        app.state.notify_error(ENCODER_RESTART_REQUIRED_MESSAGE);
        return Task::none();
    }
    let Some(raw_window_id) = app.state.ui.main_window.as_ref().and_then(|window| window.raw_id)
    else {
        app.state.notify_error("The application window is not ready for capture selection.");
        return Task::none();
    };
    let Some(screen_share) = screen_share_handle(app) else {
        return runtime_unavailable(app);
    };
    let picker = match capture::pick_platform_item(raw_window_id) {
        Ok(picker) => picker,
        Err(error) => {
            app.state.notify_error(format!("Failed to open capture picker: {error}"));
            return Task::none();
        }
    };

    Task::future(async move {
        match screen_share.begin_selection(session_id).await {
            Ok(selection) => {
                let result = picker.await.map_err(|error| error.to_string());
                Message::PeerAction(message::peer::Action::CaptureSourceSelected {
                    selection,
                    result,
                })
            }
            Err(error) => Message::PeerAction(message::peer::Action::ScreenShareCompleted(Err(
                error.to_string(),
            ))),
        }
    })
}

pub(super) fn capture_source_selected(
    app: &mut Fjarsyn,
    selection: screen_share::Selection,
    result: Result<Option<PlatformItem>, String>,
) -> Task<Message> {
    if app.state.screen_share.encoder_restart_required() {
        app.state.notify_error(ENCODER_RESTART_REQUIRED_MESSAGE);
        return cancel_selection(app, selection);
    }

    match result {
        Ok(Some(item)) => start(app, selection, item),
        Ok(None) => cancel_selection(app, selection),
        Err(error) => {
            app.state.notify_error(format!("Failed to select a capture source: {error}"));
            selection_failed(app, selection, error)
        }
    }
}

pub(super) fn stop(app: &mut Fjarsyn, session_id: SessionId) -> Task<Message> {
    let Some(screen_share) = screen_share_handle(app) else {
        return runtime_unavailable(app);
    };
    Task::future(async move {
        let result =
            screen_share.stop_screen_share(session_id).await.map_err(|error| error.to_string());
        Message::PeerAction(message::peer::Action::ScreenShareCompleted(result))
    })
}

pub(super) fn finish(app: &mut Fjarsyn, result: Result<(), String>) -> Task<Message> {
    if let Err(error) = result {
        app.state.notify_error(format!("Screen sharing failed: {error}"));
    }
    Task::none()
}

fn start(
    app: &mut Fjarsyn,
    selection: screen_share::Selection,
    item: PlatformItem,
) -> Task<Message> {
    if app.state.screen_share.encoder_restart_required() {
        app.state.notify_error(ENCODER_RESTART_REQUIRED_MESSAGE);
        return cancel_selection(app, selection);
    }
    let Some(screen_share) = screen_share_handle(app) else {
        return runtime_unavailable(app);
    };

    Task::future(async move {
        let result = screen_share
            .start_screen_share(selection, item)
            .await
            .map_err(|error| error.to_string());
        Message::PeerAction(message::peer::Action::ScreenShareCompleted(result))
    })
}

fn cancel_selection(app: &mut Fjarsyn, selection: screen_share::Selection) -> Task<Message> {
    let Some(screen_share) = screen_share_handle(app) else {
        return Task::none();
    };
    Task::future(async move {
        let _ = screen_share.cancel_selection(selection).await;
        Message::NoOp
    })
}

fn selection_failed(
    app: &mut Fjarsyn,
    selection: screen_share::Selection,
    error: String,
) -> Task<Message> {
    let Some(screen_share) = screen_share_handle(app) else {
        return Task::none();
    };
    Task::future(async move {
        let _ = screen_share.selection_failed(selection, error).await;
        Message::NoOp
    })
}

fn screen_share_handle(app: &Fjarsyn) -> Option<screen_share::ServiceHandle> {
    app.runtime.application.as_ref().map(|runtime| runtime.screen_share().clone())
}
