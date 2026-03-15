use iced::{
    Alignment, Element, Length,
    widget::{button, container, row, text},
};
use iced_fonts::lucide;

use crate::{
    services::notification_service::Notification,
    ui::message::{Message, NotificationMessage},
};

pub fn notifications_view<'a>(
    notifications: impl IntoIterator<Item = &'a Notification>,
) -> Element<'a, Message> {
    let notifications: Vec<_> = notifications.into_iter().collect();
    if notifications.is_empty() {
        return iced::widget::column![].into();
    }

    let content = iced::widget::column(
        notifications
            .iter()
            .map(|n| {
                let kind = n.kind;
                container(
                    row![
                        text(&n.message).size(14).width(Length::Fill),
                        button(lucide::x().size(12))
                            .on_press(Message::Notification(
                                NotificationMessage::DismissNotification(n.id,)
                            ))
                            .style(button::text)
                            .padding(5),
                    ]
                    .align_y(Alignment::Center)
                    .spacing(10),
                )
                .padding(12)
                .style(move |theme| crate::ui::theme::notification_container(theme, kind))
                .width(Length::Fixed(320.0))
                .into()
            })
            .collect::<Vec<_>>(),
    )
    .spacing(10)
    .align_x(Alignment::End);

    container(content)
        .padding(20)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(Alignment::End)
        .align_y(Alignment::End)
        .into()
}
