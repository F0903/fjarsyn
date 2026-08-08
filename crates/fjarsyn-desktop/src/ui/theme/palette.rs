use iced::{Color, Theme, theme::Palette};

pub(in crate::ui) const PRIMARY_COLOR: Color = Color::from_rgb(1.0, 0.5, 0.2);
pub(super) const BACKGROUND_COLOR: Color = Color::from_rgb(0.05, 0.04, 0.03);
pub(in crate::ui) const CARD_BACKGROUND: Color = Color::from_rgb(0.1, 0.09, 0.08);
pub(super) const SIDEBAR_BACKGROUND: Color = Color::from_rgb(0.08, 0.07, 0.06);
pub(in crate::ui) const BORDER_COLOR: Color = Color::from_rgb(0.18, 0.15, 0.12);
pub(super) const TEXT_PRIMARY: Color = Color::from_rgb(0.95, 0.92, 0.9);
pub(super) const TEXT_SECONDARY: Color = Color::from_rgb(0.6, 0.55, 0.5);

pub(super) const LIGHTER_RADIUS: f32 = 10.0;
pub(super) const LIGHT_RADIUS: f32 = 12.0;
pub(super) const REGULAR_RADIUS: f32 = 16.0;
pub(super) const CIRCLE_RADIUS: f32 = 100.0;

pub(in crate::ui) const CONTROL_CLOSE_HOVER: Color = Color::from_rgb(0.9, 0.2, 0.2);

pub(in crate::ui) fn fjarsyn_theme() -> Theme {
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
