use iced::Task;

use super::{
    SettingsScreen,
    workflow::{self, SettingsEffect},
};
use crate::ui::{
    message::{ConfigMessage, Message, ScreenMessage},
    shell::AppContextMut,
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

        let effects = workflow::execute_settings_message(self, ctx.as_ref(), message);
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
            SettingsEffect::SaveConfig(config) => {
                self.working_config = config.clone();
                Task::done(Message::Config(ConfigMessage::SaveRequested(config)))
            }
        }
    }
}
