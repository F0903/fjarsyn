use std::collections::HashSet;

use fjarsyn_core::{
    services::{
        contacts_service::Contact,
        messaging_service::{ConversationSummary, MessageDirection},
    },
    utils::text::{abbreviate_middle, truncate},
};
use iced::{
    Alignment, Element, Length,
    widget::{Space, button, column, container, row, scrollable, text},
};
use iced_fonts::lucide;

use crate::ui::{
    message::{Message, NavigationMessage, Route},
    shell::ShellContext,
    theme,
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
    msg: Option<Message>,
) -> iced::widget::Button<'a, Message> {
    let is_active = active_route.same_screen(&target_route);

    let mut nav_button = button(
        row![icon.size(16), text(label).size(14)]
            .spacing(10)
            .align_y(Alignment::Center)
            .width(Length::Fill),
    )
    .width(Length::Fill)
    .style(move |theme, status| theme::sidebar_button_style(theme, status, is_active));

    if let Some(msg) = msg {
        nav_button = nav_button.on_press(msg);
    }

    nav_button
}

pub fn sidebar<'a>(ctx: ShellContext<'a>, current_route: Route) -> Element<'a, Message> {
    let selected_peer_id = ctx.messaging.active_peer_id.as_deref();
    let conversations = build_sidebar_conversations(ctx, selected_peer_id);
    let contacts_available = ctx.can_use_contacts();
    let messaging_available = ctx.can_use_messaging();

    let sidebar_nav = column![
        sidebar_button(
            current_route.clone(),
            Route::Home,
            lucide::house(),
            "Home",
            Some(Message::Navigation(NavigationMessage::Navigate(Route::Home)))
        ),
        sidebar_button(
            current_route.clone(),
            Route::Contacts,
            lucide::users(),
            "Contacts",
            contacts_available
                .then(|| { Message::Navigation(NavigationMessage::Navigate(Route::Contacts)) })
        ),
    ]
    .spacing(5);

    let mut add_contact_button = button(lucide::user_plus().size(14)).style(button::text);
    if contacts_available {
        add_contact_button = add_contact_button
            .on_press(Message::Navigation(NavigationMessage::Navigate(Route::Contacts)));
    }

    let conversations_header = row![
        text("CONVERSATIONS")
            .size(12)
            .style(text::secondary)
            .font(crate::ui::fonts::outfit::BOLD)
            .width(Length::Fill),
        add_contact_button
    ]
    .spacing(10)
    .align_y(Alignment::Center);

    let mut conversations_list = column![conversations_header].spacing(8);
    if !messaging_available {
        conversations_list = conversations_list.push(
            text(messaging_unavailable_text(ctx))
                .size(12)
                .style(text::secondary)
                .width(Length::Fill)
                .align_x(iced::alignment::Horizontal::Center),
        );
    } else if conversations.is_empty() {
        conversations_list = conversations_list.push(
            text("No conversations yet")
                .size(12)
                .style(text::secondary)
                .width(Length::Fill)
                .align_x(iced::alignment::Horizontal::Center),
        );
    } else {
        for conversation in conversations {
            let is_selected = matches!(&current_route, Route::Messages { .. })
                && selected_peer_id == Some(conversation.peer_id.as_str());
            let peer_id = conversation.peer_id.clone();
            let mut conversation_button = button(
                row![
                    container(lucide::user().size(16))
                        .padding(8)
                        .style(theme::icon_bubble_container),
                    column![
                        text(conversation.title).size(14),
                        row![
                            container(Space::new().width(6)).width(6).height(6).style(move |_| {
                                container::Style {
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
                            }),
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
            .style(move |theme, status| theme::sidebar_button_style(theme, status, is_selected));

            if messaging_available {
                conversation_button = conversation_button.on_press(Message::Navigation(
                    NavigationMessage::Navigate(Route::Messages { peer_id: Some(peer_id) }),
                ));
            }

            conversations_list = conversations_list.push(conversation_button);
        }
    }

    let my_id = {
        let local_id =
            ctx.networking.local_peer_id.clone().or_else(|| ctx.config.identity.peer_id.clone());
        let local_id_display = local_id
            .as_deref()
            .map(|id| format!("{}...", truncate(id, 12)))
            .unwrap_or("Initializing...".to_owned());

        container(
            row![
                column![
                    text("YOUR ID").size(10).style(text::secondary),
                    text(local_id_display.clone()).size(12).style(text::primary),
                ]
                .width(Length::Fill),
                button(lucide::copy().size(14))
                    .on_press(Message::CopyId(local_id.unwrap_or(local_id_display)))
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
                Some(Message::Navigation(NavigationMessage::Navigate(Route::Settings))),
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

fn messaging_unavailable_text(ctx: ShellContext<'_>) -> &'static str {
    if !ctx.accepts_user_requests() {
        "Messaging is unavailable while the app is shutting down"
    } else {
        "Messaging is unavailable until the service is ready"
    }
}

fn build_sidebar_conversations(
    ctx: ShellContext<'_>,
    selected_peer_id: Option<&str>,
) -> Vec<SidebarConversation> {
    let contacts = ctx.contacts.contacts.as_slice();
    let summaries = ctx.messaging.summaries.as_slice();

    let mut conversations = Vec::new();
    let mut seen = HashSet::new();

    for summary in summaries {
        if !seen.insert(summary.peer_id.clone()) {
            continue;
        }

        conversations.push(build_sidebar_conversation(
            ctx,
            contacts,
            summary.peer_id.clone(),
            Some(summary),
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
    ctx: ShellContext<'_>,
    contacts: &[Contact],
    peer_id: String,
    summary: Option<&ConversationSummary>,
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
            .unwrap_or_else(|| abbreviate_middle(&peer_id, 14, 6)),
        subtitle: summary.map(sidebar_preview).unwrap_or_else(|| {
            if discovered.is_some() { "Online" } else { "No messages yet" }.into()
        }),
        online: discovered.is_some(),
        peer_id,
    }
}

fn sidebar_preview(summary: &ConversationSummary) -> String {
    let prefix = if matches!(summary.last_message_direction, MessageDirection::Outgoing) {
        "You: "
    } else {
        ""
    };
    let body = if summary.last_message_body.chars().count() <= 22 {
        summary.last_message_body.clone()
    } else {
        format!("{}...", summary.last_message_body.chars().take(22).collect::<String>())
    };

    format!("{}{}", prefix, body)
}
