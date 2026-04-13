#[repr(C)]
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub struct Vector2<N = i32> {
    pub x: N,
    pub y: N,
}

impl<N> Vector2<N> {
    #[inline]
    pub const fn new(x: N, y: N) -> Self {
        Self { x, y }
    }

    #[inline]
    pub fn cast<T: From<N>>(self) -> Vector2<T> {
        Vector2 { x: self.x.into(), y: self.y.into() }
    }
}

impl<N> Vector2<N>
where
    N: Copy,
{
    #[inline]
    pub const fn width(&self) -> N {
        self.x
    }

    #[inline]
    pub const fn height(&self) -> N {
        self.y
    }
}

impl<N: PartialEq> PartialEq<(N, N)> for Vector2<N> {
    fn eq(&self, other: &(N, N)) -> bool {
        self.x == other.0 && self.y == other.1
    }
}
