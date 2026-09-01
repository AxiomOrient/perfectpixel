use serde::{Deserialize, Serialize};

use super::{ArtifactRef, PixelSpec, PpError, PpResult, Raster, Sha256Digest};

pub const VERIFICATION_REPORT_SCHEMA: &str = "perfectpixel.verification-report/1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VerificationSpec {
    pub exact: Vec<ExactAssertion>,
}

impl VerificationSpec {
    pub fn validate(&self) -> PpResult<()> {
        for assertion in &self.exact {
            match assertion {
                ExactAssertion::Dimensions { width, height } if *width == 0 || *height == 0 => {
                    return Err(PpError::InvalidRequest(
                        "verification dimensions must be positive".to_string(),
                    ));
                }
                ExactAssertion::AlphaBounds { minimum, maximum } if minimum > maximum => {
                    return Err(PpError::InvalidRequest(
                        "verification alpha minimum must not exceed maximum".to_string(),
                    ));
                }
                _ => {}
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExactAssertion {
    Dimensions { width: u32, height: u32 },
    PixelSpec { expected: PixelSpec },
    AlphaBounds { minimum: u8, maximum: u8 },
    ArtifactSha256 { expected: Sha256Digest },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerificationReport {
    pub schema: &'static str,
    pub ok: bool,
    pub checks: Vec<VerificationCheck>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerificationCheck {
    pub assertion: ExactAssertion,
    pub passed: bool,
    pub evidence: VerificationEvidence,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum VerificationEvidence {
    Dimensions { width: u32, height: u32 },
    PixelSpec { actual: PixelSpec },
    AlphaBounds { minimum: u8, maximum: u8 },
    ArtifactSha256 { actual: Option<Sha256Digest> },
}

pub fn verify_raster_exact(
    spec: &VerificationSpec,
    raster: &Raster,
    pixel_spec: &PixelSpec,
    artifact: Option<&ArtifactRef>,
) -> PpResult<VerificationReport> {
    spec.validate()?;
    let observed_alpha = alpha_bounds(raster);
    let mut checks = Vec::with_capacity(spec.exact.len());

    for assertion in &spec.exact {
        let (passed, evidence) = match assertion {
            ExactAssertion::Dimensions { width, height } => (
                raster.width() == *width && raster.height() == *height,
                VerificationEvidence::Dimensions {
                    width: raster.width(),
                    height: raster.height(),
                },
            ),
            ExactAssertion::PixelSpec { expected } => (
                pixel_spec == expected,
                VerificationEvidence::PixelSpec {
                    actual: pixel_spec.clone(),
                },
            ),
            ExactAssertion::AlphaBounds { minimum, maximum } => (
                observed_alpha.0 >= *minimum && observed_alpha.1 <= *maximum,
                VerificationEvidence::AlphaBounds {
                    minimum: observed_alpha.0,
                    maximum: observed_alpha.1,
                },
            ),
            ExactAssertion::ArtifactSha256 { expected } => {
                let actual = artifact.map(|artifact| artifact.sha256().clone());
                (
                    actual.as_ref() == Some(expected),
                    VerificationEvidence::ArtifactSha256 { actual },
                )
            }
        };
        checks.push(VerificationCheck {
            assertion: assertion.clone(),
            passed,
            evidence,
        });
    }

    Ok(VerificationReport {
        schema: VERIFICATION_REPORT_SCHEMA,
        ok: checks.iter().all(|check| check.passed),
        checks,
    })
}

fn alpha_bounds(raster: &Raster) -> (u8, u8) {
    let mut minimum = u8::MAX;
    let mut maximum = u8::MIN;
    for pixel in raster.pixels().chunks_exact(4) {
        minimum = minimum.min(pixel[3]);
        maximum = maximum.max(pixel[3]);
    }
    (minimum, maximum)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{AlphaMode, ColorSpec, PixelFormat};

    #[test]
    fn exact_verification_reports_each_invariant() -> crate::PpResult<()> {
        let raster = Raster::new(2, 1, vec![255, 0, 0, 255, 0, 0, 0, 0])?;
        let pixel_spec = PixelSpec::new(PixelFormat::Rgba8, AlphaMode::Straight, ColorSpec::Srgb);
        let artifact = ArtifactRef::from_bytes("image/png", b"encoded")?;
        let spec = VerificationSpec {
            exact: vec![
                ExactAssertion::Dimensions {
                    width: 2,
                    height: 1,
                },
                ExactAssertion::PixelSpec {
                    expected: pixel_spec.clone(),
                },
                ExactAssertion::AlphaBounds {
                    minimum: 0,
                    maximum: 255,
                },
                ExactAssertion::ArtifactSha256 {
                    expected: artifact.sha256().clone(),
                },
            ],
        };

        let report = verify_raster_exact(&spec, &raster, &pixel_spec, Some(&artifact))?;
        assert!(report.ok);
        assert_eq!(report.checks.len(), 4);
        Ok(())
    }

    #[test]
    fn exact_verification_fails_closed_when_artifact_evidence_is_missing() -> crate::PpResult<()> {
        let raster = Raster::blank(1, 1)?;
        let pixel_spec = PixelSpec::new(PixelFormat::Rgba8, AlphaMode::Straight, ColorSpec::Unknown);
        let spec = VerificationSpec {
            exact: vec![ExactAssertion::ArtifactSha256 {
                expected: Sha256Digest::from_bytes(b"expected"),
            }],
        };

        let report = verify_raster_exact(&spec, &raster, &pixel_spec, None)?;
        assert!(!report.ok);
        assert!(!report.checks[0].passed);
        Ok(())
    }

    #[test]
    fn verification_spec_rejects_invalid_bounds() -> crate::PpResult<()> {
        let raster = Raster::blank(1, 1)?;
        let pixel_spec = PixelSpec::new(PixelFormat::Rgba8, AlphaMode::Straight, ColorSpec::Unknown);
        let spec = VerificationSpec {
            exact: vec![ExactAssertion::AlphaBounds {
                minimum: 200,
                maximum: 100,
            }],
        };
        assert!(verify_raster_exact(&spec, &raster, &pixel_spec, None).is_err());
        Ok(())
    }
}
