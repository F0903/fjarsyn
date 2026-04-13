pub mod call;
pub mod contacts;
pub mod home;
pub mod messages;
pub mod settings;

use iced::{Element, Subscription, Task};

use crate::ui::{
    app::{AppContext, AppContextMut},
    message::{Message, Route},
};

#[derive(Debug, thiserror::Error)]
pub enum ScreenError {
    #[error("Screen initialization error: {0}")]
    ScreenInitializationError(String),
}

pub trait Screen {
    fn update(&mut self, ctx: &mut AppContextMut<'_>, message: Message) -> Task<Message>;
    fn view<'a>(&'a self, ctx: AppContext<'a>) -> Element<'a, Message>;
    fn subscription(&self, ctx: AppContext<'_>) -> Subscription<Message>;
}

#[derive(Debug, Clone)]
pub enum ActiveScreen {
    Home(home::HomeScreen),
    Messages(messages::MessagesScreen),
    Contacts(contacts::ContactsScreen),
    Call(call::CallScreen),
    Settings(settings::SettingsScreen),
}

#[derive(Debug, Clone)]
pub struct ScreenEntry {
    pub route: Route,
    pub screen: ActiveScreen,
}

impl Screen for ActiveScreen {
    fn update(&mut self, ctx: &mut AppContextMut<'_>, message: Message) -> Task<Message> {
        match self {
            Self::Home(screen) => screen.update(ctx, message),
            Self::Messages(screen) => screen.update(ctx, message),
            Self::Contacts(screen) => screen.update(ctx, message),
            Self::Call(screen) => screen.update(ctx, message),
            Self::Settings(screen) => screen.update(ctx, message),
        }
    }

    fn view<'a>(&'a self, ctx: AppContext<'a>) -> Element<'a, Message> {
        match self {
            Self::Home(screen) => screen.view(ctx),
            Self::Messages(screen) => screen.view(ctx),
            Self::Contacts(screen) => screen.view(ctx),
            Self::Call(screen) => screen.view(ctx),
            Self::Settings(screen) => screen.view(ctx),
        }
    }

    fn subscription(&self, ctx: AppContext<'_>) -> Subscription<Message> {
        match self {
            Self::Home(screen) => screen.subscription(ctx),
            Self::Messages(screen) => screen.subscription(ctx),
            Self::Contacts(screen) => screen.subscription(ctx),
            Self::Call(screen) => screen.subscription(ctx),
            Self::Settings(screen) => screen.subscription(ctx),
        }
    }
}

impl ActiveScreen {
    pub fn get_route(&self, ctx: AppContext<'_>) -> Route {
        match self {
            Self::Home(_) => Route::Home,
            Self::Messages(_) => Route::Messages { peer_id: ctx.messaging.active_peer_id.clone() },
            Self::Contacts(_) => Route::Contacts,
            Self::Call(_) => Route::Call,
            Self::Settings(_) => Route::Settings,
        }
    }

    pub fn from_route(route: Route, ctx: AppContext<'_>) -> Self {
        match route {
            Route::Home => Self::Home(home::HomeScreen::new(ctx)),
            Route::Messages { .. } => Self::Messages(messages::MessagesScreen::new()),
            Route::Contacts => Self::Contacts(contacts::ContactsScreen::new(ctx)),
            Route::Call => Self::Call(call::CallScreen::new(ctx)),
            Route::Settings => Self::Settings(settings::SettingsScreen::new(&ctx.config)),
        }
    }
}
