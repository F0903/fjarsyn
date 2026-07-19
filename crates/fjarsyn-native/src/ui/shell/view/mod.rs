mod codec;
mod overlay;

use iced::{
    Alignment, Element, Length, Subscription, Theme, padding,
    widget::{column, container, row, stack, text},
    window as iced_window,
};

use super::{APP_TITLE, AppLifecycle, Fjarsyn};
use crate::ui::{
    components,
    message::{LifecycleMessage, Message, NavigationMessage, Route},
    screens::Screen,
    shell::ShellContext,
    subscription, theme,
};

impl Fjarsyn {
    pub fn view<'a>(&'a self, _window: iced_window::Id) -> Element<'a, Message> {
        let titlebar = components::titlebar();
        let titlebar_size = match titlebar.as_widget().size().height {
            Length::Fixed(size) => size,
            _ => 0.0,
        };

        let body: Element<'_, Message> = match &self.ctx.lifecycle {
            AppLifecycle::Starting => container(
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
            AppLifecycle::Failed(error) => container(
                column![
                    text("Fjarsyn could not start").size(24),
                    text(error.clone()).size(12).style(text::secondary),
                    row![
                        iced::widget::button("Retry")
                            .on_press(Message::Lifecycle(LifecycleMessage::RetryStartup)),
                        iced::widget::button("Settings").on_press(Message::Navigation(
                            NavigationMessage::Navigate(Route::Settings),
                        )),
                    ]
                    .spacing(10),
                ]
                .spacing(14)
                .align_x(Alignment::Center),
            )
            .center(Length::Fill)
            .into(),
            AppLifecycle::Restarting => container(
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
            AppLifecycle::RestartFailed(error) => container(
                column![
                    text("Fjarsyn could not restart").size(24),
                    text(error.clone()).size(12).style(text::secondary),
                    text("The old application services are already stopped. Try launching a clean process again or close Fjarsyn.")
                        .size(12)
                        .style(text::secondary),
                    row![
                        iced::widget::button("Try restart again")
                            .on_press(Message::Lifecycle(LifecycleMessage::RestartRequested)),
                        iced::widget::button("Close Fjarsyn")
                            .on_press(Message::Lifecycle(LifecycleMessage::ExitRequested)),
                    ]
                    .spacing(10),
                ]
                .spacing(14)
                .align_x(Alignment::Center),
            )
            .center(Length::Fill)
            .into(),
            AppLifecycle::Ready | AppLifecycle::ShuttingDown => {
                let ctx = ShellContext::new(&self.ctx);
                let route = self.active_screen.route();
                let content = container(self.active_screen.view(ctx))
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .style(theme::main_content_container);
                let application = row![components::sidebar(ctx, route), content]
                    .width(Length::Fill)
                    .height(Length::Fill);
                let mut shell = column![];
                if let Some(banner) = codec::restart_required_banner(&self.ctx.media) {
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
            components::notifications_view(self.ctx.ui.notifications.notifications());
        let maximized = self.ctx.ui.main_window.as_ref().is_some_and(|window| window.maximized);
        let controls = container(components::window_controls(maximized))
            .width(Length::Fill)
            .height(Length::Fixed(40.0))
            .padding([0, 15])
            .align_x(Alignment::End)
            .align_y(Alignment::Center);
        let content = stack![body, notifications, titlebar, controls];
        if maximized { content.into() } else { stack![content, components::resize_grid()].into() }
    }

    pub fn subscription(&self) -> Subscription<Message> {
        subscription::subscription(self)
    }

    pub fn theme(&self, _window: iced_window::Id) -> Theme {
        theme::fjarsyn_theme()
    }

    pub fn title(&self, _window: iced_window::Id) -> String {
        APP_TITLE.to_string()
    }
}
