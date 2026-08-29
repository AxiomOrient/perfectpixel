#[cfg(not(unix))]
compile_error!("perfectpixel supports Unix targets only");

#[cfg(unix)]
mod adapters;
#[cfg(unix)]
mod core;
#[cfg(unix)]
mod io;
#[cfg(unix)]
mod vector;

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
#[doc(hidden)]
pub use core::sha256::{is_sha256_hex, sha256, sha256_hex};
#[cfg(unix)]
pub use core::{
    apply_raster_edits, apply_raster_edits_with_evidence, content_bbox, inspect_raster,
    oklab_distance, plan_chroma, plan_remove_background_auto, resize_raster, AutoBackgroundPlan,
    ChromaCandidateScore, ChromaPlan, EdgePaletteEntry, FrameRect, Point, PpError, PpResult,
    Raster, RasterEdit, RasterInspection, ResampleFilter, Size, SvgContract, SvgReport,
    CHROMA_CANDIDATE_PALETTE, CHROMA_PLAN_METRIC, CHROMA_PLAN_SCHEMA,
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
