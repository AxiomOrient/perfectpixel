use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::agent_image::{CompareAssertionResult, CompareMetrics, CompareSeverity, ObjectBounds};
use crate::{sha256_hex, PpError, PpResult};

pub const AGENT_PROTOCOL_SCHEMA: &str = "perfectpixel.agent-image/2";
pub const AGENT_PROTOCOL_VERSION: &str = "2.0.0";
pub const AGENT_BEHAVIOR_VERSION: &str = "2.0.0";
pub const AGENT_CAPABILITY_MANIFEST_DIGEST_DOMAIN: &str =
    "perfectpixel.agent-image/capability-manifest/2";
pub const AGENT_INSPECT_REQUEST_SCHEMA: &str = "perfectpixel.agent-image/inspect/2";
pub const AGENT_INSPECT_RESULT_SCHEMA: &str = "perfectpixel.agent-image/inspect-result/2";
pub const AGENT_EXTRACT_REQUEST_SCHEMA: &str = "perfectpixel.agent-image/extract/2";
pub const AGENT_EXTRACT_RESULT_SCHEMA: &str = "perfectpixel.agent-image/extract-result/2";
pub const AGENT_RENDER_REQUEST_SCHEMA: &str = "perfectpixel.agent-image/render/2";
pub const AGENT_RENDER_RESULT_SCHEMA: &str = "perfectpixel.agent-image/render-result/2";
pub const AGENT_COMPARE_REQUEST_SCHEMA: &str = "perfectpixel.agent-image/compare/2";
pub const AGENT_COMPARE_RESULT_SCHEMA: &str = "perfectpixel.agent-image/compare-result/2";
pub const AGENT_RECEIPT_SCHEMA: &str = "perfectpixel.agent-image/receipt/2";
pub const AGENT_PIN_SET_SCHEMA: &str = "perfectpixel.agent-image/pin-set/2";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeterminismClass {
    BitExact,
    BackendDeterministic,
    InferenceObserved,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Capability {
    pub name: String,
    pub version: String,
    pub determinism: DeterminismClass,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CapabilityManifest {
    pub schema: String,
    pub protocol_version: String,
    pub behavior_version: String,
    pub implementation_version: String,
    pub capabilities: Vec<Capability>,
}

#[must_use]
pub fn agent_capability_manifest(implementation_version: &str) -> CapabilityManifest {
    CapabilityManifest {
        schema: AGENT_PROTOCOL_SCHEMA.to_owned(),
        protocol_version: AGENT_PROTOCOL_VERSION.to_owned(),
        behavior_version: AGENT_BEHAVIOR_VERSION.to_owned(),
        implementation_version: implementation_version.to_owned(),
        capabilities: vec![
            Capability {
                name: "inspect.basic".to_owned(),
                version: "2.0.0".to_owned(),
                determinism: DeterminismClass::BitExact,
            },
            Capability {
                name: "artifact.content_addressed".to_owned(),
                version: "2.0.0".to_owned(),
                determinism: DeterminismClass::BitExact,
            },
            Capability {
                name: "artifact.dependency_closure".to_owned(),
                version: "2.0.0".to_owned(),
                determinism: DeterminismClass::BitExact,
            },
            Capability {
                name: "artifact.pin_set".to_owned(),
                version: "2.0.0".to_owned(),
                determinism: DeterminismClass::BitExact,
            },
            Capability {
                name: "extract.alpha".to_owned(),
                version: "1.0.0".to_owned(),
                determinism: DeterminismClass::BitExact,
            },
            Capability {
                name: "extract.chroma_key".to_owned(),
                version: "1.0.0".to_owned(),
                determinism: DeterminismClass::BitExact,
            },
            Capability {
                name: "extract.color_range".to_owned(),
                version: "1.0.0".to_owned(),
                determinism: DeterminismClass::BitExact,
            },
            Capability {
                name: "extract.alpha_component".to_owned(),
                version: "1.0.0".to_owned(),
                determinism: DeterminismClass::BitExact,
            },
            Capability {
                name: "extract.provided_mask".to_owned(),
                version: "1.0.0".to_owned(),
                determinism: DeterminismClass::BitExact,
            },
            Capability {
                name: "extract.matte_refine".to_owned(),
                version: "1.0.0".to_owned(),
                determinism: DeterminismClass::BitExact,
            },
            Capability {
                name: "render.composition_dag".to_owned(),
                version: "1.0.0".to_owned(),
                determinism: DeterminismClass::BackendDeterministic,
            },
            Capability {
                name: "render.affine_3x3".to_owned(),
                version: "1.0.0".to_owned(),
                determinism: DeterminismClass::BackendDeterministic,
            },
            Capability {
                name: "render.source_over".to_owned(),
                version: "1.0.0".to_owned(),
                determinism: DeterminismClass::BitExact,
            },
            Capability {
                name: "render.text_node".to_owned(),
                version: "1.0.0".to_owned(),
                determinism: DeterminismClass::BitExact,
            },
            Capability {
                name: "compare.basic".to_owned(),
                version: "1.0.0".to_owned(),
                determinism: DeterminismClass::BitExact,
            },
            Capability {
                name: "compare.exact_spec".to_owned(),
                version: "1.0.0".to_owned(),
                determinism: DeterminismClass::BitExact,
            },
            Capability {
                name: "compare.regions".to_owned(),
                version: "1.0.0".to_owned(),
                determinism: DeterminismClass::BitExact,
            },
            Capability {
                name: "compare.masks".to_owned(),
                version: "1.0.0".to_owned(),
                determinism: DeterminismClass::BitExact,
            },
            Capability {
                name: "compare.geometry".to_owned(),
                version: "1.0.0".to_owned(),
                determinism: DeterminismClass::BitExact,
            },
            Capability {
                name: "compare.preview".to_owned(),
                version: "1.0.0".to_owned(),
                determinism: DeterminismClass::BitExact,
            },
        ],
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CanonicalCapabilityManifest<'a> {
    schema: &'a str,
    protocol_version: &'a str,
    behavior_version: &'a str,
    implementation_version: &'a str,
    capabilities: Vec<CanonicalCapability<'a>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CanonicalCapability<'a> {
    name: &'a str,
    version: &'a str,
    determinism: DeterminismClass,
}

/// Stable identity digest for the agent capability manifest. Capability order
/// is not identity, while duplicate capability names are invalid.
pub fn capability_manifest_sha256(manifest: &CapabilityManifest) -> PpResult<String> {
    if manifest.schema != AGENT_PROTOCOL_SCHEMA
        || manifest.protocol_version != AGENT_PROTOCOL_VERSION
        || manifest.behavior_version != AGENT_BEHAVIOR_VERSION
        || manifest.implementation_version.trim().is_empty()
        || manifest.implementation_version.len() > 64
        || manifest.capabilities.is_empty()
        || manifest.capabilities.len() > 256
    {
        return Err(PpError::InvalidRequest(
            "agent capability manifest identity is invalid".to_owned(),
        ));
    }
    let mut names = BTreeSet::new();
    for capability in &manifest.capabilities {
        if capability.name.is_empty()
            || capability.name.len() > 128
            || !capability.name.bytes().all(|byte| {
                byte.is_ascii_lowercase()
                    || byte.is_ascii_digit()
                    || matches!(byte, b'.' | b'_' | b'-')
            })
            || capability.version.is_empty()
            || capability.version.len() > 64
            || !names.insert(capability.name.as_str())
        {
            return Err(PpError::InvalidRequest(
                "agent capability manifest contains an invalid capability".to_owned(),
            ));
        }
    }
    let mut capabilities = manifest.capabilities.iter().collect::<Vec<_>>();
    capabilities.sort_by(|left, right| {
        (
            left.name.as_str(),
            left.version.as_str(),
            determinism_name(left.determinism),
        )
            .cmp(&(
                right.name.as_str(),
                right.version.as_str(),
                determinism_name(right.determinism),
            ))
    });
    let canonical = CanonicalCapabilityManifest {
        schema: &manifest.schema,
        protocol_version: &manifest.protocol_version,
        behavior_version: &manifest.behavior_version,
        implementation_version: &manifest.implementation_version,
        capabilities: capabilities
            .into_iter()
            .map(|capability| CanonicalCapability {
                name: &capability.name,
                version: &capability.version,
                determinism: capability.determinism,
            })
            .collect(),
    };
    let encoded = serde_json::to_vec(&canonical).map_err(|error| {
        PpError::InvalidRequest(format!(
            "agent capability manifest could not be encoded: {error}"
        ))
    })?;
    let mut input =
        Vec::with_capacity(AGENT_CAPABILITY_MANIFEST_DIGEST_DOMAIN.len() + encoded.len() + 1);
    input.extend_from_slice(AGENT_CAPABILITY_MANIFEST_DIGEST_DOMAIN.as_bytes());
    input.push(0);
    input.extend_from_slice(&encoded);
    Ok(sha256_hex(&input))
}

const fn determinism_name(value: DeterminismClass) -> &'static str {
    match value {
        DeterminismClass::BitExact => "bit_exact",
        DeterminismClass::BackendDeterministic => "backend_deterministic",
        DeterminismClass::InferenceObserved => "inference_observed",
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PixelFormat {
    Rgba8,
    Rgba16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColorSpace {
    Srgb,
    DisplayP3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlphaMode {
    Straight,
    Premultiplied,
    Opaque,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PixelSpec {
    pub format: PixelFormat,
    pub color_space: ColorSpace,
    pub alpha_mode: AlphaMode,
}

impl PixelSpec {
    #[must_use]
    pub const fn rgba8_srgb_straight() -> Self {
        Self {
            format: PixelFormat::Rgba8,
            color_space: ColorSpace::Srgb,
            alpha_mode: AlphaMode::Straight,
        }
    }

    #[must_use]
    pub const fn rgba16_srgb_straight() -> Self {
        Self {
            format: PixelFormat::Rgba16,
            color_space: ColorSpace::Srgb,
            alpha_mode: AlphaMode::Straight,
        }
    }

    #[must_use]
    pub const fn rgba8_display_p3_straight() -> Self {
        Self {
            format: PixelFormat::Rgba8,
            color_space: ColorSpace::DisplayP3,
            alpha_mode: AlphaMode::Straight,
        }
    }

    #[must_use]
    pub const fn rgba16_display_p3_straight() -> Self {
        Self {
            format: PixelFormat::Rgba16,
            color_space: ColorSpace::DisplayP3,
            alpha_mode: AlphaMode::Straight,
        }
    }

    #[must_use]
    pub const fn default_working() -> Self {
        Self::rgba8_srgb_straight()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    WorkingRaster,
    ExportRaster,
    Mask,
    Object,
    Json,
    Receipt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactRetention {
    TaskScoped,
    Cacheable,
    Pinned,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, PartialOrd, Ord)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ArtifactDependency {
    pub sha256: String,
    pub media_type: String,
    pub byte_length: u64,
}

impl ArtifactDependency {
    pub fn validate(&self) -> PpResult<()> {
        if self.sha256.len() != 64 || !self.sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(PpError::InvalidRequest(
                "artifact dependency sha256 is invalid".to_owned(),
            ));
        }
        if self.media_type.trim().is_empty() || self.media_type.len() > 256 || self.byte_length == 0
        {
            return Err(PpError::InvalidRequest(
                "artifact dependency metadata is invalid".to_owned(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InputArtifact {
    pub path: String,
    pub expected_sha256: String,
    pub media_type: String,
    pub byte_length: u64,
    pub kind: ArtifactKind,
    pub pixel_spec: PixelSpec,
}

/// Canonical strict JSON request accepted by `agent-compare`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CompareRequest {
    pub schema: String,
    pub request_id: String,
    pub before: InputArtifact,
    pub after: InputArtifact,
    pub assertions: Vec<CompareAssertionRequest>,
    pub preview: ComparePreviewRequest,
}

/// Canonical cross-process assertion representation. Evaluation remains owned
/// by `agent_image::CompareAssertion`; this type owns the strict artifact wire
/// contract used to prepare those evaluator values.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum CompareAssertionRequest {
    ExactEqual {
        id: String,
        severity: CompareSeverity,
    },
    ChangedRatio {
        id: String,
        severity: CompareSeverity,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        minimum: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        maximum: Option<f64>,
    },
    OutsideMaskChangedRatio {
        id: String,
        severity: CompareSeverity,
        maximum: f64,
        mask: InputArtifact,
    },
    InsideMaskChangedRatio {
        id: String,
        severity: CompareSeverity,
        minimum: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        maximum: Option<f64>,
        mask: InputArtifact,
    },
    UnchangedRegionExact {
        id: String,
        severity: CompareSeverity,
        region: ObjectBounds,
    },
    AlphaIou {
        id: String,
        severity: CompareSeverity,
        minimum: f64,
        mask: InputArtifact,
    },
    MaskLeakageRatio {
        id: String,
        severity: CompareSeverity,
        maximum: f64,
        mask: InputArtifact,
    },
    ObjectBounds {
        id: String,
        severity: CompareSeverity,
        expected: ObjectBounds,
        tolerance: u32,
    },
    ObjectCentroid {
        id: String,
        severity: CompareSeverity,
        expected: [f64; 2],
        tolerance: f64,
    },
    ObjectArea {
        id: String,
        severity: CompareSeverity,
        expected: u64,
        tolerance: u64,
    },
    MaximumChannelError {
        id: String,
        severity: CompareSeverity,
        maximum: u8,
    },
    MeanAbsoluteError {
        id: String,
        severity: CompareSeverity,
        maximum: f64,
    },
}

impl CompareAssertionRequest {
    #[must_use]
    pub fn id(&self) -> &str {
        match self {
            Self::ExactEqual { id, .. }
            | Self::ChangedRatio { id, .. }
            | Self::OutsideMaskChangedRatio { id, .. }
            | Self::InsideMaskChangedRatio { id, .. }
            | Self::UnchangedRegionExact { id, .. }
            | Self::AlphaIou { id, .. }
            | Self::MaskLeakageRatio { id, .. }
            | Self::ObjectBounds { id, .. }
            | Self::ObjectCentroid { id, .. }
            | Self::ObjectArea { id, .. }
            | Self::MaximumChannelError { id, .. }
            | Self::MeanAbsoluteError { id, .. } => id,
        }
    }

    #[must_use]
    pub const fn severity(&self) -> CompareSeverity {
        match self {
            Self::ExactEqual { severity, .. }
            | Self::ChangedRatio { severity, .. }
            | Self::OutsideMaskChangedRatio { severity, .. }
            | Self::InsideMaskChangedRatio { severity, .. }
            | Self::UnchangedRegionExact { severity, .. }
            | Self::AlphaIou { severity, .. }
            | Self::MaskLeakageRatio { severity, .. }
            | Self::ObjectBounds { severity, .. }
            | Self::ObjectCentroid { severity, .. }
            | Self::ObjectArea { severity, .. }
            | Self::MaximumChannelError { severity, .. }
            | Self::MeanAbsoluteError { severity, .. } => *severity,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ComparePreviewRequest {
    pub difference: bool,
    #[serde(default)]
    pub mask_overlay: bool,
    pub maximum_edge: u32,
}

/// Canonical strict preview entry returned by `agent-compare`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ComparePreviewResult {
    pub role: String,
    pub relative_path: String,
    pub descriptor: ArtifactDescriptor,
}

/// Canonical strict result returned by `agent-compare`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CompareResult {
    pub schema: String,
    pub request_id: String,
    pub status: OperationStatus,
    pub protocol_version: String,
    pub behavior_version: String,
    pub all_required_passed: bool,
    pub metrics: CompareMetrics,
    pub assertions: Vec<CompareAssertionResult>,
    pub previews: Vec<ComparePreviewResult>,
    pub receipt: OperationReceipt,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ArtifactDescriptor {
    pub sha256: String,
    pub media_type: String,
    pub byte_length: u64,
    pub kind: ArtifactKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pixel_spec: Option<PixelSpec>,
    pub retention: ArtifactRetention,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dependencies: Vec<ArtifactDependency>,
    pub dependency_closure_sha256: String,
}

impl ArtifactDescriptor {
    pub fn new(
        sha256: String,
        media_type: String,
        byte_length: u64,
        kind: ArtifactKind,
        pixel_spec: Option<PixelSpec>,
        retention: ArtifactRetention,
        mut dependencies: Vec<ArtifactDependency>,
    ) -> PpResult<Self> {
        for dependency in &dependencies {
            dependency.validate()?;
        }
        dependencies.sort();
        dependencies.dedup();
        let dependency_closure_sha256 = dependency_closure_sha256(&dependencies)?;
        Ok(Self {
            sha256,
            media_type,
            byte_length,
            kind,
            pixel_spec,
            retention,
            dependencies,
            dependency_closure_sha256,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationStatus {
    Committed,
    Rejected,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OperationReceipt {
    pub schema: String,
    pub request_id: String,
    pub operation: String,
    pub status: OperationStatus,
    pub behavior_version: String,
    pub implementation_version: String,
    pub request_sha256: String,
    pub dependency_closure_sha256: String,
    pub dependencies: Vec<ArtifactDependency>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_sha256: Option<String>,
    pub determinism: DeterminismClass,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ArtifactPinSet {
    pub schema: String,
    pub pins: Vec<String>,
}

impl ArtifactPinSet {
    pub fn validate(&self) -> PpResult<()> {
        if self.schema != AGENT_PIN_SET_SCHEMA || self.pins.len() > 4096 {
            return Err(PpError::InvalidRequest(
                "artifact pin set contract is invalid".to_owned(),
            ));
        }
        let mut seen = BTreeSet::new();
        for digest in &self.pins {
            if digest.len() != 64
                || !digest.bytes().all(|b| b.is_ascii_hexdigit())
                || !seen.insert(digest)
            {
                return Err(PpError::InvalidRequest(
                    "artifact pin set contains an invalid or duplicate digest".to_owned(),
                ));
            }
        }
        Ok(())
    }

    #[must_use]
    pub fn contains(&self, digest: &str) -> bool {
        self.pins.iter().any(|pin| pin == digest)
    }
}

pub fn dependency_closure_sha256(dependencies: &[ArtifactDependency]) -> PpResult<String> {
    let mut normalized = dependencies.to_vec();
    for dependency in &normalized {
        dependency.validate()?;
    }
    normalized.sort();
    normalized.dedup();
    let bytes = serde_json::to_vec(&normalized)
        .map_err(|error| PpError::InvalidRequest(error.to_string()))?;
    Ok(sha256_hex(&bytes))
}
