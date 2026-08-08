//! Fjarsyn palette and reusable Iced widget styles.

use iced::{Theme, widget::text_input};

mod buttons;
mod containers;
mod palette;

pub(in crate::ui) use buttons::{
    button_style, danger_button_style, sidebar_button_style, window_control_style,
};
pub(in crate::ui) use containers::{
    card_container, icon_bubble_container, id_card_container, main_content_container,
    notification_container, section_container, sidebar_container, titlebar_container,
    warning_accent_container,
};
pub(in crate::ui) use palette::{
    BORDER_COLOR, CARD_BACKGROUND, CONTROL_CLOSE_HOVER, PRIMARY_COLOR, fjarsyn_theme,
};

pub(in crate::ui) fn text_input_style(
    theme: &Theme,
    _status: text_input::Status,
) -> text_input::Style {
    let palette = theme.extended_palette();

    text_input::Style {
        background: iced::Color { a: 0.05, ..palette.background.base.text }.into(),
        border: iced::Border {
            color: palette::BORDER_COLOR,
            width: 1.0,
            radius: palette::LIGHTER_RADIUS.into(),
        },
        icon: iced::Color { a: 0.5, ..palette.background.base.text },
        placeholder: palette::TEXT_SECONDARY,
        value: palette.background.base.text,
        selection: iced::Color { a: 0.2, ..palette.primary.base.color },
    }
}
