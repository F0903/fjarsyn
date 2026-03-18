pub mod call;
pub mod contacts;
pub mod home;
pub mod settings;

use iced::{Element, Subscription, Task};

use crate::ui::{
    app::AppState,
    message::{Message, Route},
};

#[derive(Debug, thiserror::Error)]
pub enum ScreenError {
    #[error("Screen initialization error: {0}")]
    ScreenInitializationError(String),
}

pub trait Screen {
    fn update(&mut self, ctx: &mut AppState, message: Message) -> Task<Message>;
    fn view<'a>(&'a self, ctx: &'a AppState) -> Element<'a, Message>;
    fn subscription(&self, ctx: &AppState) -> Subscription<Message>;
}

macro_rules! define_active_screen {
    (
        $( $Variant:ident ( $Type:ty ) => $ctor:expr ),* $(,)?
    ) => {
        #[derive(Debug, Clone)]
        pub enum ActiveScreen {
            $( $Variant($Type), )*
        }

        impl Screen for ActiveScreen {
            fn update(&mut self, ctx: &mut AppState, message: Message) -> Task<Message> {
                match self {
                    $( Self::$Variant(screen) => screen.update(ctx, message), )*
                }
            }

            fn view<'a>(&'a self, ctx: &'a AppState) -> Element<'a, Message> {
                match self {
                    $( Self::$Variant(screen) => screen.view(ctx), )*
                }
            }

            fn subscription(&self, ctx: &AppState) -> Subscription<Message> {
                match self {
                    $( Self::$Variant(screen) => screen.subscription(ctx), )*
                }
            }
        }

        impl ActiveScreen {
            pub fn get_route(&self) -> Route {
                match self {
                    $( Self::$Variant(_) => Route::$Variant, )*
                }
            }

            pub fn from_route(route: Route, ctx: &mut AppState) -> Self {
                match route {
                    $( Route::$Variant => Self::$Variant($ctor(ctx)), )*
                }
            }
        }
    };
}

define_active_screen! {
    Home(home::HomeScreen) => |ctx: &mut AppState| home::HomeScreen::new(ctx),
    Contacts(contacts::ContactsScreen) => |ctx: &mut AppState| contacts::ContactsScreen::new(ctx),
    Call(call::CallScreen) => |ctx: &mut AppState| call::CallScreen::new(ctx.media.capture.clone()),
    Settings(settings::SettingsScreen) => |ctx: &mut AppState| settings::SettingsScreen::new(&ctx.config),
}
