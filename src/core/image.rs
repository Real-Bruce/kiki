use super::geometry::PixelRect;

/// Owned, tightly packed, top-down BGRA8 SDR frame. No platform resources escape.
pub struct Image {
    pub bounds: PixelRect,
    pub pixels: Vec<u8>,
    pub stride: u32,
}

impl Image {
    pub fn new(bounds: PixelRect) -> Result<Self, String> {
        let (width, height) = bounds.size().ok_or("无效的截图范围")?;
        let stride = width.checked_mul(4).ok_or("图像行宽溢出")?;
        let len = usize::try_from(u64::from(stride) * u64::from(height)).map_err(|_| "图像过大")?;
        // Bound desktop allocations; allocation failure remains a recoverable error.
        if len > 512 * 1024 * 1024 {
            return Err("截图超过 512 MiB 内存限制".into());
        }
        let mut pixels = Vec::new();
        pixels
            .try_reserve_exact(len)
            .map_err(|_| "没有足够内存保存截图")?;
        pixels.resize(len, 0);
        Ok(Self {
            bounds,
            pixels,
            stride,
        })
    }

    pub fn blit(&mut self, source: &Self) {
        let Some(overlap) = self.bounds.intersection(source.bounds) else {
            return;
        };
        let width = (i64::from(overlap.right) - i64::from(overlap.left)) as usize * 4;
        for y in overlap.top..overlap.bottom {
            let dst = (i64::from(y) - i64::from(self.bounds.top)) as usize * self.stride as usize
                + (i64::from(overlap.left) - i64::from(self.bounds.left)) as usize * 4;
            let src = (i64::from(y) - i64::from(source.bounds.top)) as usize
                * source.stride as usize
                + (i64::from(overlap.left) - i64::from(source.bounds.left)) as usize * 4;
            self.pixels[dst..dst + width].copy_from_slice(&source.pixels[src..src + width]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn composition_preserves_transparent_gap_and_negative_origin() {
        let mut desktop = Image::new(PixelRect {
            left: -2,
            top: 0,
            right: 2,
            bottom: 1,
        })
        .unwrap();
        let mut screen = Image::new(PixelRect {
            left: -2,
            top: 0,
            right: -1,
            bottom: 1,
        })
        .unwrap();
        screen.pixels.copy_from_slice(&[10, 20, 30, 255]);
        desktop.blit(&screen);
        assert_eq!(&desktop.pixels[..4], &[10, 20, 30, 255]);
        assert!(desktop.pixels[4..].iter().all(|v| *v == 0));
        assert!(
            Image::new(PixelRect {
                left: 0,
                top: 0,
                right: i32::MAX,
                bottom: 2
            })
            .is_err()
        );
    }
}
