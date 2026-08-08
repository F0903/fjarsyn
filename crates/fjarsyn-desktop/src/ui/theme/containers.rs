use iced::{Border, Color, Shadow, Theme, Vector, widget::container};

use super::palette::{
    BORDER_COLOR, CARD_BACKGROUND, CIRCLE_RADIUS, LIGHT_RADIUS, REGULAR_RADIUS, SIDEBAR_BACKGROUND,
};
use crate::ui::notification::Kind;

pub(in crate::ui) fn main_content_container(theme: &Theme) -> container::Style {
    let palette = theme.extended_palette();
    container::Style {
        background: Some(palette.background.base.color.into()),
        border: Border { width: 0.0, ..Default::default() },
        shadow: Shadow {
            color: Color { a: 0.25, ..Color::BLACK },
            offset: Vector::new(0.0, 0.0),
            blur_radius: 15.0,
        },
        ..Default::default()
    }
}

pub(in crate::ui) fn sidebar_container(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(SIDEBAR_BACKGROUND.into()),
        border: Border { width: 0.0, ..Default::default() },
        ..Default::default()
    }
}

pub(in crate::ui) fn card_container(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(CARD_BACKGROUND.into()),
        border: Border { color: BORDER_COLOR, width: 1.0, radius: REGULAR_RADIUS.into() },
        shadow: Shadow {
            color: Color { a: 0.15, ..Color::BLACK },
            offset: Vector::new(0.0, 4.0),
            blur_radius: 12.0,
        },
        ..Default::default()
    }
}

pub(in crate::ui) fn id_card_container(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Color { a: 0.05, ..Color::WHITE }.into()),
        border: Border { color: BORDER_COLOR, width: 1.0, radius: LIGHT_RADIUS.into() },
        ..Default::default()
    }
}

pub(in crate::ui) fn icon_bubble_container(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Color { a: 0.1, ..Color::WHITE }.into()),
        border: Border { radius: CIRCLE_RADIUS.into(), ..Default::default() },
        ..Default::default()
    }
}

pub(in crate::ui) fn warning_accent_container(theme: &Theme) -> container::Style {
    let palette = theme.extended_palette();
    container::Style {
        background: Some(Color { a: 0.15, ..palette.warning.base.color }.into()),
        border: Border { radius: LIGHT_RADIUS.into(), ..Default::default() },
        ..Default::default()
    }
}

pub(in crate::ui) fn titlebar_container(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(SIDEBAR_BACKGROUND.into()),
        border: Border { color: BORDER_COLOR, width: 1.0, ..Default::default() },
        ..Default::default()
    }
}

pub(in crate::ui) fn section_container(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Color { a: 0.03, ..Color::WHITE }.into()),
        border: Border { color: BORDER_COLOR, width: 1.0, radius: LIGHT_RADIUS.into() },
        ..Default::default()
    }
}

pub(in crate::ui) fn notification_container(theme: &Theme, kind: Kind) -> container::Style {
    let palette = theme.extended_palette();

    let color = match kind {
        Kind::Error => palette.danger.base.color,
        Kind::Info => palette.primary.base.color,
        Kind::Success => palette.success.base.color,
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
