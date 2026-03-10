use iced::{Color, Theme, color, theme::Palette};

use crate::ui::notification::NotificationKind;

pub const BACKGROUND: Color = color!(0x121212); // Deep neutral dark
pub const SURFACE: Color = color!(0x1e1e1e); // Warmer dark for the main "sheet"
pub const TEXT: Color = color!(0xeeeeee);
pub const ACCENT: Color = color!(0xfd8935); // Sunset orange
pub const SUCCESS: Color = color!(0x43a047);
pub const DANGER: Color = color!(0xd32f2f);
pub const WARNING: Color = color!(0xffb300);

pub const BORDER_RADIUS: f32 = 12.0;
pub const LARGE_BORDER_RADIUS: f32 = 24.0;

// Window Control Colors
pub const CONTROL_DARK: Color = color!(0x1a1a1a);
pub const CONTROL_CLOSE_HOVER: Color = color!(0xff6b6b); // Pastel red

pub const HOVER_COLOR_LIGHTEN: f32 = 0.05;
pub const PRESSED_COLOR_DARKEN: f32 = 0.05;

pub fn theme() -> Theme {
    Theme::custom(
        "Fjarsyn".to_string(),
        Palette {
            background: BACKGROUND,
            text: TEXT,
            primary: ACCENT,
            success: SUCCESS,
            danger: DANGER,
            warning: WARNING,
        },
    )
}

pub fn theme_fn(_state: &crate::ui::state::State, _window: iced::window::Id) -> Theme {
    theme()
}

pub fn sidebar_container(_theme: &Theme) -> iced::widget::container::Style {
    iced::widget::container::Style {
        background: Some(BACKGROUND.into()),
        border: iced::Border {
            radius: iced::border::Radius {
                top_left: BORDER_RADIUS.into(),
                top_right: 0.0.into(),
                bottom_right: 0.0.into(),
                bottom_left: BORDER_RADIUS.into(),
            },
            ..Default::default()
        },
        ..Default::default()
    }
}

pub fn titlebar_container(_theme: &Theme) -> iced::widget::container::Style {
    iced::widget::container::Style {
        background: Some(BACKGROUND.into()),
        border: iced::Border {
            radius: iced::border::Radius {
                top_left: BORDER_RADIUS.into(),
                top_right: BORDER_RADIUS.into(),
                bottom_right: 0.0.into(),
                bottom_left: 0.0.into(),
            },
            ..Default::default()
        },
        ..Default::default()
    }
}

pub fn main_content_container(_theme: &Theme) -> iced::widget::container::Style {
    iced::widget::container::Style {
        background: Some(SURFACE.into()),
        border: iced::Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: iced::border::Radius {
                top_left: LARGE_BORDER_RADIUS.into(),
                top_right: 0.0.into(),
                bottom_right: BORDER_RADIUS.into(),
                bottom_left: 0.0.into(),
            },
        },
        ..Default::default()
    }
}

pub fn card_container(_theme: &Theme) -> iced::widget::container::Style {
    iced::widget::container::Style {
        background: Some(color!(0x262626).into()),
        border: iced::Border { color: color!(0x303030), width: 1.0, radius: BORDER_RADIUS.into() },
        ..Default::default()
    }
}

pub fn section_container(_theme: &Theme) -> iced::widget::container::Style {
    iced::widget::container::Style {
        background: Some(color!(0x1a1a1a).into()),
        border: iced::Border { color: color!(0x2a2a2a), width: 1.0, radius: BORDER_RADIUS.into() },
        ..Default::default()
    }
}

pub fn id_card_container(_theme: &Theme) -> iced::widget::container::Style {
    iced::widget::container::Style {
        background: Some(color!(0x1a1a1a).into()),
        border: iced::Border { color: color!(0x2a2a2a), width: 1.0, radius: BORDER_RADIUS.into() },
        ..Default::default()
    }
}

pub fn icon_bubble_container(_theme: &Theme) -> iced::widget::container::Style {
    iced::widget::container::Style {
        background: Some(color!(0x3a3a3a).into()),
        border: iced::Border {
            radius: 100.0.into(), // Circular
            ..Default::default()
        },
        ..Default::default()
    }
}

// Get the contrasting text color for a given background color.
pub fn contrasting_text_color(background: Color) -> Color {
    let luminance = 0.2126 * background.r + 0.7152 * background.g + 0.0722 * background.b;
    if luminance > 0.45 { Color::BLACK } else { Color::WHITE }
}

pub fn window_control_style(
    _theme: &Theme,
    status: iced::widget::button::Status,
    hover_color: Option<Color>,
) -> iced::widget::button::Style {
    let bg = match status {
        iced::widget::button::Status::Hovered | iced::widget::button::Status::Pressed => {
            hover_color.unwrap_or(iced::theme::palette::lighten(CONTROL_DARK, HOVER_COLOR_LIGHTEN))
        }
        _ => CONTROL_DARK,
    };

    iced::widget::button::Style {
        background: Some(bg.into()),
        text_color: contrasting_text_color(bg),
        border: iced::Border { radius: 100.0.into(), ..Default::default() },
        ..Default::default()
    }
}

pub fn sidebar_button_style(
    _theme: &Theme,
    status: iced::widget::button::Status,
    active: bool,
) -> iced::widget::button::Style {
    use iced::theme::palette;

    let base_bg = if active { ACCENT } else { color!(0x1a1a1a) };
    let bg = match status {
        iced::widget::button::Status::Hovered => palette::lighten(base_bg, HOVER_COLOR_LIGHTEN),
        iced::widget::button::Status::Pressed => palette::darken(base_bg, PRESSED_COLOR_DARKEN),
        _ => base_bg,
    };

    iced::widget::button::Style {
        background: Some(bg.into()),
        text_color: if active { contrasting_text_color(bg) } else { TEXT },
        border: iced::Border { radius: 12.0.into(), ..Default::default() },
        ..Default::default()
    }
}

pub fn text_input_style(
    _theme: &Theme,
    status: iced::widget::text_input::Status,
) -> iced::widget::text_input::Style {
    let base = iced::widget::text_input::Style {
        background: color!(0x2a2a2a).into(),
        border: iced::Border { radius: BORDER_RADIUS.into(), width: 1.0, color: color!(0x3a3a3a) },
        icon: color!(0x666666),
        placeholder: color!(0x666666),
        value: TEXT,
        selection: ACCENT,
    };

    match status {
        iced::widget::text_input::Status::Hovered => iced::widget::text_input::Style {
            border: iced::Border { color: color!(0x4a4a4a), ..base.border },
            ..base
        },
        iced::widget::text_input::Status::Focused { .. } => iced::widget::text_input::Style {
            border: iced::Border { color: ACCENT, ..base.border },
            ..base
        },
        _ => base,
    }
}

pub fn button_style(
    _theme: &Theme,
    status: iced::widget::button::Status,
    is_primary: bool,
) -> iced::widget::button::Style {
    let base_bg = if is_primary { ACCENT } else { color!(0x2a2a2a) };
    let bg = match status {
        iced::widget::button::Status::Hovered => {
            iced::theme::palette::lighten(base_bg, HOVER_COLOR_LIGHTEN)
        }
        iced::widget::button::Status::Pressed => {
            iced::theme::palette::darken(base_bg, PRESSED_COLOR_DARKEN)
        }
        iced::widget::button::Status::Disabled => {
            let mut color = base_bg;
            color.a = 0.2; // Desaturate/transparent
            color
        }
        _ => base_bg,
    };

    iced::widget::button::Style {
        background: Some(bg.into()),
        text_color: if status == iced::widget::button::Status::Disabled {
            color!(0x666666)
        } else {
            contrasting_text_color(bg)
        },
        border: iced::Border { radius: BORDER_RADIUS.into(), ..Default::default() },
        ..Default::default()
    }
}

pub fn notification_container(
    _theme: &Theme,
    kind: NotificationKind,
) -> iced::widget::container::Style {
    let bg_color = match kind {
        NotificationKind::Info => ACCENT,
        NotificationKind::Error => DANGER,
        NotificationKind::Success => SUCCESS,
    };

    iced::widget::container::Style {
        background: Some(bg_color.into()),
        text_color: Some(contrasting_text_color(bg_color)),
        border: iced::Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: BORDER_RADIUS.into(),
        },
        shadow: iced::Shadow {
            color: Color { a: 0.4, ..Color::BLACK },
            offset: iced::Vector::new(0.0, 6.0),
            blur_radius: 15.0,
        },
        ..Default::default()
    }
}
