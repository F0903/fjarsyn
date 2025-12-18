use std::time::{Duration, Instant};

use iced::{
    Element, Length,
    widget::{container, row, text},
};

use crate::ui::message::Message;

const NOTIFICATION_INFO_COLOR: iced::Color = iced::Color::from_rgb8(0, 100, 200);
const NOTIFICATION_ERROR_COLOR: iced::Color = iced::Color::from_rgb8(200, 0, 0);
const NOTIFICATION_SUCCESS_COLOR: iced::Color = iced::Color::from_rgb8(0, 200, 0);
const NOTIFICATION_DEFAULT_DURATION: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationKind {
    Info,
    Error,
    Success,
}

#[derive(Debug, Clone)]
pub struct Notification {
    pub id: u64,
    pub message: String,
    pub kind: NotificationKind,
    pub created_at: Instant,
    pub duration: Duration,
}

impl Notification {
    pub fn new(id: u64, message: String, kind: NotificationKind) -> Self {
        Self {
            id,
            message,
            kind,
            created_at: Instant::now(),
            duration: NOTIFICATION_DEFAULT_DURATION,
        }
    }

    pub fn expired(&self, now: Instant) -> bool {
        now.duration_since(self.created_at) > self.duration
    }
}

pub fn view<'a>(notifications: &'a [Notification]) -> Element<'a, Message> {
    let content = iced::widget::column(
        notifications
            .iter()
            .map(|n| {
                let color = match n.kind {
                    NotificationKind::Info => NOTIFICATION_INFO_COLOR,
                    NotificationKind::Error => NOTIFICATION_ERROR_COLOR,
                    NotificationKind::Success => NOTIFICATION_SUCCESS_COLOR,
                };

                container(
                    row![text(&n.message).color(iced::Color::WHITE).size(14)]
                        .padding(10)
                        .spacing(10),
                )
                .style(move |_| container::Style {
                    background: Some(iced::Background::Color(color)),
                    border: iced::Border { radius: 5.0.into(), ..Default::default() },
                    ..Default::default()
                })
                .width(Length::Fixed(300.0))
                .into()
            })
            .collect::<Vec<_>>(),
    )
    .spacing(10)
    .align_x(iced::Alignment::End);

    container(content)
        .padding(20)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(iced::Alignment::End)
        .align_y(iced::Alignment::End)
        .into()
}
