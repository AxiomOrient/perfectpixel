use serde::{Deserialize, Serialize};

use super::Sha256Digest;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PixelFormat {
    Rgba8,
    Rgba16,
    RgbaF32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlphaMode {
    Opaque,
    Straight,
    Premultiplied,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ColorSpec {
    Srgb,
    LinearSrgb,
    DisplayP3,
    Icc { digest: Sha256Digest },
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PixelSpec {
    pub pixel_format: PixelFormat,
    pub alpha: AlphaMode,
    pub color: ColorSpec,
}

impl PixelSpec {
    pub const fn new(pixel_format: PixelFormat, alpha: AlphaMode, color: ColorSpec) -> Self {
        Self {
            pixel_format,
            alpha,
            color,
        }
    }

    pub const fn rgba8_srgb_straight() -> Self {
        Self::new(PixelFormat::Rgba8, AlphaMode::Straight, ColorSpec::Srgb)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pixel_spec_has_no_implicit_default() {
        let spec = PixelSpec::rgba8_srgb_straight();
        assert_eq!(spec.pixel_format, PixelFormat::Rgba8);
        assert_eq!(spec.alpha, AlphaMode::Straight);
        assert_eq!(spec.color, ColorSpec::Srgb);
    }
}
