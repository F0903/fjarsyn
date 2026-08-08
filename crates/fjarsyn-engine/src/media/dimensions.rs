#[repr(C)]
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
/// Width and height of a media frame or target resolution.
pub struct Dimensions<N = i32> {
    pub width: N,
    pub height: N,
}

impl<N> Dimensions<N> {
    #[inline]
    pub const fn new(width: N, height: N) -> Self {
        Self { width, height }
    }
}
