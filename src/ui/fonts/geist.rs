use iced::{
    Font,
    font::{Family, Weight},
};

pub const THIN: Font =
    Font { family: Family::Name("Geist"), weight: Weight::Thin, ..Font::DEFAULT };

pub const EXTRA_LIGHT: Font =
    Font { family: Family::Name("Geist"), weight: Weight::ExtraLight, ..Font::DEFAULT };

pub const LIGHT: Font =
    Font { family: Family::Name("Geist"), weight: Weight::Light, ..Font::DEFAULT };

pub const REGULAR: Font =
    Font { family: Family::Name("Geist"), weight: Weight::Normal, ..Font::DEFAULT };

pub const MEDIUM: Font =
    Font { family: Family::Name("Geist"), weight: Weight::Medium, ..Font::DEFAULT };

pub const SEMIBOLD: Font =
    Font { family: Family::Name("Geist"), weight: Weight::Semibold, ..Font::DEFAULT };

pub const BOLD: Font =
    Font { family: Family::Name("Geist"), weight: Weight::Bold, ..Font::DEFAULT };

pub const EXTRA_BOLD: Font =
    Font { family: Family::Name("Geist"), weight: Weight::ExtraBold, ..Font::DEFAULT };

pub const BLACK: Font =
    Font { family: Family::Name("Geist"), weight: Weight::Black, ..Font::DEFAULT };

// Raw bytes for inclusion in binary
pub const THIN_BYTES: &[u8] = include_bytes!("../../../assets/fonts/Geist/static/Geist-Thin.ttf");
pub const EXTRA_LIGHT_BYTES: &[u8] =
    include_bytes!("../../../assets/fonts/Geist/static/Geist-ExtraLight.ttf");
pub const LIGHT_BYTES: &[u8] = include_bytes!("../../../assets/fonts/Geist/static/Geist-Light.ttf");
pub const REGULAR_BYTES: &[u8] =
    include_bytes!("../../../assets/fonts/Geist/static/Geist-Regular.ttf");
pub const MEDIUM_BYTES: &[u8] =
    include_bytes!("../../../assets/fonts/Geist/static/Geist-Medium.ttf");
pub const SEMIBOLD_BYTES: &[u8] =
    include_bytes!("../../../assets/fonts/Geist/static/Geist-SemiBold.ttf");
pub const BOLD_BYTES: &[u8] = include_bytes!("../../../assets/fonts/Geist/static/Geist-Bold.ttf");
pub const EXTRA_BOLD_BYTES: &[u8] =
    include_bytes!("../../../assets/fonts/Geist/static/Geist-ExtraBold.ttf");
pub const BLACK_BYTES: &[u8] = include_bytes!("../../../assets/fonts/Geist/static/Geist-Black.ttf");
