use serde::{Deserialize, Serialize};

use super::Sha256Digest;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PixelFormat {
    Rgba8,
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
    pub fn new(pixel_format: PixelFormat, alpha: AlphaMode, color: ColorSpec) -> Self {
        Self {
            pixel_format,
            alpha,
            color,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pixel_spec_requires_explicit_color_semantics() {
        let spec = PixelSpec::new(PixelFormat::Rgba8, AlphaMode::Straight, ColorSpec::Unknown);
        assert_eq!(spec.pixel_format, PixelFormat::Rgba8);
        assert_eq!(spec.alpha, AlphaMode::Straight);
        assert_eq!(spec.color, ColorSpec::Unknown);
    }
}
