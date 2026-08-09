use iced::{
    Alignment, Element, Length,
    widget::{button, column, container, text},
};
use iced_fonts::lucide;

use crate::ui::{
    message::{self, Message},
    theme,
};

pub(super) fn view(reason: &str) -> Element<'static, Message> {
    container(
        container(
            column![
                lucide::triangle_alert().size(32),
                text("Live updates stopped").size(24),
                text(reason.to_owned()).size(12).style(text::secondary),
                text(
                    "Presence, sessions, messages, and screen shares may now be stale. Restart Fjarsyn before performing more peer operations."
                )
                .size(12)
                .style(text::secondary),
                button("Restart Fjarsyn")
                    .on_press(Message::Lifecycle(message::Lifecycle::RestartRequested))
                    .padding([9, 14]),
            ]
            .spacing(12)
            .align_x(Alignment::Center),
        )
        .max_width(620)
        .padding(24)
        .style(theme::warning_accent_container),
    )
    .center(Length::Fill)
    .into()
}
