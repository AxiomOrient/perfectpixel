use fast_image_resize as fir;

use super::{PpError, PpResult, Raster};

/// Resampling algorithms with stable pixel semantics for the public raster core.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResampleFilter {
    /// Copies the nearest source pixel. This is appropriate for pixel-art assets.
    Nearest,
    /// High-quality Lanczos3 resampling with alpha-aware interpolation.
    Lanczos3,
}

/// Resizes an RGBA raster without coupling the image-processing core to a file format.
///
/// The caller owns output-dimension policy. Zero-sized outputs are rejected because no
/// image encoder has a useful portable representation for them.
pub fn resize_raster(
    image: &Raster,
    width: u32,
    height: u32,
    filter: ResampleFilter,
) -> PpResult<Raster> {
    if width == 0 || height == 0 {
        return Err(PpError::InvalidRequest(
            "resize dimensions must be positive".to_string(),
        ));
    }
    if image.width() == width && image.height() == height {
        return Ok(image.clone());
    }

    match filter {
        ResampleFilter::Nearest => resize_nearest(image, width, height),
        ResampleFilter::Lanczos3 => resize_lanczos3(image, width, height),
    }
}

fn resize_nearest(image: &Raster, width: u32, height: u32) -> PpResult<Raster> {
    let mut pixels = Vec::with_capacity(
        usize::try_from(width)
            .ok()
            .and_then(|value| value.checked_mul(usize::try_from(height).ok()?))
            .and_then(|value| value.checked_mul(4))
            .ok_or_else(|| {
                PpError::InvalidRequest(format!("image dimensions overflow: {width}x{height}"))
            })?,
    );
    for y in 0..height {
        let source_y = ((u64::from(y) * u64::from(image.height())) / u64::from(height)) as u32;
        for x in 0..width {
            let source_x = ((u64::from(x) * u64::from(image.width())) / u64::from(width)) as u32;
            let source_index =
                ((source_y as usize) * (image.width() as usize) + source_x as usize) * 4;
            pixels.extend_from_slice(&image.pixels()[source_index..source_index + 4]);
        }
    }
    Raster::new(width, height, pixels)
}

fn resize_lanczos3(image: &Raster, width: u32, height: u32) -> PpResult<Raster> {
    let source = fir::images::Image::from_vec_u8(
        image.width(),
        image.height(),
        image.pixels().to_vec(),
        fir::PixelType::U8x4,
    )
    .map_err(resize_error)?;
    let mut output = fir::images::Image::new(width, height, fir::PixelType::U8x4);
    let options = fir::ResizeOptions::new()
        .resize_alg(fir::ResizeAlg::Convolution(fir::FilterType::Lanczos3))
        .use_alpha(true);
    fir::Resizer::new()
        .resize(&source, &mut output, &options)
        .map_err(resize_error)?;
    Raster::new(width, height, output.into_vec())
}

fn resize_error(error: impl std::fmt::Display) -> PpError {
    PpError::InvalidRequest(format!("raster resize failed: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearest_upscale_repeats_source_pixels_exactly() {
        let image = Raster::new(2, 1, vec![1, 2, 3, 255, 4, 5, 6, 255]).unwrap();
        let resized = resize_raster(&image, 4, 1, ResampleFilter::Nearest).unwrap();
        assert_eq!(
            resized.pixels(),
            &[1, 2, 3, 255, 1, 2, 3, 255, 4, 5, 6, 255, 4, 5, 6, 255]
        );
    }

    #[test]
    fn lanczos_resize_preserves_transparent_edge_without_hidden_color_halo() {
        let image = Raster::new(2, 1, vec![255, 0, 0, 255, 0, 255, 0, 0]).unwrap();
        let resized = resize_raster(&image, 1, 1, ResampleFilter::Lanczos3).unwrap();
        assert!(resized.pixels()[0] > resized.pixels()[1]);
        assert!(resized.pixels()[3] > 0 && resized.pixels()[3] < 255);
    }

    #[test]
    fn resize_rejects_zero_dimension() {
        let image = Raster::blank(1, 1).unwrap();
        assert!(matches!(
            resize_raster(&image, 0, 1, ResampleFilter::Nearest),
            Err(PpError::InvalidRequest(_))
        ));
    }
}
