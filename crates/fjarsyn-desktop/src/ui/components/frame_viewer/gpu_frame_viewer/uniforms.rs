use fjarsyn_engine::media::frame::Frame;
use iced::Rectangle;

#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(super) struct Uniforms {
    ndc_min: [f32; 2],
    ndc_max: [f32; 2],
}

impl Uniforms {
    pub(super) fn for_frame(bounds: &Rectangle, frame: &Frame) -> Option<Self> {
        Self::for_dimensions(bounds, frame.size.width, frame.size.height)
    }

    pub(super) fn key(self) -> UniformKey {
        UniformKey([
            self.ndc_min[0].to_bits(),
            self.ndc_min[1].to_bits(),
            self.ndc_max[0].to_bits(),
            self.ndc_max[1].to_bits(),
        ])
    }

    fn for_dimensions(bounds: &Rectangle, frame_width: i32, frame_height: i32) -> Option<Self> {
        if !bounds.width.is_finite()
            || !bounds.height.is_finite()
            || bounds.width <= 0.0
            || bounds.height <= 0.0
            || frame_width <= 0
            || frame_height <= 0
        {
            return None;
        }

        let aspect_widget = bounds.width / bounds.height;
        let aspect_image = frame_width as f32 / frame_height as f32;
        let (scale_x, scale_y) = if aspect_image > aspect_widget {
            (1.0, aspect_widget / aspect_image)
        } else {
            (aspect_image / aspect_widget, 1.0)
        };

        Some(Self { ndc_min: [-scale_x, scale_y], ndc_max: [scale_x, -scale_y] })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct UniformKey([u32; 4]);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aspect_fit_isolated_by_view_geometry() {
        let wide = Uniforms::for_dimensions(
            &Rectangle::new([0.0, 0.0].into(), [1600.0, 900.0].into()),
            1920,
            1080,
        )
        .unwrap();
        let square = Uniforms::for_dimensions(
            &Rectangle::new([0.0, 0.0].into(), [900.0, 900.0].into()),
            1920,
            1080,
        )
        .unwrap();

        assert_eq!(wide, Uniforms { ndc_min: [-1.0, 1.0], ndc_max: [1.0, -1.0] });
        assert_ne!(wide.key(), square.key());
        assert!(square.ndc_min[1] < 1.0);
    }

    #[test]
    fn invalid_dimensions_are_rejected() {
        let bounds = Rectangle::new([0.0, 0.0].into(), [900.0, 900.0].into());

        assert!(Uniforms::for_dimensions(&bounds, 0, 1080).is_none());
    }
}
