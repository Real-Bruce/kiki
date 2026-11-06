use super::geometry::PixelRect;

/// Owned, tightly packed, top-down BGRA8 SDR frame. No platform resources escape.
pub struct Image {
    bounds: PixelRect,
    pixels: Vec<u8>,
    stride: u32,
}

impl Image {
    pub fn bounds(&self) -> PixelRect {
        self.bounds
    }
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }
    pub fn pixels_mut(&mut self) -> &mut [u8] {
        &mut self.pixels
    }
    pub fn stride(&self) -> u32 {
        self.stride
    }

    /// Copy an independently owned region; uncovered pixels remain transparent.
    #[cfg(any(feature = "diagnostics", test))]
    pub fn region(&self, bounds: PixelRect) -> Result<Self, String> {
        let mut output = Self::new(bounds)?;
        output.blit(self);
        Ok(output)
    }
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

    #[test]
    fn clipping_all_edges_uses_source_stride_and_preserves_global_origin() {
        let mut source = Image::new(PixelRect {
            left: -2,
            top: -1,
            right: 2,
            bottom: 2,
        })
        .unwrap();
        for (index, pixel) in source.pixels.chunks_exact_mut(4).enumerate() {
            pixel.copy_from_slice(&[index as u8, 0, 0, 255]);
        }
        let clipped = source
            .region(PixelRect {
                left: -1,
                top: 0,
                right: 1,
                bottom: 1,
            })
            .unwrap();
        assert_eq!(clipped.bounds.left, -1);
        assert_eq!(clipped.stride, 8);
        assert_eq!(clipped.pixels, vec![5, 0, 0, 255, 6, 0, 0, 255]);
        let expanded = source
            .region(PixelRect {
                left: -3,
                top: -2,
                right: 3,
                bottom: 3,
            })
            .unwrap();
        assert_eq!(expanded.pixels[..28], [0; 28]);
        assert_eq!(expanded.pixels[28..32], [0, 0, 0, 255]);
        assert_eq!(expanded.pixels[76..80], [8, 0, 0, 255]);
        let outside = source
            .region(PixelRect {
                left: 10,
                top: 10,
                right: 12,
                bottom: 12,
            })
            .unwrap();
        assert!(outside.pixels.iter().all(|v| *v == 0));
        assert!(
            source
                .region(PixelRect {
                    left: 1,
                    top: 1,
                    right: 1,
                    bottom: 2
                })
                .is_err()
        );
    }
}
