use std::{collections::HashMap, sync::atomic::AtomicU64, time::Instant};

use iced::{
    Element, Length,
    widget::{button, container, text},
};

use crate::ui::{
    message::Message,
    notification::{Notification, NotificationKind},
};

const NOTIFICATION_INFO_COLOR: iced::Color = iced::Color::from_rgb8(0, 100, 200);
const NOTIFICATION_ERROR_COLOR: iced::Color = iced::Color::from_rgb8(200, 0, 0);
const NOTIFICATION_SUCCESS_COLOR: iced::Color = iced::Color::from_rgb8(0, 200, 0);

static NEXT_ID: AtomicU64 = AtomicU64::new(0);

pub struct NotificationProvider {
    notifications: HashMap<u64, Notification>,
}

impl NotificationProvider {
    pub fn new() -> Self {
        Self { notifications: HashMap::new() }
    }

    pub fn error(&mut self, message: impl Into<String>) {
        let id = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.notifications
            .insert(id, Notification::new(id, message.into(), NotificationKind::Error));
    }

    pub fn info(&mut self, message: impl Into<String>) {
        let id = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.notifications
            .insert(id, Notification::new(id, message.into(), NotificationKind::Info));
    }

    pub fn success(&mut self, message: impl Into<String>) {
        let id = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.notifications
            .insert(id, Notification::new(id, message.into(), NotificationKind::Success));
    }

    pub fn dismiss(&mut self, id: u64) {
        self.notifications.remove(&id);
    }

    pub fn dismiss_expired(&mut self, now: Instant) {
        self.notifications.retain(|_k, n| !n.expired(now));
    }

    pub fn view<'a>(&'a self) -> Element<'a, Message> {
        let content = iced::widget::column(
            self.notifications
                .values()
                .map(|n| {
                    let color = match n.kind {
                        NotificationKind::Info => NOTIFICATION_INFO_COLOR,
                        NotificationKind::Error => NOTIFICATION_ERROR_COLOR,
                        NotificationKind::Success => NOTIFICATION_SUCCESS_COLOR,
                    };

                    container(
                        iced::widget::column![
                            text(&n.message).color(iced::Color::WHITE).size(14).width(Length::Fill),
                            button(text("Dismiss").size(14))
                                .on_press(Message::DismissNotification(n.id))
                                .padding(5)
                        ]
                        .align_x(iced::Alignment::Center)
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
}
