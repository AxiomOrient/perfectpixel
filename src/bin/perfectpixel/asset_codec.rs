use std::path::Path;

use image::{
    codecs::{
        jpeg::JpegEncoder as ImageJpegEncoder, png::PngEncoder as ImagePngEncoder,
        webp::WebPEncoder as ImageWebPEncoder,
    },
    ExtendedColorType, ImageEncoder,
};
use perfectpixel::{PpError, PpResult, Raster};

pub(super) const DEFAULT_JPEG_QUALITY: u8 = 85;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AssetOutputFormat {
    Png,
    Jpeg,
    Webp,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct AssetEncodeOptions {
    pub format: AssetOutputFormat,
    pub jpeg_quality: u8,
    pub background: Option<[u8; 3]>,
}

pub(super) fn output_format(path: &Path) -> PpResult<AssetOutputFormat> {
    match path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase())
        .as_deref()
    {
        Some("png") => Ok(AssetOutputFormat::Png),
        Some("jpg") | Some("jpeg") => Ok(AssetOutputFormat::Jpeg),
        Some("webp") => Ok(AssetOutputFormat::Webp),
        _ => Err(PpError::InvalidRequest(
            "asset output must use one of these extensions: png, jpg, jpeg, webp".to_string(),
        )),
    }
}

pub(super) fn encode_raster(image: &Raster, options: AssetEncodeOptions) -> PpResult<Vec<u8>> {
    match options.format {
        AssetOutputFormat::Png => encode_png(image),
        AssetOutputFormat::Jpeg => encode_jpeg(image, options.jpeg_quality, options.background),
        AssetOutputFormat::Webp => encode_webp(image),
    }
}

pub(super) fn parse_background(raw: &str) -> PpResult<[u8; 3]> {
    let Some(hex) = raw.strip_prefix('#') else {
        return Err(PpError::InvalidOption(
            "--background must be a #RRGGBB color".to_string(),
        ));
    };
    if hex.len() != 6 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(PpError::InvalidOption(
            "--background must be a #RRGGBB color".to_string(),
        ));
    }
    let channel = |range| u8::from_str_radix(&hex[range], 16).expect("validated hexadecimal");
    Ok([channel(0..2), channel(2..4), channel(4..6)])
}

fn encode_png(image: &Raster) -> PpResult<Vec<u8>> {
    let mut bytes = Vec::new();
    ImagePngEncoder::new(&mut bytes)
        .write_image(
            image.pixels(),
            image.width(),
            image.height(),
            ExtendedColorType::Rgba8,
        )
        .map_err(encode_error)?;
    Ok(bytes)
}

fn encode_jpeg(image: &Raster, quality: u8, background: Option<[u8; 3]>) -> PpResult<Vec<u8>> {
    let rgb = flatten_for_jpeg(image, background)?;
    let mut bytes = Vec::new();
    ImageJpegEncoder::new_with_quality(&mut bytes, quality)
        .write_image(&rgb, image.width(), image.height(), ExtendedColorType::Rgb8)
        .map_err(encode_error)?;
    Ok(bytes)
}

fn encode_webp(image: &Raster) -> PpResult<Vec<u8>> {
    let mut bytes = Vec::new();
    ImageWebPEncoder::new_lossless(&mut bytes)
        .write_image(
            image.pixels(),
            image.width(),
            image.height(),
            ExtendedColorType::Rgba8,
        )
        .map_err(encode_error)?;
    Ok(bytes)
}

fn flatten_for_jpeg(image: &Raster, background: Option<[u8; 3]>) -> PpResult<Vec<u8>> {
    let has_transparency = image.pixels().chunks_exact(4).any(|pixel| pixel[3] != 255);
    if has_transparency && background.is_none() {
        return Err(PpError::InvalidRequest(
            "JPEG output requires --background #RRGGBB when the input has transparency".to_string(),
        ));
    }

    let background = background.unwrap_or([0, 0, 0]);
    let mut rgb = Vec::with_capacity(image.pixels().len() / 4 * 3);
    for pixel in image.pixels().chunks_exact(4) {
        let alpha = f64::from(pixel[3]) / 255.0;
        for channel in 0..3 {
            let foreground = srgb_to_linear(pixel[channel]);
            let back = srgb_to_linear(background[channel]);
            rgb.push(linear_to_srgb(foreground * alpha + back * (1.0 - alpha)));
        }
    }
    Ok(rgb)
}

fn srgb_to_linear(value: u8) -> f64 {
    let value = f64::from(value) / 255.0;
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(value: f64) -> u8 {
    let value = value.clamp(0.0, 1.0);
    let value = if value <= 0.003_130_8 {
        value * 12.92
    } else {
        1.055 * value.powf(1.0 / 2.4) - 0.055
    };
    (value * 255.0).round() as u8
}

fn encode_error(error: impl std::fmt::Display) -> PpError {
    PpError::ImageEncode {
        message: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transparent_jpeg_requires_an_explicit_background() {
        let image = Raster::new(1, 1, vec![255, 0, 0, 127]).unwrap();
        assert!(matches!(
            encode_raster(
                &image,
                AssetEncodeOptions {
                    format: AssetOutputFormat::Jpeg,
                    jpeg_quality: DEFAULT_JPEG_QUALITY,
                    background: None,
                },
            ),
            Err(PpError::InvalidRequest(_))
        ));
    }

    #[test]
    fn webp_lossless_round_trips_rgba() {
        let image = Raster::new(1, 1, vec![10, 20, 30, 40]).unwrap();
        let bytes = encode_raster(
            &image,
            AssetEncodeOptions {
                format: AssetOutputFormat::Webp,
                jpeg_quality: DEFAULT_JPEG_QUALITY,
                background: None,
            },
        )
        .unwrap();
        let decoded = image::load_from_memory(&bytes).unwrap().into_rgba8();
        assert_eq!(decoded.as_raw(), image.pixels());
    }
}
