use super::{config::CaptureParameter, geometry::PixelRect};

pub fn blend_over_white(channel: u8, alpha: u8) -> u8 {
    let alpha = u16::from(alpha);
    ((u16::from(channel) * alpha + 255 * (255 - alpha) + 127) / 255) as u8
}

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

    /// Replace color within a mosaic rectangle with the average of each tile
    /// in the frozen source. Clip writes independently of the tile grid so
    /// changing the selection cannot change the color of a visible tile.
    pub fn pixelate(&mut self, source: &Self, bounds: PixelRect, size: u16) {
        let Some(area) = bounds.intersection(source.bounds) else {
            return;
        };
        let Some(visible) = area.intersection(self.bounds) else {
            return;
        };
        let (min, max) = CaptureParameter::MosaicSize.bounds();
        let size = i64::from(size.clamp(min, max));
        let axis = |anchor: i32, start: i32, end: i32| {
            let first = i64::from(start) - (i64::from(start) - i64::from(anchor)) % size;
            (first..i64::from(end)).step_by(size as usize)
        };
        for y in axis(bounds.top, visible.top, visible.bottom) {
            for x in axis(bounds.left, visible.left, visible.right) {
                let tile = PixelRect {
                    left: x.max(i64::from(area.left)) as i32,
                    top: y.max(i64::from(area.top)) as i32,
                    right: (x + size).min(i64::from(area.right)) as i32,
                    bottom: (y + size).min(i64::from(area.bottom)) as i32,
                };
                let mut sum = [0u32; 3];
                let mut weight = 0u32;
                for row in tile.top..tile.bottom {
                    for pixel in source.row(tile, row).chunks_exact(4) {
                        let alpha = u32::from(pixel[3]);
                        weight += alpha;
                        for channel in 0..3 {
                            sum[channel] += u32::from(pixel[channel]) * alpha;
                        }
                    }
                }
                if weight == 0 {
                    continue;
                }
                let color = sum.map(|channel| ((channel + weight / 2) / weight) as u8);
                let Some(write) = tile.intersection(visible) else {
                    continue;
                };
                for row in write.top..write.bottom {
                    let range = self.row_range(write, row);
                    for (dst, src) in self.pixels[range]
                        .chunks_exact_mut(4)
                        .zip(source.row(write, row).chunks_exact(4))
                    {
                        if src[3] != 0 {
                            dst[..3].copy_from_slice(&color);
                            dst[3] = src[3];
                        }
                    }
                }
            }
        }
    }

    fn row_range(&self, area: PixelRect, row: i32) -> std::ops::Range<usize> {
        let offset = (i64::from(row) - i64::from(self.bounds.top)) as usize * self.stride as usize;
        let left = (i64::from(area.left) - i64::from(self.bounds.left)) as usize * 4;
        let right = (i64::from(area.right) - i64::from(self.bounds.left)) as usize * 4;
        offset + left..offset + right
    }

    fn row(&self, area: PixelRect, row: i32) -> &[u8] {
        &self.pixels[self.row_range(area, row)]
    }

    /// Convert in place for upload to a premultiplied Direct2D bitmap.
    pub fn premultiply(&mut self) {
        for pixel in self.pixels.chunks_exact_mut(4) {
            for channel in 0..3 {
                pixel[channel] =
                    ((u16::from(pixel[channel]) * u16::from(pixel[3]) + 127) / 255) as u8;
            }
        }
    }

    /// Mask a finished straight-alpha image in place. Only the four corner tiles
    /// can change. Preserve RGB on partially covered pixels to avoid dark fringes.
    pub fn round_corners(&mut self, configured: u16) {
        let radius = self.bounds.corner_radius(configured);
        if radius == 0. {
            return;
        }
        let stride = self.stride as usize;
        let width = stride / 4;
        let height = self.pixels.len() / stride;
        let extent = radius.ceil() as usize;
        let radius_squared = radius * radius;
        for y in 0..extent {
            for x in 0..extent {
                let dx = radius - x as f32;
                let dy = radius - y as f32;
                if dx * dx + dy * dy <= radius_squared {
                    continue;
                }
                let near_x = (dx - 1.).max(0.);
                let near_y = (dy - 1.).max(0.);
                let coverage = if near_x * near_x + near_y * near_y >= radius_squared {
                    0
                } else {
                    // Supersample just the pixels crossed by the arc.
                    let mut inside = 0;
                    let span_x = (width - 2 * x) as f32;
                    let span_y = (height - 2 * y) as f32;
                    for sy in 0..8 {
                        let sy = (sy as f32 + 0.5) / 8.;
                        let b = (dy - sy.min(span_y - sy)).max(0.);
                        for sx in 0..8 {
                            let sx = (sx as f32 + 0.5) / 8.;
                            let a = (dx - sx.min(span_x - sx)).max(0.);
                            inside += u16::from(a * a + b * b <= radius_squared);
                        }
                    }
                    inside
                };
                // The mirrored row or column can coincide on an odd short side.
                for row in [y, height - y - 1]
                    .into_iter()
                    .take((height - 2 * y).min(2))
                {
                    for column in [x, width - x - 1].into_iter().take((width - 2 * x).min(2)) {
                        let offset = row * stride + column * 4;
                        let pixel = &mut self.pixels[offset..offset + 4];
                        pixel[3] = ((u16::from(pixel[3]) * coverage + 32) / 64) as u8;
                        if pixel[3] == 0 {
                            pixel.fill(0);
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mosaic_grid_is_stable_when_cropped_and_overlaps_read_only_the_frozen_source() {
        let bounds = PixelRect {
            left: -5,
            top: -3,
            right: 3,
            bottom: 2,
        };
        let mut source = Image::new(bounds).unwrap();
        for (i, pixel) in source.pixels_mut().chunks_exact_mut(4).enumerate() {
            if i % 8 != 4 {
                pixel.copy_from_slice(&[(4 * i) as u8, 0, 0, 255]);
            }
        }
        let original = source.pixels().to_vec();
        let back = PixelRect {
            left: -6,
            top: -4,
            right: 3,
            bottom: 1,
        };
        let front = PixelRect {
            left: -2,
            top: -3,
            right: 3,
            bottom: 2,
        };
        let crop = PixelRect {
            left: -4,
            top: -2,
            right: 2,
            bottom: 1,
        };
        let mut full = source.region(bounds).unwrap();
        full.pixelate(&source, back, 3);
        // This first tile is cropped by the source to two rows and two columns.
        assert_eq!(&full.pixels()[..8], &[18, 0, 0, 255, 18, 0, 0, 255]);
        full.pixelate(&source, front, 2);
        let mut cropped = source.region(crop).unwrap();
        cropped.pixelate(&source, back, 3);
        cropped.pixelate(&source, front, 2);
        assert_eq!(cropped.pixels(), full.region(crop).unwrap().pixels());
        let mut front_only = source.region(front).unwrap();
        front_only.pixelate(&source, front, 2);
        assert_eq!(full.region(front).unwrap().pixels(), front_only.pixels());
        assert!(
            full.pixels()
                .chunks_exact(4)
                .skip(4)
                .step_by(8)
                .all(|p| p == [0; 4])
        );
        assert_eq!(source.pixels(), original);
    }
    #[test]
    fn rounded_mask_preserves_straight_color_and_clamps_on_tiny_images() {
        let mut image = Image::new(PixelRect {
            left: -12,
            top: -3,
            right: 1,
            bottom: 6,
        })
        .unwrap();
        for pixel in image.pixels_mut().chunks_exact_mut(4) {
            pixel.copy_from_slice(&[200, 50, 25, 128]);
        }
        let original = image.pixels().to_vec();
        image.round_corners(0);
        assert_eq!(image.pixels(), original);
        let gap = 4 * image.stride() as usize + 8;
        image.pixels_mut()[gap..gap + 4].fill(0);
        assert_eq!(image.bounds().corner_radius(256), 4.5);
        image.round_corners(256);
        let pixel = |x: usize, y: usize| {
            let offset = y * image.stride() as usize + 4 * x;
            &image.pixels()[offset..offset + 4]
        };
        assert_eq!(pixel(0, 0), [0; 4]);
        assert_eq!(pixel(12, 8), [0; 4]);
        let edge = pixel(2, 0);
        assert!((1..128).contains(&edge[3]));
        assert_eq!(&edge[..3], &[200, 50, 25]);
        assert_eq!(edge, pixel(10, 8));
        assert_eq!(pixel(6, 4), [200, 50, 25, 128]);
        assert_eq!(pixel(2, 4), [0; 4]);

        let mut tiny = Image::new(PixelRect {
            left: 0,
            top: 0,
            right: 1,
            bottom: 1,
        })
        .unwrap();
        tiny.pixels_mut().copy_from_slice(&[60, 100, 200, 255]);
        tiny.round_corners(256);
        assert_eq!(tiny.bounds().corner_radius(256), 0.5);
        assert_eq!(&tiny.pixels()[..3], &[60, 100, 200]);
        assert!((190..=220).contains(&tiny.pixels()[3]));
    }
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
