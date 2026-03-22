use iced::{
    Alignment, Element, Length, Padding, Subscription, Theme, padding,
    widget::{button, column, container, row, stack, text},
    window as iced_window,
};
use iced_fonts::lucide;

use super::{APP_TITLE, ActiveScreen, Fjarsyn, Screen};
use crate::ui::{components, message::Message, subscription, theme};

impl Fjarsyn {
    fn incoming_call_popup<'a>(&self) -> Element<'a, Message> {
        let sender_id = match &self.ctx.session.incoming_call_id {
            Some(id) => id,
            None => {
                return column![].into();
            }
        };

        let sender_name = self
            .ctx
            .networking
            .discovered_peers
            .iter()
            .find(|p| p.id == *sender_id)
            .map(|p| p.instance_name.clone())
            .unwrap_or_else(|| {
                format!("{}...", crate::utils::string_utils::truncate(sender_id, 8))
            });

        use crate::ui::message::CallActionMessage;
        container(
            container(
                column![
                    text("Incoming Call").size(14).style(text::secondary),
                    text(sender_name).size(20).style(text::primary),
                    row![
                        button(row![lucide::phone_incoming().size(16), text("Accept")].spacing(10))
                            .on_press(Message::CallAction(CallActionMessage::AcceptCall))
                            .style(button::success)
                            .padding(10),
                        button(row![lucide::phone_off().size(16), text("Decline")].spacing(10))
                            .on_press(Message::CallAction(CallActionMessage::DeclineCall))
                            .style(button::danger)
                            .padding(10),
                    ]
                    .spacing(15)
                ]
                .spacing(15)
                .align_x(iced::Alignment::Center),
            )
            .padding(20)
            .style(theme::card_container),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .style(|_| container::Style {
            background: Some(iced::Color { a: 0.8, ..iced::Color::BLACK }.into()),
            ..Default::default()
        })
        .into()
    }

    pub fn view<'a>(&'a self, _window: iced_window::Id) -> Element<'a, Message> {
        let screen_content = self.active_screen.view(&self.ctx);
        let current_route = self.active_screen.get_route();

        let titlebar = components::titlebar();
        let titlebar_size = match titlebar.as_widget().size().height {
            Length::Fixed(s) => s,
            _ => {
                tracing::warn!("Could not get titlebar_size in pixels!");
                0.0
            }
        };

        let main_content = container(screen_content)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(theme::main_content_container);

        // Don't include sidebar in the call screen.
        let sidebar = match self.active_screen {
            ActiveScreen::Call(_) => None,
            _ => {
                let selected_peer_id = match &self.active_screen {
                    ActiveScreen::Messages(screen) => screen.selected_peer_id.as_deref(),
                    _ => None,
                };

                Some(components::sidebar(
                    &self.ctx,
                    current_route,
                    self.ctx.services.call_service.as_ref().map(|c| c.local_id().to_owned()),
                    selected_peer_id,
                ))
            }
        };

        let mut main_layout = row![].padding(padding::top(titlebar_size));
        if let Some(sidebar) = sidebar {
            main_layout = main_layout.push(sidebar);
        }
        let main_layout = main_layout.push(main_content);

        let call_popup =
            self.ctx.session.incoming_call_id.is_some().then(|| self.incoming_call_popup());
        let mut call_popup_stack = stack![main_layout];
        if let Some(popup) = call_popup {
            call_popup_stack = call_popup_stack.push(popup);
        }

        let notifications =
            components::notifications_view(self.ctx.ui.notifications.notifications());
        let is_maximized = self.ctx.ui.main_window.as_ref().map(|w| w.maximized).unwrap_or(false);

        let controls = container(components::window_controls(is_maximized))
            .width(Length::Fill)
            .height(Length::Fixed(40.0))
            .padding(Padding::from([0, 15]))
            .align_x(Alignment::End)
            .align_y(Alignment::Center);

        let content_stack = stack![call_popup_stack, notifications, titlebar, controls];

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
