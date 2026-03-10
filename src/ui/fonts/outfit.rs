use iced::{
    Font,
    font::{Family, Weight},
};

pub const THIN: Font =
    Font { family: Family::Name("Outfit"), weight: Weight::Thin, ..Font::DEFAULT };

pub const EXTRA_LIGHT: Font =
    Font { family: Family::Name("Outfit"), weight: Weight::ExtraLight, ..Font::DEFAULT };

pub const LIGHT: Font =
    Font { family: Family::Name("Outfit"), weight: Weight::Light, ..Font::DEFAULT };

pub const REGULAR: Font =
    Font { family: Family::Name("Outfit"), weight: Weight::Normal, ..Font::DEFAULT };

pub const MEDIUM: Font =
    Font { family: Family::Name("Outfit"), weight: Weight::Medium, ..Font::DEFAULT };

pub const SEMIBOLD: Font =
    Font { family: Family::Name("Outfit"), weight: Weight::Semibold, ..Font::DEFAULT };

pub const BOLD: Font =
    Font { family: Family::Name("Outfit"), weight: Weight::Bold, ..Font::DEFAULT };

pub const EXTRA_BOLD: Font =
    Font { family: Family::Name("Outfit"), weight: Weight::ExtraBold, ..Font::DEFAULT };

pub const BLACK: Font =
    Font { family: Family::Name("Outfit"), weight: Weight::Black, ..Font::DEFAULT };

// Raw bytes for inclusion in binary
pub const THIN_BYTES: &[u8] = include_bytes!("../../../assets/fonts/Outfit/static/Outfit-Thin.ttf");
pub const EXTRA_LIGHT_BYTES: &[u8] =
    include_bytes!("../../../assets/fonts/Outfit/static/Outfit-ExtraLight.ttf");
pub const LIGHT_BYTES: &[u8] =
    include_bytes!("../../../assets/fonts/Outfit/static/Outfit-Light.ttf");
pub const REGULAR_BYTES: &[u8] =
    include_bytes!("../../../assets/fonts/Outfit/static/Outfit-Regular.ttf");
pub const MEDIUM_BYTES: &[u8] =
    include_bytes!("../../../assets/fonts/Outfit/static/Outfit-Medium.ttf");
pub const SEMIBOLD_BYTES: &[u8] =
    include_bytes!("../../../assets/fonts/Outfit/static/Outfit-SemiBold.ttf");
pub const BOLD_BYTES: &[u8] = include_bytes!("../../../assets/fonts/Outfit/static/Outfit-Bold.ttf");
pub const EXTRA_BOLD_BYTES: &[u8] =
    include_bytes!("../../../assets/fonts/Outfit/static/Outfit-ExtraBold.ttf");
pub const BLACK_BYTES: &[u8] =
    include_bytes!("../../../assets/fonts/Outfit/static/Outfit-Black.ttf");
