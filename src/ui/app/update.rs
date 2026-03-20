use iced::Task;

use super::{Fjarsyn, Screen, global};
use crate::ui::message::Message;

impl Fjarsyn {
    pub fn update(&mut self, message: Message) -> Task<Message> {
        // Let the active screen update its own state first, then run any app-wide
        // orchestration that the message implies.
        let screen_task = self.active_screen.update(&mut self.ctx, message.clone());
        let global_task = global::handle_global_message(self, message);

        Task::batch([screen_task, global_task])
    }
}
