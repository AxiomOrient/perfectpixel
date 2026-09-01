use serde::{Deserialize, Serialize};

use super::{
    delta_e2000, srgb8_to_lab, ArtifactRef, PixelSpec, PpError, PpResult, Raster, Sha256Digest,
};

pub const VERIFICATION_REPORT_SCHEMA: &str = "perfectpixel.verification-report/1";
const MAX_SRGB_DELTA_E_MILLI: usize = 200_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VerificationSpec {
    pub exact: Vec<ExactAssertion>,
    #[serde(default)]
    pub perceptual: Vec<PerceptualAssertion>,
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
        for assertion in &self.perceptual {
            match assertion {
                PerceptualAssertion::DeltaE2000 {
                    mean_milli_max,
                    p95_milli_max,
                    max_milli_max,
                } if mean_milli_max > p95_milli_max || p95_milli_max > max_milli_max => {
                    return Err(PpError::InvalidRequest(
                        "DeltaE2000 thresholds must satisfy mean <= p95 <= max".to_string(),
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PerceptualAssertion {
    /// Thresholds are ΔE00 * 1000. A value of 1500 means ΔE00 <= 1.5.
    DeltaE2000 {
        mean_milli_max: u32,
        p95_milli_max: u32,
        max_milli_max: u32,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerificationReport {
    pub schema: &'static str,
    pub ok: bool,
    pub exact: Vec<ExactCheck>,
    pub perceptual: Vec<PerceptualCheck>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExactCheck {
    pub assertion: ExactAssertion,
    pub passed: bool,
    pub evidence: ExactEvidence,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ExactEvidence {
    Dimensions { width: u32, height: u32 },
    PixelSpec { actual: PixelSpec },
    AlphaBounds { minimum: u8, maximum: u8 },
    ArtifactSha256 { actual: Option<Sha256Digest> },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PerceptualCheck {
    pub assertion: PerceptualAssertion,
    pub passed: bool,
    pub evidence: PerceptualEvidence,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PerceptualEvidence {
    MissingReference,
    DimensionMismatch {
        actual_width: u32,
        actual_height: u32,
        reference_width: u32,
        reference_height: u32,
    },
    DeltaE2000 {
        mean_milli: u32,
        p95_milli: u32,
        max_milli: u32,
        compared_pixels: u64,
        alpha_support_mismatch_pixels: u64,
    },
}

pub fn verify_raster_exact(
    spec: &VerificationSpec,
    raster: &Raster,
    pixel_spec: &PixelSpec,
    artifact: Option<&ArtifactRef>,
) -> PpResult<VerificationReport> {
    if !spec.perceptual.is_empty() {
        return Err(PpError::InvalidRequest(
            "verify_raster_exact does not accept perceptual assertions; use verify_raster"
                .to_string(),
        ));
    }
    verify_raster(spec, raster, pixel_spec, artifact, None)
}

pub fn verify_raster(
    spec: &VerificationSpec,
    raster: &Raster,
    pixel_spec: &PixelSpec,
    artifact: Option<&ArtifactRef>,
    reference: Option<&Raster>,
) -> PpResult<VerificationReport> {
    spec.validate()?;
    let observed_alpha = alpha_bounds(raster);
    let mut exact = Vec::with_capacity(spec.exact.len());
    for assertion in &spec.exact {
        let (passed, evidence) = match assertion {
            ExactAssertion::Dimensions { width, height } => (
                raster.width() == *width && raster.height() == *height,
                ExactEvidence::Dimensions {
                    width: raster.width(),
                    height: raster.height(),
                },
            ),
            ExactAssertion::PixelSpec { expected } => (
                pixel_spec == expected,
                ExactEvidence::PixelSpec {
                    actual: pixel_spec.clone(),
                },
            ),
            ExactAssertion::AlphaBounds { minimum, maximum } => (
                observed_alpha.0 >= *minimum && observed_alpha.1 <= *maximum,
                ExactEvidence::AlphaBounds {
                    minimum: observed_alpha.0,
                    maximum: observed_alpha.1,
                },
            ),
            ExactAssertion::ArtifactSha256 { expected } => {
                let actual = artifact.map(|artifact| artifact.sha256().clone());
                (
                    actual.as_ref() == Some(expected),
                    ExactEvidence::ArtifactSha256 { actual },
                )
            }
        };
        exact.push(ExactCheck {
            assertion: assertion.clone(),
            passed,
            evidence,
        });
    }

    let perceptual = spec
        .perceptual
        .iter()
        .map(|assertion| evaluate_perceptual(assertion, raster, reference))
        .collect::<PpResult<Vec<_>>>()?;
    let ok = exact.iter().all(|check| check.passed)
        && perceptual.iter().all(|check| check.passed);
    Ok(VerificationReport {
        schema: VERIFICATION_REPORT_SCHEMA,
        ok,
        exact,
        perceptual,
    })
}

fn evaluate_perceptual(
    assertion: &PerceptualAssertion,
    actual: &Raster,
    reference: Option<&Raster>,
) -> PpResult<PerceptualCheck> {
    let Some(reference) = reference else {
        return Ok(PerceptualCheck {
            assertion: assertion.clone(),
            passed: false,
            evidence: PerceptualEvidence::MissingReference,
        });
    };
    if actual.width() != reference.width() || actual.height() != reference.height() {
        return Ok(PerceptualCheck {
            assertion: assertion.clone(),
            passed: false,
            evidence: PerceptualEvidence::DimensionMismatch {
                actual_width: actual.width(),
                actual_height: actual.height(),
                reference_width: reference.width(),
                reference_height: reference.height(),
            },
        });
    }
    match assertion {
        PerceptualAssertion::DeltaE2000 {
            mean_milli_max,
            p95_milli_max,
            max_milli_max,
        } => {
            let metrics = delta_e_metrics(actual, reference)?;
            let passed = metrics.alpha_support_mismatch_pixels == 0
                && metrics.mean_milli <= *mean_milli_max
                && metrics.p95_milli <= *p95_milli_max
                && metrics.max_milli <= *max_milli_max;
            Ok(PerceptualCheck {
                assertion: assertion.clone(),
                passed,
                evidence: PerceptualEvidence::DeltaE2000 {
                    mean_milli: metrics.mean_milli,
                    p95_milli: metrics.p95_milli,
                    max_milli: metrics.max_milli,
                    compared_pixels: metrics.compared_pixels,
                    alpha_support_mismatch_pixels: metrics.alpha_support_mismatch_pixels,
                },
            })
        }
    }
}

struct DeltaEMetrics {
    mean_milli: u32,
    p95_milli: u32,
    max_milli: u32,
    compared_pixels: u64,
    alpha_support_mismatch_pixels: u64,
}

fn delta_e_metrics(actual: &Raster, reference: &Raster) -> PpResult<DeltaEMetrics> {
    let mut histogram = vec![0u64; MAX_SRGB_DELTA_E_MILLI + 1];
    let mut sum = 0u64;
    let mut max = 0u32;
    let mut compared = 0u64;
    let mut alpha_support_mismatch = 0u64;
    for (actual_pixel, reference_pixel) in actual
        .pixels()
        .chunks_exact(4)
        .zip(reference.pixels().chunks_exact(4))
    {
        let actual_present = actual_pixel[3] != 0;
        let reference_present = reference_pixel[3] != 0;
        if actual_present != reference_present {
            alpha_support_mismatch = alpha_support_mismatch.saturating_add(1);
        }
        if !actual_present || !reference_present {
            continue;
        }
        let difference = delta_e2000(
            srgb8_to_lab([actual_pixel[0], actual_pixel[1], actual_pixel[2]]),
            srgb8_to_lab([reference_pixel[0], reference_pixel[1], reference_pixel[2]]),
        );
        if !difference.is_finite() || difference < 0.0 {
            return Err(PpError::InvalidRequest(
                "DeltaE2000 produced a non-finite metric".to_string(),
            ));
        }
        let milli = (difference * 1000.0).round().clamp(0.0, u32::MAX as f64) as u32;
        let bucket = (milli as usize).min(MAX_SRGB_DELTA_E_MILLI);
        histogram[bucket] = histogram[bucket].saturating_add(1);
        sum = sum.saturating_add(u64::from(milli));
        max = max.max(milli);
        compared = compared.saturating_add(1);
    }
    let mean = if compared == 0 {
        0
    } else {
        ((sum + compared / 2) / compared).min(u64::from(u32::MAX)) as u32
    };
    let p95 = percentile95(&histogram, compared);
    Ok(DeltaEMetrics {
        mean_milli: mean,
        p95_milli: p95,
        max_milli: max,
        compared_pixels: compared,
        alpha_support_mismatch_pixels: alpha_support_mismatch,
    })
}

fn percentile95(histogram: &[u64], samples: u64) -> u32 {
    if samples == 0 {
        return 0;
    }
    let target = (samples.saturating_mul(95).saturating_add(99)) / 100;
    let mut cumulative = 0u64;
    for (index, &count) in histogram.iter().enumerate() {
        cumulative = cumulative.saturating_add(count);
        if cumulative >= target {
            return index as u32;
        }
    }
    (histogram.len() - 1) as u32
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

    fn exact_spec(assertions: Vec<ExactAssertion>) -> VerificationSpec {
        VerificationSpec {
            exact: assertions,
            perceptual: Vec::new(),
        }
    }

    #[test]
    fn exact_verification_reports_each_invariant() -> crate::PpResult<()> {
        let raster = Raster::new(2, 1, vec![255, 0, 0, 255, 0, 0, 0, 0])?;
        let pixel_spec = PixelSpec::new(PixelFormat::Rgba8, AlphaMode::Straight, ColorSpec::Srgb);
        let artifact = ArtifactRef::from_bytes("image/png", b"encoded")?;
        let spec = exact_spec(vec![
            ExactAssertion::Dimensions { width: 2, height: 1 },
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
        ]);
        let report = verify_raster_exact(&spec, &raster, &pixel_spec, Some(&artifact))?;
        assert!(report.ok);
        assert_eq!(report.exact.len(), 4);
        Ok(())
    }

    #[test]
    fn exact_verification_fails_closed_when_artifact_evidence_is_missing() -> crate::PpResult<()> {
        let raster = Raster::blank(1, 1)?;
        let pixel_spec = PixelSpec::new(PixelFormat::Rgba8, AlphaMode::Straight, ColorSpec::Unknown);
        let spec = exact_spec(vec![ExactAssertion::ArtifactSha256 {
            expected: Sha256Digest::from_bytes(b"expected"),
        }]);
        let report = verify_raster_exact(&spec, &raster, &pixel_spec, None)?;
        assert!(!report.ok);
        assert!(!report.exact[0].passed);
        Ok(())
    }

    #[test]
    fn delta_e_requires_alpha_support_to_match() -> crate::PpResult<()> {
        let actual = Raster::new(1, 1, vec![255, 0, 0, 0])?;
        let reference = Raster::new(1, 1, vec![255, 0, 0, 255])?;
        let pixel_spec = PixelSpec::new(PixelFormat::Rgba8, AlphaMode::Straight, ColorSpec::Srgb);
        let spec = VerificationSpec {
            exact: Vec::new(),
            perceptual: vec![PerceptualAssertion::DeltaE2000 {
                mean_milli_max: 0,
                p95_milli_max: 0,
                max_milli_max: 0,
            }],
        };
        let report = verify_raster(&spec, &actual, &pixel_spec, None, Some(&reference))?;
        assert!(!report.ok);
        assert!(!report.perceptual[0].passed);
        Ok(())
    }

    #[test]
    fn verification_spec_rejects_invalid_bounds() -> crate::PpResult<()> {
        let raster = Raster::blank(1, 1)?;
        let pixel_spec = PixelSpec::new(PixelFormat::Rgba8, AlphaMode::Straight, ColorSpec::Unknown);
        let spec = exact_spec(vec![ExactAssertion::AlphaBounds {
            minimum: 200,
            maximum: 100,
        }]);
        assert!(verify_raster_exact(&spec, &raster, &pixel_spec, None).is_err());
        Ok(())
    }
}
