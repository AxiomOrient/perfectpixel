use std::{
    num::{NonZeroU32, NonZeroUsize},
    path::PathBuf,
    time::Duration,
};

use serde::{Deserialize, Serialize};

use crate::{ResampleFilter, SvgProfile, UnitScore, VectorDetail, VectorPresetSelection};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SideEffectClass {
    None,
    Read,
    Publish,
    ExternalProcess,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationRisk {
    None,
    LocalRead,
    LocalMutation,
    ExternalExecution,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OperationSpec {
    pub name: &'static str,
    pub summary: &'static str,
    pub side_effect: SideEffectClass,
    pub risk: OperationRisk,
    pub timeout: Option<Duration>,
    pub capabilities: &'static [&'static str],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JpegQuality(u8);

impl JpegQuality {
    pub fn new(value: u8) -> Result<Self, OperationInputError> {
        if (1..=100).contains(&value) {
            Ok(Self(value))
        } else {
            Err(OperationInputError::new("jpeg quality must be in 1..=100"))
        }
    }

    pub fn get(self) -> u8 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScaleFactor(NonZeroU32);

impl ScaleFactor {
    pub fn new(value: u32) -> Result<Self, OperationInputError> {
        if value < 2 {
            return Err(OperationInputError::new("upscale factor must be >= 2"));
        }
        Ok(Self(NonZeroU32::new(value).expect("value >= 2")))
    }

    pub fn get(self) -> u32 {
        self.0.get()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationInputError {
    message: String,
}

impl OperationInputError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl std::fmt::Display for OperationInputError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for OperationInputError {}

pub fn parse_vector_preset(value: &str) -> Result<VectorPresetSelection, OperationInputError> {
    match value {
        "auto" => Ok(VectorPresetSelection::Auto),
        "pixel-art" => Ok(VectorPresetSelection::PixelArt),
        "legacy-lossless" => Ok(VectorPresetSelection::LegacyLossless),
        "flat-icon" => Ok(VectorPresetSelection::FlatIcon),
        "line-art" => Ok(VectorPresetSelection::LineArt),
        "bounded-illustration" => Ok(VectorPresetSelection::BoundedIllustration),
        _ => Err(OperationInputError::new(
            "preset must be auto, pixel-art, legacy-lossless, flat-icon, line-art, or bounded-illustration",
        )),
    }
}

pub fn parse_vector_profile(value: &str) -> Result<SvgProfile, OperationInputError> {
    match value {
        "compact" => Ok(SvgProfile::Compact),
        "motion-structure-ready" => Ok(SvgProfile::MotionStructureReady),
        _ => Err(OperationInputError::new(
            "profile must be compact or motion-structure-ready",
        )),
    }
}

pub fn parse_vector_detail(value: &str) -> Result<Option<VectorDetail>, OperationInputError> {
    if value == "auto" {
        return Ok(None);
    }
    let value = value
        .parse::<u8>()
        .map_err(|_| OperationInputError::new("detail must be auto or an integer from 1 through 5"))?;
    VectorDetail::new(value)
        .map(Some)
        .map_err(|error| OperationInputError::new(error.to_string()))
}

pub fn parse_unit_score(value: &str) -> Result<UnitScore, OperationInputError> {
    let parsed = value
        .parse::<f64>()
        .map_err(|_| OperationInputError::new("quality threshold must be a finite number from 0 through 1"))?;
    UnitScore::new(parsed).map_err(|error| OperationInputError::new(error.to_string()))
}

/// Single semantic command authority shared by transports. Paths identify requested I/O but no
/// file is read while constructing this state. Request/policy reads belong to application Effects.
pub enum Operation {
    Schema,
    Inspect {
        input: PathBuf,
    },
    Convert {
        input: PathBuf,
        output: PathBuf,
        width: Option<NonZeroU32>,
        height: Option<NonZeroU32>,
        filter: ResampleFilter,
        jpeg_quality: Option<JpegQuality>,
        background: Option<[u8; 3]>,
    },
    Upscale {
        input: PathBuf,
        output: PathBuf,
        scale: ScaleFactor,
        filter: ResampleFilter,
        jpeg_quality: Option<JpegQuality>,
        background: Option<[u8; 3]>,
    },
    Edit {
        request: PathBuf,
    },
    ExportPsd {
        request: PathBuf,
    },
    ChromaPlan {
        request: PathBuf,
    },
    NormalizeSprite {
        request: PathBuf,
        output_dir: PathBuf,
    },
    CompileSprite {
        request: PathBuf,
        output_dir: PathBuf,
    },
    CompileVector {
        input: PathBuf,
        output: PathBuf,
        preset: VectorPresetSelection,
        profile: SvgProfile,
        detail: Option<VectorDetail>,
        minimum_quality: Option<UnitScore>,
        maximum_quality_loss: Option<UnitScore>,
        maximum_paths: Option<NonZeroUsize>,
        policy: Option<PathBuf>,
        report: Option<PathBuf>,
        diagnostics: Option<PathBuf>,
    },
    AnalyzeVector {
        input: PathBuf,
        preset: VectorPresetSelection,
        profile: SvgProfile,
        policy: Option<PathBuf>,
        report: Option<PathBuf>,
    },
    ScaffoldMotion {
        input: PathBuf,
        output_dir: PathBuf,
    },
    CompileMotion {
        request: PathBuf,
        output_dir: PathBuf,
    },
}

impl Operation {
    pub const fn spec(&self) -> OperationSpec {
        match self {
            Self::Schema => spec("system.schema", "Return the product schema", SideEffectClass::None, OperationRisk::None, None, &[]),
            Self::Inspect { .. } => spec("image.inspect", "Inspect one raster artifact", SideEffectClass::Read, OperationRisk::LocalRead, None, &["filesystem.read"]),
            Self::Convert { .. } => spec("image.convert", "Convert or resize one raster artifact", SideEffectClass::Publish, OperationRisk::LocalMutation, None, &["filesystem.read", "filesystem.publish"]),
            Self::Upscale { .. } => spec("image.upscale", "Integer-upscale one raster artifact", SideEffectClass::Publish, OperationRisk::LocalMutation, None, &["filesystem.read", "filesystem.publish"]),
            Self::Edit { .. } => spec("image.edit", "Apply deterministic raster edits", SideEffectClass::Publish, OperationRisk::LocalMutation, None, &["filesystem.read", "filesystem.publish"]),
            Self::ExportPsd { .. } => spec("document.export_psd", "Export the deterministic PSD contract", SideEffectClass::Publish, OperationRisk::LocalMutation, None, &["filesystem.read", "filesystem.publish"]),
            Self::ChromaPlan { .. } => spec("image.chroma_plan", "Plan a controlled chroma background", SideEffectClass::Read, OperationRisk::LocalRead, None, &["filesystem.read"]),
            Self::NormalizeSprite { .. } => spec("sprite.normalize", "Normalize sprite frames", SideEffectClass::Publish, OperationRisk::LocalMutation, None, &["filesystem.read", "filesystem.publish"]),
            Self::CompileSprite { .. } => spec("sprite.compile", "Compile a sprite atlas bundle", SideEffectClass::Publish, OperationRisk::LocalMutation, None, &["filesystem.read", "filesystem.publish"]),
            Self::CompileVector { .. } => spec("vector.compile", "Compile a quality-gated SVG", SideEffectClass::Publish, OperationRisk::LocalMutation, None, &["filesystem.read", "filesystem.publish"]),
            Self::AnalyzeVector { .. } => spec("vector.analyze", "Analyze vectorization without publishing SVG", SideEffectClass::Read, OperationRisk::LocalRead, None, &["filesystem.read"]),
            Self::ScaffoldMotion { .. } => spec("motion.scaffold", "Scaffold motion metadata from SVG", SideEffectClass::Publish, OperationRisk::LocalMutation, None, &["filesystem.read", "filesystem.publish"]),
            Self::CompileMotion { .. } => spec("motion.compile", "Compile deterministic motion artifacts", SideEffectClass::Publish, OperationRisk::LocalMutation, None, &["filesystem.read", "filesystem.publish"]),
        }
    }
}

const fn spec(
    name: &'static str,
    summary: &'static str,
    side_effect: SideEffectClass,
    risk: OperationRisk,
    timeout: Option<Duration>,
    capabilities: &'static [&'static str],
) -> OperationSpec {
    OperationSpec {
        name,
        summary,
        side_effect,
        risk,
        timeout,
        capabilities,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationErrorCode {
    InvalidArgument,
    Unsupported,
    NotFound,
    Conflict,
    PreconditionFailed,
    ResourceLimit,
    Timeout,
    Cancelled,
    DependencyFailed,
    VerificationFailed,
    Internal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OperationFailure {
    pub code: OperationErrorCode,
    pub operation: String,
    pub cause: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub context: Vec<FailureContext>,
    pub retryable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FailureContext {
    pub key: String,
    pub value: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_scalars_reject_invalid_state() {
        assert!(JpegQuality::new(0).is_err());
        assert!(JpegQuality::new(101).is_err());
        assert_eq!(JpegQuality::new(85).expect("valid").get(), 85);
        assert!(ScaleFactor::new(1).is_err());
        assert_eq!(ScaleFactor::new(2).expect("valid").get(), 2);
    }

    #[test]
    fn operation_metadata_has_one_owner() {
        let operation = Operation::Inspect {
            input: "a.png".into(),
        };
        let spec = operation.spec();
        assert_eq!(spec.name, "image.inspect");
        assert_eq!(spec.side_effect, SideEffectClass::Read);
        assert_eq!(spec.risk, OperationRisk::LocalRead);
        assert_eq!(spec.capabilities, &["filesystem.read"]);
    }

    #[test]
    fn canonical_vector_parsers_reject_invalid_values() {
        assert!(parse_vector_preset("unknown").is_err());
        assert!(parse_vector_profile("unknown").is_err());
        assert!(parse_vector_detail("6").is_err());
        assert!(parse_unit_score("nan").is_err());
    }
}
