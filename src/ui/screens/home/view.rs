use iced::{
    Alignment, Element, Length,
    widget::{button, column, container, row, scrollable, text, text_input},
};
use iced_fonts::lucide;

use super::{HomeMessage, HomeScreen};
use crate::ui::{
    app::{AppState, NetworkingState},
    fonts,
    message::{
        CallActionMessage, CallTarget, ContactsServiceMessage, Message, NavigationMessage, Route,
        ScreenMessage,
    },
    theme,
};

impl HomeScreen {
    pub fn render_view<'a>(&'a self, ctx: &'a AppState) -> Element<'a, Message> {
        let content = column![
            self.view_header(),
            self.view_manual_call(),
            self.view_nearby_peers(&ctx.networking)
        ]
        .spacing(30);

        container(scrollable(content)).width(Length::Fill).height(Length::Fill).padding(20).into()
    }

    fn view_header(&self) -> Element<'_, Message> {
        text("Discovery").size(32).style(text::primary).font(fonts::outfit::BOLD).into()
    }

    fn view_manual_call(&self) -> Element<'_, Message> {
        container(
            column![
                text("Manual Call").size(18),
                row![
                    text_input(
                        "Enter IP:Port (e.g. 192.168.1.50:8080)...",
                        &self.manual_target_address
                    )
                    .on_input(|val| Message::Screen(ScreenMessage::Home(
                        HomeMessage::TargetAddressChanged(val),
                    )))
                    .padding(10)
                    .style(theme::text_input_style),
                    button(row![lucide::phone().size(16), text("Call")].spacing(10))
                        .on_press(Message::CallAction(CallActionMessage::StartCall(
                            CallTarget::Address(self.manual_target_address.clone()),
                        )))
                        .padding(10)
                        .style(|theme, status| theme::button_style(theme, status, true)),
                ]
                .spacing(10)
            ]
            .spacing(10),
        )
        .padding(20)
        .style(crate::ui::theme::card_container)
        .into()
    }

    fn view_nearby_peers(&self, networking: &NetworkingState) -> Element<'_, Message> {
        let mut nearby_section = column![
            row![lucide::antenna().size(20), text("Nearby Peers").size(20)]
                .spacing(10)
                .align_y(Alignment::Center)
        ]
        .spacing(15);

        if networking.discovered_peers.is_empty() {
            nearby_section = nearby_section.push(
                container(text("Searching for peers on your local network...").size(14))
                    .padding(20)
                    .width(Length::Fill)
                    .align_x(Alignment::Center),
            );
        } else {
            let mut peers_list = column![].spacing(10);
            for peer in &networking.discovered_peers {
                let peer_card = container(
                    row![
                        // Icon bubble
                        container(container(lucide::user().size(20).center()).center(Length::Fill))
                            .padding(6)
                            .style(crate::ui::theme::icon_bubble_container)
                            .width(Length::Fixed(44.0))
                            .height(Length::Fixed(44.0)),
                        // Content area
                        container(
                            row![
                                column![
                                    text(peer.addresses[0].to_string()).size(16),
                                    text(format!("ID: {}", &peer.id))
                                        .size(12)
                                        .style(text::secondary),
                                ]
                                .spacing(2)
                                .width(Length::Fill),
                                row![
                                    self.action_button(
                                        lucide::message_square(),
                                        Message::Navigation(NavigationMessage::Navigate(
                                            Route::Messages { peer_id: Some(peer.id.clone()) },
                                        )),
                                        false
                                    ),
                                    self.action_button(
                                        lucide::phone(),
                                        Message::CallAction(CallActionMessage::StartCall(
                                            CallTarget::PeerId(peer.id.clone()),
                                        )),
                                        false
                                    ),
                                    self.action_button(
                                        lucide::user_plus(),
                                        Message::ContactData(ContactsServiceMessage::SaveContact {
                                            peer_id: peer.id.clone(),
                                            name: peer.instance_name.clone(),
                                            address: peer
                                                .addresses
                                                .first()
                                                .map(|ip| format!("{}:{}", ip, peer.port)),
                                        }),
                                        true
                                    ),
                                ]
                                .spacing(8)
                            ]
                            .spacing(15)
                            .align_y(Alignment::Center)
                        )
                        .width(Length::Fill)
                    ]
                    .spacing(15)
                    .align_y(Alignment::Center),
                )
                .padding(12)
                .style(crate::ui::theme::card_container);

                peers_list = peers_list.push(peer_card);
            }
            nearby_section = nearby_section.push(peers_list);
        }

        nearby_section.into()
    }

    fn action_button<'a>(
        &self,
        icon: iced::widget::Text<'a>,
        msg: Message,
        is_primary: bool,
    ) -> iced::widget::Button<'a, Message> {
        button(container(icon.size(16)).center_x(Length::Fill).center_y(Length::Fill))
            .on_press(msg)
            .width(Length::Fixed(36.0))
            .height(Length::Fixed(36.0))
            .style(move |theme, status| theme::button_style(theme, status, is_primary))
    }
}
