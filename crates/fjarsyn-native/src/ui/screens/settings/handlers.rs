use iced::Task;

use super::{
    SettingsScreen,
    workflow::{self, SettingsEffect},
};
use crate::ui::{
    app::AppContextMut,
    message::{Message, ScreenMessage},
};

impl SettingsScreen {
    pub(crate) fn handle_message(
        &mut self,
        ctx: &mut AppContextMut<'_>,
        message: Message,
    ) -> Task<Message> {
        let message = match message {
            Message::Screen(ScreenMessage::Settings(message)) => message,
            _ => return Task::none(),
        };

        let effects = workflow::reduce(self, ctx.as_ref(), message);
        self.run_effects(ctx, effects)
    }

    fn run_effects(
        &mut self,
        ctx: &mut AppContextMut<'_>,
        effects: Vec<SettingsEffect>,
    ) -> Task<Message> {
        Task::batch(effects.into_iter().map(|effect| self.run_effect(ctx, effect)))
    }

    fn run_effect(&mut self, ctx: &mut AppContextMut<'_>, effect: SettingsEffect) -> Task<Message> {
        match effect {
            SettingsEffect::NotifyError(message) => {
                ctx.notify_error(message);
                Task::none()
            }
            SettingsEffect::PersistConfig(config) => {
                ctx.config = config;
                if let Err(err) = ctx.config.save() {
                    ctx.notify_error(format!("Unable to save settings: {}", err));
                }
                Task::none()
            }
            SettingsEffect::ApplyCaptureReadback { enabled } => {
                let Some(capture) = ctx.media.capture.clone() else {
                    return Task::none();
                };

                Task::future(async move {
                    capture.write().await.set_cpu_readback_enabled(enabled);
                    Message::NoOp
                })
            }
        }
    }
}
