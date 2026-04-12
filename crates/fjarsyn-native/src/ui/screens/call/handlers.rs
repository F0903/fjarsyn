use fjarsyn_core::services::call_service::CallEvent;
use iced::Task;

use super::{
    CallMessage, CallScreen,
    workflow::{self, CallEffect},
};
use crate::ui::{
    app::AppContextMut,
    message::{CallServiceMessage, Message, ScreenMessage},
};

impl CallScreen {
    pub(crate) fn handle_update(
        &mut self,
        ctx: &mut AppContextMut<'_>,
        message: Message,
    ) -> Task<Message> {
        match message {
            Message::Screen(ScreenMessage::Call(msg)) => {
                let effects = workflow::reduce(self, ctx, msg);
                self.run_effects(ctx, effects)
            }

            // End the call if the peer disconnects
            Message::CallService(CallServiceMessage::CallEvent(CallEvent::CallEnded)) => {
                Task::done(Message::Screen(ScreenMessage::Call(CallMessage::EndCall)))
            }
            Message::CallService(CallServiceMessage::CallEvent(CallEvent::RemoteStreamStarted)) => {
                Task::done(Message::Screen(ScreenMessage::Call(CallMessage::RemoteStreamStarted)))
            }
            Message::CallService(CallServiceMessage::CallEvent(CallEvent::RemoteStreamEnded)) => {
                Task::done(Message::Screen(ScreenMessage::Call(CallMessage::RemoteStreamEnded)))
            }

            _ => Task::none(),
        }
    }

    fn run_effects(
        &mut self,
        ctx: &mut AppContextMut<'_>,
        effects: Vec<CallEffect>,
    ) -> Task<Message> {
        Task::batch(effects.into_iter().map(|effect| self.run_effect(ctx, effect)))
    }

    fn run_effect(&mut self, ctx: &mut AppContextMut<'_>, effect: CallEffect) -> Task<Message> {
        match effect {
            CallEffect::NotifyError(message) => {
                ctx.notify_error(message);
                Task::none()
            }
            CallEffect::NotifyInfo(message) => {
                ctx.notify_info(message);
                Task::none()
            }
            CallEffect::InitializeCapture => self.perform_initialize_capture(ctx.as_ref()),
            CallEffect::OpenCapturePicker { window_handle } => {
                self.perform_open_capture_picker(window_handle)
            }
            CallEffect::RunCaptureStart { capture_item } => {
                self.perform_capture_start(ctx, capture_item)
            }
            CallEffect::RunCaptureStop => self.perform_capture_stop(ctx),
            CallEffect::StartLocalCapturePipeline => {
                if let Err(err) = self.start_local_capture_pipeline(ctx) {
                    ctx.notify_error(err);
                }
                Task::none()
            }
            CallEffect::EndCall => self.perform_end_call(ctx.as_ref()),
        }
    }
}
