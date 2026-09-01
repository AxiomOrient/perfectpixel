use moxcms::{ColorProfile, Layout, TransformOptions};
use serde::{Deserialize, Serialize};

use super::{AlphaMode, ColorSpec, PixelFormat, PixelSpec, PpError, PpResult, Raster, Sha256Digest};

const MAX_ICC_PROFILE_BYTES: usize = 16 * 1024 * 1024;

/// Machine-readable evidence for one deterministic ICC conversion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ColorTransformReceipt {
    pub source_profile_sha256: Sha256Digest,
    pub destination: ColorSpec,
    pub pixel_count: u64,
}

/// Converts one straight/opaque RGBA8 raster from the exact embedded ICC profile to sRGB.
///
/// The caller must provide the `PixelSpec` observed at decode time and the exact ICC bytes whose
/// digest appears in that spec. Unknown color is never guessed. Premultiplied RGB is rejected
/// because applying a color transform to premultiplied channel values would change semantics.
pub fn transform_icc_rgba8_to_srgb(
    raster: &Raster,
    pixel_spec: &PixelSpec,
    icc_profile: &[u8],
) -> PpResult<(Raster, PixelSpec, ColorTransformReceipt)> {
    if icc_profile.is_empty() || icc_profile.len() > MAX_ICC_PROFILE_BYTES {
        return Err(PpError::InvalidRequest(format!(
            "ICC profile size must be within 1..={MAX_ICC_PROFILE_BYTES} bytes"
        )));
    }
    if pixel_spec.pixel_format != PixelFormat::Rgba8 {
        return Err(PpError::InvalidRequest(
            "ICC transform supports RGBA8 pixels only".to_string(),
        ));
    }
    if pixel_spec.alpha == AlphaMode::Premultiplied {
        return Err(PpError::InvalidRequest(
            "ICC transform requires straight or opaque alpha; unpremultiply explicitly first"
                .to_string(),
        ));
    }

    let source_profile_sha256 = Sha256Digest::from_bytes(icc_profile);
    match &pixel_spec.color {
        ColorSpec::Icc { digest } if digest == &source_profile_sha256 => {}
        ColorSpec::Icc { .. } => {
            return Err(PpError::InvalidRequest(
                "ICC bytes do not match PixelSpec profile digest".to_string(),
            ));
        }
        _ => {
            return Err(PpError::InvalidRequest(
                "ICC transform requires PixelSpec::Icc; unknown or named color is not inferred"
                    .to_string(),
            ));
        }
    }

    let source_profile = ColorProfile::new_from_slice(icc_profile).map_err(|error| {
        PpError::InvalidRequest(format!("invalid ICC source profile: {error}"))
    })?;
    let destination_profile = ColorProfile::new_srgb();
    let transform = source_profile
        .create_transform_8bit(
            Layout::Rgba,
            &destination_profile,
            Layout::Rgba,
            TransformOptions::default(),
        )
        .map_err(|error| PpError::InvalidRequest(format!("ICC transform creation failed: {error}")))?;

    let mut converted = vec![0u8; raster.pixels().len()];
    transform
        .transform(raster.pixels(), &mut converted)
        .map_err(|error| PpError::InvalidRequest(format!("ICC transform failed: {error}")))?;

    // Alpha is semantic data, not a color component. Fail closed if a backend ever changes it.
    if raster
        .pixels()
        .chunks_exact(4)
        .zip(converted.chunks_exact(4))
        .any(|(source, destination)| source[3] != destination[3])
    {
        return Err(PpError::InvalidRequest(
            "ICC transform changed alpha channel".to_string(),
        ));
    }

    let output = Raster::new(raster.width(), raster.height(), converted)?;
    let output_spec = PixelSpec::new(PixelFormat::Rgba8, pixel_spec.alpha, ColorSpec::Srgb);
    let receipt = ColorTransformReceipt {
        source_profile_sha256,
        destination: ColorSpec::Srgb,
        pixel_count: u64::from(raster.width()) * u64::from(raster.height()),
    };
    Ok((output, output_spec, receipt))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transform_rejects_profile_digest_mismatch_before_parsing_profile() -> PpResult<()> {
        let raster = Raster::new(1, 1, vec![1, 2, 3, 255])?;
        let spec = PixelSpec::new(
            PixelFormat::Rgba8,
            AlphaMode::Opaque,
            ColorSpec::Icc {
                digest: Sha256Digest::from_bytes(b"different"),
            },
        );
        let error = transform_icc_rgba8_to_srgb(&raster, &spec, b"not-an-icc")
            .expect_err("digest mismatch must fail before ICC parsing");
        assert!(error.to_string().contains("do not match"));
        Ok(())
    }

    #[test]
    fn transform_rejects_implicit_color_assumptions() -> PpResult<()> {
        let raster = Raster::new(1, 1, vec![1, 2, 3, 255])?;
        let spec = PixelSpec::new(PixelFormat::Rgba8, AlphaMode::Opaque, ColorSpec::Unknown);
        let error = transform_icc_rgba8_to_srgb(&raster, &spec, b"profile")
            .expect_err("unknown color must not be inferred");
        assert!(error.to_string().contains("PixelSpec::Icc"));
        Ok(())
    }
}
