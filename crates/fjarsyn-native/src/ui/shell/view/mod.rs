mod lifecycle;
mod overlay;

use iced::{
    Alignment, Element, Length, Padding, Subscription, Theme, padding,
    widget::{column, container, row, stack},
    window as iced_window,
};

use super::{APP_TITLE, Fjarsyn};
use crate::ui::{
    components, message::Message, screens::Screen, shell::AppContext, subscription, theme,
};

impl Fjarsyn {
    fn retry_startup_message() -> Message {
        Message::Lifecycle(crate::ui::message::LifecycleMessage::RetryStartup)
    }

    pub fn view<'a>(&'a self, _window: iced_window::Id) -> Element<'a, Message> {
        let titlebar = components::titlebar();
        let titlebar_size = match titlebar.as_widget().size().height {
            Length::Fixed(s) => s,
            _ => {
                tracing::warn!("Could not get titlebar_size in pixels!");
                0.0
            }
        };

        let shell_body = if matches!(self.ctx.lifecycle, fjarsyn_core::app::AppLifecycle::Failed) {
            match self.active_screen {
                super::ActiveScreen::Settings(_) => {
                    let ctx = AppContext { state: &self.ctx, runtime: &self.runtime };
                    self.failed_settings_shell(ctx, titlebar_size)
                }
                _ => self.failed_shell(),
            }
        } else {
            let ctx = AppContext { state: &self.ctx, runtime: &self.runtime };
            let screen_content = self.active_screen.view(ctx);
            let current_route = self.active_screen.get_route(ctx);

            let main_content = container(screen_content)
                .width(Length::Fill)
                .height(Length::Fill)
                .style(theme::main_content_container);

            let sidebar = match self.active_screen {
                super::ActiveScreen::Call(_) => None,
                _ => Some(components::sidebar(ctx, current_route)),
            };

            let mut main_layout = row![]
                .padding(padding::top(titlebar_size))
                .width(Length::Fill)
                .height(Length::Fill);
            if let Some(sidebar) = sidebar {
                main_layout = main_layout.push(sidebar);
            }
            let main_layout = main_layout.push(main_content);

            let mut shell_content = column![].width(Length::Fill).height(Length::Fill).spacing(10);
            if let Some(panel) = self.lifecycle_panel() {
                shell_content =
                    shell_content.push(container(panel).padding(Padding::from([0, 12])));
            }
            shell_content = shell_content.push(main_layout);

            let call_popup =
                self.ctx.session.incoming_call_id.is_some().then(|| self.incoming_call_popup());
            let mut call_popup_stack = stack![shell_content];
            if let Some(popup) = call_popup {
                call_popup_stack = call_popup_stack.push(popup);
            }

            call_popup_stack.into()
        };

        let notifications =
            components::notifications_view(self.ctx.ui.notifications.notifications());
        let is_maximized = self.ctx.ui.main_window.as_ref().map(|w| w.maximized).unwrap_or(false);

        let controls = container(components::window_controls(is_maximized))
            .width(Length::Fill)
            .height(Length::Fixed(40.0))
            .padding(Padding::from([0, 15]))
            .align_x(Alignment::End)
            .align_y(Alignment::Center);

        let content_stack = stack![shell_body, notifications, titlebar, controls];

        let final_stack = if is_maximized {
            content_stack
        } else {
            stack![content_stack, components::resize_grid()]
        };

        final_stack.into()
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
