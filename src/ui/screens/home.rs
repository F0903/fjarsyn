use iced::{
    Alignment, Element, Length, Subscription, Task,
    widget::{button, column, container, row, scrollable, text, text_input},
};
use iced_fonts::lucide;

use super::Screen;
use crate::{
    networking::webrtc::WebRTCEvent,
    ui::{
        message::{Message, Route},
        state::AppContext,
    },
};

#[derive(Debug, Clone)]
pub enum HomeMessage {
    StartCall(String),
    AcceptCall,
    DeclineCall,
    CopyId(String),
    TargetIdChanged(String),
}

#[derive(Debug, Clone)]
pub struct HomeScreen {
    incoming_call: Option<String>,
    manual_target_id: String,
}

impl HomeScreen {
    pub fn new(_ctx: &mut AppContext) -> Self {
        Self { incoming_call: None, manual_target_id: String::new() }
    }
}

impl Screen for HomeScreen {
    fn subscription(&self, _ctx: &AppContext) -> Subscription<Message> {
        Subscription::none()
    }

    fn update(&mut self, ctx: &mut AppContext, message: Message) -> Task<Message> {
        match message {
            Message::WebRTCEvent(WebRTCEvent::IncomingCall(sender)) => {
                self.incoming_call = Some(sender);
                Task::none()
            }
            Message::WebRTCEvent(WebRTCEvent::Disconnected) => {
                self.incoming_call = None;
                Task::none()
            }
            Message::Home(msg) => match msg {
                HomeMessage::TargetIdChanged(val) => {
                    self.manual_target_id = val;
                    Task::none()
                }
                HomeMessage::StartCall(target_id) => {
                    if let Some(webrtc) = &ctx.webrtc {
                        let webrtc_clone = webrtc.clone();
                        let discovered_peer =
                            ctx.discovered_peers.iter().find(|p| p.id == target_id).cloned();

                        Task::future(async move {
                            if let Some(peer) = discovered_peer {
                                let mut success = false;
                                for addr in &peer.addresses {
                                    let socket_addr = std::net::SocketAddr::new(*addr, peer.port);
                                    tracing::info!(
                                        "Attempting direct signaling to {}",
                                        socket_addr
                                    );
                                    if let Ok(_) = webrtc_clone.dial_direct(socket_addr).await {
                                        tracing::info!(
                                            "Successfully connected to signaling at {}",
                                            socket_addr
                                        );
                                        success = true;
                                        break;
                                    }
                                }

                                if !success {
                                    tracing::error!(
                                        "Failed to connect to any address for peer {}",
                                        target_id
                                    );
                                    return Message::NoOp;
                                }
                            }

                            match webrtc_clone.create_offer().await {
                                Ok(_) => Message::Navigate(Route::Call),
                                Err(e) => {
                                    tracing::error!("Failed to create offer: {}", e);
                                    Message::NoOp
                                }
                            }
                        })
                    } else {
                        tracing::warn!("WebRTC not initialized...");
                        Task::none()
                    }
                }
                HomeMessage::AcceptCall => {
                    if let Some(webrtc) = &ctx.webrtc {
                        let webrtc_clone = webrtc.clone();
                        Task::future(async move {
                            match webrtc_clone.accept_call().await {
                                Ok(_) => Message::Navigate(Route::Call),
                                Err(e) => {
                                    tracing::error!("Failed to accept call: {}", e);
                                    Message::NoOp
                                }
                            }
                        })
                    } else {
                        Task::none()
                    }
                }
                HomeMessage::DeclineCall => {
                    self.incoming_call = None;
                    if let Some(webrtc) = &ctx.webrtc {
                        let webrtc_clone = webrtc.clone();
                        Task::future(async move {
                            let _ = webrtc_clone.decline_call().await;
                            Message::NoOp
                        })
                    } else {
                        Task::none()
                    }
                }
                HomeMessage::CopyId(id) => iced::clipboard::write(id),
            },
            _ => Task::none(),
        }
    }

    fn view<'a>(&'a self, ctx: &'a AppContext) -> Element<'a, Message> {
        let title = text("Discovery").size(32).style(text::primary);

        let mut content = column![title].spacing(30);

        // Call by ID section
        let call_by_id = container(
            column![
                text("Call by ID").size(18),
                row![
                    text_input("Enter Peer ID...", &self.manual_target_id)
                        .on_input(|val| Message::Home(HomeMessage::TargetIdChanged(val)))
                        .padding(10),
                    button(row![lucide::phone().size(16), text("Call")].spacing(10))
                        .on_press(Message::Home(HomeMessage::StartCall(
                            self.manual_target_id.clone()
                        )))
                        .padding(10)
                        .style(button::primary),
                ]
                .spacing(10)
            ]
            .spacing(10),
        )
        .padding(20)
        .style(crate::ui::theme::card_container);

        content = content.push(call_by_id);

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
                        container(lucide::user().size(20))
                            .width(Length::Fixed(44.0))
                            .height(Length::Fixed(44.0))
                            .center_x(Length::Fill)
                            .center_y(Length::Fill)
                            .style(crate::ui::theme::icon_bubble_container),
                        column![
                            text(&peer.instance_name).size(16),
                            text(format!("ID: {}", &peer.id)).size(12).style(text::secondary),
                        ]
                        .spacing(2)
                        .width(Length::Fill),
                        row![
                            action_button(lucide::message_square(), Message::NoOp, false),
                            action_button(
                                lucide::phone(),
                                Message::Home(HomeMessage::StartCall(peer.id.clone())),
                                false
                            ),
                            action_button(
                                lucide::user_plus(),
                                Message::Navigate(Route::Contacts),
                                true
                            ),
                        ]
                        .spacing(8)
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
        .style(if is_primary { button::primary } else { button::secondary })
}
