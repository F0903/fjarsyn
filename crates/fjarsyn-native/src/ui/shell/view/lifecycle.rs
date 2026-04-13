use fjarsyn_core::app::{AppLifecycle, ServicePhase};
use iced::{
    Alignment, Element, Length, Padding, padding,
    widget::{button, column, container, row, text},
};

use super::super::{Fjarsyn, ShellContext};
use crate::ui::{
    message::{Message, NavigationMessage, Route, WindowControlMessage},
    screens::Screen,
    theme,
};

impl Fjarsyn {
    pub(super) fn failed_shell_actions<'a>(
        &self,
        show_open_settings: bool,
    ) -> Element<'a, Message> {
        let mut actions = row![].spacing(12);

        if show_open_settings {
            actions = actions.push(
                button("Open Settings")
                    .on_press(Message::Navigation(NavigationMessage::Navigate(Route::Settings)))
                    .padding([10, 16]),
            );
        } else {
            actions = actions.push(
                button("Back to Diagnostics")
                    .on_press(Message::Navigation(NavigationMessage::Navigate(Route::Home)))
                    .padding([10, 16]),
            );
        }

        actions = actions.push(
            button("Retry Startup")
                .on_press(Self::retry_startup_message())
                .style(button::primary)
                .padding([10, 16]),
        );

        actions
            .push(
                button("Close App")
                    .on_press(Message::WindowControl(WindowControlMessage::Close))
                    .style(button::danger)
                    .padding([10, 16]),
            )
            .into()
    }

    pub(super) fn lifecycle_panel<'a>(&self) -> Option<Element<'a, Message>> {
        let (title, message, background) = match self.ctx.lifecycle {
            AppLifecycle::Bootstrapping => (
                "Starting Services",
                "Fjarsyn is still initializing runtime services. Some features stay disabled until startup finishes.",
                iced::Color::from_rgb(0.16, 0.24, 0.34),
            ),
            AppLifecycle::Degraded => (
                "Limited Availability",
                "One or more non-critical services are unavailable. The shell stays usable, but some features are limited.",
                iced::Color::from_rgb(0.38, 0.28, 0.12),
            ),
            AppLifecycle::ShuttingDown => (
                "Shutting Down",
                "New work is blocked while the app exits.",
                iced::Color::from_rgb(0.22, 0.22, 0.22),
            ),
            AppLifecycle::Ready | AppLifecycle::Failed => return None,
        };

        Some(
            container(
                column![
                    text(title).size(13).style(text::primary),
                    text(message).size(11).style(text::secondary),
                    self.service_status_list(true),
                ]
                .spacing(8),
            )
            .width(Length::Fill)
            .padding(12)
            .style(move |_| container::Style {
                background: Some(background.into()),
                border: iced::Border {
                    radius: 10.0.into(),
                    width: 1.0,
                    color: iced::Color { a: 0.25, ..iced::Color::WHITE },
                },
                ..Default::default()
            })
            .into(),
        )
    }

    pub(super) fn failed_shell<'a>(&self) -> Element<'a, Message> {
        let body = container(
            column![
                text("Fjarsyn Could Not Start").size(26),
                text(
                    "A required runtime service failed during startup. Open settings to adjust configuration, or retry startup after correcting the issue."
                )
                .size(12)
                .style(text::secondary),
                self.service_status_list(false),
                self.failed_shell_actions(true),
            ]
            .spacing(18)
            .max_width(680),
        )
        .padding(28)
        .style(theme::card_container);

        container(body)
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .into()
    }

    pub(super) fn failed_settings_shell<'a>(
        &'a self,
        ctx: ShellContext<'a>,
        titlebar_size: f32,
    ) -> Element<'a, Message> {
        let screen_content = self.active_screen.view(ctx);
        let settings_content = container(screen_content)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(theme::main_content_container);

        let diagnostics = container(
            column![
                text("Recovery Mode").size(22),
                text(
                    "Settings remain available so you can adjust configuration before retrying startup."
                )
                .size(12)
                .style(text::secondary),
                self.service_status_list(true),
                self.failed_shell_actions(false),
            ]
            .spacing(14),
        )
        .padding(20)
        .style(theme::card_container);

        column![container(diagnostics).padding(Padding::from([0, 12])), settings_content,]
            .padding(padding::top(titlebar_size))
            .width(Length::Fill)
            .height(Length::Fill)
            .spacing(10)
            .into()
    }

    fn service_status_list<'a>(&self, compact: bool) -> Element<'a, Message> {
        let mut list = column![].spacing(if compact { 6 } else { 10 });

        for (label, phase, detail) in self.service_rows() {
            let badge = self.service_phase_badge(phase);
            let row = if compact {
                row![text(label).size(11).width(Length::Fill), badge,]
                    .align_y(Alignment::Center)
                    .spacing(8)
            } else {
                row![
                    column![text(label).size(13), text(detail).size(11).style(text::secondary),]
                        .spacing(3)
                        .width(Length::Fill),
                    badge,
                ]
                .align_y(Alignment::Center)
                .spacing(12)
            };

            let padding = if compact { 8 } else { 10 };
            list = list.push(container(row).padding(padding).style(theme::section_container));
        }

        list.into()
    }

    fn service_phase_badge<'a>(&self, phase: ServicePhase) -> Element<'a, Message> {
        let (label, color) = match phase {
            ServicePhase::Pending => ("Pending", iced::Color::from_rgb(0.55, 0.55, 0.60)),
            ServicePhase::Ready => ("Ready", iced::Color::from_rgb(0.20, 0.72, 0.34)),
            ServicePhase::Failed => ("Failed", iced::Color::from_rgb(0.82, 0.28, 0.28)),
        };

        container(text(label).size(10))
            .padding([4, 8])
            .style(move |_| container::Style {
                background: Some(color.into()),
                border: iced::Border { radius: 999.0.into(), ..Default::default() },
                text_color: Some(iced::Color::WHITE),
                ..Default::default()
            })
            .into()
    }

    fn service_rows(&self) -> [(&'static str, ServicePhase, &'static str); 4] {
        [
            ("Database", self.ctx.services.database, "Contacts and persisted message history."),
            ("Call Service", self.ctx.services.call, "Peer identity, signaling, and call control."),
            (
                "Discovery",
                self.ctx.services.discovery,
                "Nearby peer visibility and automatic reachability.",
            ),
            ("Messaging", self.ctx.services.messaging, "Conversation sync and message delivery."),
        ]
    }
}
