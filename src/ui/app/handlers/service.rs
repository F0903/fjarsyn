use iced::Task;

use crate::ui::{
    app::{
        Fjarsyn,
        workflows::service::{self, ServiceEffect},
    },
    message::{
        CallServiceMessage, CaptureMessage, ContactsServiceMessage, DatabaseMessage, Message,
        NavigationMessage, ScreenMessage,
    },
};

pub fn handle_call_service_msg(app: &mut Fjarsyn, message: CallServiceMessage) -> Task<Message> {
    let effects = service::reduce_call_service(app, message);
    run_effects(app, effects)
}

pub fn handle_capture_msg(app: &mut Fjarsyn, message: CaptureMessage) -> Task<Message> {
    let effects = service::reduce_capture(app, message);
    run_effects(app, effects)
}

pub fn handle_database_msg(app: &mut Fjarsyn, message: DatabaseMessage) -> Task<Message> {
    let effects = service::reduce_database(app, message);
    run_effects(app, effects)
}

fn run_effects(app: &mut Fjarsyn, effects: Vec<ServiceEffect>) -> Task<Message> {
    let mut tasks = Vec::with_capacity(effects.len());
    for effect in effects {
        tasks.push(run_effect(app, effect));
    }
    Task::batch(tasks)
}

fn run_effect(app: &mut Fjarsyn, effect: ServiceEffect) -> Task<Message> {
    match effect {
        ServiceEffect::NotifyError(message) => {
            app.ctx.notify_error(message);
            Task::none()
        }
        ServiceEffect::NotifyInfo(message) => {
            app.ctx.notify_info(message);
            Task::none()
        }
        ServiceEffect::SaveConfig => {
            if let Err(err) = app.ctx.config.save() {
                app.ctx.notify_error(format!("Failed to save peer ID: {}", err));
            }
            Task::none()
        }
        ServiceEffect::Navigate(route) => {
            Task::done(Message::Navigation(NavigationMessage::Navigate(route)))
        }
        ServiceEffect::LoadContacts => {
            Task::done(Message::ContactData(ContactsServiceMessage::LoadContacts))
        }
        ServiceEffect::RetryCallCaptureStart => Task::done(Message::Screen(ScreenMessage::Call(
            crate::ui::screens::call::CallMessage::StartCapture,
        ))),
    }
}
