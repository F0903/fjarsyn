use iced::{Element, Task};

use super::{contacts, home, peer, settings};
use crate::ui::{
    message::{Message, Route},
    presentation::Context,
};

pub(in crate::ui) trait Screen {
    fn update(&mut self, context: Context<'_>, message: Message) -> Task<Message>;
    fn view<'a>(&'a self, context: Context<'a>) -> Element<'a, Message>;
}

#[derive(Debug, Clone)]
pub(in crate::ui) struct Active(Kind);

#[derive(Debug, Clone)]
enum Kind {
    Home(home::Screen),
    Contacts(contacts::Screen),
    Peer(peer::Screen),
    Settings(settings::Screen),
}

impl Screen for Active {
    fn update(&mut self, context: Context<'_>, message: Message) -> Task<Message> {
        match &mut self.0 {
            Kind::Home(screen) => screen.update(context, message),
            Kind::Contacts(screen) => screen.update(context, message),
            Kind::Peer(screen) => screen.update(context, message),
            Kind::Settings(screen) => screen.update(context, message),
        }
    }

    fn view<'a>(&'a self, context: Context<'a>) -> Element<'a, Message> {
        match &self.0 {
            Kind::Home(screen) => screen.view(context),
            Kind::Contacts(screen) => screen.view(context),
            Kind::Peer(screen) => screen.view(context),
            Kind::Settings(screen) => screen.view(context),
        }
    }
}

impl Active {
    pub(in crate::ui) fn route(&self) -> Route {
        match &self.0 {
            Kind::Home(_) => Route::Home,
            Kind::Contacts(_) => Route::Contacts,
            Kind::Peer(screen) => Route::Peer { peer_id: screen.peer_id().clone() },
            Kind::Settings(_) => Route::Settings,
        }
    }

    pub(in crate::ui) fn from_route(route: Route, context: Context<'_>) -> Self {
        Self(match route {
            Route::Home => Kind::Home(home::Screen::new()),
            Route::Contacts => Kind::Contacts(contacts::Screen::new()),
            Route::Peer { peer_id } => Kind::Peer(peer::Screen::new(peer_id)),
            Route::Settings => Kind::Settings(settings::Screen::new(context.config())),
        })
    }
}
