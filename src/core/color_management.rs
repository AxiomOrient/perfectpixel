use serde::{Deserialize, Serialize};

use super::{AlphaMode, ColorSpec, PixelFormat, PixelSpec, PpError, PpResult, Raster, Sha256Digest};

const MAX_ICC_PROFILE_BYTES: usize = 16 * 1024 * 1024;

/// Evidence shape retained for callers that persist color-operation receipts. This minimal build
/// deliberately does not provide an ICC conversion engine; a successful receipt is therefore not
/// produced until an explicit color Effect is added again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ColorTransformReceipt {
    pub source_profile_sha256: Sha256Digest,
    pub destination: ColorSpec,
    pub pixel_count: u64,
}

/// Validates ICC provenance and then fails closed because this dependency-minimal build has no ICC
/// transform engine. Unknown/ICC pixels are never silently treated as sRGB.
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
            "ICC transform boundary accepts RGBA8 pixels only".to_string(),
        ));
    }
    if pixel_spec.alpha == AlphaMode::Premultiplied {
        return Err(PpError::InvalidRequest(
            "ICC transform boundary requires straight or opaque alpha".to_string(),
        ));
    }

    let observed = Sha256Digest::from_bytes(icc_profile);
    match &pixel_spec.color {
        ColorSpec::Icc { digest } if digest == &observed => {}
        ColorSpec::Icc { .. } => {
            return Err(PpError::PreconditionFailed {
                operation: "color.transform_icc".to_string(),
                cause: "ICC bytes do not match PixelSpec profile digest".to_string(),
            });
        }
        _ => {
            return Err(PpError::InvalidRequest(
                "ICC transform requires explicit PixelSpec::Icc provenance".to_string(),
            ));
        }
    }

    let _ = raster;
    Err(PpError::Unsupported {
        operation: "color.transform_icc".to_string(),
        cause: "embedded ICC conversion is not included in the dependency-minimal build; use an explicitly declared unprofiled sRGB/linear source or add a bounded color Effect"
            .to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icc_digest_mismatch_is_a_precondition_failure() -> PpResult<()> {
        let raster = Raster::new(1, 1, vec![1, 2, 3, 255])?;
        let spec = PixelSpec::new(
            PixelFormat::Rgba8,
            AlphaMode::Opaque,
            ColorSpec::Icc {
                digest: Sha256Digest::from_bytes(b"different"),
            },
        );
        assert!(matches!(
            transform_icc_rgba8_to_srgb(&raster, &spec, b"profile"),
            Err(PpError::PreconditionFailed { .. })
        ));
        Ok(())
    }

    #[test]
    fn matching_icc_provenance_is_explicitly_unsupported_not_silently_normalized() -> PpResult<()> {
        let profile = b"profile";
        let raster = Raster::new(1, 1, vec![1, 2, 3, 255])?;
        let spec = PixelSpec::new(
            PixelFormat::Rgba8,
            AlphaMode::Opaque,
            ColorSpec::Icc {
                digest: Sha256Digest::from_bytes(profile),
            },
        );
        assert!(matches!(
            transform_icc_rgba8_to_srgb(&raster, &spec, profile),
            Err(PpError::Unsupported { .. })
        ));
        Ok(())
    }
}
