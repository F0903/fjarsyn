use crate::utils::vector2::Vector2;

#[derive(Debug, Clone)]
pub struct Rect<N = f32> {
    pub position: Vector2<N>,
    pub size: Vector2<N>,
}

impl From<windows::Foundation::Rect> for Rect<f32> {
    fn from(rect: windows::Foundation::Rect) -> Self {
        Rect {
            position: Vector2 { x: rect.X, y: rect.Y },
            size: Vector2 { x: rect.Width, y: rect.Height },
        }
    }
}

impl From<Rect<f32>> for windows::Foundation::Rect {
    fn from(rect: Rect<f32>) -> Self {
        windows::Foundation::Rect {
            X: rect.position.x,
            Y: rect.position.y,
            Width: rect.size.x,
            Height: rect.size.y,
        }
    }
}

impl From<windows::Graphics::RectInt32> for Rect<i32> {
    fn from(rect: windows::Graphics::RectInt32) -> Self {
        Rect {
            position: Vector2 { x: rect.X, y: rect.Y },
            size: Vector2 { x: rect.Width, y: rect.Height },
        }
    }
}

impl From<Rect<i32>> for windows::Graphics::RectInt32 {
    fn from(rect: Rect<i32>) -> Self {
        windows::Graphics::RectInt32 {
            X: rect.position.x,
            Y: rect.position.y,
            Width: rect.size.x,
            Height: rect.size.y,
        }
    }
}
