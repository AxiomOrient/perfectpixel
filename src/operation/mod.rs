use std::{num::{NonZeroU32, NonZeroUsize}, path::PathBuf};

use serde::{Deserialize, Serialize};

use crate::{ResampleFilter, SvgProfile, UnitScore, VectorDetail, VectorPolicy, VectorPresetSelection};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SideEffectClass {
    None,
    Read,
    Publish,
    ExternalProcess,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OperationSpec {
    pub name: &'static str,
    pub summary: &'static str,
    pub side_effect: SideEffectClass,
}

/// Validated JPEG quality. Invalid values cannot enter canonical operation state.
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

    pub fn get(self) -> u8 { self.0 }
}

/// Validated integer upscale factor. Factor 1 is convert/identity, not an upscale operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScaleFactor(NonZeroU32);

impl ScaleFactor {
    pub fn new(value: u32) -> Result<Self, OperationInputError> {
        if value < 2 {
            return Err(OperationInputError::new("upscale factor must be >= 2"));
        }
        Ok(Self(NonZeroU32::new(value).expect("value >= 2")))
    }

    pub fn get(self) -> u32 { self.0.get() }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationInputError {
    message: String,
}

impl OperationInputError {
    pub fn new(message: impl Into<String>) -> Self { Self { message: message.into() } }
    pub fn message(&self) -> &str { &self.message }
}

impl std::fmt::Display for OperationInputError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for OperationInputError {}

/// The single semantic command authority shared by CLI and MCP. Transport-specific validation
/// resolves paths and scalar syntax before constructing this enum. Product handlers receive no
/// argv strings and do not know which transport initiated the operation.
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
        policy: VectorPolicy,
        report: Option<PathBuf>,
        diagnostics: Option<PathBuf>,
    },
    AnalyzeVector {
        input: PathBuf,
        preset: VectorPresetSelection,
        profile: SvgProfile,
        policy: VectorPolicy,
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
            Self::Schema => spec("system.schema", "Return the product schema", SideEffectClass::None),
            Self::Inspect { .. } => spec("image.inspect", "Inspect one raster artifact", SideEffectClass::Read),
            Self::Convert { .. } => spec("image.convert", "Convert or resize one raster artifact", SideEffectClass::Publish),
            Self::Upscale { .. } => spec("image.upscale", "Integer-upscale one raster artifact", SideEffectClass::Publish),
            Self::Edit { .. } => spec("image.edit", "Apply deterministic raster edits", SideEffectClass::Publish),
            Self::ExportPsd { .. } => spec("document.export_psd", "Export the current deterministic PSD contract", SideEffectClass::Publish),
            Self::ChromaPlan { .. } => spec("image.chroma_plan", "Plan a controlled chroma background", SideEffectClass::Read),
            Self::NormalizeSprite { .. } => spec("sprite.normalize", "Normalize sprite frames", SideEffectClass::Publish),
            Self::CompileSprite { .. } => spec("sprite.compile", "Compile a sprite atlas bundle", SideEffectClass::Publish),
            Self::CompileVector { .. } => spec("vector.compile", "Compile a quality-gated SVG", SideEffectClass::Publish),
            Self::AnalyzeVector { .. } => spec("vector.analyze", "Analyze vectorization without publishing SVG", SideEffectClass::Read),
            Self::ScaffoldMotion { .. } => spec("motion.scaffold", "Scaffold motion metadata from SVG", SideEffectClass::Publish),
            Self::CompileMotion { .. } => spec("motion.compile", "Compile deterministic motion artifacts", SideEffectClass::Publish),
        }
    }
}

const fn spec(name: &'static str, summary: &'static str, side_effect: SideEffectClass) -> OperationSpec {
    OperationSpec { name, summary, side_effect }
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
    fn operation_name_and_side_effect_have_one_owner() {
        let operation = Operation::Inspect { input: "a.png".into() };
        assert_eq!(operation.spec().name, "image.inspect");
        assert_eq!(operation.spec().side_effect, SideEffectClass::Read);
    }
}
