//! Application navigation, contact status, and local identity sidebar.

use iced::{
    Element, Length,
    widget::{column, container, scrollable},
};

use crate::ui::{
    message::{Message, Route},
    presentation::Context,
    theme,
};

mod contacts;
mod identity_card;
mod navigation;

pub(in crate::ui) fn sidebar<'a>(
    context: Context<'a>,
    current_route: Route,
) -> Element<'a, Message> {
    container(
        column![
            navigation::primary(&current_route),
            container(scrollable(contacts::contact_list(context, &current_route)))
                .height(Length::Fill),
            identity_card::identity_card(context),
            navigation::settings(&current_route),
        ]
        .padding(10)
        .spacing(15),
    )
    .width(Length::Fixed(240.0))
    .height(Length::Fill)
    .style(theme::sidebar_container)
    .into()
}

fn truncate_with_ellipsis(value: &str, length: usize) -> String {
    if value.chars().count() <= length {
        value.to_owned()
    } else {
        let truncated =
            value.char_indices().nth(length).map_or(value, |(index, _)| &value[..index]);
        format!("{truncated}...")
    }
}
