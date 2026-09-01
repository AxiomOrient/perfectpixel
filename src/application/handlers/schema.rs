use serde::Serialize;

use crate::{
    Operation, PpResult, CHROMA_CANDIDATE_PALETTE, CHROMA_PLAN_METRIC, CHROMA_PLAN_SCHEMA,
    MOTION_SCHEMA, PSD_DEFAULT_ALPHA_THRESHOLD, PSD_DEFAULT_MAX_KNOTS, PSD_EXPORT_SCHEMA,
    PSD_MAX_DIMENSION, PSD_MAX_KNOTS,
};

use super::super::shared::serialize_json;

pub(super) fn schema() -> PpResult<String> {
    let operations = operation_specs();
    serialize_json(
        &SchemaPayload {
            cli_version: env!("CARGO_PKG_VERSION"),
            inspect_schema: "perfectpixel.asset-inspection/1",
            role: "deterministic-asset-compiler",
            model_inference: false,
            network_required: false,
            publication_policy: "evaluate-before-publish",
            commands: &[
                "schema",
                "inspect",
                "convert",
                "upscale",
                "edit",
                "psd",
                "chroma-plan",
                "vector",
                "vector-analyze",
                "normalize",
                "bundle",
                "motion-scaffold",
                "motion-build",
            ],
            operations,
            normalize_schema: crate::NORMALIZE_SCHEMA,
            normalize_outputs: &[
                "normalize-report.json",
                "sprite-request.json",
                "frames/<state>/frame-NN.png",
            ],
            bundle_schema: crate::SPRITE_SCHEMA,
            bundle_outputs: &[
                "sprite-sheet.png",
                "manifest.json",
                "sprite-sheet.json",
                "frames/<state>/frame-NN.png",
            ],
            motion_schema: MOTION_SCHEMA,
            motion_outputs: &[
                "animated.svg",
                "animation.json",
                "motion-report.json",
                "preview.html",
                "dotlottie/manifest.json",
                "dotlottie/a/<name>.json",
            ],
            vector_policy_schema: "perfectpixel.vector-policy/1",
            vector_evaluation_schema: crate::VECTOR_EVALUATION_SCHEMA,
            vector_analysis_schema: crate::VECTOR_ANALYSIS_SCHEMA,
            vector_presets: &[
                "auto",
                "pixel-art",
                "legacy-lossless",
                "flat-icon",
                "line-art",
                "bounded-illustration",
            ],
            vector_profiles: &["compact", "motion-structure-ready"],
            asset_adapter: AssetAdapterSchema {
                raster_inputs: &["png", "jpg", "jpeg", "webp"],
                raster_outputs: &["png", "jpg", "jpeg", "webp"],
                commands: &["inspect", "convert", "upscale"],
                inspection_schema: "perfectpixel.asset-inspection/1",
                transform_schema: "perfectpixel.asset-transform/1",
                digest_encoding: "sha256-lowercase-hex",
                jpeg_alpha: "transparent input requires --background #RRGGBB",
                webp_output: "lossless RGBA",
                convert_filters: &["nearest", "lanczos3"],
                upscale_default_filter: "nearest",
            },
            edit_command: EditCommandSchema {
                request_schema: "perfectpixel.image-edit/1",
                input: "input raster path (png, jpg, jpeg, or webp)",
                output: "atomic PNG output path",
                operations: &[
                    "crop",
                    "rotate",
                    "flip",
                    "resize",
                    "remove_background",
                    "remove_background_auto",
                ],
                filters: &["nearest", "lanczos3"],
                maximum_steps: 64,
                semantic_editing: false,
            },
            psd_export_command: PsdExportCommandSchema {
                request_schema: PSD_EXPORT_SCHEMA,
                operation: "export_psd",
                schema_version: 1,
                required_fields: &["schemaVersion", "operation", "input", "output", "path"],
                path_required_fields: &["alphaThreshold", "maxKnots"],
                input: "input raster path (png, jpg, jpeg, or webp)",
                output: "atomic PSD output path (.psd)",
                alpha_threshold: "1..=255",
                recommended_alpha_threshold: PSD_DEFAULT_ALPHA_THRESHOLD,
                max_knots: "1..=32768",
                recommended_max_knots: PSD_DEFAULT_MAX_KNOTS,
                max_dimension: PSD_MAX_DIMENSION,
                max_knots_limit: PSD_MAX_KNOTS,
                max_output_bytes: crate::PSD_MAX_OUTPUT_BYTES,
                preserves: &["RGBA bytes", "soft alpha", "closed even-odd paths"],
                resources: &["1025 Working Path", "2000 Cutout Path", "2999 clipping path name"],
                photoshop_native_open: false,
            },
            chroma_plan_schema: CHROMA_PLAN_SCHEMA,
            chroma_plan_command: ChromaPlanCommandSchema {
                request_schema: CHROMA_PLAN_SCHEMA,
                operation: "chroma_plan",
                schema_version: 1,
                required_fields: &["schemaVersion", "operation", "subjectRgbColors"],
                subject_rgb_colors: "1..=32 unique RGB triplets",
                minimum_subject_colors: 1,
                maximum_subject_colors: 32,
                candidate_palette: &CHROMA_CANDIDATE_PALETTE,
                candidate_count: CHROMA_CANDIDATE_PALETTE.len(),
                metric: CHROMA_PLAN_METRIC,
            },
            vector_authority: "Embedded routes and thresholds are immutable; --policy may only select or tighten request constraints.",
            packing_defaults: PackingDefaultsPayload {
                algorithm: "binpack2d/maxrects",
                max_width: 2048,
                max_height: 2048,
                padding: 2,
                trim: true,
                allow_rotation: false,
                multipack: true,
            },
        },
        "<schema>",
    )
}

fn operation_specs() -> Vec<OperationSchema> {
    let specs = [
        Operation::Schema.spec(),
        Operation::Inspect { input: Default::default() }.spec(),
        Operation::Convert {
            input: Default::default(), output: Default::default(), width: None, height: None,
            filter: None, jpeg_quality: None, background: None,
        }.spec(),
        Operation::Upscale {
            input: Default::default(), output: Default::default(),
            scale: crate::ScaleFactor::new(2).expect("static valid scale"),
            filter: None, jpeg_quality: None, background: None,
        }.spec(),
        Operation::Edit { request: Default::default() }.spec(),
        Operation::ExportPsd { request: Default::default() }.spec(),
        Operation::ChromaPlan { request: Default::default() }.spec(),
        Operation::NormalizeSprite { request: Default::default(), output_dir: Default::default() }.spec(),
        Operation::CompileSprite { request: Default::default(), output_dir: Default::default() }.spec(),
        Operation::CompileVector {
            input: Default::default(), output: Default::default(),
            preset: crate::VectorPresetSelection::Auto, profile: crate::SvgProfile::Compact,
            detail: None, minimum_quality: None, maximum_quality_loss: None, maximum_paths: None,
            policy: None, report: None, diagnostics: None,
        }.spec(),
        Operation::AnalyzeVector {
            input: Default::default(), preset: crate::VectorPresetSelection::Auto,
            profile: crate::SvgProfile::Compact, policy: None, report: None,
        }.spec(),
        Operation::ScaffoldMotion { input: Default::default(), output_dir: Default::default() }.spec(),
        Operation::CompileMotion { request: Default::default(), output_dir: Default::default() }.spec(),
    ];
    specs
        .into_iter()
        .map(|spec| OperationSchema {
            name: spec.name,
            summary: spec.summary,
            side_effect: format!("{:?}", spec.side_effect).to_ascii_lowercase(),
            risk: format!("{:?}", spec.risk).to_ascii_lowercase(),
            timeout_ms: spec.timeout.map(|value| value.as_millis()),
            capabilities: spec.capabilities,
        })
        .collect()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct OperationSchema {
    name: &'static str,
    summary: &'static str,
    side_effect: String,
    risk: String,
    timeout_ms: Option<u128>,
    capabilities: &'static [&'static str],
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SchemaPayload {
    cli_version: &'static str,
    inspect_schema: &'static str,
    role: &'static str,
    model_inference: bool,
    network_required: bool,
    publication_policy: &'static str,
    commands: &'static [&'static str],
    operations: Vec<OperationSchema>,
    chroma_plan_schema: &'static str,
    chroma_plan_command: ChromaPlanCommandSchema,
    normalize_schema: &'static str,
    normalize_outputs: &'static [&'static str],
    bundle_schema: &'static str,
    bundle_outputs: &'static [&'static str],
    motion_schema: &'static str,
    motion_outputs: &'static [&'static str],
    vector_policy_schema: &'static str,
    vector_evaluation_schema: &'static str,
    vector_analysis_schema: &'static str,
    vector_presets: &'static [&'static str],
    vector_profiles: &'static [&'static str],
    asset_adapter: AssetAdapterSchema,
    edit_command: EditCommandSchema,
    psd_export_command: PsdExportCommandSchema,
    vector_authority: &'static str,
    packing_defaults: PackingDefaultsPayload,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AssetAdapterSchema {
    raster_inputs: &'static [&'static str],
    raster_outputs: &'static [&'static str],
    commands: &'static [&'static str],
    inspection_schema: &'static str,
    transform_schema: &'static str,
    digest_encoding: &'static str,
    jpeg_alpha: &'static str,
    webp_output: &'static str,
    convert_filters: &'static [&'static str],
    upscale_default_filter: &'static str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct EditCommandSchema {
    request_schema: &'static str,
    input: &'static str,
    output: &'static str,
    operations: &'static [&'static str],
    filters: &'static [&'static str],
    maximum_steps: usize,
    semantic_editing: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PsdExportCommandSchema {
    request_schema: &'static str,
    operation: &'static str,
    schema_version: u32,
    required_fields: &'static [&'static str],
    path_required_fields: &'static [&'static str],
    input: &'static str,
    output: &'static str,
    alpha_threshold: &'static str,
    recommended_alpha_threshold: u8,
    max_knots: &'static str,
    recommended_max_knots: usize,
    max_dimension: u32,
    max_knots_limit: usize,
    max_output_bytes: usize,
    preserves: &'static [&'static str],
    resources: &'static [&'static str],
    photoshop_native_open: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ChromaPlanCommandSchema {
    request_schema: &'static str,
    operation: &'static str,
    schema_version: u32,
    required_fields: &'static [&'static str],
    subject_rgb_colors: &'static str,
    minimum_subject_colors: usize,
    maximum_subject_colors: usize,
    candidate_palette: &'static [[u8; 3]; 8],
    candidate_count: usize,
    metric: &'static str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PackingDefaultsPayload {
    algorithm: &'static str,
    max_width: u32,
    max_height: u32,
    padding: u32,
    trim: bool,
    allow_rotation: bool,
    multipack: bool,
}
