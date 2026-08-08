//! Bundled application font family.

use iced::{
    Font,
    font::{Family, Weight},
};

pub(in crate::ui) const REGULAR: Font =
    Font { family: Family::Name("Outfit"), weight: Weight::Normal, ..Font::DEFAULT };

pub(in crate::ui) const SEMIBOLD: Font =
    Font { family: Family::Name("Outfit"), weight: Weight::Semibold, ..Font::DEFAULT };

pub(in crate::ui) const BOLD: Font =
    Font { family: Family::Name("Outfit"), weight: Weight::Bold, ..Font::DEFAULT };

// Raw bytes for the three weights used by the UI.
pub(in crate::ui) const REGULAR_BYTES: &[u8] =
    include_bytes!("../../assets/fonts/Outfit/static/Outfit-Regular.ttf");
pub(in crate::ui) const SEMIBOLD_BYTES: &[u8] =
    include_bytes!("../../assets/fonts/Outfit/static/Outfit-SemiBold.ttf");
pub(in crate::ui) const BOLD_BYTES: &[u8] =
    include_bytes!("../../assets/fonts/Outfit/static/Outfit-Bold.ttf");
