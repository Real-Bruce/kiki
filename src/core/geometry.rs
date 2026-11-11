/// Global desktop physical pixels; negative origins are valid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PixelRect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl PixelRect {
    pub fn corner_radius(self, configured: u16) -> f32 {
        self.size().map_or(0., |(width, height)| {
            f32::from(configured).min(width.min(height) as f32 / 2.)
        })
    }

    pub fn size(self) -> Option<(u32, u32)> {
        let width = u32::try_from(i64::from(self.right) - i64::from(self.left)).ok()?;
        let height = u32::try_from(i64::from(self.bottom) - i64::from(self.top)).ok()?;
        (width > 0 && height > 0).then_some((width, height))
    }

    pub fn intersection(self, other: Self) -> Option<Self> {
        let result = Self {
            left: self.left.max(other.left),
            top: self.top.max(other.top),
            right: self.right.min(other.right),
            bottom: self.bottom.min(other.bottom),
        };
        result.size().map(|_| result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn negative_origin_and_extreme_span() {
        let r = PixelRect {
            left: -1920,
            top: -1080,
            right: 1920,
            bottom: 1080,
        };
        assert_eq!(r.size(), Some((3840, 2160)));
        assert_eq!(
            PixelRect {
                left: i32::MIN,
                right: i32::MAX,
                ..r
            }
            .size(),
            Some((u32::MAX, 2160))
        );
        assert!(
            r.intersection(PixelRect {
                left: 1920,
                right: 2000,
                ..r
            })
            .is_none()
        );
    }
}
