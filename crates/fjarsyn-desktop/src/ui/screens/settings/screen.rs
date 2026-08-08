use std::sync::Arc;

use fjarsyn_engine::config::Config;
use iced::Task;

use super::{Tab, workflow};
use crate::ui::{
    message::{self, Message},
    presentation::Context,
};

#[derive(Debug, Clone)]
pub(in crate::ui::screens) struct Screen {
    pub(super) working_config: Config,
    pub(super) active_tab: Arc<dyn Tab>,
}

impl Screen {
    pub(in crate::ui::screens) fn new(config: &Config) -> Self {
        Self { working_config: config.clone(), active_tab: super::tabs::default_tab() }
    }

    fn handle_message(&mut self, context: Context<'_>, message: Message) -> Task<Message> {
        let message = match message {
            Message::Screen(message::Screen::Settings(message)) => message,
            _ => return Task::none(),
        };

        let effects = workflow::execute_settings_message(self, context, message);
        Task::batch(effects.into_iter().map(|effect| self.run_effect(effect)))
    }

    fn run_effect(&mut self, effect: workflow::Effect) -> Task<Message> {
        match effect {
            workflow::Effect::NotifyError(message) => {
                Task::done(Message::Notification(message::Notification::NotifyError(message)))
            }
            workflow::Effect::SaveConfig(config) => {
                self.working_config = config.clone();
                Task::done(Message::Config(message::Config::SaveRequested(config)))
            }
        }
    }
}

impl crate::ui::screens::Screen for Screen {
    fn update(
        &mut self,
        context: Context<'_>,
        message: message::Message,
    ) -> Task<message::Message> {
        self.handle_message(context, message)
    }

    fn view<'a>(&'a self, context: Context<'a>) -> iced::Element<'a, message::Message> {
        self.render_view(context)
    }
}
