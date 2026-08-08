//! Registry and views for the available settings tabs.

use std::{
    fmt::Debug,
    sync::{Arc, LazyLock},
};

use fjarsyn_engine::config::Config;
use iced::Element;

use crate::ui::message::{Message, screen::settings::TabId};

mod capture;
mod network;
mod transcoding;

pub(super) trait Tab: Debug + Send + Sync {
    fn id(&self) -> TabId;
    fn label(&self) -> &'static str;
    fn icon(&self) -> iced::widget::Text<'static>;
    fn view(&self, config: &Config) -> Element<'_, Message>;
}

static TABS: LazyLock<Vec<Arc<dyn Tab>>> = LazyLock::new(|| {
    vec![Arc::new(capture::Capture), Arc::new(network::Network), Arc::new(transcoding::Transcoding)]
});

pub(super) fn get(tab_id: TabId) -> Arc<dyn Tab> {
    TABS.iter()
        .find(|tab| tab.id() == tab_id)
        .cloned()
        .expect("every settings tab identifier is registered")
}

pub(super) fn default_tab() -> Arc<dyn Tab> {
    get(TabId::Capture)
}

pub(super) fn iter() -> impl Iterator<Item = &'static Arc<dyn Tab>> {
    TABS.iter()
}
