mod capture;
mod encoding;
mod network;

use std::{
    collections::BTreeMap,
    fmt::Debug,
    sync::{Arc, LazyLock},
};

use iced::Element;

use crate::{config::Config, ui::message::Message};

macro_rules! register_tabs {
    ($($($segment:ident)::+),*) => {
        pub(super) static TABS: LazyLock<BTreeMap<&'static str, Arc<dyn SettingsTab>>> = LazyLock::new(|| BTreeMap::from([
            $( (crate::get_type_name!($($segment)::+), Arc::new($($segment)::+) as Arc<dyn SettingsTab>) ),*
        ]));
    }
}

pub(super) fn get_tab(tab_name: &str) -> Option<Arc<dyn SettingsTab>> {
    TABS.get(tab_name).cloned()
}

#[macro_export]
macro_rules! settings_tab {
    ($($segment:ident)::+) => {
        crate::ui::screens::settings::tabs::get_tab(crate::get_type_name!($($segment)::+)).unwrap()
    };
}

#[macro_export]
macro_rules! define_tab {
    (
        $name:ident,
        icon: $icon:expr,
        view: |$config:ident| $view:expr
    ) => {
        #[derive(Debug)]
        pub(super) struct $name;

        impl SettingsTab for $name {
            fn label(&self) -> &'static str {
                stringify!($name)
            }

            fn icon(&self) -> iced::widget::Text<'static> {
                $icon
            }

            fn view(&self, $config: &Config) -> Element<'_, Message> {
                $view
            }
        }
    };
}

pub trait SettingsTab: Debug + Send + Sync {
    fn label(&self) -> &'static str;
    fn icon(&self) -> iced::widget::Text<'static>;
    fn view(&self, config: &Config) -> Element<'_, Message>;
}

impl PartialEq for dyn SettingsTab {
    fn eq(&self, other: &Self) -> bool {
        self.label() == other.label()
    }
}

register_tabs!(capture::Capture, encoding::Encoding, network::Network);
