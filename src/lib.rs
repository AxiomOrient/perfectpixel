#[cfg(not(unix))]
compile_error!("perfectpixel supports Unix targets only");

#[cfg(unix)]
mod adapters;
#[cfg(unix)]
mod core;
#[cfg(unix)]
mod effects;
#[cfg(unix)]
mod io;
#[cfg(unix)]
mod operation;
#[cfg(unix)]
mod runtime;
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
    apply_raster_edits, apply_raster_edits_with_evidence, composite_source_over_linear_srgb,
    content_bbox, decontaminate_known_background, delta_e2000, encode_psd, inspect_raster,
    is_valid_premultiplied_rgba8, linear16_to_srgb8, oklab_distance, plan_chroma,
    plan_remove_background_auto, premultiply_rgba8, resize_raster, srgb8_to_lab,
    srgb8_to_linear16, transform_icc_rgba8_to_srgb, unpremultiply_rgba8, verify_raster,
    verify_raster_exact, AlphaHistogram, AlphaMode, ArtifactRef, AutoBackgroundPlan, BlendMode,
    CanvasSpec, ChromaCandidateScore, ChromaPlan, ColorSpec, ColorTransformReceipt,
    ConnectedComponent, DeltaEThresholds, Document, EdgePaletteEntry, ExactAssertion, ExactCheck,
    ExactEvidence, FrameRect, GroupLayer, Lab, Layer, LayerCommon, Mask, PerceptualAssertion,
    PerceptualCheck, PerceptualEvidence, PixelFormat, PixelLayer, PixelSpec, Point, Point2, PpError,
    PpResult, PsdEncoded, PsdPathOptions, Raster, RasterEdit, RasterInspection, Rect2,
    RegionAssertion, RegionCheck, RegionEvidence, ResampleFilter, Sha256Digest, Size, SvgContract,
    SvgReport, Transform3, VerificationProfile, VerificationReport, VerificationSpec,
    CHROMA_CANDIDATE_PALETTE, CHROMA_PLAN_METRIC, CHROMA_PLAN_SCHEMA, DOCUMENT_SCHEMA,
    PSD_DEFAULT_ALPHA_THRESHOLD, PSD_DEFAULT_MAX_KNOTS, PSD_EXPORT_SCHEMA, PSD_MAX_DIMENSION,
    PSD_MAX_KNOTS, PSD_MAX_OUTPUT_BYTES, VERIFICATION_REPORT_SCHEMA,
};
#[cfg(unix)]
pub(crate) use core::{
    inspect_ktx2, verify_ktx2_contract, Ktx2Info, ResolvedDocumentRaster, TextureSemantic,
};
#[cfg(unix)]
pub use effects::{
    EffectCompletion, EffectFailure, EffectFailureCode, EffectIdentity, EffectResult,
};
#[cfg(unix)]
pub(crate) use effects::{
    run_ktx2_effect, run_ktx2_extract_effect, ExternalToolReceipt, Ktx2EffectRequest,
    Ktx2ExtractRequest, KtxEncoding,
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
    AtomicDirectoryEntry, AtomicDirectoryWriter, AtomicFileWriter, DecodeLimits, DecodedRaster,
    FilePrecondition, ImageCodec, PngEncoder,
};
#[cfg(unix)]
pub(crate) use io::{publish_directory_checked, DirectoryPrecondition};
#[cfg(unix)]
pub use operation::{
    operation_specs, parse_resample_filter, parse_unit_score, parse_vector_detail,
    parse_vector_preset, parse_vector_profile, FailureContext, JpegQuality, Operation,
    OperationErrorCode, OperationFailure, OperationInputError, OperationRisk, OperationSpec,
    ScaleFactor, SideEffectClass,
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
