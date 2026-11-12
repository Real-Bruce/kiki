use super::{geometry::PixelRect, image::Image, selection::PixelPoint};

const MAX_HEIGHT: usize = 32_768;
const MAX_BYTES: usize = 256 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Added,
    Known,
    Unmatched,
    Limit,
}

pub struct Stitcher {
    image: Image,
    rows: Vec<RowKey>,
    frame_height: usize,
    viewed: usize,
}

type RowKey = [u64; 3];

fn fingerprint(row: &[u8]) -> RowKey {
    let width = row.len() / 4;
    let inset = (width / 16).min(48);
    let body = width - 2 * inset;
    std::array::from_fn(|lane| {
        let start = inset + body * lane / 3;
        let end = inset + body * (lane + 1) / 3;
        (start..end)
            .step_by(((end - start) / 32).max(1))
            .fold(0xcbf29ce484222325, |hash, x| {
                let i = x * 4;
                let rgb = u32::from_le_bytes([row[i], row[i + 1], row[i + 2], row[i + 3]]);
                (hash ^ u64::from(rgb)).wrapping_mul(0x100000001b3)
            })
    })
}

fn matching_lanes(a: RowKey, b: RowKey) -> usize {
    a.into_iter().zip(b).filter(|(a, b)| a == b).count()
}

fn enough_detail(rows: impl IntoIterator<Item = RowKey>) -> bool {
    let mut distinct = [None; 4];
    for row in rows {
        if distinct.contains(&Some(row)) {
            continue;
        }
        if let Some(slot) = distinct.iter_mut().find(|slot| slot.is_none()) {
            *slot = Some(row);
            if distinct[3].is_some() {
                return true;
            }
        }
    }
    false
}

fn coarse_match(old: &[RowKey], new: &[RowKey], changes: &[usize]) -> bool {
    let changes = &changes[..changes.partition_point(|&y| y < old.len())];
    let samples = changes.len().min(24);
    if samples < 3 {
        return false;
    }
    let mut checked = 0;
    let mut matched = 0;
    for n in 0..samples {
        let y = changes[n * changes.len() / samples];
        for lane in 0..3 {
            if new[y - 1][lane] != new[y][lane] {
                checked += 1;
                matched += usize::from(
                    new[y - 1][lane] == old[y - 1][lane] && new[y][lane] == old[y][lane],
                );
            }
        }
    }
    matched * 100 >= checked * 68
}

fn reliable_overlap(old: &[RowKey], new: &[RowKey]) -> bool {
    let mut matched = 0;
    let mut transitions = 0;
    let mut aligned_transitions = 0;
    for (y, (a, b)) in old.iter().zip(new).enumerate() {
        matched += matching_lanes(*a, *b);
        if y == 0 {
            continue;
        }
        for lane in 0..3 {
            let previous = old[y - 1][lane];
            let incoming = new[y - 1][lane];
            if previous != a[lane] || incoming != b[lane] {
                transitions += 1;
                aligned_transitions += usize::from(previous == incoming && a[lane] == b[lane]);
            }
        }
    }
    // Flat background alone cannot establish a seam. Most changes between consecutive
    // rows must align as well, and there must be several distinct matching rows.
    matched * 100 >= old.len() * 3 * 94
        && aligned_transitions >= 6
        && aligned_transitions * 100 >= transitions * 80
        && enough_detail(
            old.iter()
                .zip(new)
                .filter_map(|(a, b)| (a == b).then_some(*a)),
        )
}

impl Stitcher {
    pub fn new(image: Image) -> Result<Self, String> {
        let (_, height) = image.bounds().size().ok_or("无效长截图尺寸")?;
        if height as usize > MAX_HEIGHT || image.pixels().len() > MAX_BYTES {
            return Err("选区超过长截图像素上限".into());
        }
        let mut rows = Vec::new();
        rows.try_reserve_exact(MAX_HEIGHT)
            .map_err(|_| "没有足够内存建立拼接索引")?;
        rows.extend(
            image
                .pixels()
                .chunks_exact(image.stride() as usize)
                .map(fingerprint),
        );
        Ok(Self {
            image,
            rows,
            frame_height: height as usize,
            viewed: 0,
        })
    }

    pub fn image(&self) -> &Image {
        &self.image
    }

    pub fn into_image(self) -> Image {
        self.image
    }

    pub fn viewport(&self) -> (usize, usize) {
        (self.viewed, self.frame_height)
    }

    pub fn accept(&mut self, frame: &Image, down: bool) -> Result<Step, String> {
        let stride = self.image.stride() as usize;
        let height = self.frame_height;
        if frame.stride() as usize != stride || frame.pixels().len() != stride * height {
            return Err("长截图视口尺寸已变化".into());
        }
        let data = frame.pixels();
        let last = self.rows.len() - height;
        let old = self.image.pixels();
        if old[self.viewed * stride..(self.viewed + height) * stride] == *data {
            return Ok(Step::Known);
        }
        let mut hashes = Vec::new();
        hashes
            .try_reserve_exact(height)
            .map_err(|_| "没有足够内存匹配长截图")?;
        hashes.extend(data.chunks_exact(stride).map(fingerprint));
        let mut found = None;
        // Require at least a quarter of the viewport and several distinct lines. Search
        // both known content and the tail before accepting a unique pixel-exact seam.
        let min_overlap = (height / 4).max(12);
        let end = if down {
            self.rows.len().saturating_sub(min_overlap)
        } else {
            last
        };
        let start = if down { self.viewed } else { 0 };
        for offset in start..=end {
            if !down && offset > self.viewed {
                break;
            }
            let overlap = (self.rows.len() - offset).min(height);
            if overlap == 0
                || self.rows[offset] != hashes[0]
                || self.rows[offset + overlap - 1] != hashes[overlap - 1]
                || self.rows[offset + overlap / 2] != hashes[overlap / 2]
                || self.rows[offset..offset + overlap] != hashes[..overlap]
                || old[offset * stride..(offset + overlap) * stride] != data[..overlap * stride]
            {
                continue;
            }
            if found.is_some() {
                return Ok(Step::Unmatched);
            }
            found = Some((offset, overlap));
        }
        if found.is_none() && frame.stride() >= 64 * 4 {
            let mut changes = Vec::new();
            changes
                .try_reserve_exact(height)
                .map_err(|_| "没有足够内存检查长截图接缝")?;
            changes.extend((1..height).filter(|&y| hashes[y - 1] != hashes[y]));
            let mut plausible = 0;
            for offset in start..=end {
                if !down && offset > self.viewed {
                    break;
                }
                let overlap = (self.rows.len() - offset).min(height);
                if overlap < min_overlap {
                    continue;
                }
                let old = &self.rows[offset..offset + overlap];
                let new = &hashes[..overlap];
                if !coarse_match(old, new, &changes) {
                    continue;
                }
                plausible += 1;
                // Repeated or largely empty pages can generate many candidates. Keep
                // the search bounded and pause if the alignment is ambiguous.
                if plausible > 64 {
                    return Ok(Step::Unmatched);
                }
                if reliable_overlap(old, new) {
                    if found.is_some() {
                        return Ok(Step::Unmatched);
                    }
                    found = Some((offset, overlap));
                }
            }
        }
        let Some((offset, overlap)) = found else {
            return Ok(Step::Unmatched);
        };
        if overlap == height {
            self.viewed = offset;
            return Ok(Step::Known);
        }
        if !enough_detail(hashes[..overlap].iter().copied()) {
            return Ok(Step::Unmatched);
        }
        let max_rows = MAX_HEIGHT.min(MAX_BYTES / stride);
        let new_rows = height - overlap;
        if self.rows.len() + new_rows > max_rows {
            return Ok(Step::Limit);
        }
        self.image
            .append_rows(&data[overlap * stride..], max_rows * stride)?;
        self.rows.extend_from_slice(&hashes[overlap..]);
        self.viewed = offset;
        Ok(Step::Added)
    }
}

pub fn thumbnail(source: &Image) -> Result<Image, String> {
    let bounds = source.bounds();
    let (width, height) = bounds.size().ok_or("无效长图预览尺寸")?;
    let divisor = width.div_ceil(160).max(height.div_ceil(1000)).max(1);
    source.downsample(width.div_ceil(divisor), height.div_ceil(divisor))
}
#[derive(Clone, Copy)]
pub struct View {
    pub canvas: PixelRect,
    pub scale: f32,
    pub offset: i32,
    pub document: PixelRect,
}

impl View {
    pub fn new(canvas: PixelRect, document: PixelRect) -> Option<Self> {
        let (width, _) = canvas.size()?;
        let (doc_width, _) = document.size()?;
        let scale = (width as f32 / doc_width as f32).min(1.);
        let fitted = (doc_width as f32 * scale).round() as i32;
        let left = canvas.left + (width as i32 - fitted) / 2;
        Some(Self {
            canvas: PixelRect {
                left,
                right: left + fitted,
                ..canvas
            },
            scale,
            offset: 0,
            document,
        })
    }
    pub fn visible_rows(self) -> i32 {
        ((self.canvas.bottom - self.canvas.top) as f32 / self.scale).ceil() as i32
    }
    pub fn max_offset(self) -> i32 {
        (self.document.bottom - self.document.top)
            .saturating_sub(self.visible_rows())
            .max(0)
    }
    pub fn band(self) -> PixelRect {
        let top = self.document.top + self.offset;
        PixelRect {
            top,
            bottom: top
                .saturating_add(self.visible_rows())
                .min(self.document.bottom),
            ..self.document
        }
    }
    pub fn to_document(self, point: PixelPoint) -> PixelPoint {
        PixelPoint {
            x: self.document.left
                + ((point.x - self.canvas.left) as f32 / self.scale).floor() as i32,
            y: self.document.top
                + self.offset
                + ((point.y - self.canvas.top) as f32 / self.scale).floor() as i32,
        }
    }
    pub fn to_screen(self, point: PixelPoint) -> PixelPoint {
        PixelPoint {
            x: self.canvas.left
                + ((point.x - self.document.left) as f32 * self.scale).round() as i32,
            y: self.canvas.top
                + ((point.y - self.document.top - self.offset) as f32 * self.scale).round() as i32,
        }
    }
}

#[cfg(any(test, feature = "diagnostics"))]
pub fn fixture(width: i32, top: i32, height: i32) -> Image {
    let Ok(mut image) = Image::new(PixelRect {
        left: 0,
        top: 0,
        right: width,
        bottom: height,
    }) else {
        unreachable!("fixture dimensions are bounded constants");
    };
    for (i, pixel) in image.pixels_mut().chunks_exact_mut(4).enumerate() {
        let y = (i / width as usize) as u32 + top as u32;
        let x = (i % width as usize) as u32;
        let color = y.wrapping_mul(2654435761) ^ x.wrapping_mul(2166136261);
        let [b, g, r, _] = color.to_le_bytes();
        pixel.copy_from_slice(&[b, g, r, 255]);
    }
    image
}

#[cfg(test)]
mod tests {
    use super::*;

    fn document_view(top: usize, scrollbar: usize) -> Image {
        let width = 480;
        let mut image = fixture(width, 0, 280);
        for (y, row) in image
            .pixels_mut()
            .chunks_exact_mut(width as usize * 4)
            .enumerate()
        {
            let line = (top + y) / 19;
            let within_line = (top + y) % 19;
            for (x, px) in row.chunks_exact_mut(4).enumerate() {
                let ink = within_line < 10
                    && x > 30
                    && x < 150 + (line * 97 % 270)
                    && (x + line * 5 + within_line * 11) % 23 > 5;
                let slider = x > 469 && (scrollbar..scrollbar + 28).contains(&y);
                let gray = if ink || slider { 30 } else { 250 };
                px.copy_from_slice(&[gray, gray, gray, 255]);
            }
        }
        image
    }

    #[test]
    fn scrolling_text_stitches_with_a_moving_scrollbar_and_a_small_repaint() {
        let initial = document_view(0, 0);
        let mut next = document_view(89, 23);
        let stride = next.stride() as usize;
        for y in 50..54 {
            next.pixels_mut()[y * stride + 212 * 4..y * stride + 220 * 4].fill(80);
        }
        let mut stitch = Stitcher::new(initial).unwrap();
        assert_eq!(stitch.accept(&next, true).unwrap(), Step::Added);
        assert_eq!(stitch.viewport(), (89, 280));
        assert_eq!(
            &stitch.image().pixels()[280 * stride..],
            &next.pixels()[191 * stride..]
        );
        assert_eq!(
            stitch.accept(&document_view(89, 65), true).unwrap(),
            Step::Known
        );
        assert_eq!(
            stitch.accept(&document_view(41, 15), false).unwrap(),
            Step::Known
        );
        assert_eq!(stitch.image().bounds().bottom, 369);
        assert_eq!(
            stitch.accept(&document_view(1500, 99), true).unwrap(),
            Step::Unmatched
        );
    }

    #[test]
    fn dirty_gutters_do_not_turn_repeated_patterns_into_a_seam() {
        let pattern = |shift: usize, gutter: u8| {
            let mut image = fixture(256, 0, 200);
            for (y, row) in image.pixels_mut().chunks_exact_mut(1024).enumerate() {
                row.fill(((y + shift) % 8) as u8);
                row[1000..].fill(gutter);
            }
            image
        };
        let mut stitch = Stitcher::new(pattern(0, 0)).unwrap();
        assert_eq!(
            stitch.accept(&pattern(3, 1), true).unwrap(),
            Step::Unmatched
        );
        assert_eq!(stitch.image().bounds().bottom, 200);
    }

    #[test]
    fn stitches_without_duplicates_on_reverse_and_repeated_frames() {
        let mut s = Stitcher::new(fixture(32, 0, 100)).unwrap();
        assert_eq!(s.accept(&fixture(32, 60, 100), true).unwrap(), Step::Added);
        assert_eq!(s.accept(&fixture(32, 60, 100), true).unwrap(), Step::Known);
        assert_eq!(s.accept(&fixture(32, 20, 100), false).unwrap(), Step::Known);
        assert_eq!(s.viewport(), (20, 100));
        assert_eq!(s.accept(&fixture(32, 60, 100), true).unwrap(), Step::Known);
        assert_eq!(s.accept(&fixture(32, 124, 100), true).unwrap(), Step::Added);
        assert_eq!(s.image().pixels(), fixture(32, 0, 224).pixels());
    }

    #[test]
    fn ambiguous_or_disjoint_rows_preserve_the_confirmed_document() {
        let repeated = |offset| {
            let mut image = fixture(32, 0, 100);
            for (row, pixels) in image.pixels_mut().chunks_exact_mut(128).enumerate() {
                pixels.fill(((row + offset) % 8) as u8);
            }
            image
        };
        let mut s = Stitcher::new(repeated(0)).unwrap();
        assert_eq!(s.accept(&repeated(3), true).unwrap(), Step::Unmatched);
        assert_eq!(
            s.accept(&fixture(32, 510, 100), true).unwrap(),
            Step::Unmatched
        );
        assert_eq!(s.image().pixels(), repeated(0).pixels());
    }

    #[test]
    fn enforces_both_limits_and_maps_bottom_view_in_original_pixels() {
        assert_eq!(MAX_HEIGHT.min(MAX_BYTES / (4096 * 4)), 16_384);
        let mut s = Stitcher::new(fixture(2, 0, 16_384)).unwrap();
        assert_eq!(
            s.accept(&fixture(2, 8_192, 16_384), true).unwrap(),
            Step::Added
        );
        assert_eq!(
            s.accept(&fixture(2, 16_384, 16_384), true).unwrap(),
            Step::Added
        );
        assert_eq!(
            s.accept(&fixture(2, 16_385, 16_384), true).unwrap(),
            Step::Limit
        );
        assert_eq!(s.image().bounds().bottom, MAX_HEIGHT as i32);
        let mut v = View::new(
            PixelRect {
                left: -300,
                top: 10,
                right: -100,
                bottom: 410,
            },
            PixelRect {
                left: 0,
                top: 0,
                right: 400,
                bottom: 32_768,
            },
        )
        .unwrap();
        v.offset = v.max_offset();
        assert_eq!(v.band().bottom, 32_768);
        let p = PixelPoint { x: -101, y: 409 };
        assert_eq!(v.to_screen(v.to_document(p)), p);
    }
}
