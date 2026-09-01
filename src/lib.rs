#[cfg(not(unix))]
compile_error!("perfectpixel supports Unix targets only");

#[cfg(unix)]
mod adapters;
#[cfg(unix)]
pub mod agent;
#[cfg(unix)]
pub mod agent_image;
#[cfg(unix)]
mod core;
#[cfg(unix)]
mod io;
#[cfg(unix)]
mod vector;

#[cfg(unix)]
pub mod text;

#[cfg(unix)]
#[doc(hidden)]
pub mod application;

#[cfg(unix)]
#[doc(hidden)]
pub mod mcp;

#[cfg(unix)]
pub use adapters::motion::{
    assess_motion_structure, MotionAssessment, MotionAssessmentBinding, MotionAssessmentEvidence,
    MotionAssessmentNotEvaluatedReason, MotionAssessmentRequest, MotionAssessmentStatus,
    MotionAuthoredPath, MotionBuildOutput, MotionCompiler, MotionInterpolation, MotionKeyframe,
    MotionLayer, MotionLayersDocument, MotionMarker, MotionPart, MotionReport, MotionRequest,
    MotionScaffoldOutput, MotionTrack, GENERATOR_PREFIX, MOTION_ASSESSMENT_CONTRACT_VERSION,
    MOTION_ASSESSMENT_MAX_GEOMETRY_BYTES, MOTION_ASSESSMENT_MAX_PATHS,
    MOTION_ASSESSMENT_MAX_SOURCE_BYTES, MOTION_REPORT_SCHEMA, MOTION_SCHEMA,
};
#[cfg(unix)]
pub use adapters::sprite::{
    build_aseprite_jsons, compose_bundle, compose_bundle_with_packing, normalize_schema,
    normalize_sprite, validate_normalize_plan_contract, AnimationEntry, AsepriteJsonOutput,
    BundlePlan, FrameEntry, FrameOutput, Manifest, NormalizeChroma, NormalizeFit,
    NormalizeGateReport, NormalizeOutline, NormalizePlan, NormalizeQuality, NormalizeReport,
    NormalizeRequest, NormalizeStateImages, NormalizeStateReport, NormalizeStateRequest,
    NormalizeStateSource, NormalizedStateOutput, PackingInfo, PackingRequest, SheetInfo,
    SheetOutput, SpriteBundleRequest, StateFrames, StateRequest, NORMALIZE_SCHEMA, SPRITE_SCHEMA,
};
#[cfg(unix)]
pub use agent::{
    agent_capability_manifest, capability_manifest_sha256, dependency_closure_sha256,
    AlphaMode as AgentAlphaMode, ArtifactDependency as AgentArtifactDependency,
    ArtifactDescriptor as AgentArtifactDescriptor, ArtifactKind as AgentArtifactKind,
    ArtifactPinSet as AgentArtifactPinSet, ArtifactRetention as AgentArtifactRetention,
    Capability as AgentCapability, CapabilityManifest as AgentCapabilityManifest,
    ColorSpace as AgentColorSpace, CompareAssertionRequest as AgentCompareAssertionRequest,
    ComparePreviewRequest as AgentComparePreviewRequest,
    ComparePreviewResult as AgentComparePreviewResult, CompareRequest as AgentCompareRequest,
    CompareResult as AgentCompareResult, DeterminismClass as AgentDeterminismClass,
    InputArtifact as AgentInputArtifact, OperationReceipt as AgentOperationReceipt,
    OperationStatus as AgentOperationStatus, PixelFormat as AgentPixelFormat,
    PixelSpec as AgentPixelSpec, AGENT_BEHAVIOR_VERSION, AGENT_CAPABILITY_MANIFEST_DIGEST_DOMAIN,
    AGENT_COMPARE_REQUEST_SCHEMA, AGENT_COMPARE_RESULT_SCHEMA, AGENT_EXTRACT_REQUEST_SCHEMA,
    AGENT_EXTRACT_RESULT_SCHEMA, AGENT_INSPECT_REQUEST_SCHEMA, AGENT_INSPECT_RESULT_SCHEMA,
    AGENT_PIN_SET_SCHEMA, AGENT_PROTOCOL_SCHEMA, AGENT_PROTOCOL_VERSION, AGENT_RECEIPT_SCHEMA,
    AGENT_RENDER_REQUEST_SCHEMA, AGENT_RENDER_RESULT_SCHEMA,
};
#[cfg(unix)]
pub use agent_image::{
    compare_images, difference_preview, extract_object, feather_matte_for_source,
    mask_overlay_preview, preprocess_render_source, render_composition, validate_compare_workload,
    AffineTransform, CompareAssertion, CompareAssertionResult, CompareMetrics, CompareOutcome,
    CompareSeverity, ExtractSelector, ExtractedObject, MatteRefinement, NormalizedPoint,
    ObjectBounds, RenderCanvas, RenderFilter, RenderNode, MAX_AGENT_IMAGE_DIMENSION,
    MAX_AGENT_IMAGE_PIXELS, MAX_COMPARE_ASSERTIONS, MAX_COMPARE_DECODED_PIXELS,
    MAX_COMPARE_PIXEL_WORK, MAX_EXTRACT_FEATHER_RADIUS, MAX_RENDER_NODES, MAX_RENDER_PIXEL_WORK,
};
#[cfg(unix)]
#[doc(hidden)]
pub use core::sha256::{is_sha256_hex, sha256, sha256_hex};
#[cfg(unix)]
pub use core::{
    apply_raster_edits, apply_raster_edits_with_evidence, content_bbox, encode_psd, inspect_raster,
    oklab_distance, plan_chroma, plan_remove_background_auto, resize_raster, verify_raster_exact,
    AlphaMode, ArtifactRef, AutoBackgroundPlan, ChromaCandidateScore, ChromaPlan, ColorSpec,
    EdgePaletteEntry, ExactAssertion, FrameRect, PixelFormat, PixelSpec, Point, PpError, PpResult,
    PsdEncoded, PsdPathOptions, Raster, RasterEdit, RasterInspection, ResampleFilter, Sha256Digest,
    Size, SvgContract, SvgReport, VerificationCheck, VerificationEvidence, VerificationReport,
    VerificationSpec, CHROMA_CANDIDATE_PALETTE, CHROMA_PLAN_METRIC, CHROMA_PLAN_SCHEMA,
    PSD_DEFAULT_ALPHA_THRESHOLD, PSD_DEFAULT_MAX_KNOTS, PSD_EXPORT_SCHEMA, PSD_MAX_DIMENSION,
    PSD_MAX_KNOTS, PSD_MAX_OUTPUT_BYTES, VERIFICATION_REPORT_SCHEMA,
};
#[cfg(unix)]
#[doc(hidden)]
pub use io::{
    parallel_map, parallel_map_owned, reject_blocked_managed_parents, ArtifactSetConditionPhase,
    AtomicArtifactSetEntry, AtomicArtifactSetOwnedEntry, AtomicArtifactSetOwnedPlan,
    AtomicArtifactSetWriter,
};
#[cfg(unix)]
pub use io::{
    AtomicDirectoryEntry, AtomicDirectoryWriter, AtomicFileWriter, DecodeLimits, ImageCodec,
    PngEncoder,
};
#[cfg(unix)]
pub use text::{
    LineBreakMode, TextAlignment, TextDirection, TextGlyphSnapshot, TextLayoutSnapshot,
    TextLineSnapshot, TextNode, TextRenderOutput, TEXT_NODE_SCHEMA,
};
#[cfg(unix)]
pub use vector::{
    AnalysisNormalization, ApprovedVectorOutput, ArtifactIntent, CandidateFacts, ConstraintValues,
    DiagnosticArtifact, DiagnosticArtifactSet, DiagnosticsIntent, EvaluationDecision,
    EvaluationDigests, EvaluationReport, GateActualValue, GateComparator, GateMeasurement,
    GateMeasurementFamilies, GateThreshold, PredicateAvailability, ProfileEvidence,
    RejectedVectorOutput, RequestValidationError, SvgProfile, UnitScore, VectorAnalysis,
    VectorAnalysisRequest, VectorDetail, VectorOutcome, VectorPolicy, VectorPresetSelection,
    VectorRejectionCode, VectorRequest, Vectorizer, VECTOR_ANALYSIS_SCHEMA,
    VECTOR_EVALUATION_SCHEMA,
};
