use iced::{Element, Length, widget::container};

use crate::ui::{message::Message, theme};

pub fn vertical_spacer<'a>() -> Element<'a, Message> {
    container(container("").width(Length::Fill).height(1).style(theme::spacer_style)).into()
}
