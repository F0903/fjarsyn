use iced::{
    Alignment, Color, Element, Length,
    widget::{button, container, row},
};
use iced_fonts::lucide;

use crate::ui::{
    message::{self, Message},
    theme,
};

pub(in crate::ui) fn window_controls<'a>(is_maximized: bool) -> Element<'a, Message> {
    let control_button = |icon: iced::widget::Text<'a>, msg: Message, hover: Option<Color>| {
        button(
            container(icon.size(10))
                .width(Length::Fill)
                .height(Length::Fill)
                .center_x(Length::Fill)
                .center_y(Length::Fill),
        )
        .on_press(msg)
        .style(move |theme, status| theme::window_control_style(theme, status, hover))
        .width(23)
        .height(23)
        .padding(0)
    };

    row![
        control_button(
            lucide::minus(),
            Message::WindowControl(message::window::Control::Minimize),
            None
        ),
        control_button(
            if is_maximized { lucide::copy() } else { lucide::maximize() },
            Message::WindowControl(message::window::Control::Maximize),
            None
        ),
        control_button(
            lucide::x(),
            Message::WindowControl(message::window::Control::Close),
            Some(theme::CONTROL_CLOSE_HOVER)
        ),
    ]
    .spacing(10)
    .align_y(Alignment::Center)
    .into()
}
