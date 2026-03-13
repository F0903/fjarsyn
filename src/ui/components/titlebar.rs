use iced::{
    Alignment, Color, Element, Length, Padding,
    widget::{button, container, mouse_area, row, text},
};
use iced_fonts::lucide;

use crate::ui::{app::APP_TITLE, fonts, message::Message, theme};

pub fn window_controls<'a>(is_maximized: bool) -> Element<'a, Message> {
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
        control_button(lucide::minus(), Message::Minimize, None),
        control_button(
            if is_maximized { lucide::copy() } else { lucide::maximize() },
            Message::Maximize,
            None
        ),
        control_button(lucide::x(), Message::Close, Some(theme::CONTROL_CLOSE_HOVER)),
    ]
    .spacing(10)
    .align_y(Alignment::Center)
    .into()
}

pub fn titlebar<'a>() -> Element<'a, Message> {
    let title = container(text(APP_TITLE).size(12).style(text::primary).font(fonts::outfit::BOLD))
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
    .on_press(Message::Drag)
    .on_double_click(Message::Maximize)
    .into()
}
