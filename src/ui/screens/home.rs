use iced::{
    Alignment, Element, Length, Subscription, Task,
    widget::{button, column, container, row, scrollable, text, text_input},
};
use iced_fonts::lucide;

use super::Screen;
use crate::ui::{
    fonts,
    message::{CallTarget, Message},
    state::AppContext,
    theme,
};

#[derive(Debug, Clone)]
pub enum HomeMessage {
    CopyId(String),
    TargetAddressChanged(String),
}

#[derive(Debug, Clone)]
pub struct HomeScreen {
    manual_target_address: String,
}

impl HomeScreen {
    pub fn new(_ctx: &mut AppContext) -> Self {
        Self { manual_target_address: String::new() }
    }
}

impl Screen for HomeScreen {
    fn subscription(&self, _ctx: &AppContext) -> Subscription<Message> {
        Subscription::none()
    }

    fn update(&mut self, _ctx: &mut AppContext, message: Message) -> Task<Message> {
        match message {
            Message::Home(msg) => match msg {
                HomeMessage::TargetAddressChanged(val) => {
                    self.manual_target_address = val;
                    Task::none()
                }
                HomeMessage::CopyId(id) => iced::clipboard::write(id),
            },
            _ => Task::none(),
        }
    }

    fn view<'a>(&'a self, ctx: &'a AppContext) -> Element<'a, Message> {
        let title = text("Discovery").size(32).style(text::primary).font(fonts::outfit::BOLD);

        let mut content = column![title].spacing(30);

        // Call by Address section
        let call_by_addr = container(
            column![
                text("Manual Call").size(18),
                row![
                    text_input(
                        "Enter IP:Port (e.g. 192.168.1.50:8080)...",
                        &self.manual_target_address
                    )
                    .on_input(|val| Message::Home(HomeMessage::TargetAddressChanged(val)))
                    .padding(10)
                    .style(theme::text_input_style),
                    button(row![lucide::phone().size(16), text("Call")].spacing(10))
                        .on_press(Message::StartCall(CallTarget::Address(
                            self.manual_target_address.clone()
                        )))
                        .padding(10)
                        .style(|theme, status| theme::button_style(theme, status, true)),
                ]
                .spacing(10)
            ]
            .spacing(10),
        )
        .padding(20)
        .style(crate::ui::theme::card_container);

        content = content.push(call_by_addr);

        let mut nearby_section = column![
            row![lucide::antenna().size(20), text("Nearby Peers").size(20)]
                .spacing(10)
                .align_y(Alignment::Center)
        ]
        .spacing(15);

        if ctx.discovered_peers.is_empty() {
            nearby_section = nearby_section.push(
                container(text("Searching for peers on your local network...").size(14))
                    .padding(20)
                    .width(Length::Fill)
                    .align_x(Alignment::Center),
            );
        } else {
            let mut peers_list = column![].spacing(10);
            for peer in &ctx.discovered_peers {
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
                                    action_button(lucide::message_square(), Message::NoOp, false),
                                    action_button(
                                        lucide::phone(),
                                        Message::StartCall(CallTarget::PeerId(peer.id.clone())),
                                        false
                                    ),
                                    action_button(
                                        lucide::user_plus(),
                                        Message::SaveContact {
                                            peer_id: peer.id.clone(),
                                            name: peer.instance_name.clone(),
                                            address: peer
                                                .addresses
                                                .first()
                                                .map(|ip| format!("{}:{}", ip, peer.port)),
                                        },
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

        content = content.push(nearby_section);

        container(scrollable(content)).width(Length::Fill).height(Length::Fill).padding(20).into()
    }
}

fn action_button<'a>(
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
