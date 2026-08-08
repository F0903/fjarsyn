use iced::{
    Alignment, Element, Length,
    widget::{button, column, row, text},
};
use iced_fonts::lucide;

use crate::ui::{
    message::{self, Message, Route},
    theme,
};

pub(super) fn primary(current_route: &Route) -> Element<'static, Message> {
    column![
        sidebar_button(current_route, Route::Home, lucide::house(), "Home"),
        sidebar_button(current_route, Route::Contacts, lucide::users(), "Contacts"),
    ]
    .spacing(5)
    .into()
}

pub(super) fn settings(current_route: &Route) -> Element<'static, Message> {
    sidebar_button(current_route, Route::Settings, lucide::settings(), "Settings").into()
}

fn sidebar_button(
    active_route: &Route,
    target_route: Route,
    icon: iced::widget::Text<'static>,
    label: &'static str,
) -> iced::widget::Button<'static, Message> {
    let is_active = active_route.same_screen(&target_route);
    button(
        row![icon.size(16), text(label).size(14)]
            .spacing(10)
            .align_y(Alignment::Center)
            .width(Length::Fill),
    )
    .on_press(Message::Navigation(message::Navigation::Navigate(target_route)))
    .width(Length::Fill)
    .style(move |theme, status| theme::sidebar_button_style(theme, status, is_active))
}
