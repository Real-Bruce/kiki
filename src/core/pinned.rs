//! Physical image display geometry. Zoom never resamples the owned output image.
#[derive(Clone, Copy, Debug)]
pub struct View {
    pub width: u32,
    pub height: u32,
    pub scale: f64,
}
impl View {
    pub fn fit(width: u32, height: u32, work_width: i32, work_height: i32) -> Option<Self> {
        if width == 0 || height == 0 || work_width <= 0 || work_height <= 0 {
            return None;
        }
        Some(Self {
            width,
            height,
            scale: 1f64
                .min(f64::from(work_width) / f64::from(width))
                .min(f64::from(work_height) / f64::from(height))
                .min(maximum_scale(width, height)),
        })
    }
    pub fn dimensions(self, minimum: (i32, i32)) -> (i32, i32) {
        (
            (f64::from(self.width) * self.scale)
                .ceil()
                .max(f64::from(minimum.0)) as i32,
            (f64::from(self.height) * self.scale)
                .ceil()
                .max(f64::from(minimum.1)) as i32,
        )
    }
    pub fn image_origin(self, minimum: (i32, i32)) -> (f64, f64) {
        let (w, h) = self.dimensions(minimum);
        (
            (f64::from(w) - f64::from(self.width) * self.scale) / 2.,
            (f64::from(h) - f64::from(self.height) * self.scale) / 2.,
        )
    }
    pub fn change_scale(
        &mut self,
        requested: f64,
        origin: (i32, i32),
        pointer: (i32, i32),
        minimum: (i32, i32),
    ) -> (i32, i32) {
        let old = self.scale;
        let (ox, oy) = self.image_origin(minimum);
        let max = maximum_scale(self.width, self.height);
        self.scale = if requested.is_finite() {
            requested.clamp(0.01f64.min(max), max)
        } else {
            old
        };
        let (nx, ny) = self.image_origin(minimum);
        let x = f64::from(origin.0) + f64::from(pointer.0)
            - nx
            - (f64::from(pointer.0) - ox) * self.scale / old;
        let y = f64::from(origin.1) + f64::from(pointer.1)
            - ny
            - (f64::from(pointer.1) - oy) * self.scale / old;
        (
            x.round().clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32,
            y.round().clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32,
        )
    }
}
fn maximum_scale(width: u32, height: u32) -> f64 {
    4f64.min(8192. / f64::from(width))
        .min(8192. / f64::from(height))
        .min((16_777_216. / (f64::from(width) * f64::from(height))).sqrt())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fit_and_zoom_preserve_image_dimensions_and_pointer_anchor() {
        let mut v = View::fit(3840, 2160, 1920, 1080).unwrap();
        assert_eq!(v.scale, 0.5);
        let origin = v.change_scale(1., (-1920, -1080), (400, 200), (144, 64));
        assert_eq!(origin, (-2320, -1280));
        assert_eq!(v.dimensions((144, 64)), (3840, 2160));
        assert_eq!((v.width, v.height), (3840, 2160));
    }
    #[test]
    fn tiny_images_letterbox_and_extreme_zoom_are_bounded() {
        let mut v = View::fit(1, 1, 1920, 1080).unwrap();
        assert_eq!(v.dimensions((144, 64)), (144, 64));
        assert_eq!(
            v.change_scale(2., (-10, -10), (72, 32), (144, 64)),
            (-10, -10)
        );
        let initial = View::fit(10000, 10000, 20000, 20000).unwrap();
        assert_eq!(initial.dimensions((144, 64)), (4096, 4096));
        let mut large = View::fit(10000, 10000, 1920, 1080).unwrap();
        large.change_scale(100., (0, 0), (0, 0), (144, 64));
        let (w, h) = large.dimensions((144, 64));
        assert!(i64::from(w) * i64::from(h) <= 16_777_216);
    }
}
