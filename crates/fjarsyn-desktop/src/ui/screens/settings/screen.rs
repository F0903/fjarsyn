use std::sync::Arc;

use iced::Task;

use super::{SettingsDraft, Tab, workflow};
use crate::{
    settings::Settings,
    ui::{
        message::{self, Message},
        presentation::Context,
    },
};

#[derive(Debug, Clone)]
pub(in crate::ui::screens) struct Screen {
    pub(super) draft: SettingsDraft,
    pub(super) active_tab: Arc<dyn Tab>,
}

impl Screen {
    pub(in crate::ui::screens) fn new(settings: &Settings) -> Self {
        Self { draft: SettingsDraft::new(settings), active_tab: super::tabs::default_tab() }
    }

    pub(in crate::ui::screens) fn update(
        &mut self,
        context: Context<'_>,
        message: message::screen::settings::Message,
    ) -> Task<Message> {
        let effects = workflow::execute_settings_message(self, context.settings(), message);
        Task::batch(effects.into_iter().map(|effect| self.run_effect(effect)))
    }

    fn run_effect(&mut self, effect: workflow::Effect) -> Task<Message> {
        match effect {
            workflow::Effect::SaveSettings(settings) => {
                Task::done(Message::Settings(message::Settings::SaveRequested(settings)))
            }
            workflow::Effect::SaveAndRetryStartup(settings) => {
                Task::done(Message::Settings(message::Settings::SaveAndRetryRequested(settings)))
            }
        }
    }
}
