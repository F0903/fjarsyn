pub mod call;
pub mod contacts;
pub mod home;
pub mod settings;

use iced::{Element, Subscription, Task};

use crate::ui::{
    message::{Message, Route},
    state::AppContext,
};

#[derive(Debug, thiserror::Error)]
pub enum ScreenError {
    #[error("Screen initialization error: {0}")]
    ScreenInitializationError(String),
}

pub trait Screen {
    fn update(&mut self, ctx: &mut AppContext, message: Message) -> Task<Message>;
    fn view<'a>(&'a self, ctx: &'a AppContext) -> Element<'a, Message>;
    fn subscription(&self, ctx: &AppContext) -> Subscription<Message>;
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
            fn update(&mut self, ctx: &mut AppContext, message: Message) -> Task<Message> {
                match self {
                    $( Self::$Variant(screen) => screen.update(ctx, message), )*
                }
            }

            fn view<'a>(&'a self, ctx: &'a AppContext) -> Element<'a, Message> {
                match self {
                    $( Self::$Variant(screen) => screen.view(ctx), )*
                }
            }

            fn subscription(&self, ctx: &AppContext) -> Subscription<Message> {
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

            pub fn from_route(route: Route, ctx: &mut AppContext) -> Self {
                match route {
                    $( Route::$Variant => Self::$Variant($ctor(ctx)), )*
                }
            }
        }
    };
}

define_active_screen! {
    Home(home::HomeScreen) => |ctx: &mut AppContext| home::HomeScreen::new(ctx),
    Contacts(contacts::ContactsScreen) => |ctx: &mut AppContext| contacts::ContactsScreen::new(ctx),
    Call(call::CallScreen) => |ctx: &mut AppContext| call::CallScreen::new(ctx.capture.clone().expect("Capture provider must be initialized")),
    Settings(settings::SettingsScreen) => |ctx: &mut AppContext| settings::SettingsScreen::new(ctx.config.clone()),
}
