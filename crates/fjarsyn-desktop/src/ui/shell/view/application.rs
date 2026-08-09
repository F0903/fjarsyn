use iced::{
    Alignment, Element, Length, Subscription, Theme, padding,
    widget::{column, container, row, stack, text},
    window,
};

use super::{codec, degraded};
use crate::ui::{
    APP_TITLE, components,
    message::{self, Message, Route},
    shell::{Fjarsyn, Lifecycle},
    subscription, theme,
};

impl Fjarsyn {
    pub(in crate::ui) fn view<'a>(&'a self, _window: window::Id) -> Element<'a, Message> {
        let titlebar = components::titlebar();
        let titlebar_size = match titlebar.as_widget().size().height {
            Length::Fixed(size) => size,
            _ => 0.0,
        };

        let body: Element<'_, Message> = match &self.state.lifecycle {
            Lifecycle::Starting => container(
                column![
                    text("Starting Fjarsyn").size(24),
                    text("Initializing contacts, authenticated peer sessions, presence, and messaging...")
                        .size(13)
                        .style(text::secondary),
                ]
                .spacing(10)
                .align_x(Alignment::Center),
            )
            .center(Length::Fill)
            .into(),
            Lifecycle::StartupFailed(error) if self.active_screen.is_settings() => {
                let context = self.state.presentation();
                container(self.active_screen.startup_recovery_view(context, error))
                    .padding(padding::top(titlebar_size))
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into()
            }
            Lifecycle::StartupFailed(error) => container(
                column![
                    text("Fjarsyn could not start").size(24),
                    text(error.clone()).size(12).style(text::secondary),
                    row![
                        iced::widget::button("Retry")
                            .on_press(Message::Lifecycle(message::Lifecycle::RetryStartup)),
                        iced::widget::button("Settings").on_press(Message::Navigation(
                            message::Navigation::Navigate(Route::Settings),
                        )),
                    ]
                    .spacing(10),
                ]
                .spacing(14)
                .align_x(Alignment::Center),
            )
            .center(Length::Fill)
            .into(),
            Lifecycle::Degraded(error) => degraded::view(error),
            Lifecycle::Restarting => container(
                column![
                    text("Restarting Fjarsyn").size(24),
                    text("Closing active connections and media workers before starting a clean process...")
                        .size(13)
                        .style(text::secondary),
                ]
                .spacing(10)
                .align_x(Alignment::Center),
            )
            .center(Length::Fill)
            .into(),
            Lifecycle::RestartFailed(error) => container(
                column![
                    text("Fjarsyn could not restart").size(24),
                    text(error.clone()).size(12).style(text::secondary),
                    text("The old engine runtime is already stopped. Try launching a clean process again or close Fjarsyn.")
                        .size(12)
                        .style(text::secondary),
                    row![
                        iced::widget::button("Try restart again")
                            .on_press(Message::Lifecycle(message::Lifecycle::RestartRequested)),
                        iced::widget::button("Close Fjarsyn")
                            .on_press(Message::Lifecycle(message::Lifecycle::ExitRequested)),
                    ]
                    .spacing(10),
                ]
                .spacing(14)
                .align_x(Alignment::Center),
            )
            .center(Length::Fill)
            .into(),
            Lifecycle::Ready | Lifecycle::ShuttingDown => {
                let context = self.state.presentation();
                let route = self.active_screen.route();
                let content = container(self.active_screen.view(context))
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .style(theme::main_content_container);
                let application = row![components::sidebar(context, route), content]
                    .width(Length::Fill)
                    .height(Length::Fill);
                let mut shell = column![];
                if let Some(banner) = codec::restart_required_banner(&self.state.screen_share) {
                    shell = shell.push(banner);
                }
                let shell = shell
                    .push(application)
                    .padding(padding::top(titlebar_size))
                    .width(Length::Fill)
                    .height(Length::Fill);
                let mut layers = stack![shell];
                if let Some(incoming) = self.incoming_session_popup() {
                    layers = layers.push(incoming);
                }
                layers.into()
            }
        };

        let notifications =
            components::notifications_view(self.state.ui.notifications.notifications());
        let maximized = self.state.ui.main_window.as_ref().is_some_and(|window| window.maximized);
        let controls = container(components::window_controls(maximized))
            .width(Length::Fill)
            .height(Length::Fixed(40.0))
            .padding([0, 15])
            .align_x(Alignment::End)
            .align_y(Alignment::Center);
        let content = stack![body, notifications, titlebar, controls];
        if maximized { content.into() } else { stack![content, components::resize_grid()].into() }
    }

    pub(in crate::ui) fn subscription(&self) -> Subscription<Message> {
        subscription::subscription(
            self.runtime.engine.as_ref().map(|runtime| runtime.receivers()),
            self.state.ui.started_at,
            self.state.ui.notifications.next_deadline(),
        )
    }

    pub(in crate::ui) fn theme(&self, _window: window::Id) -> Theme {
        theme::fjarsyn_theme()
    }

    pub(in crate::ui) fn title(&self, _window: window::Id) -> String {
        APP_TITLE.to_string()
    }
}
