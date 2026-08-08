use iced::{
    Element, Length, Padding,
    widget::{container, mouse_area, text},
};

use crate::ui::{
    APP_TITLE, fonts,
    message::{self, Message},
    theme,
};

pub(in crate::ui) fn titlebar<'a>() -> Element<'a, Message> {
    let title = container(text(APP_TITLE).size(12).style(text::primary).font(fonts::BOLD))
        .width(Length::Fill)
        .height(Length::Fill)
        .center_y(Length::Fill);

    mouse_area(
        container(title)
            .width(Length::Fill)
            .height(Length::Fixed(40.0))
            .padding(Padding::from([0, 15]))
            .style(theme::titlebar_container),
    )
    .on_press(Message::WindowControl(message::window::Control::Drag))
    .on_double_click(Message::WindowControl(message::window::Control::Maximize))
    .into()
}
