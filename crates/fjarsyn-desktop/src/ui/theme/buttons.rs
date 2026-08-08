use iced::{Border, Color, Theme, widget::button};

use super::palette::{LIGHT_RADIUS, LIGHTER_RADIUS, TEXT_PRIMARY, TEXT_SECONDARY};

pub(in crate::ui) fn sidebar_button_style(
    theme: &Theme,
    status: button::Status,
    is_active: bool,
) -> button::Style {
    let palette = theme.extended_palette();

    let base = button::Style {
        background: if is_active {
            Some(Color { a: 0.1, ..palette.background.base.text }.into())
        } else {
            None
        },
        text_color: if is_active { palette.background.base.text } else { TEXT_SECONDARY },
        border: Border { radius: LIGHTER_RADIUS.into(), ..Default::default() },
        ..Default::default()
    };

    match status {
        button::Status::Hovered => button::Style {
            background: Some(Color { a: 0.05, ..palette.background.base.text }.into()),
            text_color: palette.background.base.text,
            ..base
        },
        button::Status::Pressed => button::Style {
            background: Some(Color { a: 0.15, ..palette.background.base.text }.into()),
            ..base
        },
        _ => base,
    }
}

pub(in crate::ui) fn window_control_style(
    _theme: &Theme,
    status: button::Status,
    hover_color: Option<Color>,
) -> button::Style {
    let base = button::Style {
        border: Border { radius: LIGHT_RADIUS.into(), ..Default::default() },
        ..Default::default()
    };

    match status {
        button::Status::Hovered => button::Style {
            background: Some(hover_color.unwrap_or(Color { a: 0.1, ..Color::WHITE }).into()),
            text_color: TEXT_PRIMARY,
            ..base
        },
        button::Status::Pressed => {
            button::Style { background: Some(Color { a: 0.2, ..Color::WHITE }.into()), ..base }
        }
        _ => button::Style { text_color: TEXT_SECONDARY, ..Default::default() },
    }
}

pub(in crate::ui) fn button_style(
    theme: &Theme,
    status: button::Status,
    is_primary: bool,
) -> button::Style {
    let palette = theme.extended_palette();

    let base = button::Style {
        background: if is_primary {
            Some(palette.primary.base.color.into())
        } else {
            Some(Color { a: 0.1, ..palette.background.base.text }.into())
        },
        text_color: if is_primary {
            palette.primary.base.text
        } else {
            palette.background.base.text
        },
        border: Border { radius: LIGHT_RADIUS.into(), ..Default::default() },
        ..Default::default()
    };

    match status {
        button::Status::Hovered => button::Style {
            background: if is_primary {
                Some(Color { r: 1.0, g: 0.6, b: 0.3, a: 1.0 }.into())
            } else {
                Some(Color { a: 0.15, ..palette.background.base.text }.into())
            },
            ..base
        },
        button::Status::Pressed => button::Style {
            background: if is_primary {
                Some(Color { r: 0.9, g: 0.4, b: 0.1, a: 1.0 }.into())
            } else {
                Some(Color { a: 0.2, ..palette.background.base.text }.into())
            },
            ..base
        },
        _ => base,
    }
}

pub(in crate::ui) fn danger_button_style(theme: &Theme, status: button::Status) -> button::Style {
    let palette = theme.extended_palette();

    let base = button::Style {
        background: Some(palette.danger.base.color.into()),
        text_color: palette.danger.base.text,
        border: Border { radius: LIGHT_RADIUS.into(), ..Default::default() },
        ..Default::default()
    };

    match status {
        button::Status::Hovered => button::Style {
            background: Some(Color { r: 0.9, g: 0.3, b: 0.3, a: 1.0 }.into()),
            ..base
        },
        button::Status::Pressed => button::Style {
            background: Some(Color { r: 0.7, g: 0.1, b: 0.1, a: 1.0 }.into()),
            ..base
        },
        _ => base,
    }
}
