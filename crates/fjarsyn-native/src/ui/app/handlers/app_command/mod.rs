mod call;
mod contacts;
mod messaging;
mod runtime;
mod settings;

use fjarsyn_core::{
    app::{AppCommand, NotificationLevel},
    executors::AppCommands,
};
use iced::Task;

use crate::ui::{
    app::Fjarsyn,
    message::{Message, NavigationMessage},
};

pub fn run_app_commands(app: &mut Fjarsyn, commands: AppCommands) -> Task<Message> {
    Task::batch(commands.into_iter().map(|command| run_app_command(app, command)))
}

pub fn run_app_command(app: &mut Fjarsyn, command: AppCommand) -> Task<Message> {
    match command {
        AppCommand::Notify { level, message } => {
            run_notification(app, level, message);
            Task::none()
        }
        AppCommand::SaveConfig { success_message, error_message } => {
            settings::run_save_config(app, success_message, error_message)
        }
        AppCommand::ApplyCaptureReadback { enabled } => {
            settings::run_apply_capture_readback(app, enabled)
        }
        AppCommand::RetryStartup => runtime::run_retry_startup(app),
        AppCommand::Navigate(route) => {
            Task::done(Message::Navigation(NavigationMessage::Navigate(route)))
        }
        AppCommand::LoadContacts => contacts::run_load_contacts(app),
        AppCommand::SaveContact { peer_id, name, address } => {
            contacts::run_save_contact(app, peer_id, name, address)
        }
        AppCommand::DeleteContact { id } => contacts::run_delete_contact(app, id),
        AppCommand::UpdateContactAddress { id, peer_id, name, address } => {
            contacts::run_update_contact_address(app, id, peer_id, name, address)
        }
        AppCommand::AcceptCall => call::run_accept_call(app),
        AppCommand::DeclineCall => call::run_decline_call(app),
        AppCommand::StartCall { target } => call::run_start_call(app, target),
        AppCommand::SendMessage { peer_id, address, body } => {
            messaging::run_send_message(app, peer_id, address, body)
        }
        AppCommand::InitializeDiscovery { local_peer_id, signaling_port } => {
            runtime::run_initialize_discovery(app, local_peer_id, signaling_port)
        }
        AppCommand::InitializeMessaging => runtime::run_initialize_messaging(app),
        AppCommand::RefreshActiveConversation => messaging::run_refresh_active_conversation(app),
        AppCommand::ClearMessageDraft(peer_id) => messaging::run_clear_message_draft(peer_id),
    }
}

fn run_notification(app: &mut Fjarsyn, level: NotificationLevel, message: String) {
    match level {
        NotificationLevel::Error => app.ctx.notify_error(message),
        NotificationLevel::Info => app.ctx.notify_info(message),
        NotificationLevel::Success => app.ctx.notify_success(message),
    }
}
