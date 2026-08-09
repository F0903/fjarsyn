use iced::{Element, Task};

use super::{contacts, home, peer, settings};
use crate::ui::{
    message::{self, Message, Route},
    presentation::Context,
};

#[derive(Debug, Clone)]
pub(in crate::ui) struct Active(Kind);

#[derive(Debug, Clone)]
enum Kind {
    Home(home::Screen),
    Contacts(contacts::Screen),
    Peer(peer::Screen),
    Settings(settings::Screen),
}

impl Active {
    pub(in crate::ui) fn update(
        &mut self,
        context: Context<'_>,
        message: message::Screen,
    ) -> Task<Message> {
        match message {
            message::Screen::Contacts(message) => match &mut self.0 {
                Kind::Contacts(screen) => screen.update(context, message),
                _ => Task::none(),
            },
            message::Screen::Peer(message) => match &mut self.0 {
                Kind::Peer(screen) => screen.update(context, message),
                _ => Task::none(),
            },
            message::Screen::Settings(message) => match &mut self.0 {
                Kind::Settings(screen) => screen.update(context, message),
                _ => Task::none(),
            },
        }
    }

    pub(in crate::ui) fn view<'a>(&'a self, context: Context<'a>) -> Element<'a, Message> {
        match &self.0 {
            Kind::Home(screen) => screen.render_view(context),
            Kind::Contacts(screen) => screen.render_view(context),
            Kind::Peer(screen) => screen.render_view(context),
            Kind::Settings(screen) => screen.render_view(context),
        }
    }

    pub(in crate::ui) fn startup_recovery_view<'a>(
        &'a self,
        context: Context<'a>,
        startup_error: &'a str,
    ) -> Element<'a, Message> {
        let Kind::Settings(screen) = &self.0 else {
            unreachable!("startup recovery settings are rendered only on the settings route");
        };
        screen.render_startup_recovery_view(context, startup_error)
    }

    pub(in crate::ui) fn contact_save_finished(
        &mut self,
        operation_id: message::screen::contacts::OperationId,
        succeeded: bool,
    ) {
        if let Kind::Contacts(screen) = &mut self.0 {
            screen.finish_contact_save(operation_id, succeeded);
        }
    }

    pub(in crate::ui) fn contact_identity_update_finished(
        &mut self,
        operation_id: message::screen::contacts::OperationId,
        succeeded: bool,
    ) {
        if let Kind::Contacts(screen) = &mut self.0 {
            screen.finish_identity_replacement(operation_id, succeeded);
        }
    }

    pub(in crate::ui) fn contact_delete_finished(
        &mut self,
        operation_id: message::screen::contacts::OperationId,
        contact_id: i64,
        succeeded: bool,
    ) {
        if let Kind::Contacts(screen) = &mut self.0 {
            screen.finish_contact_delete(operation_id, contact_id, succeeded);
        }
    }

    pub(in crate::ui) fn route(&self) -> Route {
        match &self.0 {
            Kind::Home(_) => Route::Home,
            Kind::Contacts(_) => Route::Contacts,
            Kind::Peer(screen) => Route::Peer { peer_id: screen.peer_id().clone() },
            Kind::Settings(_) => Route::Settings,
        }
    }

    pub(in crate::ui) fn is_settings(&self) -> bool {
        matches!(&self.0, Kind::Settings(_))
    }

    pub(in crate::ui) fn from_route(route: Route, context: Context<'_>) -> Self {
        Self(match route {
            Route::Home => Kind::Home(home::Screen::new()),
            Route::Contacts => Kind::Contacts(contacts::Screen::new()),
            Route::Peer { peer_id } => Kind::Peer(peer::Screen::new(peer_id)),
            Route::Settings => Kind::Settings(settings::Screen::new(context.settings())),
        })
    }
}
