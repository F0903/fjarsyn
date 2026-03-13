use iced::{
    Alignment, Element, Length,
    widget::{Space, button, column, container, row, scrollable, text},
};
use iced_fonts::lucide;

use crate::ui::{
    app::AppContext,
    message::{Message, Route},
    theme,
};

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

pub fn sidebar<'a>(ctx: &'a AppContext, current_route: Route) -> Element<'a, Message> {
    let sidebar_nav = column![
        sidebar_button(
            current_route,
            Route::Home,
            lucide::house(),
            "Home",
            Message::Navigate(Route::Home)
        ),
        sidebar_button(
            current_route,
            Route::Contacts,
            lucide::users(),
            "Contacts",
            Message::Navigate(Route::Contacts)
        ),
    ]
    .spacing(5);

    let contacts_header = row![
        text("CONTACTS")
            .size(12)
            .style(text::secondary)
            .font(crate::ui::fonts::outfit::BOLD)
            .width(Length::Fill),
        button(lucide::user_plus().size(14))
            .on_press(Message::Navigate(Route::Contacts))
            .style(button::text)
    ]
    .spacing(10)
    .align_y(Alignment::Center);

    let mut contacts_list = column![contacts_header].spacing(8);
    if ctx.contacts.is_empty() {
        contacts_list = contacts_list.push(
            text("No contacts")
                .size(12)
                .style(text::secondary)
                .width(Length::Fill)
                .align_x(iced::alignment::Horizontal::Center),
        );
    } else {
        for contact in &ctx.contacts {
            let is_online = ctx.discovered_peers.iter().any(|p| p.id == contact.peer_id);

            contacts_list = contacts_list.push(
                button(
                    row![
                        container(lucide::user().size(16))
                            .padding(8)
                            .style(theme::icon_bubble_container),
                        column![
                            text(&contact.name).size(14),
                            row![
                                container(Space::new().width(6)).width(6).height(6).style(
                                    move |_| container::Style {
                                        background: Some(
                                            if is_online {
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
                                text(if is_online { "Online" } else { "Offline" })
                                    .size(10)
                                    .style(text::secondary),
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
                .on_press(Message::StartCall(crate::ui::message::CallTarget::ContactId(contact.id)))
                .style(button::text),
            );
        }
    }

    let my_id = if let Some(service) = &ctx.call_service {
        let id = service.webrtc().get_local_id();
        let id_clone = id.clone();
        container(
            row![
                column![
                    text("YOUR ID").size(10).style(text::secondary),
                    text(format!("{}...", crate::utils::string_utils::truncate(&id_clone, 12)))
                        .size(12)
                        .style(text::primary),
                ]
                .width(Length::Fill),
                button(lucide::copy().size(14)).on_press(Message::CopyId(id)).style(button::text)
            ]
            .align_y(Alignment::Center)
            .spacing(5),
        )
        .padding(10)
        .style(theme::id_card_container)
    } else {
        container(text("Initializing...").size(12).style(text::secondary))
    };

    container(
        column![
            sidebar_nav,
            container(scrollable(contacts_list)).height(Length::Fill),
            my_id,
            sidebar_button(
                current_route,
                Route::Settings,
                lucide::settings(),
                "Settings",
                Message::Navigate(Route::Settings),
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
