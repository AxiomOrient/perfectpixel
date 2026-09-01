use std::{
    num::{NonZeroU32, NonZeroUsize},
    path::PathBuf,
};

use crate::{
    parse_resample_filter, parse_vector_detail, parse_vector_preset, parse_vector_profile,
    JpegQuality, Operation, PpError, PpResult, ScaleFactor, SvgProfile, UnitScore,
    VectorPresetSelection,
};

use super::asset_codec::parse_background;

/// MCP transport DTO retained for protocol compatibility. It is not a semantic authority.
/// Conversion to the canonical `Operation` is pure and performs all transport-independent scalar
/// validation exactly once; no argv strings are reconstructed and no file I/O occurs here.
#[derive(Debug, Clone)]
pub enum ApplicationRequest {
    Schema,
    Inspect { input: PathBuf },
    Convert {
        input: PathBuf,
        output: PathBuf,
        width: Option<u32>,
        height: Option<u32>,
        filter: Option<String>,
        jpeg_quality: Option<u8>,
        background: Option<String>,
    },
    Upscale {
        input: PathBuf,
        output: PathBuf,
        scale: u32,
        filter: Option<String>,
        jpeg_quality: Option<u8>,
        background: Option<String>,
    },
    Normalize { request: PathBuf, output_dir: PathBuf },
    Bundle { request: PathBuf, output_dir: PathBuf },
    Vector {
        input: PathBuf,
        output: PathBuf,
        preset: Option<String>,
        profile: Option<String>,
        detail: Option<u8>,
        min_quality: Option<f64>,
        max_quality_loss: Option<f64>,
        max_paths: Option<usize>,
        policy: Option<PathBuf>,
        report: Option<PathBuf>,
        diagnostics: Option<PathBuf>,
    },
    VectorAnalyze {
        input: PathBuf,
        preset: Option<String>,
        profile: Option<String>,
        policy: Option<PathBuf>,
        report: Option<PathBuf>,
    },
    MotionScaffold { input: PathBuf, output_dir: PathBuf },
    MotionBuild { request: PathBuf, output_dir: PathBuf },
}

impl TryFrom<ApplicationRequest> for Operation {
    type Error = PpError;

    fn try_from(request: ApplicationRequest) -> PpResult<Self> {
        Ok(match request {
            ApplicationRequest::Schema => Self::Schema,
            ApplicationRequest::Inspect { input } => Self::Inspect { input },
            ApplicationRequest::Convert { input, output, width, height, filter, jpeg_quality, background } => Self::Convert {
                input,
                output,
                width: optional_nonzero(width, "width")?,
                height: optional_nonzero(height, "height")?,
                filter: filter
                    .as_deref()
                    .map(parse_resample_filter)
                    .transpose()
                    .map_err(operation_input_error)?,
                jpeg_quality: optional_jpeg_quality(jpeg_quality)?,
                background: background.as_deref().map(parse_background).transpose()?,
            },
            ApplicationRequest::Upscale { input, output, scale, filter, jpeg_quality, background } => Self::Upscale {
                input,
                output,
                scale: ScaleFactor::new(scale).map_err(operation_input_error)?,
                filter: filter
                    .as_deref()
                    .map(parse_resample_filter)
                    .transpose()
                    .map_err(operation_input_error)?,
                jpeg_quality: optional_jpeg_quality(jpeg_quality)?,
                background: background.as_deref().map(parse_background).transpose()?,
            },
            ApplicationRequest::Normalize { request, output_dir } => Self::NormalizeSprite { request, output_dir },
            ApplicationRequest::Bundle { request, output_dir } => Self::CompileSprite { request, output_dir },
            ApplicationRequest::Vector {
                input, output, preset, profile, detail, min_quality, max_quality_loss, max_paths,
                policy, report, diagnostics,
            } => Self::CompileVector {
                input,
                output,
                preset: preset
                    .as_deref()
                    .map(parse_vector_preset)
                    .transpose()
                    .map_err(operation_input_error)?
                    .unwrap_or(VectorPresetSelection::Auto),
                profile: profile
                    .as_deref()
                    .map(parse_vector_profile)
                    .transpose()
                    .map_err(operation_input_error)?
                    .unwrap_or(SvgProfile::Compact),
                detail: detail
                    .map(|value| parse_vector_detail(&value.to_string()))
                    .transpose()
                    .map_err(operation_input_error)?
                    .flatten(),
                minimum_quality: min_quality
                    .map(|value| UnitScore::new(value).map_err(|error| error.to_string()))
                    .transpose()
                    .map_err(PpError::InvalidOption)?,
                maximum_quality_loss: max_quality_loss
                    .map(|value| UnitScore::new(value).map_err(|error| error.to_string()))
                    .transpose()
                    .map_err(PpError::InvalidOption)?,
                maximum_paths: max_paths
                    .map(|value| NonZeroUsize::new(value).ok_or_else(|| PpError::InvalidOption("--max-paths must be a positive integer".to_string())))
                    .transpose()?,
                policy,
                report,
                diagnostics,
            },
            ApplicationRequest::VectorAnalyze { input, preset, profile, policy, report } => Self::AnalyzeVector {
                input,
                preset: preset
                    .as_deref()
                    .map(parse_vector_preset)
                    .transpose()
                    .map_err(operation_input_error)?
                    .unwrap_or(VectorPresetSelection::Auto),
                profile: profile
                    .as_deref()
                    .map(parse_vector_profile)
                    .transpose()
                    .map_err(operation_input_error)?
                    .unwrap_or(SvgProfile::Compact),
                policy,
                report,
            },
            ApplicationRequest::MotionScaffold { input, output_dir } => Self::ScaffoldMotion { input, output_dir },
            ApplicationRequest::MotionBuild { request, output_dir } => Self::CompileMotion { request, output_dir },
        })
    }
}

fn optional_nonzero(value: Option<u32>, label: &str) -> PpResult<Option<NonZeroU32>> {
    value
        .map(|value| NonZeroU32::new(value).ok_or_else(|| PpError::InvalidOption(format!("{label} must be a positive integer"))))
        .transpose()
}

fn optional_jpeg_quality(value: Option<u8>) -> PpResult<Option<JpegQuality>> {
    value.map(JpegQuality::new).transpose().map_err(operation_input_error)
}

fn operation_input_error(error: impl std::fmt::Display) -> PpError {
    PpError::InvalidOption(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ResampleFilter;

    #[test]
    fn transport_request_converts_without_argv_roundtrip() -> PpResult<()> {
        let operation = Operation::try_from(ApplicationRequest::Convert {
            input: "input.png".into(), output: "output.png".into(), width: Some(64), height: None,
            filter: Some("nearest".to_string()), jpeg_quality: None, background: None,
        })?;
        match operation {
            Operation::Convert { width, filter, .. } => {
                assert_eq!(width.map(NonZeroU32::get), Some(64));
                assert_eq!(filter, Some(ResampleFilter::Nearest));
            }
            _ => panic!("wrong operation"),
        }
        Ok(())
    }

    #[test]
    fn transport_request_rejects_invalid_scalar_state() {
        let result = Operation::try_from(ApplicationRequest::Upscale {
            input: "input.png".into(), output: "output.png".into(), scale: 1, filter: None,
            jpeg_quality: None, background: None,
        });
        assert!(result.is_err());
    }
}
