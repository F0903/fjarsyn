#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Vector2<N = f32> {
    pub x: N,
    pub y: N,
}

impl<N> Vector2<N> {
    pub fn new(x: N, y: N) -> Self {
        Self { x, y }
    }
}
