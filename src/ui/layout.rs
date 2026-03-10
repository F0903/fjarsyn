use iced::{
    Alignment, Color, Element, Length, Padding, mouse,
    widget::{Space, button, column, container, mouse_area, row, scrollable, stack, text},
    window,
};
use iced_fonts::lucide;

use crate::ui::{
    fonts,
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

pub fn render_window_controls<'a>(is_maximized: bool) -> Element<'a, Message> {
    let control_button = |icon: iced::widget::Text<'a>, msg: Message, hover: Option<Color>| {
        button(
            container(icon.size(10))
                .width(Length::Fill)
                .height(Length::Fill)
                .center_x(Length::Fill)
                .center_y(Length::Fill),
        )
        .on_press(msg)
        .style(move |theme, status| theme::window_control_style(theme, status, hover))
        .width(23)
        .height(23)
        .padding(0)
    };

    row![
        control_button(lucide::minus(), Message::Minimize, None),
        control_button(
            if is_maximized { lucide::copy() } else { lucide::maximize() },
            Message::Maximize,
            None
        ),
        control_button(lucide::x(), Message::Close, Some(theme::CONTROL_CLOSE_HOVER)),
    ]
    .spacing(10)
    .align_y(Alignment::Center)
    .into()
}

pub fn render_titlebar<'a>(_ctx: &AppContext) -> Element<'a, Message> {
    let title = container(text(APP_TITLE).size(12).style(text::primary).font(fonts::outfit::BOLD))
        .width(Length::Fill)
        .height(Length::Fill)
        .center_y(Length::Fill);

    mouse_area(
        container(title)
            .width(Length::Fill)
            .height(Length::Fixed(40.0))
            .padding(Padding::from([0, 15]))
            .style(theme::titlebar_container),
    )
    .on_press(Message::Drag)
    .on_double_click(Message::Maximize)
    .into()
}

pub fn render_resize_grid<'a>() -> Element<'a, Message> {
    let handle_size = 5.0;
    let corner_size = handle_size;

    let resize_handle =
        |direction: window::Direction, width: Length, height: Length| -> Element<'a, Message> {
            mouse_area(container(Space::new()).width(width).height(height))
                .on_press(Message::Resize(direction))
                .interaction(match direction {
                    window::Direction::North | window::Direction::South => {
                        mouse::Interaction::ResizingVertically
                    }
                    window::Direction::West | window::Direction::East => {
                        mouse::Interaction::ResizingHorizontally
                    }
                    window::Direction::NorthWest | window::Direction::SouthEast => {
                        mouse::Interaction::ResizingDiagonallyDown
                    }
                    window::Direction::NorthEast | window::Direction::SouthWest => {
                        mouse::Interaction::ResizingDiagonallyUp
                    }
                })
                .into()
        };

    column![
        row![
            resize_handle(window::Direction::NorthWest, corner_size.into(), corner_size.into()),
            resize_handle(window::Direction::North, Length::Fill, handle_size.into()),
            resize_handle(window::Direction::NorthEast, corner_size.into(), corner_size.into()),
        ]
        .spacing(0)
        .align_y(Alignment::Start),
        row![
            resize_handle(window::Direction::West, handle_size.into(), Length::Fill),
            Space::new().width(Length::Fill).height(Length::Fill),
            resize_handle(window::Direction::East, handle_size.into(), Length::Fill),
        ]
        .height(Length::Fill)
        .spacing(0),
        row![
            resize_handle(window::Direction::SouthWest, corner_size.into(), corner_size.into()),
            resize_handle(window::Direction::South, Length::Fill, handle_size.into()),
            resize_handle(window::Direction::SouthEast, corner_size.into(), corner_size.into()),
        ]
        .spacing(0)
        .align_y(Alignment::End),
    ]
    .width(Length::Fill)
    .height(Length::Fill)
    .spacing(0)
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

    let chats_header = row![
        text("CHATS").size(12).style(text::secondary).font(fonts::outfit::BOLD).width(Length::Fill),
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
                .on_press(Message::StartCall(crate::ui::message::CallTarget::PeerId(
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

    let titlebar = render_titlebar(&state.ctx);

    let mut main_layout: Element<'a, Message, iced::Theme, iced::Renderer> =
        match state.active_screen {
            ActiveScreen::Call(_) => column![titlebar, screen_content].into(),
            _ => {
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
        main_layout = stack![main_layout, popup].into();
    }

    let notifications = state.ctx.notifications.view();

    let is_maximized = state.ctx.main_window.as_ref().map(|w| w.maximized).unwrap_or(false);

    let controls = container(render_window_controls(is_maximized))
        .width(Length::Fill)
        .height(Length::Fixed(40.0))
        .padding(Padding::from([0, 15]))
        .align_x(Alignment::End)
        .align_y(Alignment::Center);

    let content_stack = stack![main_layout, notifications, controls];

    if is_maximized {
        content_stack.into()
    } else {
        stack![content_stack, render_resize_grid()].into()
    }
}
