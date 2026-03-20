use iced::{
    Alignment, Element, Length, Padding,
    widget::{column, container, row, text},
};

use crate::ui::{fonts, message::Message, theme};

pub(super) fn settings_section<'a>(
    icon: iced::widget::Text<'a>,
    title: impl Into<String>,
    content: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    container(
        column![
            row![
                icon.size(20).style(text::primary),
                text(title.into()).size(20).font(fonts::outfit::BOLD)
            ]
            .spacing(12)
            .align_y(Alignment::Center),
            container(content).padding(Padding::ZERO.top(12.0))
        ]
        .spacing(18),
    )
    .padding(22)
    .style(theme::section_container)
    .width(Length::Fill)
    .into()
}

pub(super) fn setting_row<'a>(
    label: impl Into<String>,
    description: impl Into<String>,
    control: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    row![
        column![
            text(label.into()).size(16),
            text(description.into()).size(12).style(text::secondary).width(Length::Fill)
        ]
        .spacing(4)
        .width(Length::Fill),
        container(control).width(Length::FillPortion(2)).align_x(Alignment::End)
    ]
    .spacing(24)
    .align_y(Alignment::Center)
    .width(Length::Fill)
    .into()
}
