use iced::{Theme, widget::text_input};

use super::palette::{BORDER_COLOR, LIGHTER_RADIUS, TEXT_SECONDARY};

pub fn text_input_style(theme: &Theme, _status: text_input::Status) -> text_input::Style {
    let palette = theme.extended_palette();

    text_input::Style {
        background: iced::Color { a: 0.05, ..palette.background.base.text }.into(),
        border: iced::Border { color: BORDER_COLOR, width: 1.0, radius: LIGHTER_RADIUS.into() },
        icon: iced::Color { a: 0.5, ..palette.background.base.text },
        placeholder: TEXT_SECONDARY,
        value: palette.background.base.text,
        selection: iced::Color { a: 0.2, ..palette.primary.base.color },
    }
}
