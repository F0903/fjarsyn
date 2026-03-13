use iced::{
    Border, Color, Shadow, Theme, Vector,
    theme::Palette,
    widget::{button, container, text_input},
};

use crate::services::notification_service::NotificationKind;

// --- Palette Constants ---
pub const PRIMARY_COLOR: Color = Color::from_rgb(1.0, 0.5, 0.2);
pub const BACKGROUND_COLOR: Color = Color::from_rgb(0.05, 0.04, 0.03);
pub const CARD_BACKGROUND: Color = Color::from_rgb(0.1, 0.09, 0.08);
pub const SIDEBAR_BACKGROUND: Color = Color::from_rgb(0.08, 0.07, 0.06);
pub const BORDER_COLOR: Color = Color::from_rgb(0.18, 0.15, 0.12);
pub const TEXT_PRIMARY: Color = Color::from_rgb(0.95, 0.92, 0.9);
pub const TEXT_SECONDARY: Color = Color::from_rgb(0.6, 0.55, 0.5);

pub const LIGHTER_RADIUS: f32 = 10.0;
pub const LIGHT_RADIUS: f32 = 12.0;
pub const REGULAR_RADIUS: f32 = 16.0;
pub const CIRCLE_RADIUS: f32 = 100.0;

pub const CONTROL_CLOSE_HOVER: Color = Color::from_rgb(0.9, 0.2, 0.2);

/// Returns the custom Fjarsyn theme.
pub fn fjarsyn_theme() -> Theme {
    Theme::custom(
        "Fjarsyn".to_string(),
        Palette {
            background: BACKGROUND_COLOR,
            text: TEXT_PRIMARY,
            primary: PRIMARY_COLOR,
            success: Color::from_rgb(0.2, 0.7, 0.2),
            danger: Color::from_rgb(0.8, 0.2, 0.2),
            warning: Color::from_rgb(0.9, 0.6, 0.1),
        },
    )
}

pub fn main_content_container(theme: &Theme) -> container::Style {
    let palette = theme.extended_palette();
    container::Style {
        background: Some(palette.background.base.color.into()),
        border: Border { width: 0.0, ..Default::default() },
        shadow: Shadow {
            color: Color { a: 0.25, ..Color::BLACK },
            offset: Vector::new(0.0, 0.0),
            blur_radius: 15.0,
            ..Default::default()
        },
        ..Default::default()
    }
}

pub fn sidebar_container(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(SIDEBAR_BACKGROUND.into()),
        border: Border { width: 0.0, ..Default::default() },
        ..Default::default()
    }
}

pub fn card_container(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(CARD_BACKGROUND.into()),
        border: Border {
            color: BORDER_COLOR,
            width: 1.0,
            radius: REGULAR_RADIUS.into(),
            ..Default::default()
        },
        shadow: Shadow {
            color: Color { a: 0.15, ..Color::BLACK },
            offset: Vector::new(0.0, 4.0),
            blur_radius: 12.0,
        },
        ..Default::default()
    }
}

pub fn id_card_container(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Color { a: 0.05, ..Color::WHITE }.into()),
        border: Border {
            color: BORDER_COLOR,
            width: 1.0,
            radius: LIGHT_RADIUS.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}

pub fn icon_bubble_container(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Color { a: 0.1, ..Color::WHITE }.into()),
        border: Border { radius: CIRCLE_RADIUS.into(), ..Default::default() },
        ..Default::default()
    }
}

pub fn warning_accent_container(theme: &Theme) -> container::Style {
    let palette = theme.extended_palette();
    container::Style {
        background: Some(Color { a: 0.15, ..palette.warning.base.color }.into()),
        border: Border { radius: LIGHT_RADIUS.into(), ..Default::default() },
        ..Default::default()
    }
}

pub fn titlebar_container(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(SIDEBAR_BACKGROUND.into()),
        border: Border { color: BORDER_COLOR, width: 1.0, ..Default::default() },
        ..Default::default()
    }
}

pub fn section_container(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Color { a: 0.03, ..Color::WHITE }.into()),
        border: Border { color: BORDER_COLOR, width: 1.0, radius: LIGHT_RADIUS.into() },
        ..Default::default()
    }
}

pub fn notification_container(theme: &Theme, kind: NotificationKind) -> container::Style {
    let palette = theme.extended_palette();

    let color = match kind {
        NotificationKind::Error => palette.danger.base.color,
        NotificationKind::Info => palette.primary.base.color,
        NotificationKind::Success => palette.success.base.color,
    };

    container::Style {
        background: Some(CARD_BACKGROUND.into()),
        border: Border { color, width: 1.0, radius: LIGHT_RADIUS.into() },
        shadow: Shadow {
            color: Color { a: 0.2, ..Color::BLACK },
            offset: Vector::new(0.0, 4.0),
            blur_radius: 12.0,
        },
        ..Default::default()
    }
}

pub fn sidebar_button_style(
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

pub fn window_control_style(
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

pub fn button_style(theme: &Theme, status: button::Status, is_primary: bool) -> button::Style {
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

pub fn danger_button_style(theme: &Theme, status: button::Status) -> button::Style {
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

pub fn text_input_style(theme: &Theme, _status: text_input::Status) -> text_input::Style {
    let palette = theme.extended_palette();

    text_input::Style {
        background: Color { a: 0.05, ..palette.background.base.text }.into(),
        border: Border { color: BORDER_COLOR, width: 1.0, radius: LIGHTER_RADIUS.into() },
        icon: Color { a: 0.5, ..palette.background.base.text },
        placeholder: TEXT_SECONDARY,
        value: palette.background.base.text,
        selection: Color { a: 0.2, ..palette.primary.base.color },
    }
}

pub fn spacer_style(_theme: &Theme) -> container::Style {
    container::Style { background: Some(BORDER_COLOR.into()), ..Default::default() }
}
