use iced::{
    Element, Length, Padding,
    widget::{button, column, container, row, scrollable, stack, text},
    window,
};
use iced_fonts::lucide;

use crate::ui::{
    message::{Message, Route},
    screens::{ActiveScreen, Screen},
    state::{AppContext, State},
    theme,
};

pub const APP_TITLE: &'static str = "Fjarsyn";

pub fn title(_state: &State, _window: window::Id) -> String {
    APP_TITLE.to_string()
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
            .align_y(iced::Alignment::Center)
            .width(Length::Fill),
    )
    .width(Length::Fill)
    .on_press(msg)
    .style(move |theme, status| theme::sidebar_button_style(theme, status, is_active))
}

pub fn render_titlebar<'a>() -> Element<'a, Message> {
    container(
        row![text(APP_TITLE).size(18).width(Length::Fill)]
            .padding(Padding::from([5, 15]))
            .align_y(iced::Alignment::Center),
    )
    .width(Length::Fill)
    .style(theme::titlebar_container)
    .into()
}

pub fn render_sidebar<'a>(ctx: &'a AppContext, current_route: Route) -> Element<'a, Message> {
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

    // Recent conversations/peers (Discord style)
    let chats_header = row![
        text("CHATS").size(12).style(text::secondary).width(Length::Fill),
        button(lucide::user_plus().size(14))
            .on_press(Message::Navigate(Route::Contacts))
            .style(button::text)
    ]
    .spacing(10)
    .align_y(iced::Alignment::Center);

    let mut recent_peers_list = column![chats_header].spacing(8);
    if ctx.recent_peers.is_empty() {
        recent_peers_list = recent_peers_list.push(
            text("No recent chats")
                .size(12)
                .style(text::secondary)
                .width(Length::Fill)
                .align_x(iced::alignment::Horizontal::Center),
        );
    } else {
        for peer in &ctx.recent_peers {
            recent_peers_list = recent_peers_list.push(
                button(
                    row![
                        container(lucide::user().size(16))
                            .padding(8)
                            .style(theme::icon_bubble_container),
                        column![
                            text(&peer.instance_name).size(14),
                            text(&peer.id[..8]).size(10).style(text::secondary),
                        ]
                        .spacing(2),
                    ]
                    .spacing(10)
                    .align_y(iced::Alignment::Center),
                )
                .width(Length::Fill)
                .on_press(Message::Home(crate::ui::screens::home::HomeMessage::StartCall(
                    peer.id.clone(),
                )))
                .style(button::text),
            );
        }
    }

    let my_id = if let Some(webrtc) = &ctx.webrtc {
        let id = webrtc.get_local_id();
        let id_clone = id.clone();
        container(
            row![
                column![
                    text("YOUR ID").size(10).style(text::secondary),
                    text(format!("{}...", &id_clone[..12])).size(12).style(text::primary),
                ]
                .width(Length::Fill),
                button(lucide::copy().size(14))
                    .on_press(Message::Home(crate::ui::screens::home::HomeMessage::CopyId(id,)))
                    .style(button::text)
            ]
            .align_y(iced::Alignment::Center)
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
            container(scrollable(recent_peers_list)).height(Length::Fill),
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

pub fn render_incoming_call_popup<'a>(ctx: &'a AppContext) -> Element<'a, Message> {
    if let Some(ref sender_id) = ctx.incoming_call_id {
        let sender_name = ctx
            .discovered_peers
            .iter()
            .find(|p| p.id == *sender_id)
            .map(|p| p.instance_name.clone())
            .unwrap_or_else(|| format!("{}...", &sender_id[..8]));

        container(
            container(
                column![
                    text("Incoming Call").size(14).style(text::secondary),
                    text(sender_name).size(20).style(text::primary),
                    row![
                        button(row![lucide::phone_incoming().size(16), text("Accept")].spacing(10))
                            .on_press(Message::AcceptCall)
                            .style(button::success)
                            .padding(10),
                        button(row![lucide::phone_off().size(16), text("Decline")].spacing(10))
                            .on_press(Message::DeclineCall)
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
    } else {
        column![].into()
    }
}

pub fn view<'a>(
    state: &'a State,
    _window: window::Id,
) -> Element<'a, Message, iced::Theme, iced::Renderer> {
    let screen_content = state.active_screen.view(&state.ctx);
    let current_route = state.active_screen.get_route();

    let mut layout = match state.active_screen {
        ActiveScreen::Call(_) => screen_content,
        _ => {
            let titlebar = render_titlebar();
            let sidebar = render_sidebar(&state.ctx, current_route);
            let main_content = container(screen_content)
                .width(Length::Fill)
                .height(Length::Fill)
                .style(theme::main_content_container);

            column![titlebar, row![sidebar, main_content]].into()
        }
    };

    if state.ctx.incoming_call_id.is_some() {
        let popup = render_incoming_call_popup(&state.ctx);
        layout = stack![layout, popup].into();
    }

    stack![layout, state.ctx.notifications.view()].into()
}
