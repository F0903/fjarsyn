use std::collections::HashSet;

use iced::{
    Alignment, Element, Length,
    widget::{Space, button, column, container, row, scrollable, text},
};
use iced_fonts::lucide;

use crate::{
    services::{
        contacts_service::Contact,
        messaging_service::{ConversationMessage, MessageDirection},
    },
    ui::{
        app::AppState,
        message::{Message, MessagingServiceMessage, NavigationMessage, Route},
        theme,
    },
};

#[derive(Debug, Clone)]
struct SidebarConversation {
    peer_id: String,
    title: String,
    subtitle: String,
    online: bool,
}

pub fn sidebar_button<'a>(
    active_route: Route,
    target_route: Route,
    icon: iced::widget::Text<'a>,
    label: &'a str,
    msg: Message,
) -> iced::widget::Button<'a, Message> {
    let is_active = active_route == target_route;

    button(
        row![icon.size(16), text(label).size(14)]
            .spacing(10)
            .align_y(Alignment::Center)
            .width(Length::Fill),
    )
    .width(Length::Fill)
    .on_press(msg)
    .style(move |theme, status| theme::sidebar_button_style(theme, status, is_active))
}

pub fn sidebar<'a>(
    ctx: &'a AppState,
    current_route: Route,
    local_id: Option<String>,
    selected_peer_id: Option<&'a str>,
) -> Element<'a, Message> {
    let conversations = build_sidebar_conversations(ctx, selected_peer_id);

    let sidebar_nav = column![
        sidebar_button(
            current_route,
            Route::Home,
            lucide::house(),
            "Home",
            Message::Navigation(NavigationMessage::Navigate(Route::Home))
        ),
        sidebar_button(
            current_route,
            Route::Messages,
            lucide::message_square(),
            "Messages",
            Message::Navigation(NavigationMessage::Navigate(Route::Messages))
        ),
        sidebar_button(
            current_route,
            Route::Contacts,
            lucide::users(),
            "Contacts",
            Message::Navigation(NavigationMessage::Navigate(Route::Contacts))
        ),
    ]
    .spacing(5);

    let conversations_header = row![
        text("CONVERSATIONS")
            .size(12)
            .style(text::secondary)
            .font(crate::ui::fonts::outfit::BOLD)
            .width(Length::Fill),
        button(lucide::user_plus().size(14))
            .on_press(Message::Navigation(NavigationMessage::Navigate(Route::Contacts)))
            .style(button::text)
    ]
    .spacing(10)
    .align_y(Alignment::Center);

    let mut conversations_list = column![conversations_header].spacing(8);
    if conversations.is_empty() {
        conversations_list = conversations_list.push(
            text("No conversations yet")
                .size(12)
                .style(text::secondary)
                .width(Length::Fill)
                .align_x(iced::alignment::Horizontal::Center),
        );
    } else {
        for conversation in conversations {
            let is_selected = current_route == Route::Messages
                && selected_peer_id == Some(conversation.peer_id.as_str());

            conversations_list = conversations_list.push(
                button(
                    row![
                        container(lucide::user().size(16))
                            .padding(8)
                            .style(theme::icon_bubble_container),
                        column![
                            text(conversation.title).size(14),
                            row![
                                container(Space::new().width(6)).width(6).height(6).style(
                                    move |_| container::Style {
                                        background: Some(
                                            if conversation.online {
                                                iced::Color::from_rgb(0.2, 0.8, 0.2)
                                            } else {
                                                iced::Color::from_rgb(0.5, 0.5, 0.5)
                                            }
                                            .into(),
                                        ),
                                        border: iced::Border {
                                            radius: 3.0.into(),
                                            ..Default::default()
                                        },
                                        ..Default::default()
                                    }
                                ),
                                text(conversation.subtitle).size(10).style(text::secondary),
                            ]
                            .spacing(5)
                            .align_y(Alignment::Center),
                        ]
                        .spacing(2),
                    ]
                    .spacing(10)
                    .align_y(Alignment::Center),
                )
                .width(Length::Fill)
                .on_press(Message::Messaging(MessagingServiceMessage::OpenConversation(
                    conversation.peer_id,
                )))
                .style(move |theme, status| {
                    theme::sidebar_button_style(theme, status, is_selected)
                }),
            );
        }
    }

    let my_id = {
        let local_id_value = local_id
            .map(|s| format!("{}...", crate::utils::string_utils::truncate(&s, 12)))
            .unwrap_or("Initializing...".to_owned());

        container(
            row![
                column![
                    text("YOUR ID").size(10).style(text::secondary),
                    text(local_id_value.clone()).size(12).style(text::primary),
                ]
                .width(Length::Fill),
                button(lucide::copy().size(14))
                    .on_press(Message::CopyId(local_id_value))
                    .style(button::text)
            ]
            .align_y(Alignment::Center)
            .spacing(5),
        )
        .padding(10)
        .style(theme::id_card_container)
    };

    container(
        column![
            sidebar_nav,
            container(scrollable(conversations_list)).height(Length::Fill),
            my_id,
            sidebar_button(
                current_route,
                Route::Settings,
                lucide::settings(),
                "Settings",
                Message::Navigation(NavigationMessage::Navigate(Route::Settings)),
            ),
        ]
        .padding(10)
        .spacing(15),
    )
    .width(Length::Fixed(240.0))
    .height(Length::Fill)
    .style(theme::sidebar_container)
    .into()
}

fn build_sidebar_conversations(
    ctx: &AppState,
    selected_peer_id: Option<&str>,
) -> Vec<SidebarConversation> {
    let contacts = ctx.services.contacts_service.as_ref().map(|service| service.contacts());
    let contacts = contacts.as_ref().map(|contacts| contacts.as_slice()).unwrap_or(&[]);
    let messages = ctx.services.messaging_service.as_ref().map(|service| service.messages());
    let messages = messages.as_ref().map(|messages| messages.as_slice()).unwrap_or(&[]);

    let mut conversations = Vec::new();
    let mut seen = HashSet::new();

    for message in messages.iter().rev() {
        if !seen.insert(message.peer_id.clone()) {
            continue;
        }

        conversations.push(build_sidebar_conversation(
            ctx,
            contacts,
            message.peer_id.clone(),
            Some(message),
        ));
    }

    if let Some(selected_peer_id) = selected_peer_id
        && seen.insert(selected_peer_id.to_string())
    {
        conversations.push(build_sidebar_conversation(
            ctx,
            contacts,
            selected_peer_id.to_string(),
            None,
        ));
    }

    conversations
}

fn build_sidebar_conversation(
    ctx: &AppState,
    contacts: &[Contact],
    peer_id: String,
    last_message: Option<&ConversationMessage>,
) -> SidebarConversation {
    let discovered = ctx.networking.discovered_peers.iter().find(|peer| peer.id == peer_id);
    let contact = contacts.iter().find(|contact| contact.peer_id == peer_id);

    SidebarConversation {
        title: contact
            .map(|contact| contact.name.clone())
            .or_else(|| {
                discovered
                    .map(|peer| peer.instance_name.trim().to_string())
                    .filter(|name| !name.is_empty())
            })
            .unwrap_or_else(|| crate::utils::string_utils::truncate(&peer_id, 12).to_string()),
        subtitle: last_message.map(sidebar_preview).unwrap_or_else(|| {
            if discovered.is_some() { "Online" } else { "No messages yet" }.into()
        }),
        online: discovered.is_some(),
        peer_id,
    }
}

fn sidebar_preview(message: &ConversationMessage) -> String {
    let prefix = if matches!(message.direction, MessageDirection::Outgoing) { "You: " } else { "" };
    let body = if message.body.chars().count() <= 22 {
        message.body.clone()
    } else {
        format!("{}...", message.body.chars().take(22).collect::<String>())
    };

    format!("{}{}", prefix, body)
}
