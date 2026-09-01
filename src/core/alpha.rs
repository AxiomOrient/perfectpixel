use std::collections::VecDeque;

use super::{FrameRect, PpError, PpResult, Raster};

/// Canonical single-channel alpha/mask storage. Values are straight coverage in 0..=255.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mask {
    width: u32,
    height: u32,
    values: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AlphaHistogram {
    pub bins: [u32; 256],
    pub transparent: u32,
    pub opaque: u32,
    pub partial: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectedComponent {
    pub bounds: FrameRect,
    pub pixels: u32,
}

impl Mask {
    pub fn new(width: u32, height: u32, values: Vec<u8>) -> PpResult<Self> {
        let len = mask_len(width, height)?;
        if values.len() != len {
            return Err(PpError::InvalidRequest(format!(
                "mask buffer length mismatch: got {}, expected {} for {}x{}",
                values.len(), len, width, height
            )));
        }
        Ok(Self { width, height, values })
    }

    pub fn blank(width: u32, height: u32) -> PpResult<Self> {
        let len = mask_len(width, height)?;
        Ok(Self {
            width,
            height,
            values: vec![0; len],
        })
    }

    pub fn from_raster_alpha(raster: &Raster) -> PpResult<Self> {
        let values = raster
            .pixels()
            .chunks_exact(4)
            .map(|pixel| pixel[3])
            .collect();
        Self::new(raster.width(), raster.height(), values)
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn values(&self) -> &[u8] {
        &self.values
    }

    pub fn threshold(&self, threshold: u8) -> Self {
        let values = self
            .values
            .iter()
            .map(|&value| if value >= threshold { 255 } else { 0 })
            .collect();
        Self {
            width: self.width,
            height: self.height,
            values,
        }
    }

    pub fn erode(&self, radius: u32) -> PpResult<Self> {
        morphology(self, radius, Morphology::Minimum)
    }

    pub fn dilate(&self, radius: u32) -> PpResult<Self> {
        morphology(self, radius, Morphology::Maximum)
    }

    pub fn open(&self, radius: u32) -> PpResult<Self> {
        self.erode(radius)?.dilate(radius)
    }

    pub fn close(&self, radius: u32) -> PpResult<Self> {
        self.dilate(radius)?.erode(radius)
    }

    /// Deterministic separable box feather. Radius zero is identity.
    pub fn feather(&self, radius: u32) -> PpResult<Self> {
        const MAX_RADIUS: u32 = 1024;
        if radius > MAX_RADIUS {
            return Err(PpError::InvalidRequest(format!(
                "feather radius {radius} exceeds {MAX_RADIUS}"
            )));
        }
        if radius == 0 {
            return Ok(self.clone());
        }
        let horizontal = box_blur_axis(self, radius, true)?;
        box_blur_axis(&horizontal, radius, false)
    }

    pub fn bounding_box(&self, threshold: u8) -> FrameRect {
        let mut min_x = self.width;
        let mut min_y = self.height;
        let mut max_x = None;
        let mut max_y = None;
        for y in 0..self.height {
            for x in 0..self.width {
                if self.get(x, y) < threshold {
                    continue;
                }
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = Some(max_x.map_or(x, |value: u32| value.max(x)));
                max_y = Some(max_y.map_or(y, |value: u32| value.max(y)));
            }
        }
        let (Some(max_x), Some(max_y)) = (max_x, max_y) else {
            return FrameRect::default();
        };
        FrameRect {
            x: min_x,
            y: min_y,
            w: max_x - min_x + 1,
            h: max_y - min_y + 1,
        }
    }

    pub fn histogram(&self) -> AlphaHistogram {
        let mut bins = [0u32; 256];
        for &value in &self.values {
            bins[value as usize] = bins[value as usize].saturating_add(1);
        }
        let transparent = bins[0];
        let opaque = bins[255];
        let partial = self
            .values
            .len()
            .saturating_sub(transparent as usize)
            .saturating_sub(opaque as usize) as u32;
        AlphaHistogram {
            bins,
            transparent,
            opaque,
            partial,
        }
    }

    /// Number of pixels meeting `threshold`, expressed as integer basis points.
    pub fn coverage_basis_points(&self, threshold: u8) -> u16 {
        let covered = self.values.iter().filter(|&&value| value >= threshold).count() as u64;
        let total = self.values.len() as u64;
        if total == 0 {
            return 0;
        }
        ((covered.saturating_mul(10_000) + total / 2) / total).min(10_000) as u16
    }

    /// Four-connected components over pixels meeting `threshold`, sorted deterministically by
    /// decreasing area and then top-left bounds.
    pub fn connected_components(&self, threshold: u8) -> Vec<ConnectedComponent> {
        let mut visited = vec![false; self.values.len()];
        let mut components = Vec::new();
        for y in 0..self.height {
            for x in 0..self.width {
                let start = self.index(x, y);
                if visited[start] || self.values[start] < threshold {
                    continue;
                }
                visited[start] = true;
                let mut queue = VecDeque::from([(x, y)]);
                let mut min_x = x;
                let mut min_y = y;
                let mut max_x = x;
                let mut max_y = y;
                let mut pixels = 0u32;
                while let Some((cx, cy)) = queue.pop_front() {
                    pixels = pixels.saturating_add(1);
                    min_x = min_x.min(cx);
                    min_y = min_y.min(cy);
                    max_x = max_x.max(cx);
                    max_y = max_y.max(cy);
                    for (nx, ny) in neighbors4(cx, cy, self.width, self.height) {
                        let index = self.index(nx, ny);
                        if !visited[index] && self.values[index] >= threshold {
                            visited[index] = true;
                            queue.push_back((nx, ny));
                        }
                    }
                }
                components.push(ConnectedComponent {
                    bounds: FrameRect {
                        x: min_x,
                        y: min_y,
                        w: max_x - min_x + 1,
                        h: max_y - min_y + 1,
                    },
                    pixels,
                });
            }
        }
        components.sort_by_key(|component| {
            (
                std::cmp::Reverse(component.pixels),
                component.bounds.y,
                component.bounds.x,
                component.bounds.h,
                component.bounds.w,
            )
        });
        components
    }

    /// Fills zero-valued holes that are not four-connected to an image edge. Non-zero alpha is
    /// preserved exactly; filled holes become fully opaque.
    pub fn fill_holes(&self) -> Self {
        let mut exterior = vec![false; self.values.len()];
        let mut queue = VecDeque::new();
        for x in 0..self.width {
            enqueue_zero(self, x, 0, &mut exterior, &mut queue);
            if self.height > 1 {
                enqueue_zero(self, x, self.height - 1, &mut exterior, &mut queue);
            }
        }
        for y in 0..self.height {
            enqueue_zero(self, 0, y, &mut exterior, &mut queue);
            if self.width > 1 {
                enqueue_zero(self, self.width - 1, y, &mut exterior, &mut queue);
            }
        }
        while let Some((x, y)) = queue.pop_front() {
            for (nx, ny) in neighbors4(x, y, self.width, self.height) {
                enqueue_zero(self, nx, ny, &mut exterior, &mut queue);
            }
        }
        let values = self
            .values
            .iter()
            .enumerate()
            .map(|(index, &value)| {
                if value == 0 && !exterior[index] {
                    255
                } else {
                    value
                }
            })
            .collect();
        Self {
            width: self.width,
            height: self.height,
            values,
        }
    }

    fn get(&self, x: u32, y: u32) -> u8 {
        self.values[self.index(x, y)]
    }

    fn index(&self, x: u32, y: u32) -> usize {
        y as usize * self.width as usize + x as usize
    }
}

/// Removes a known matte color from straight RGBA pixels and replaces alpha with the supplied
/// matte. This is deterministic algebra, not semantic segmentation. `foreground` and `matte`
/// must have identical dimensions.
pub fn decontaminate_known_background(
    foreground: &Raster,
    matte: &Mask,
    background: [u8; 3],
) -> PpResult<Raster> {
    if foreground.width() != matte.width() || foreground.height() != matte.height() {
        return Err(PpError::InvalidRequest(
            "foreground and matte dimensions must match".to_string(),
        ));
    }
    let mut pixels = foreground.pixels().to_vec();
    for (index, pixel) in pixels.chunks_exact_mut(4).enumerate() {
        let alpha = u32::from(matte.values[index]);
        pixel[3] = matte.values[index];
        if alpha == 0 {
            pixel[0] = 0;
            pixel[1] = 0;
            pixel[2] = 0;
            continue;
        }
        for channel in 0..3 {
            let observed = i64::from(pixel[channel]) * 255;
            let contamination = i64::from(background[channel]) * i64::from(255 - alpha);
            let unmatted = div_round_nearest(observed - contamination, i64::from(alpha));
            pixel[channel] = unmatted.clamp(0, 255) as u8;
        }
    }
    Raster::new(foreground.width(), foreground.height(), pixels)
}

fn div_round_nearest(numerator: i64, denominator: i64) -> i64 {
    debug_assert!(denominator > 0);
    if numerator >= 0 {
        (numerator + denominator / 2) / denominator
    } else {
        (numerator - denominator / 2) / denominator
    }
}

fn mask_len(width: u32, height: u32) -> PpResult<usize> {
    if width == 0 || height == 0 {
        return Err(PpError::InvalidRequest(format!(
            "mask dimensions must be positive: {width}x{height}"
        )));
    }
    (width as usize)
        .checked_mul(height as usize)
        .ok_or_else(|| PpError::InvalidRequest("mask dimensions overflow".to_string()))
}

#[derive(Clone, Copy)]
enum Morphology {
    Minimum,
    Maximum,
}

fn morphology(mask: &Mask, radius: u32, operation: Morphology) -> PpResult<Mask> {
    const MAX_RADIUS: u32 = 1024;
    if radius > MAX_RADIUS {
        return Err(PpError::InvalidRequest(format!(
            "morphology radius {radius} exceeds {MAX_RADIUS}"
        )));
    }
    if radius == 0 {
        return Ok(mask.clone());
    }
    let horizontal = morphology_axis(mask, radius, true, operation)?;
    morphology_axis(&horizontal, radius, false, operation)
}

fn morphology_axis(mask: &Mask, radius: u32, horizontal: bool, operation: Morphology) -> PpResult<Mask> {
    let mut out = Mask::blank(mask.width, mask.height)?;
    for y in 0..mask.height {
        for x in 0..mask.width {
            let start = if horizontal { x.saturating_sub(radius) } else { y.saturating_sub(radius) };
            let limit = if horizontal { mask.width } else { mask.height };
            let coordinate = if horizontal { x } else { y };
            let end = coordinate.saturating_add(radius).min(limit - 1);
            let mut value = match operation {
                Morphology::Minimum => u8::MAX,
                Morphology::Maximum => u8::MIN,
            };
            for offset in start..=end {
                let sample = if horizontal { mask.get(offset, y) } else { mask.get(x, offset) };
                value = match operation {
                    Morphology::Minimum => value.min(sample),
                    Morphology::Maximum => value.max(sample),
                };
            }
            let index = out.index(x, y);
            out.values[index] = value;
        }
    }
    Ok(out)
}

fn box_blur_axis(mask: &Mask, radius: u32, horizontal: bool) -> PpResult<Mask> {
    let mut out = Mask::blank(mask.width, mask.height)?;
    for y in 0..mask.height {
        for x in 0..mask.width {
            let coordinate = if horizontal { x } else { y };
            let limit = if horizontal { mask.width } else { mask.height };
            let start = coordinate.saturating_sub(radius);
            let end = coordinate.saturating_add(radius).min(limit - 1);
            let mut sum = 0u64;
            for offset in start..=end {
                sum += u64::from(if horizontal { mask.get(offset, y) } else { mask.get(x, offset) });
            }
            let count = u64::from(end - start + 1);
            let value = ((sum + count / 2) / count) as u8;
            let index = out.index(x, y);
            out.values[index] = value;
        }
    }
    Ok(out)
}

fn neighbors4(x: u32, y: u32, width: u32, height: u32) -> impl Iterator<Item = (u32, u32)> {
    let mut values = [(0, 0); 4];
    let mut count = 0usize;
    if x > 0 {
        values[count] = (x - 1, y);
        count += 1;
    }
    if x + 1 < width {
        values[count] = (x + 1, y);
        count += 1;
    }
    if y > 0 {
        values[count] = (x, y - 1);
        count += 1;
    }
    if y + 1 < height {
        values[count] = (x, y + 1);
        count += 1;
    }
    values.into_iter().take(count)
}

fn enqueue_zero(
    mask: &Mask,
    x: u32,
    y: u32,
    exterior: &mut [bool],
    queue: &mut VecDeque<(u32, u32)>,
) {
    let index = mask.index(x, y);
    if !exterior[index] && mask.values[index] == 0 {
        exterior[index] = true;
        queue.push_back((x, y));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn components_and_hole_fill_are_four_connected() -> PpResult<()> {
        let mask = Mask::new(
            5,
            5,
            vec![
                0, 0, 0, 0, 0,
                0, 255, 255, 255, 0,
                0, 255, 0, 255, 0,
                0, 255, 255, 255, 0,
                0, 0, 0, 0, 255,
            ],
        )?;
        let components = mask.connected_components(1);
        assert_eq!(components.len(), 2);
        assert_eq!(components[0].pixels, 8);
        let filled = mask.fill_holes();
        assert_eq!(filled.get(2, 2), 255);
        Ok(())
    }

    #[test]
    fn decontamination_unmixes_known_background() -> PpResult<()> {
        let foreground = Raster::new(1, 1, vec![255, 128, 128, 255])?;
        let matte = Mask::new(1, 1, vec![128])?;
        let result = decontaminate_known_background(&foreground, &matte, [255, 255, 255])?;
        assert_eq!(result.pixels()[3], 128);
        assert!(result.pixels()[1] <= 2);
        assert!(result.pixels()[2] <= 2);
        Ok(())
    }
}
