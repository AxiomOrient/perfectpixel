use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};

use crate::{
    apply_raster_edits_with_evidence, compose_bundle_with_packing, inspect_raster,
    normalize_sprite, resize_raster, validate_normalize_plan_contract, AtomicArtifactSetWriter,
    AtomicDirectoryEntry, AtomicDirectoryWriter, AtomicFileWriter, BundlePlan, DecodeLimits,
    ImageCodec, MotionCompiler, MotionRequest, NormalizePlan, NormalizeRequest,
    NormalizeStateImages, NormalizeStateSource, PngEncoder, Raster, RasterInspection,
    ResampleFilter, SpriteBundleRequest, StateFrames, UnitScore, VectorAnalysisRequest,
    VectorDetail, VectorOutcome, VectorPolicy, VectorRequest, Vectorizer, CHROMA_CANDIDATE_PALETTE,
    CHROMA_PLAN_METRIC, CHROMA_PLAN_SCHEMA, MOTION_SCHEMA, PSD_DEFAULT_ALPHA_THRESHOLD,
    PSD_DEFAULT_MAX_KNOTS, PSD_EXPORT_SCHEMA, PSD_MAX_DIMENSION, PSD_MAX_KNOTS,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};

mod asset_codec;
mod cli_input;
mod generation;
mod generation_adapter;
mod request;

pub use request::ApplicationRequest;

pub use crate::{PpError, PpResult};
use asset_codec::{
    encode_raster, output_format, parse_background, AssetEncodeOptions, AssetOutputFormat,
    DEFAULT_JPEG_QUALITY,
};
use cli_input::*;
use generation::GenerationWorkflow;
use generation_adapter::{
    plan_generation_publication, validate_generation_artifact_count, validate_input_snapshot_count,
    validate_input_snapshot_set, verify_generation_publication, verify_input_snapshots,
    GeneratedArtifact, GenerationPublicationRequest, InputSnapshot,
};
const MAX_CONTROL_READ_BYTES: usize = 8 * 1024 * 1024;
const MAX_SVG_READ_BYTES: usize = 16 * 1024 * 1024;
const MAX_ARTIFACT_FILE_BYTES: usize = 64 * 1024 * 1024;
const MAX_RASTER_READ_BYTES: usize = 320 * 1024 * 1024;
const CHROMA_PLAN_OPERATION: &str = "chroma_plan";
const MAX_GENERATION_DECODED_RASTER_BYTES: usize = 512 * 1024 * 1024;
const VECTOR_DIAGNOSTICS_OWNERSHIP_FILE: &str = ".perfectpixel-vector-diagnostics.json";
const VECTOR_DIAGNOSTICS_OWNERSHIP_SCHEMA: &str = "perfectpixel.vector-diagnostics-ownership/1";
const MAX_VECTOR_DIAGNOSTIC_ENTRIES: usize = 64;
const ASSET_INSPECTION_SCHEMA: &str = "perfectpixel.asset-inspection/1";
const ASSET_TRANSFORM_SCHEMA: &str = "perfectpixel.asset-transform/1";
const ASSET_DIGEST_ENCODING: &str = "sha256-lowercase-hex";

const HELP: &str = r#"perfectpixel 0.3.1

PRODUCT
  deterministic asset compiler for AI-generated or authored local assets.
  It finalizes generated assets; it does not generate images or run AI models.
  No network is required. Successful outputs are evaluated before atomic publication.

USAGE
  perfectpixel schema
  perfectpixel inspect <input.png|jpg|jpeg|webp>
  perfectpixel convert <input.png|jpg|jpeg|webp> --out <output.png|jpg|jpeg|webp> [--width <positive integer>] [--height <positive integer>] [--filter nearest|lanczos3] [--jpeg-quality <1..100>] [--background <#RRGGBB>]
  perfectpixel upscale <input.png|jpg|jpeg|webp> --out <output.png|jpg|jpeg|webp> --scale <integer >=2> [--filter nearest|lanczos3] [--jpeg-quality <1..100>] [--background <#RRGGBB>]
  perfectpixel edit --request <edit-request.json>
  perfectpixel psd --request <psd-export-request.json>
  perfectpixel chroma-plan --request <chroma-plan-request.json>
  perfectpixel vector <input.png|jpg|jpeg|webp> --out <output.svg> [--preset auto|pixel-art|legacy-lossless|flat-icon|line-art|bounded-illustration] [--profile compact|motion-structure-ready] [--detail auto|1|2|3|4|5] [--min-quality <0..1>] [--max-quality-loss <0..1>] [--max-paths <positive integer>] [--policy <vector-policy.json>] [--report <evaluation.json>] [--diagnostics <dir>]
  perfectpixel vector-analyze <input.png|jpg|jpeg|webp> [--preset auto|pixel-art|legacy-lossless|flat-icon|line-art|bounded-illustration] [--profile compact|motion-structure-ready] [--policy <vector-policy.json>] [--report <analysis.json>]
  perfectpixel normalize --request <normalize-request.json> --out-dir <dir>
  perfectpixel bundle --request <sprite-request.json> --out-dir <dir>
  perfectpixel motion-scaffold <input.svg> --out-dir <dir>
  perfectpixel motion-build --request <motion-request.json> --out-dir <dir>

EDIT REQUEST
  {"schemaVersion":1,"operation":"edit","input":"source.png","output":"edited.png","steps":[
    {"op":"crop","x":0,"y":0,"width":512,"height":512},
    {"op":"rotate","quarterTurns":1},
    {"op":"flip","axis":"horizontal"},
    {"op":"resize","width":256,"height":256,"filter":"lanczos3"}
  ]}

CHROMA PLAN REQUEST
  {"schemaVersion":1,"operation":"chroma_plan","subjectRgbColors":[[255,255,255],[32,32,32]]}
  Selects one fixed high-saturation background candidate by OKLab maximin distance.
  This is controlled chroma planning, not semantic segmentation or matting.

SPRITE REQUEST
  {
    "character": "hero",
    "sheetImage": "sprite-sheet.png",
    "cellWidth": 256,
    "cellHeight": 256,
    "packing": {
      "maxWidth": 2048,
      "maxHeight": 2048,
      "padding": 2,
      "trim": true,
      "allowRotation": false,
      "multipack": true
    },
    "states": [
      { "name": "idle", "fps": 8, "loop": true, "frames": ["frames/idle/frame-00.png"] }
    ]
  }

NORMALIZE REQUEST
  {
    "character": "hero",
    "cellWidth": 64,
    "cellHeight": 64,
    "chroma": { "rgb": [255, 0, 255] },
    "fit": { "pixelPerfect": true, "logicalHeight": 32 },
    "states": [
      { "name": "idle", "frames": ["raw/idle-00.png", "raw/idle-01.png"] },
      { "name": "walk", "strip": "raw/walk.png", "frameCount": 4 }
    ]
  }

NORMALIZE OUTPUT
  normalize-report.json
  sprite-request.json
  frames/<state>/frame-NN.png

BUNDLE OUTPUT
  manifest.json
  sprite-sheet.png, or sprite-sheet-00.png + sprite-sheet-01.png when multipack is needed
  sprite-sheet.json, or sprite-sheet-00.json + sprite-sheet-01.json when multipack is needed
  frames/<state>/frame-NN.png

VECTOR WORKFLOW
  vector is the sole raster-to-SVG quality-gated publication command and publishes only approved exact SVG bytes.
  vector-analyze produces analysis evidence only and cannot publish SVG or diagnostics.
  JPEG output rejects transparent pixels unless --background explicitly selects a #RRGGBB matte.
  WebP output is lossless and preserves RGBA; lossy WebP is intentionally not an implicit fallback.

MOTION WORKFLOW
  motion-scaffold assigns stable path IDs and writes scene.svg, layers.json,
  motion-request.json, and layer-inspector.html. Fill the starter request's parts and tracks.
  motion-build may publish derived animated.svg from an accepted raster-free SVG source. It writes
  animation.json, motion-report.json, preview.html, and an
  exploded dotLottie v2 layout under dotlottie/. Version 1 animates transform and opacity.
  The exploded layout is not a packed .lottie archive; use an official dotLottie packer later.
"#;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplicationOutput {
    pub stdout: String,
    pub exit_code: i32,
}

impl ApplicationOutput {
    fn from_text(text: String, exit_code: i32) -> Self {
        Self {
            stdout: format!("{text}\n"),
            exit_code,
        }
    }
}

pub fn execute(request: ApplicationRequest) -> ApplicationOutput {
    execute_cli(request.into_cli_args())
}

pub fn execute_cli(args: Vec<String>) -> ApplicationOutput {
    let error_phase = cli_error_phase(&args);
    match run(args) {
        Ok(text) => ApplicationOutput::from_text(text, 0),
        Err(PpError::VectorRejected { payload }) => ApplicationOutput::from_text(payload, 4),
        Err(PpError::CliTransactionFailed { exit_code, payload }) => {
            ApplicationOutput::from_text(payload, exit_code)
        }
        Err(error) => {
            let payload = ErrorPayload {
                ok: false,
                message: cli_error_message(&error),
                phase: error_phase,
                path: cli_error_path(&error),
                original_error: cli_original_error(&error),
            };
            let text = match serde_json::to_string(&payload) {
                Ok(payload) => payload,
                Err(source) => source.to_string(),
            };
            ApplicationOutput::from_text(text, exit_code(&error))
        }
    }
}

fn cli_error_message(error: &PpError) -> String {
    match error {
        PpError::InvalidOption(message) if message.starts_with("unknown command '") => {
            format!("invalid option: {message}")
        }
        PpError::InvalidOption(message) | PpError::InvalidRequest(message) => message.clone(),
        PpError::InvalidOptionSource { message, .. }
        | PpError::InvalidRequestSource { message, .. } => message.clone(),
        _ => error.to_string(),
    }
}

fn cli_original_error(error: &PpError) -> String {
    match error {
        PpError::InvalidOptionSource { original_error, .. }
        | PpError::InvalidRequestSource { original_error, .. } => original_error.clone(),
        _ => error.to_string(),
    }
}
fn cli_error_phase(args: &[String]) -> &'static str {
    match args.first().map(String::as_str) {
        Some("convert") | Some("upscale") => "asset",
        Some("psd") => "psd",
        Some("vector") => "vector",
        Some("vector-analyze") => "vectorAnalyze",
        _ => "cli",
    }
}

fn cli_error_path(error: &PpError) -> Option<String> {
    match error {
        PpError::FileIo { path, .. }
        | PpError::ImageDecode { path, .. }
        | PpError::ImageTooLarge { path, .. }
        | PpError::InvalidOptionSource { path, .. }
        | PpError::InvalidRequestSource { path, .. }
        | PpError::Json { path, .. } => Some(path.display().to_string()),
        PpError::InvalidRequest(message) => message
            .strip_prefix("destination '")
            .and_then(|value| value.split_once("':"))
            .map(|(path, _)| path.to_owned()),
        _ => None,
    }
}

fn run(args: Vec<String>) -> PpResult<String> {
    let Some(command) = args.first().map(|value| value.as_str()) else {
        return Ok(HELP.to_string());
    };
    match command {
        "--help" | "-h" => {
            reject_extra_args(&args[1..], "help")?;
            Ok(HELP.to_string())
        }
        "schema" => {
            reject_extra_args(&args[1..], "schema")?;
            schema()
        }
        "inspect" => inspect(&args[1..]),
        "convert" => convert(&args[1..]),
        "upscale" => upscale(&args[1..]),
        "edit" => edit(&args[1..]),
        "psd" => psd(&args[1..]),
        "chroma-plan" => chroma_plan(&args[1..]),
        "vector" => vector(&args[1..]),
        "vector-analyze" => vector_analyze(&args[1..]),
        "normalize" => normalize(&args[1..]),
        "bundle" => bundle(&args[1..]),
        "motion-scaffold" => motion_scaffold(&args[1..]),
        "motion-build" => motion_build(&args[1..]),
        other => Err(PpError::InvalidOption(format!(
            "unknown command '{}'; use schema, inspect, convert, upscale, edit, psd, chroma-plan, vector, vector-analyze, normalize, bundle, motion-scaffold, or motion-build",
            other
        ))),
    }
}

fn schema() -> PpResult<String> {
    serde_json::to_string_pretty(&SchemaPayload {
        cli_version: env!("CARGO_PKG_VERSION"),
        inspect_schema: ASSET_INSPECTION_SCHEMA,
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
            "auto", "pixel-art", "legacy-lossless", "flat-icon", "line-art",
            "bounded-illustration",
        ],
        vector_profiles: &["compact", "motion-structure-ready"],
        asset_adapter: AssetAdapterSchema {
            raster_inputs: &["png", "jpg", "jpeg", "webp"],
            raster_outputs: &["png", "jpg", "jpeg", "webp"],
            commands: &["inspect", "convert", "upscale"],
            inspection_schema: ASSET_INSPECTION_SCHEMA,
            transform_schema: ASSET_TRANSFORM_SCHEMA,
            digest_encoding: ASSET_DIGEST_ENCODING,
            jpeg_alpha: "transparent input requires --background #RRGGBB",
            webp_output: "lossless RGBA",
            convert_filters: &["nearest", "lanczos3"],
            upscale_default_filter: "nearest",
        },
        edit_command: EditCommandSchema {
            request_schema: EDIT_SCHEMA,
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
            operation: CHROMA_PLAN_OPERATION,
            schema_version: 1,
            required_fields: &["schemaVersion", "operation", "subjectRgbColors"],
            subject_rgb_colors: "1..=32 unique RGB triplets",
            minimum_subject_colors: 1,
            maximum_subject_colors: 32,
            candidate_palette: &CHROMA_CANDIDATE_PALETTE,
            candidate_count: CHROMA_CANDIDATE_PALETTE.len(),
            metric: CHROMA_PLAN_METRIC,
            output: &[
                "schema",
                "schemaVersion",
                "ok",
                "operation",
                "subjectRgbColors",
                "metric",
                "selectedRgb",
                "selectedHex",
                "minDistance",
                "candidateScores",
            ],
        },
        vector_command: VectorCommandSchema {
            arguments: &["<input.png|jpg|jpeg|webp>"],
            options: &[
                "--out", "--preset", "--profile", "--detail", "--min-quality",
                "--max-quality-loss", "--max-paths", "--policy", "--report", "--diagnostics",
            ],
            defaults: VectorGenerationDefaults {
                preset: "auto", profile: "compact", detail: "auto",
                min_quality: None, max_quality_loss: None, max_paths: None,
            },
            publication_order: &["report", "diagnostics", "finalSvg"],
            artifact_order: &[
                "candidate.svg (image/svg+xml)", "render-back.png (image/png)",
            ],
        },
        vector_analyze_command: VectorAnalyzeCommandSchema {
            arguments: &["<input.png|jpg|jpeg|webp>"],
            options: &["--preset", "--profile", "--policy", "--report"],
            defaults: VectorAnalysisDefaults {
                preset: "auto",
                profile: "compact",
            },
            publication_order: &["report"],
            artifact_order: &[],
            publishes_svg: false,
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
    })
    .map_err(|source| PpError::Json {
        path: PathBuf::from("<schema>"),
        message: source.to_string(),
    })
}

fn inspect(args: &[String]) -> PpResult<String> {
    if args.len() != 1 {
        return Err(PpError::InvalidOption(
            "inspect requires exactly one <input>".to_string(),
        ));
    }
    let input = PathBuf::from(&args[0]);
    validate_raster_input_path(&input)?;
    let bytes = read_bytes_limited(&input, MAX_RASTER_READ_BYTES)?;
    let image = ImageCodec::decode_rgba_bytes(&input, &bytes, DecodeLimits::default())?;
    let inspection = inspect_raster(&image);
    serde_json::to_string_pretty(&InspectPayload {
        schema: ASSET_INSPECTION_SCHEMA,
        schema_version: 1,
        ok: true,
        input: input.display().to_string(),
        input_sha256: crate::sha256_hex(&bytes),
        input_byte_count: asset_byte_count(&bytes)?,
        inspection,
    })
    .map_err(|source| PpError::Json {
        path: PathBuf::from("<inspect>"),
        message: source.to_string(),
    })
}

fn chroma_plan(args: &[String]) -> PpResult<String> {
    validate_options(args, &["--request"])?;
    let request_path = option_path(args, "--request")?;
    validate_file_extension(&request_path, &["json"], "chroma plan request")?;
    let (request, _request_snapshot): (ChromaPlanRequest, _) =
        read_json_request_snapshot(&request_path)?;
    if request.schema_version != 1 || request.operation != CHROMA_PLAN_OPERATION {
        return Err(PpError::InvalidRequest(
            "chroma plan request schemaVersion must be 1 and operation must be 'chroma_plan'"
                .to_string(),
        ));
    }
    let plan = crate::plan_chroma(&request.subject_rgb_colors)?;
    let candidate_scores = plan
        .candidates
        .iter()
        .map(|candidate| ChromaCandidateScorePayload {
            rgb: candidate.rgb,
            hex: rgb_hex(candidate.rgb),
            score: candidate.min_distance,
        })
        .collect();
    serialize_json(
        &ChromaPlanResponse {
            schema: CHROMA_PLAN_SCHEMA,
            schema_version: 1,
            ok: true,
            operation: CHROMA_PLAN_OPERATION,
            subject_rgb_colors: request.subject_rgb_colors,
            metric: CHROMA_PLAN_METRIC,
            selected_rgb: plan.selected_rgb,
            selected_hex: rgb_hex(plan.selected_rgb),
            min_distance: plan.min_distance,
            candidate_scores,
        },
        "<chroma-plan>",
    )
}

fn rgb_hex(rgb: [u8; 3]) -> String {
    format!("#{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2])
}

fn convert(args: &[String]) -> PpResult<String> {
    let options = asset_options(args, "convert", ResampleFilter::Lanczos3)?;
    let target = convert_dimensions(&options, args)?;
    write_asset_transform("convert", options, target)
}

fn upscale(args: &[String]) -> PpResult<String> {
    let options = asset_options(args, "upscale", ResampleFilter::Nearest)?;
    let scale = option_value(args, "--scale")
        .ok_or_else(|| PpError::InvalidOption("--scale is required".to_string()))
        .and_then(parse_upscale_factor)?;
    let width = options
        .source_width
        .checked_mul(scale)
        .ok_or_else(|| PpError::InvalidRequest("upscale dimensions overflow".to_string()))?;
    let height = options
        .source_height
        .checked_mul(scale)
        .ok_or_else(|| PpError::InvalidRequest("upscale dimensions overflow".to_string()))?;
    validate_output_dimensions(width, height)?;
    write_asset_transform("upscale", options, (width, height))
}

fn edit(args: &[String]) -> PpResult<String> {
    validate_options(args, &["--request"])?;
    let request_path = option_path(args, "--request")?;
    validate_file_extension(&request_path, &["json"], "edit request")?;
    let (request, _request_snapshot): (EditRequest, _) = read_json_request_snapshot(&request_path)?;
    let base_dir = request_path.parent().unwrap_or_else(|| Path::new("."));
    let input = resolve_edit_path(base_dir, &request.input, "input")?;
    let output = resolve_edit_path(base_dir, &request.output, "output")?;
    validate_raster_input_path(&input)?;
    validate_file_extension(&output, &["png"], "edit output")?;
    reject_same_path(&input, &output, "edit input and output must not collide")?;
    if request.schema_version != 1 || request.operation != "edit" {
        return Err(PpError::InvalidRequest(
            "edit request schemaVersion must be 1 and operation must be 'edit'".to_string(),
        ));
    }
    if request.steps.is_empty() || request.steps.len() > 64 {
        return Err(PpError::InvalidRequest(
            "edit request steps must contain 1..=64 entries".to_string(),
        ));
    }
    let (source, _input_snapshot) = decode_raster_snapshot(&input)?;
    let edits = request
        .steps
        .iter()
        .map(EditStep::to_raster_edit)
        .collect::<PpResult<Vec<_>>>()?;
    let (result, auto_plans) = apply_raster_edits_with_evidence(&source, &edits)?;
    let encoded = PngEncoder::encode_rgba(&result)?;
    reject_same_path(
        &request_path,
        &output,
        "edit output must not overwrite its request",
    )?;
    AtomicFileWriter::write_bytes(&output, &encoded)?;
    serialize_json(
        &EditSummary {
            schema: EDIT_SCHEMA,
            ok: true,
            operation: "edit",
            input: input.display().to_string(),
            output: output.display().to_string(),
            input_width: source.width(),
            input_height: source.height(),
            output_width: result.width(),
            output_height: result.height(),
            steps: request.steps.iter().map(EditStep::evidence_name).collect(),
            output_sha256: crate::sha256_hex(&encoded),
            auto_background: auto_plans
                .into_iter()
                .map(|plan| AutoBackgroundEvidence {
                    selected_keys: plan.selected_keys,
                    edge_coverage_basis_points: plan.edge_coverage_basis_points,
                })
                .collect(),
        },
        "<edit-summary>",
    )
}

fn psd(args: &[String]) -> PpResult<String> {
    validate_options(args, &["--request"])?;
    let request_path = option_path(args, "--request")?;
    validate_file_extension(&request_path, &["json"], "PSD export request")?;
    let (request, _request_snapshot): (PsdExportRequest, _) =
        read_json_request_snapshot(&request_path)?;
    if request.schema_version != 1 || request.operation != "export_psd" {
        return Err(PpError::InvalidRequest(
            "PSD export request schemaVersion must be 1 and operation must be 'export_psd'"
                .to_owned(),
        ));
    }
    let base_dir = request_path.parent().unwrap_or_else(|| Path::new("."));
    let input = resolve_psd_path(base_dir, &request.input, "input")?;
    let output = resolve_psd_path(base_dir, &request.output, "output")?;
    validate_raster_input_path(&input)?;
    validate_file_extension(&output, &["psd"], "PSD output")?;
    reject_same_path(
        &request_path,
        &output,
        "PSD output must not overwrite its request",
    )?;
    reject_same_path(&input, &output, "PSD input and output must not collide")?;
    let (source, _input_snapshot) = decode_raster_snapshot(&input)?;
    let encoded = crate::encode_psd(
        &source,
        crate::PsdPathOptions {
            alpha_threshold: request.path.alpha_threshold,
            max_knots: request.path.max_knots,
        },
    )?;
    let output_bytes = encoded.bytes();
    let output_sha256 = crate::sha256_hex(output_bytes);
    let output_byte_count = output_bytes.len();
    let contour_count = encoded.contour_count();
    let knot_count = encoded.knot_count();
    AtomicFileWriter::write_bytes(&output, output_bytes)?;
    serialize_json(
        &PsdExportSummary {
            schema: PSD_EXPORT_SCHEMA,
            schema_version: 1,
            ok: true,
            operation: "export_psd",
            input: input.display().to_string(),
            output: output.display().to_string(),
            width: source.width(),
            height: source.height(),
            channels: 4,
            depth: 8,
            color_mode: "RGB",
            alpha_threshold: request.path.alpha_threshold,
            max_knots: request.path.max_knots,
            contour_count,
            knot_count,
            output_sha256,
            output_byte_count,
            flattened: true,
            photoshop_native_open: false,
        },
        "<psd-export-summary>",
    )
}

fn resolve_psd_path(base_dir: &Path, value: &str, label: &str) -> PpResult<PathBuf> {
    if value.trim().is_empty()
        || value != value.trim()
        || value.contains('\0')
        || value.contains('\\')
    {
        return Err(PpError::InvalidRequest(format!(
            "PSD {label} path must be a non-empty printable path"
        )));
    }
    if value.len() > 4096 {
        return Err(PpError::InvalidRequest(format!(
            "PSD {label} path exceeds 4096 bytes"
        )));
    }
    let path = Path::new(value);
    if path
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(PpError::InvalidRequest(format!(
            "PSD {label} path must not contain '..'"
        )));
    }
    Ok(if path.is_absolute() {
        path.to_path_buf()
    } else {
        base_dir.join(path)
    })
}

const EDIT_SCHEMA: &str = "perfectpixel.image-edit/1";

fn resolve_edit_path(base_dir: &Path, value: &str, label: &str) -> PpResult<PathBuf> {
    if value.trim().is_empty() || value != value.trim() || value.contains('\0') {
        return Err(PpError::InvalidRequest(format!(
            "edit {label} path must be a non-empty printable path"
        )));
    }
    let path = Path::new(value);
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(base_dir.join(path))
    }
}

fn asset_options(
    args: &[String],
    command: &str,
    default_filter: ResampleFilter,
) -> PpResult<AssetCliOptions> {
    let Some(input_raw) = args.first() else {
        return Err(PpError::InvalidOption(format!(
            "{command} requires <input> and --out <output>"
        )));
    };
    if input_raw.starts_with("--") {
        return Err(PpError::InvalidOption(format!(
            "{command} requires <input> and --out <output>"
        )));
    }
    let allowed = if command == "upscale" {
        &[
            "--out",
            "--scale",
            "--filter",
            "--jpeg-quality",
            "--background",
        ][..]
    } else {
        &[
            "--out",
            "--width",
            "--height",
            "--filter",
            "--jpeg-quality",
            "--background",
        ][..]
    };
    validate_options(&args[1..], allowed)?;

    let input = PathBuf::from(input_raw);
    validate_raster_input_path(&input)?;
    let output = option_path(args, "--out")?;
    let format = output_format(&output)?;
    reject_same_path(&input, &output, "asset input and output must not collide")?;

    let jpeg_quality_raw = option_value(args, "--jpeg-quality");
    let background_raw = option_value(args, "--background");
    if format != AssetOutputFormat::Jpeg && jpeg_quality_raw.is_some() {
        return Err(PpError::InvalidOption(
            "--jpeg-quality is only valid for JPEG output".to_string(),
        ));
    }
    if format != AssetOutputFormat::Jpeg && background_raw.is_some() {
        return Err(PpError::InvalidOption(
            "--background is only valid for JPEG output".to_string(),
        ));
    }
    let jpeg_quality = jpeg_quality_raw
        .map(parse_jpeg_quality)
        .transpose()?
        .unwrap_or(DEFAULT_JPEG_QUALITY);
    let background = background_raw.map(parse_background).transpose()?;
    let filter = option_value(args, "--filter")
        .map(parse_resample_filter)
        .transpose()?
        .unwrap_or(default_filter);
    let source_bytes = read_bytes_limited(&input, MAX_RASTER_READ_BYTES)?;
    let input_sha256 = crate::sha256_hex(&source_bytes);
    let input_byte_count = asset_byte_count(&source_bytes)?;
    let source = ImageCodec::decode_rgba_bytes(&input, &source_bytes, DecodeLimits::default())?;

    Ok(AssetCliOptions {
        input,
        output,
        format,
        filter,
        jpeg_quality,
        background,
        input_sha256,
        input_byte_count,
        source_width: source.width(),
        source_height: source.height(),
        source,
    })
}

fn convert_dimensions(options: &AssetCliOptions, args: &[String]) -> PpResult<(u32, u32)> {
    let width = option_value(args, "--width")
        .map(parse_positive_u32)
        .transpose()?;
    let height = option_value(args, "--height")
        .map(parse_positive_u32)
        .transpose()?;
    let dimensions = match (width, height) {
        (None, None) => (options.source_width, options.source_height),
        (Some(width), Some(height)) => (width, height),
        (Some(width), None) => (
            width,
            proportional_dimension(options.source_height, options.source_width, width)?,
        ),
        (None, Some(height)) => (
            proportional_dimension(options.source_width, options.source_height, height)?,
            height,
        ),
    };
    if dimensions == (options.source_width, options.source_height)
        && option_value(args, "--filter").is_some()
    {
        return Err(PpError::InvalidOption(
            "--filter requires --width or --height for convert".to_string(),
        ));
    }
    validate_output_dimensions(dimensions.0, dimensions.1)?;
    Ok(dimensions)
}

fn proportional_dimension(
    source_numerator: u32,
    source_denominator: u32,
    target: u32,
) -> PpResult<u32> {
    let numerator = u64::from(source_numerator)
        .checked_mul(u64::from(target))
        .ok_or_else(|| PpError::InvalidRequest("resize dimensions overflow".to_string()))?;
    let denominator = u64::from(source_denominator);
    let rounded = numerator
        .checked_add(denominator / 2)
        .ok_or_else(|| PpError::InvalidRequest("resize dimensions overflow".to_string()))?
        / denominator;
    u32::try_from(rounded.max(1))
        .map_err(|_| PpError::InvalidRequest("resize dimensions overflow".to_string()))
}

fn validate_output_dimensions(width: u32, height: u32) -> PpResult<()> {
    DecodeLimits::default().validate(width, height)
}

fn write_asset_transform(
    command: &'static str,
    options: AssetCliOptions,
    target: (u32, u32),
) -> PpResult<String> {
    let image = if target == (options.source_width, options.source_height) {
        options.source
    } else {
        resize_raster(&options.source, target.0, target.1, options.filter)?
    };
    let bytes = encode_raster(
        &image,
        AssetEncodeOptions {
            format: options.format,
            jpeg_quality: options.jpeg_quality,
            background: options.background,
        },
    )?;
    let output_sha256 = crate::sha256_hex(&bytes);
    let output_byte_count = asset_byte_count(&bytes)?;
    AtomicFileWriter::write_bytes(&options.output, &bytes)?;
    serialize_json(
        &AssetTransformSummary {
            schema: ASSET_TRANSFORM_SCHEMA,
            ok: true,
            command,
            input: options.input.display().to_string(),
            output: options.output.display().to_string(),
            input_sha256: options.input_sha256,
            input_byte_count: options.input_byte_count,
            output_sha256,
            output_byte_count,
            input_width: options.source_width,
            input_height: options.source_height,
            output_width: image.width(),
            output_height: image.height(),
            format: asset_output_format_name(options.format),
            filter: resample_filter_name(options.filter),
        },
        "<asset-transform-summary>",
    )
}

fn asset_byte_count(bytes: &[u8]) -> PpResult<u64> {
    u64::try_from(bytes.len())
        .map_err(|_| PpError::InvalidRequest("asset byte count overflow".to_string()))
}

fn parse_positive_u32(raw: &str) -> PpResult<u32> {
    let value = raw
        .parse::<u32>()
        .map_err(|_| PpError::InvalidOption("dimension must be a positive integer".to_string()))?;
    if value == 0 {
        return Err(PpError::InvalidOption(
            "dimension must be a positive integer".to_string(),
        ));
    }
    Ok(value)
}

fn parse_upscale_factor(raw: &str) -> PpResult<u32> {
    let value = raw.parse::<u32>().map_err(|_| {
        PpError::InvalidOption("--scale must be an integer greater than or equal to 2".to_string())
    })?;
    if value < 2 {
        return Err(PpError::InvalidOption(
            "--scale must be an integer greater than or equal to 2".to_string(),
        ));
    }
    Ok(value)
}

fn parse_jpeg_quality(raw: &str) -> PpResult<u8> {
    let value = raw.parse::<u8>().map_err(|_| {
        PpError::InvalidOption("--jpeg-quality must be from 1 through 100".to_string())
    })?;
    if !(1..=100).contains(&value) {
        return Err(PpError::InvalidOption(
            "--jpeg-quality must be from 1 through 100".to_string(),
        ));
    }
    Ok(value)
}

fn parse_resample_filter(raw: &str) -> PpResult<ResampleFilter> {
    match raw {
        "nearest" => Ok(ResampleFilter::Nearest),
        "lanczos3" => Ok(ResampleFilter::Lanczos3),
        _ => Err(PpError::InvalidOption(
            "--filter must be nearest or lanczos3".to_string(),
        )),
    }
}

fn asset_output_format_name(format: AssetOutputFormat) -> &'static str {
    match format {
        AssetOutputFormat::Png => "png",
        AssetOutputFormat::Jpeg => "jpeg",
        AssetOutputFormat::Webp => "webp-lossless",
    }
}

fn resample_filter_name(filter: ResampleFilter) -> &'static str {
    match filter {
        ResampleFilter::Nearest => "nearest",
        ResampleFilter::Lanczos3 => "lanczos3",
    }
}

fn normalize(args: &[String]) -> PpResult<String> {
    validate_options(args, &["--request", "--out-dir"])?;
    let request_path = option_path(args, "--request")?;
    let out_dir = option_path(args, "--out-dir")?;
    let (request, request_snapshot): (NormalizeRequest, _) =
        read_json_request_snapshot(&request_path)?;
    validate_input_snapshot_count(normalize_input_snapshot_count(&request)?)?;
    validate_generation_artifact_count(normalize_max_artifact_count(&request)?)?;
    let request_base = request_path.parent().unwrap_or_else(|| Path::new("."));
    let (sources, mut input_snapshots) = load_normalize_sources(request_base, &request)?;
    input_snapshots.insert(0, request_snapshot);
    validate_input_snapshot_set(&input_snapshots)?;
    let plan = normalize_sprite(&request, sources)?;
    validate_generation_artifact_count(normalize_output_relative_paths(&plan).len())?;
    reject_normalize_output_collisions(&out_dir, &plan, &input_snapshots)?;
    if plan.report.ok {
        validate_normalize_plan_contract(&request, &plan)?;
    }
    write_normalize_outputs(&out_dir, &plan, input_snapshots)?;
    if !plan.report.ok {
        let message = if plan.report.errors.is_empty() {
            "normalize quality gate failed".to_string()
        } else {
            plan.report.errors.join("; ")
        };
        return Err(PpError::QualityGate {
            gate: "normalize".to_string(),
            message,
        });
    }
    serde_json::to_string_pretty(&NormalizeSummary {
        ok: true,
        out_dir: out_dir.display().to_string(),
        character: request.character,
        states: plan.states.len(),
        frames: plan.states.iter().map(|state| state.frames.len()).sum(),
        failed_gates: 0,
        files: normalize_output_files(&plan),
    })
    .map_err(|source| PpError::Json {
        path: PathBuf::from("<normalize-summary>"),
        message: source.to_string(),
    })
}

fn bundle(args: &[String]) -> PpResult<String> {
    validate_options(args, &["--request", "--out-dir"])?;
    let request_path = option_path(args, "--request")?;
    let out_dir = option_path(args, "--out-dir")?;
    let (request, request_snapshot): (SpriteBundleRequest, _) =
        read_json_request_snapshot(&request_path)?;
    validate_input_snapshot_count(bundle_input_snapshot_count(&request)?)?;
    validate_generation_artifact_count(bundle_min_artifact_count(&request)?)?;
    let request_base = request_path.parent().unwrap_or_else(|| Path::new("."));
    let (states, mut input_snapshots) = load_states(request_base, &request)?;
    input_snapshots.insert(0, request_snapshot);
    validate_input_snapshot_set(&input_snapshots)?;
    let plan = compose_bundle_with_packing(
        &request.character,
        &request.sheet_image,
        states,
        request.cell_width,
        request.cell_height,
        request.packing.clone(),
    )?;
    validate_generation_artifact_count(bundle_output_relative_paths(&plan).len())?;
    reject_bundle_output_collisions(&out_dir, &plan, &input_snapshots)?;
    write_bundle(&out_dir, &plan, input_snapshots)?;
    serde_json::to_string_pretty(&BundleSummary {
        ok: true,
        out_dir: out_dir.display().to_string(),
        character: request.character,
        animations: plan.manifest.animations.len(),
        sheets: plan.sheets.len(),
        files: bundle_output_files(&plan),
    })
    .map_err(|source| PpError::Json {
        path: PathBuf::from("<bundle-summary>"),
        message: source.to_string(),
    })
}

fn vector(args: &[String]) -> PpResult<String> {
    let options = vector_options(args, true)?;
    preflight_vector_static_destinations(&options)?;
    let (image, input_snapshot) = decode_raster_snapshot(&options.input)?;
    let outcome = Vectorizer::new()?.run(&image, &options.generation_request()?)?;
    let report = match &outcome {
        VectorOutcome::Approved(output) => output.report(),
        VectorOutcome::Rejected(output) => output.report(),
    };
    let report_json = serialize_json(report, "<vector-report>")?;
    let artifacts = match &outcome {
        VectorOutcome::Approved(output) => output.artifacts(),
        VectorOutcome::Rejected(output) => output.artifacts(),
    };
    let mut transaction =
        CliTransactionOutcome::new(options.report.is_some(), options.diagnostics.is_some());
    let diagnostics_manifest = match options
        .diagnostics
        .as_ref()
        .map(|_| diagnostic_ownership_manifest(artifacts))
        .transpose()
    {
        Ok(manifest) => manifest,
        Err(error) => {
            transaction = transaction.reduce(CliTransactionEvent::DiagnosticsFailed)?;
            return Err(vector_transaction_failure(
                report,
                transaction,
                "diagnostics",
                error,
            )?);
        }
    };
    let mut diagnostic_entries = match options
        .diagnostics
        .as_ref()
        .map(|_| diagnostic_entries(artifacts))
        .transpose()
    {
        Ok(entries) => entries,
        Err(error) => {
            transaction = transaction.reduce(CliTransactionEvent::DiagnosticsFailed)?;
            return Err(vector_transaction_failure(
                report,
                transaction,
                "diagnostics",
                error,
            )?);
        }
    };
    if let (Some(manifest), Some(entries)) =
        (diagnostics_manifest.as_deref(), diagnostic_entries.as_mut())
    {
        entries.push(AtomicDirectoryEntry {
            relative_path: Path::new(VECTOR_DIAGNOSTICS_OWNERSHIP_FILE),
            bytes: manifest,
            sha256: crate::sha256(manifest),
        });
    }
    if let Some(entries) = diagnostic_entries.as_deref() {
        if let Err(error) = preflight_diagnostic_entries(entries) {
            transaction = transaction.reduce(CliTransactionEvent::DiagnosticsFailed)?;
            return Err(vector_transaction_failure(
                report,
                transaction,
                "diagnostics",
                error,
            )?);
        }
    }

    if let Some(path) = &options.report {
        if let Err(error) = preflight_vector_file_destination(path) {
            transaction = transaction.reduce(CliTransactionEvent::ReportFailed)?;
            return Err(vector_transaction_failure(
                report,
                transaction,
                "report",
                error,
            )?);
        }
        if let Err(error) = verify_input_snapshots(std::slice::from_ref(&input_snapshot)) {
            transaction = transaction.reduce(CliTransactionEvent::ReportFailed)?;
            return Err(vector_transaction_failure(
                report,
                transaction,
                "report",
                error,
            )?);
        }
        if let Err(error) = AtomicFileWriter::write_text(path, &report_json) {
            transaction = transaction.reduce(CliTransactionEvent::ReportFailed)?;
            return Err(vector_transaction_failure(
                report,
                transaction,
                "report",
                error,
            )?);
        }
        transaction = transaction.reduce(CliTransactionEvent::ReportCommitted)?;
    }

    if let (Some(path), Some(entries)) = (&options.diagnostics, diagnostic_entries.as_deref()) {
        if let Err(error) = preflight_vector_diagnostics_destination(path) {
            transaction = transaction.reduce(CliTransactionEvent::DiagnosticsFailed)?;
            return Err(vector_transaction_failure(
                report,
                transaction,
                "diagnostics",
                error,
            )?);
        }
        if let Err(error) = verify_input_snapshots(std::slice::from_ref(&input_snapshot)) {
            transaction = transaction.reduce(CliTransactionEvent::DiagnosticsFailed)?;
            return Err(vector_transaction_failure(
                report,
                transaction,
                "diagnostics",
                error,
            )?);
        }
        if let Err(error) = AtomicDirectoryWriter::replace(path, entries) {
            transaction = transaction.reduce(CliTransactionEvent::DiagnosticsFailed)?;
            return Err(vector_transaction_failure(
                report,
                transaction,
                "diagnostics",
                error,
            )?);
        }
        transaction = transaction.reduce(CliTransactionEvent::DiagnosticsCommitted)?;
    }

    match &outcome {
        VectorOutcome::Approved(output) => {
            if let Err(error) = preflight_vector_file_destination(&options.output) {
                transaction = transaction.reduce(CliTransactionEvent::FinalSvgFailed)?;
                return Err(vector_transaction_failure(
                    report,
                    transaction,
                    "finalSvg",
                    error,
                )?);
            }
            if let Err(error) = verify_input_snapshots(std::slice::from_ref(&input_snapshot)) {
                transaction = transaction.reduce(CliTransactionEvent::FinalSvgFailed)?;
                return Err(vector_transaction_failure(
                    report,
                    transaction,
                    "finalSvg",
                    error,
                )?);
            }
            if let Err(error) =
                AtomicFileWriter::write_bytes(&options.output, output.exact_svg_bytes())
            {
                transaction = transaction.reduce(CliTransactionEvent::FinalSvgFailed)?;
                return Err(vector_transaction_failure(
                    report,
                    transaction,
                    "finalSvg",
                    error,
                )?);
            }
            transaction = transaction.reduce(CliTransactionEvent::FinalSvgCommitted)?;
            serialize_json(
                &VectorResult {
                    schema: "perfectpixel.vector-result/1",
                    ok: true,
                    decision: report.actual_decision(),
                    report,
                    transaction,
                    failure: None,
                },
                "<vector-result>",
            )
        }
        VectorOutcome::Rejected(_) => {
            let payload = serialize_json(
                &VectorResult {
                    schema: "perfectpixel.vector-result/1",
                    ok: false,
                    decision: report.actual_decision(),
                    report,
                    transaction,
                    failure: None,
                },
                "<vector-result>",
            )?;
            Err(PpError::VectorRejected { payload })
        }
    }
}

fn vector_analyze(args: &[String]) -> PpResult<String> {
    let options = vector_options(args, false)?;
    preflight_vector_static_destinations(&options)?;
    let request = options.analysis_request()?;
    let (image, input_snapshot) = decode_raster_snapshot(&options.input)?;
    let analysis = Vectorizer::new()?.analyze(&image, &request)?;
    let analysis_json = serialize_json(&analysis, "<vector-analysis>")?;
    let mut transaction = CliTransactionOutcome::analysis(options.report.is_some());
    if let Some(path) = &options.report {
        if let Err(error) = preflight_vector_file_destination(path) {
            transaction = transaction.reduce(CliTransactionEvent::ReportFailed)?;
            return Err(vector_analysis_transaction_failure(
                &analysis,
                transaction,
                "report",
                error,
            )?);
        }
        if let Err(error) = verify_input_snapshots(std::slice::from_ref(&input_snapshot)) {
            transaction = transaction.reduce(CliTransactionEvent::ReportFailed)?;
            return Err(vector_analysis_transaction_failure(
                &analysis,
                transaction,
                "report",
                error,
            )?);
        }
        if let Err(error) = AtomicFileWriter::write_text(path, &analysis_json) {
            transaction = transaction.reduce(CliTransactionEvent::ReportFailed)?;
            return Err(vector_analysis_transaction_failure(
                &analysis,
                transaction,
                "report",
                error,
            )?);
        }
        transaction = transaction.reduce(CliTransactionEvent::ReportCommitted)?;
    }
    serialize_json(
        &VectorAnalysisResult {
            schema: "perfectpixel.vector-analysis-result/1",
            ok: true,
            analysis: &analysis,
            transaction,
            failure: None,
        },
        "<vector-analysis-result>",
    )
}

fn vector_options(args: &[String], generation: bool) -> PpResult<VectorCliOptions> {
    let allowed = if generation {
        &[
            "--out",
            "--preset",
            "--profile",
            "--detail",
            "--min-quality",
            "--max-quality-loss",
            "--max-paths",
            "--policy",
            "--report",
            "--diagnostics",
        ][..]
    } else {
        &["--preset", "--profile", "--policy", "--report"][..]
    };
    if !generation
        && args.iter().any(|arg| {
            matches!(
                arg.as_str(),
                "--out"
                    | "--detail"
                    | "--min-quality"
                    | "--max-quality-loss"
                    | "--max-paths"
                    | "--diagnostics"
            )
        })
    {
        return Err(PpError::InvalidOption("vector-analyze does not accept --out, candidate-detail, quality-limit, path-limit, or diagnostics options".to_string()));
    }
    if args.is_empty()
        || (generation && (args.len() < 3 || args[0].starts_with("--")))
        || (!generation && !args.is_empty() && args[0].starts_with("--"))
    {
        return Err(PpError::InvalidOption(if generation {
            "vector requires <input> and --out <output.svg>".to_string()
        } else {
            "vector-analyze requires exactly one raster input".to_string()
        }));
    }
    let input = PathBuf::from(&args[0]);
    if !generation {
        let mut index = 1usize;
        while index < args.len() {
            if !args[index].starts_with("--") {
                return Err(PpError::InvalidOption(
                    "vector-analyze requires exactly one raster input".to_string(),
                ));
            }
            index += 2;
        }
    }
    validate_options(&args[1..], allowed)?;
    if generation && option_value(&args[1..], "--out").is_none() {
        return Err(PpError::InvalidOption(
            "vector requires <input> and --out <output.svg>".to_string(),
        ));
    }
    validate_raster_input_path(&input)?;
    let output = if generation {
        let output = option_path(&args[1..], "--out")?;
        validate_file_extension(&output, &["svg"], "vector output")?;
        output
    } else {
        PathBuf::new()
    };
    let report = option_path_optional(&args[1..], "--report");
    if let Some(path) = &report {
        validate_file_extension(path, &["json"], "vector report")?;
    }
    let policy_path = option_path_optional(&args[1..], "--policy");
    let policy = load_vector_policy(policy_path.clone())?;
    let diagnostics = option_path_optional(&args[1..], "--diagnostics");
    if generation {
        preflight_vector_parent(&output)?;
    }
    if let Some(path) = report.as_deref() {
        preflight_vector_parent(path)?;
    }
    if let Some(path) = diagnostics.as_deref() {
        preflight_vector_parent(path)?;
    }
    reject_vector_destination_collisions(
        &input,
        policy_path.as_deref(),
        &output,
        report.as_deref(),
        diagnostics.as_deref(),
        generation,
    )?;
    Ok(VectorCliOptions {
        input,
        output,
        report,
        diagnostics,
        preset: option_value(&args[1..], "--preset")
            .map(parse_vector_preset)
            .transpose()?
            .unwrap_or(crate::VectorPresetSelection::Auto),
        profile: option_value(&args[1..], "--profile")
            .map(parse_vector_profile)
            .transpose()?
            .unwrap_or(crate::SvgProfile::Compact),
        detail: option_value(&args[1..], "--detail")
            .map(parse_vector_detail)
            .transpose()?
            .flatten(),
        minimum_quality: option_value(&args[1..], "--min-quality")
            .map(|raw| parse_unit_float(raw, "--min-quality"))
            .transpose()?,
        maximum_quality_loss: option_value(&args[1..], "--max-quality-loss")
            .map(|raw| parse_unit_float(raw, "--max-quality-loss"))
            .transpose()?,
        maximum_paths: option_value(&args[1..], "--max-paths")
            .map(|raw| parse_positive_usize(raw, "--max-paths"))
            .transpose()?,
        policy,
    })
}

fn load_vector_policy(path: Option<PathBuf>) -> PpResult<VectorPolicy> {
    let Some(path) = path else {
        return Ok(VectorPolicy::default());
    };
    if validate_file_extension(&path, &["json"], "vector policy").is_err() {
        return Err(PpError::InvalidOption(
            "--policy must reference a .json perfectpixel.vector-policy/1 document".to_string(),
        ));
    }
    let text = read_utf8_limited(&path, MAX_CONTROL_READ_BYTES)?;
    let policy: VectorPolicy =
        serde_json::from_str(&text).map_err(|source| PpError::InvalidOptionSource {
            message: "--policy must reference a .json perfectpixel.vector-policy/1 document"
                .to_string(),
            path: path.clone(),
            original_error: source.to_string(),
        })?;
    if policy.schema() != VectorPolicy::SCHEMA {
        return Err(PpError::InvalidOption(
            "--policy must reference a .json perfectpixel.vector-policy/1 document".to_string(),
        ));
    }
    policy.validate().map_err(|_| {
        PpError::InvalidOption(
            "--policy must reference a .json perfectpixel.vector-policy/1 document".to_string(),
        )
    })?;
    Ok(policy)
}

fn reject_vector_destination_collisions(
    input: &Path,
    policy: Option<&Path>,
    output: &Path,
    report: Option<&Path>,
    diagnostics: Option<&Path>,
    generation: bool,
) -> PpResult<()> {
    if generation {
        reject_vector_path_overlap(input, output, "vector input and output must not collide")?;
        if let Some(policy) = policy {
            reject_vector_path_overlap(
                policy,
                output,
                "vector policy and output must not collide",
            )?;
        }
    }
    if let Some(report) = report {
        reject_vector_path_overlap(input, report, "vector input and report must not collide")?;
        if let Some(policy) = policy {
            reject_vector_path_overlap(
                policy,
                report,
                "vector policy and report must not collide",
            )?;
        }
        if generation {
            reject_vector_path_overlap(
                output,
                report,
                "vector output and report must not collide",
            )?;
        }
    }
    if let Some(diagnostics) = diagnostics {
        reject_vector_path_overlap(
            input,
            diagnostics,
            "vector input and diagnostics must not collide",
        )?;
        if let Some(policy) = policy {
            reject_vector_path_overlap(
                policy,
                diagnostics,
                "vector policy and diagnostics must not collide",
            )?;
        }
        if generation {
            reject_vector_path_overlap(
                output,
                diagnostics,
                "vector output and diagnostics must not collide",
            )?;
        }
        if let Some(report) = report {
            reject_vector_path_overlap(
                report,
                diagnostics,
                "vector report and diagnostics must not collide",
            )?;
        }
    }
    Ok(())
}
fn preflight_vector_static_destinations(options: &VectorCliOptions) -> PpResult<()> {
    let mut destinations: Vec<&Path> = Vec::new();
    if !options.output.as_os_str().is_empty() {
        destinations.push(&options.output);
    }
    if let Some(report) = options.report.as_deref() {
        destinations.push(report);
    }
    if let Some(diagnostics) = options.diagnostics.as_deref() {
        destinations.push(diagnostics);
    }
    if !options.output.as_os_str().is_empty() {
        preflight_vector_file_destination(&options.output)?;
    }
    if let Some(report) = options.report.as_deref() {
        preflight_vector_file_destination(report)?;
    }
    if let Some(diagnostics) = options.diagnostics.as_deref() {
        preflight_vector_diagnostics_destination(diagnostics)?;
    }
    for (index, left) in destinations.iter().enumerate() {
        for right in destinations.iter().skip(index + 1) {
            if vector_paths_equivalent(left, right)? {
                return Err(destination_error(
                    left,
                    "destination collides with another destination",
                ));
            }
        }
    }
    Ok(())
}

fn vector_paths_equivalent(left: &Path, right: &Path) -> PpResult<bool> {
    let left = vector_comparable_path(left)?;
    let right = vector_comparable_path(right)?;
    Ok(left == right
        || same_file_identity(&left, &right)?
        || left.to_string_lossy().to_lowercase() == right.to_string_lossy().to_lowercase())
}

fn preflight_vector_file_destination(path: &Path) -> PpResult<()> {
    preflight_vector_parent(path)?;
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => Err(
            destination_error(path, "file destination must be a regular non-symlink file"),
        ),
        Ok(_) => Ok(()),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(destination_error(path, &source.to_string())),
    }
}

fn preflight_vector_diagnostics_destination(path: &Path) -> PpResult<()> {
    preflight_vector_parent(path)?;
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            Err(destination_error(
                path,
                "diagnostics destination must be a non-symlink directory",
            ))
        }
        Ok(_) => {
            if vector_diagnostics_owned(path)? {
                Ok(())
            } else {
                Err(destination_error(
                    path,
                    "diagnostics destination is not owned by perfectpixel vector",
                ))
            }
        }
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(destination_error(path, &source.to_string())),
    }
}

fn vector_diagnostics_owned(path: &Path) -> PpResult<bool> {
    let mut names = BTreeSet::new();
    for entry in fs::read_dir(path).map_err(|source| PpError::FileIo {
        path: path.to_owned(),
        message: source.to_string(),
    })? {
        let entry = entry.map_err(|source| PpError::FileIo {
            path: path.to_owned(),
            message: source.to_string(),
        })?;
        names.insert(entry.file_name());
        if names.len() > MAX_VECTOR_DIAGNOSTIC_ENTRIES + 1 {
            return Ok(false);
        }
    }
    let marker_path = path.join(VECTOR_DIAGNOSTICS_OWNERSHIP_FILE);
    let marker_metadata = match fs::symlink_metadata(&marker_path) {
        Ok(metadata)
            if metadata.is_file()
                && !metadata.file_type().is_symlink()
                && metadata.len() <= MAX_CONTROL_READ_BYTES as u64 =>
        {
            metadata
        }
        Ok(_) => return Ok(false),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(source) => {
            return Err(PpError::FileIo {
                path: marker_path,
                message: source.to_string(),
            });
        }
    };
    let marker = fs::read(&marker_path).map_err(|source| PpError::FileIo {
        path: marker_path.clone(),
        message: source.to_string(),
    })?;
    if marker.len() as u64 != marker_metadata.len() {
        return Ok(false);
    }
    let manifest: VectorDiagnosticsOwnership = match serde_json::from_slice(&marker) {
        Ok(manifest) => manifest,
        Err(_) => return Ok(false),
    };
    if manifest.schema != VECTOR_DIAGNOSTICS_OWNERSHIP_SCHEMA
        || manifest.artifacts.len() > MAX_VECTOR_DIAGNOSTIC_ENTRIES
    {
        return Ok(false);
    }
    let mut expected =
        BTreeSet::from([std::ffi::OsString::from(VECTOR_DIAGNOSTICS_OWNERSHIP_FILE)]);
    for artifact in &manifest.artifacts {
        if !matches!(artifact.path.as_str(), "candidate.svg" | "render-back.png")
            || !crate::is_sha256_hex(&artifact.sha256)
            || !expected.insert(std::ffi::OsString::from(artifact.path.as_str()))
        {
            return Ok(false);
        }
    }
    if names != expected {
        return Ok(false);
    }
    for artifact in manifest.artifacts {
        let artifact_path = path.join(&artifact.path);
        let metadata = match fs::symlink_metadata(&artifact_path) {
            Ok(metadata)
                if metadata.is_file()
                    && !metadata.file_type().is_symlink()
                    && metadata.len() <= MAX_ARTIFACT_FILE_BYTES as u64 =>
            {
                metadata
            }
            Ok(_) => return Ok(false),
            Err(source) => {
                return Err(PpError::FileIo {
                    path: artifact_path,
                    message: source.to_string(),
                });
            }
        };
        let bytes = fs::read(&artifact_path).map_err(|source| PpError::FileIo {
            path: artifact_path,
            message: source.to_string(),
        })?;
        if bytes.len() as u64 != metadata.len() || crate::sha256_hex(&bytes) != artifact.sha256 {
            return Ok(false);
        }
    }
    Ok(true)
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct VectorDiagnosticsOwnership {
    schema: String,
    artifacts: Vec<VectorDiagnosticsOwnershipArtifact>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct VectorDiagnosticsOwnershipArtifact {
    path: String,
    sha256: String,
}

fn preflight_vector_parent(path: &Path) -> PpResult<()> {
    let mut parent = path.parent();
    while let Some(candidate) = parent {
        match fs::symlink_metadata(candidate) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                return Err(destination_error(
                    candidate,
                    "destination parent must be a non-symlink directory",
                ));
            }
            Ok(_) => return Ok(()),
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
                parent = candidate.parent()
            }
            Err(source) => return Err(destination_error(candidate, &source.to_string())),
        }
    }
    Ok(())
}

fn destination_error(path: &Path, message: &str) -> PpError {
    PpError::InvalidRequest(format!("destination '{}': {message}", path.display()))
}

fn preflight_diagnostic_entries(entries: &[AtomicDirectoryEntry<'_>]) -> PpResult<()> {
    if entries.len() > 64 {
        return Err(PpError::FileIo {
            path: PathBuf::from("<diagnostics>"),
            message: "too many directory entries".to_owned(),
        });
    }
    let mut paths = BTreeSet::new();
    let mut total = 0usize;
    for entry in entries {
        let path = entry.relative_path;
        if path.as_os_str().is_empty()
            || path.as_os_str().len() > 1024
            || path
                .components()
                .any(|component| !matches!(component, Component::Normal(_)))
            || path.components().count() > 16
            || !paths.insert(path.to_path_buf())
        {
            return Err(PpError::FileIo {
                path: path.to_path_buf(),
                message: "invalid or duplicate diagnostic entry path".to_owned(),
            });
        }
        total = total
            .checked_add(entry.bytes.len())
            .ok_or_else(|| PpError::FileIo {
                path: path.to_path_buf(),
                message: "diagnostic artifact bytes overflow".to_owned(),
            })?;
        if total > MAX_ARTIFACT_FILE_BYTES {
            return Err(PpError::FileIo {
                path: path.to_path_buf(),
                message: "diagnostic artifact bytes exceed limit".to_owned(),
            });
        }
    }
    for path in &paths {
        if path
            .ancestors()
            .skip(1)
            .any(|ancestor| paths.contains(ancestor))
        {
            return Err(PpError::FileIo {
                path: path.to_path_buf(),
                message: "diagnostic entry collides with another entry".to_owned(),
            });
        }
    }
    Ok(())
}

fn reject_vector_path_overlap(left: &Path, right: &Path, message: &str) -> PpResult<()> {
    let left = vector_comparable_path(left)?;
    let right = vector_comparable_path(right)?;
    if left == right
        || same_file_identity(left.as_path(), right.as_path())?
        || left.to_string_lossy().to_lowercase() == right.to_string_lossy().to_lowercase()
        || left.starts_with(&right)
        || right.starts_with(&left)
    {
        return Err(PpError::InvalidRequest(message.to_string()));
    }
    Ok(())
}

fn vector_comparable_path(path: &Path) -> PpResult<PathBuf> {
    match fs::canonicalize(path) {
        Ok(real_path) => return Ok(real_path),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {}
        Err(source) => {
            return Err(PpError::FileIo {
                path: path.to_owned(),
                message: source.to_string(),
            });
        }
    }
    let current_dir = env::current_dir().map_err(|source| PpError::FileIo {
        path: PathBuf::from("."),
        message: source.to_string(),
    })?;
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        current_dir.join(path)
    };
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Normal(value) => normalized.push(value),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
        }
    }

    let mut existing_prefix = normalized.clone();
    let mut missing_suffix = Vec::new();
    loop {
        match fs::symlink_metadata(&existing_prefix) {
            Ok(_) => break,
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
                let Some(file_name) = existing_prefix.file_name().map(ToOwned::to_owned) else {
                    return Err(PpError::FileIo {
                        path: normalized,
                        message: "destination has no resolvable existing parent".to_owned(),
                    });
                };
                missing_suffix.push(file_name);
                existing_prefix.pop();
            }
            Err(source) => {
                return Err(PpError::FileIo {
                    path: existing_prefix,
                    message: source.to_string(),
                });
            }
        }
    }
    let mut canonical_prefix =
        fs::canonicalize(&existing_prefix).map_err(|source| PpError::FileIo {
            path: existing_prefix,
            message: source.to_string(),
        })?;
    for component in missing_suffix.into_iter().rev() {
        canonical_prefix.push(component);
    }
    Ok(canonical_prefix)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct FileIdentity {
    device: u64,
    file: u64,
}

fn same_file_identity(left: &Path, right: &Path) -> PpResult<bool> {
    Ok(matches!(
        (file_identity(left)?, file_identity(right)?),
        (Some(left), Some(right)) if left == right
    ))
}

fn file_identity(path: &Path) -> PpResult<Option<FileIdentity>> {
    use std::os::unix::fs::MetadataExt;

    match fs::metadata(path) {
        Ok(metadata) => Ok(Some(FileIdentity {
            device: metadata.dev(),
            file: metadata.ino(),
        })),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(PpError::FileIo {
            path: path.to_owned(),
            message: source.to_string(),
        }),
    }
}

fn motion_scaffold(args: &[String]) -> PpResult<String> {
    let input = args.first().ok_or_else(|| {
        PpError::InvalidOption("motion-scaffold requires <input.svg>".to_string())
    })?;
    validate_options(&args[1..], &["--out-dir"])?;
    let input = PathBuf::from(input);
    validate_file_extension(&input, &["svg"], "motion SVG input")?;
    let out_dir = option_path(&args[1..], "--out-dir")?;
    let (source, input_snapshot) = read_utf8_snapshot(&input, MAX_SVG_READ_BYTES)?;
    validate_input_snapshot_set(std::slice::from_ref(&input_snapshot))?;
    let scaffold = MotionCompiler::scaffold(&source)?;
    let layers_json = serialize_json(&scaffold.layers, "<motion-layers>")?;
    let starter_request = MotionRequest {
        schema: MOTION_SCHEMA.to_string(),
        name: derived_motion_name(&input),
        source_svg: "scene.svg".to_string(),
        source_svg_sha256: MotionCompiler::scene_sha256(&scaffold.scene_svg),
        fps: 30,
        duration_ms: 1_000,
        looped: true,
        authored_paths: Vec::new(),
        parts: Vec::new(),
        tracks: Vec::new(),
        markers: Vec::new(),
    };
    let request_json = serialize_json(&starter_request, "<motion-request>")?;
    let files = vec![
        "scene.svg".to_string(),
        "layers.json".to_string(),
        "motion-request.json".to_string(),
        "layer-inspector.html".to_string(),
    ];
    reject_generated_output_collisions(&out_dir, &files, std::slice::from_ref(&input_snapshot))?;
    let artifacts = vec![
        text_artifact("scene.svg", scaffold.scene_svg),
        text_artifact("layers.json", layers_json),
        text_artifact("motion-request.json", request_json),
        text_artifact("layer-inspector.html", scaffold.inspector_html),
    ];
    publish_generated_artifacts(
        &out_dir,
        GenerationWorkflow::MotionScaffold,
        artifacts,
        vec![GenerationWorkflow::MotionBuild],
        vec![input_snapshot],
    )?;
    serialize_json(
        &MotionScaffoldSummary {
            ok: true,
            out_dir: out_dir.display().to_string(),
            paths: scaffold.layers.layers.len(),
            files,
        },
        "<motion-scaffold-summary>",
    )
}

fn derived_motion_name(input: &Path) -> String {
    let stem = input
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    let mut name = String::with_capacity(stem.len());
    let mut pending_hyphen = false;
    for byte in stem.bytes() {
        if byte.is_ascii_alphanumeric() {
            if pending_hyphen && !name.is_empty() {
                name.push('-');
            }
            name.push((byte as char).to_ascii_lowercase());
            pending_hyphen = false;
        } else {
            pending_hyphen = true;
        }
    }
    if name.is_empty() {
        "motion".to_string()
    } else {
        name
    }
}

fn motion_build(args: &[String]) -> PpResult<String> {
    validate_options(args, &["--request", "--out-dir"])?;
    let request_path = option_path(args, "--request")?;
    validate_file_extension(&request_path, &["json"], "motion request")?;
    let out_dir = option_path(args, "--out-dir")?;
    let (request, request_snapshot): (MotionRequest, _) =
        read_json_request_snapshot(&request_path)?;
    let request_base = request_path.parent().unwrap_or_else(|| Path::new("."));
    let source_path = resolve_motion_source_path(request_base, &request.source_svg)?;
    let (source, source_snapshot) = read_utf8_snapshot(&source_path, MAX_SVG_READ_BYTES)?;
    let build = MotionCompiler::build(&source, &request)?;
    let report_json = serialize_json(&build.report, "<motion-report>")?;
    let animation_relative = format!("dotlottie/a/{}.json", request.name);
    let files = vec![
        "animated.svg".to_string(),
        "animation.json".to_string(),
        "motion-report.json".to_string(),
        "preview.html".to_string(),
        "dotlottie/manifest.json".to_string(),
        animation_relative.clone(),
    ];
    let motion_inputs = vec![request_snapshot, source_snapshot];
    validate_input_snapshot_set(&motion_inputs)?;
    reject_generated_output_collisions(&out_dir, &files, &motion_inputs)?;
    let artifacts = vec![
        text_artifact("animated.svg", build.animated_svg),
        text_artifact("animation.json", build.lottie_json.clone()),
        text_artifact("motion-report.json", report_json),
        text_artifact("preview.html", build.preview_html),
        text_artifact("dotlottie/manifest.json", build.dotlottie_manifest_json),
        text_artifact(&animation_relative, build.lottie_json),
    ];
    publish_generated_artifacts(
        &out_dir,
        GenerationWorkflow::MotionBuild,
        artifacts,
        Vec::new(),
        motion_inputs,
    )?;
    serialize_json(
        &MotionBuildSummary {
            ok: true,
            out_dir: out_dir.display().to_string(),
            name: request.name,
            paths: build.report.path_count,
            parts: build.report.part_count,
            tracks: build.report.track_count,
            lottie_shapes: build.report.lottie_shape_count,
            dotlottie_archive_created: build.report.dotlottie_archive_created,
            files,
        },
        "<motion-build-summary>",
    )
}

fn resolve_motion_source_path(base_dir: &Path, value: &str) -> PpResult<PathBuf> {
    let relative = managed_relative_path(value).map_err(|_| {
        PpError::InvalidRequest(format!(
            "motion sourceSvg '{}' must be a safe path relative to the request file",
            value
        ))
    })?;
    validate_file_extension(&relative, &["svg"], "motion sourceSvg")?;
    Ok(base_dir.join(relative))
}

fn reject_generated_output_collisions(
    out_dir: &Path,
    outputs: &[String],
    inputs: &[InputSnapshot],
) -> PpResult<()> {
    for relative in outputs {
        let output = out_dir.join(managed_relative_path(relative)?);
        for input in inputs {
            reject_same_path(
                input.source_path(),
                &output,
                "generated output must not overwrite its request or input",
            )?;
        }
    }
    Ok(())
}

fn text_artifact(relative_path: &str, text: String) -> BundleArtifact {
    BundleArtifact {
        relative_path: relative_path.to_string(),
        bytes: text.into_bytes(),
    }
}

fn serialize_json<T: Serialize>(value: &T, path: &str) -> PpResult<String> {
    serde_json::to_string_pretty(value).map_err(|source| PpError::Json {
        path: PathBuf::from(path),
        message: source.to_string(),
    })
}

fn read_bytes_limited(path: &Path, limit: usize) -> PpResult<Vec<u8>> {
    let mut file = fs::File::open(path).map_err(|source| PpError::FileIo {
        path: path.to_path_buf(),
        message: source.to_string(),
    })?;
    let read_limit = u64::try_from(limit)
        .ok()
        .and_then(|value| value.checked_add(1))
        .ok_or_else(|| PpError::InvalidRequest("file read limit overflow".to_string()))?;
    let mut bytes = Vec::new();
    file.by_ref()
        .take(read_limit)
        .read_to_end(&mut bytes)
        .map_err(|source| PpError::FileIo {
            path: path.to_path_buf(),
            message: source.to_string(),
        })?;
    if bytes.len() > limit {
        return Err(PpError::FileIo {
            path: path.to_path_buf(),
            message: format!("file exceeds {limit}-byte read limit"),
        });
    }
    Ok(bytes)
}

fn read_utf8_limited(path: &Path, limit: usize) -> PpResult<String> {
    String::from_utf8(read_bytes_limited(path, limit)?).map_err(|source| PpError::FileIo {
        path: path.to_path_buf(),
        message: source.to_string(),
    })
}

fn read_utf8_snapshot(path: &Path, limit: usize) -> PpResult<(String, InputSnapshot)> {
    let (bytes, snapshot) = InputSnapshot::capture(path, limit)?;
    let text = String::from_utf8(bytes).map_err(|source| PpError::FileIo {
        path: path.to_path_buf(),
        message: source.to_string(),
    })?;
    Ok((text, snapshot))
}

fn read_json_request_snapshot<T: DeserializeOwned>(path: &Path) -> PpResult<(T, InputSnapshot)> {
    let (text, snapshot) = read_utf8_snapshot(path, MAX_CONTROL_READ_BYTES)?;
    let request = serde_json::from_str(&text).map_err(|source| PpError::InvalidRequestSource {
        message: format!("invalid JSON request at '{}': {source}", path.display()),
        path: path.to_path_buf(),
        original_error: source.to_string(),
    })?;
    Ok((request, snapshot))
}

fn decode_raster_snapshot(path: &Path) -> PpResult<(Raster, InputSnapshot)> {
    let (bytes, snapshot) = InputSnapshot::capture(path, MAX_RASTER_READ_BYTES)?;
    let raster = ImageCodec::decode_rgba_bytes(path, &bytes, DecodeLimits::default())?;
    Ok((raster, snapshot))
}

fn normalize_input_snapshot_count(request: &NormalizeRequest) -> PpResult<usize> {
    request.states.iter().try_fold(1usize, |total, state| {
        let state_count = if state.frames.is_empty() {
            usize::from(state.strip.is_some())
        } else {
            state.frames.len()
        };
        total
            .checked_add(state_count)
            .ok_or_else(|| PpError::InvalidRequest("normalize input count overflow".to_string()))
    })
}

fn bundle_input_snapshot_count(request: &SpriteBundleRequest) -> PpResult<usize> {
    request.states.iter().try_fold(1usize, |total, state| {
        total
            .checked_add(state.frames.len())
            .ok_or_else(|| PpError::InvalidRequest("bundle input count overflow".to_string()))
    })
}

fn normalize_max_artifact_count(request: &NormalizeRequest) -> PpResult<usize> {
    let frame_count = request.states.iter().try_fold(0usize, |total, state| {
        let count = if state.frames.is_empty() {
            match state.frame_count {
                Some(value) => usize::try_from(value).map_err(|_| {
                    PpError::InvalidRequest(
                        "normalize frame count does not fit this platform".to_string(),
                    )
                })?,
                None => 0,
            }
        } else {
            state.frames.len()
        };
        total.checked_add(count).ok_or_else(|| {
            PpError::InvalidRequest("normalize output artifact count overflow".to_string())
        })
    })?;
    frame_count.checked_add(2).ok_or_else(|| {
        PpError::InvalidRequest("normalize output artifact count overflow".to_string())
    })
}

fn bundle_min_artifact_count(request: &SpriteBundleRequest) -> PpResult<usize> {
    let frame_count = request.states.iter().try_fold(0usize, |total, state| {
        total.checked_add(state.frames.len()).ok_or_else(|| {
            PpError::InvalidRequest("bundle output artifact count overflow".to_string())
        })
    })?;
    // A valid bundle publishes one manifest, at least one sheet image, one
    // Aseprite JSON, and one copied output for every input frame.
    frame_count
        .checked_add(3)
        .ok_or_else(|| PpError::InvalidRequest("bundle output artifact count overflow".to_string()))
}

fn account_decoded_raster_bytes(total: &mut usize, raster: &Raster) -> PpResult<()> {
    *total = total
        .checked_add(raster.pixels().len())
        .ok_or_else(|| PpError::InvalidRequest("decoded raster byte count overflow".to_string()))?;
    if *total > MAX_GENERATION_DECODED_RASTER_BYTES {
        return Err(PpError::InvalidRequest(format!(
            "generation decoded rasters exceed {MAX_GENERATION_DECODED_RASTER_BYTES}-byte limit"
        )));
    }
    Ok(())
}

fn load_states(
    base_dir: &Path,
    request: &SpriteBundleRequest,
) -> PpResult<(Vec<StateFrames>, Vec<InputSnapshot>)> {
    let mut states = Vec::with_capacity(request.states.len());
    let mut snapshots = Vec::new();
    let mut decoded_raster_bytes = 0usize;
    for state in &request.states {
        let mut frames = Vec::with_capacity(state.frames.len());
        for frame in &state.frames {
            let path = resolve_request_frame_path(base_dir, frame)?;
            let (image, snapshot) = decode_raster_snapshot(&path)?;
            account_decoded_raster_bytes(&mut decoded_raster_bytes, &image)?;
            frames.push(image);
            snapshots.push(snapshot);
        }
        states.push(StateFrames {
            name: state.name.clone(),
            fps: state.fps,
            looped: state.looped,
            frames,
        });
    }
    Ok((states, snapshots))
}

fn load_normalize_sources(
    base_dir: &Path,
    request: &NormalizeRequest,
) -> PpResult<(Vec<NormalizeStateImages>, Vec<InputSnapshot>)> {
    let mut sources = Vec::with_capacity(request.states.len());
    let mut snapshots = Vec::new();
    let mut decoded_raster_bytes = 0usize;
    for state in &request.states {
        let source = if !state.frames.is_empty() {
            let mut frames = Vec::with_capacity(state.frames.len());
            for frame in &state.frames {
                let path = resolve_request_frame_path(base_dir, frame)?;
                let (image, snapshot) = decode_raster_snapshot(&path)?;
                account_decoded_raster_bytes(&mut decoded_raster_bytes, &image)?;
                frames.push(image);
                snapshots.push(snapshot);
            }
            NormalizeStateSource::Frames(frames)
        } else if let Some(strip) = state.strip.as_deref() {
            let path = resolve_request_frame_path(base_dir, strip)?;
            let (image, snapshot) = decode_raster_snapshot(&path)?;
            account_decoded_raster_bytes(&mut decoded_raster_bytes, &image)?;
            snapshots.push(snapshot);
            NormalizeStateSource::Strip {
                image,
                frame_count: state.frame_count.unwrap_or(0),
            }
        } else {
            NormalizeStateSource::Frames(Vec::new())
        };
        sources.push(NormalizeStateImages {
            name: state.name.clone(),
            fps: state.fps,
            looped: state.looped,
            source,
        });
    }
    Ok((sources, snapshots))
}

fn reject_normalize_output_collisions(
    out_dir: &Path,
    plan: &NormalizePlan,
    input_snapshots: &[InputSnapshot],
) -> PpResult<()> {
    for output_path in normalize_output_paths(out_dir, plan) {
        for input in input_snapshots {
            reject_same_path(
                input.source_path(),
                &output_path,
                "normalize output must not overwrite request or frame inputs",
            )?;
        }
    }
    Ok(())
}

fn normalize_output_paths(out_dir: &Path, plan: &NormalizePlan) -> Vec<PathBuf> {
    normalize_output_relative_paths(plan)
        .into_iter()
        .map(|path| out_dir.join(path))
        .collect()
}

fn normalize_output_files(plan: &NormalizePlan) -> Vec<String> {
    let mut files = normalize_output_relative_paths(plan)
        .into_iter()
        .collect::<Vec<_>>();
    files.sort();
    files
}

fn normalize_output_relative_paths(plan: &NormalizePlan) -> BTreeSet<String> {
    let mut paths = BTreeSet::new();
    paths.insert("normalize-report.json".to_string());
    if plan.report.ok {
        paths.insert("sprite-request.json".to_string());
        for state in &plan.states {
            for index in 0..state.frames.len() {
                paths.insert(format!("frames/{}/frame-{index:02}.png", state.name));
            }
        }
    }
    paths
}

fn write_normalize_outputs(
    out_dir: &Path,
    plan: &NormalizePlan,
    input_snapshots: Vec<InputSnapshot>,
) -> PpResult<()> {
    let mut artifacts = Vec::new();
    let report_json =
        serde_json::to_string_pretty(&plan.report).map_err(|source| PpError::Json {
            path: out_dir.join("normalize-report.json"),
            message: source.to_string(),
        })?;
    artifacts.push(BundleArtifact {
        relative_path: "normalize-report.json".to_string(),
        bytes: report_json.into_bytes(),
    });
    if plan.report.ok {
        let request_json =
            serde_json::to_string_pretty(&plan.bundle_request).map_err(|source| PpError::Json {
                path: out_dir.join("sprite-request.json"),
                message: source.to_string(),
            })?;
        artifacts.push(BundleArtifact {
            relative_path: "sprite-request.json".to_string(),
            bytes: request_json.into_bytes(),
        });
        for state in &plan.states {
            for (index, frame) in state.frames.iter().enumerate() {
                artifacts.push(BundleArtifact {
                    relative_path: format!("frames/{}/frame-{index:02}.png", state.name),
                    bytes: PngEncoder::encode_rgba(frame)?,
                });
            }
        }
    }
    publish_generated_artifacts(
        out_dir,
        GenerationWorkflow::Normalize,
        artifacts,
        Vec::new(),
        input_snapshots,
    )
}

fn reject_bundle_output_collisions(
    out_dir: &Path,
    plan: &BundlePlan,
    input_snapshots: &[InputSnapshot],
) -> PpResult<()> {
    for output_path in bundle_output_paths(out_dir, plan) {
        for input in input_snapshots {
            reject_same_path(
                input.source_path(),
                &output_path,
                "bundle output must not overwrite request or frame inputs",
            )?;
        }
    }
    Ok(())
}

fn bundle_output_paths(out_dir: &Path, plan: &BundlePlan) -> Vec<PathBuf> {
    bundle_output_relative_paths(plan)
        .into_iter()
        .map(|path| out_dir.join(path))
        .collect()
}

fn bundle_output_files(plan: &BundlePlan) -> Vec<String> {
    let mut files = Vec::with_capacity(1 + plan.sheets.len() * 2 + 1);
    for sheet in &plan.sheets {
        files.push(sheet.relative_path.clone());
    }
    files.push("manifest.json".to_string());
    for sheet in &plan.sheets {
        files.push(sheet.aseprite_json_path.clone());
    }
    files.push("frames/".to_string());
    files
}

fn write_bundle(
    out_dir: &Path,
    plan: &BundlePlan,
    input_snapshots: Vec<InputSnapshot>,
) -> PpResult<()> {
    let artifacts = bundle_artifacts(out_dir, plan)?;
    publish_generated_artifacts(
        out_dir,
        GenerationWorkflow::Bundle,
        artifacts,
        Vec::new(),
        input_snapshots,
    )
}

fn bundle_output_relative_paths(plan: &BundlePlan) -> BTreeSet<String> {
    let mut paths = BTreeSet::new();
    paths.insert("manifest.json".to_string());
    for sheet in &plan.sheets {
        paths.insert(sheet.relative_path.clone());
        paths.insert(sheet.aseprite_json_path.clone());
    }
    for frame in &plan.frame_outputs {
        paths.insert(frame.relative_path.clone());
    }
    paths
}

struct BundleArtifact {
    relative_path: String,
    bytes: Vec<u8>,
}

fn bundle_artifacts(out_dir: &Path, plan: &BundlePlan) -> PpResult<Vec<BundleArtifact>> {
    let mut artifacts = Vec::with_capacity(1 + plan.sheets.len() * 2 + plan.frame_outputs.len());
    for sheet in &plan.sheets {
        artifacts.push(BundleArtifact {
            relative_path: sheet.relative_path.clone(),
            bytes: PngEncoder::encode_rgba(&sheet.image)?,
        });
        artifacts.push(BundleArtifact {
            relative_path: sheet.aseprite_json_path.clone(),
            bytes: sheet.aseprite_json.as_bytes().to_vec(),
        });
    }
    let manifest_json =
        serde_json::to_string_pretty(&plan.manifest).map_err(|source| PpError::Json {
            path: out_dir.join("manifest.json"),
            message: source.to_string(),
        })?;
    artifacts.push(BundleArtifact {
        relative_path: "manifest.json".to_string(),
        bytes: manifest_json.into_bytes(),
    });
    for frame in &plan.frame_outputs {
        artifacts.push(BundleArtifact {
            relative_path: frame.relative_path.clone(),
            bytes: PngEncoder::encode_rgba(&frame.image)?,
        });
    }
    Ok(artifacts)
}

fn publish_generated_artifacts(
    out_dir: &Path,
    workflow: GenerationWorkflow,
    artifacts: Vec<BundleArtifact>,
    invalidates: Vec<GenerationWorkflow>,
    input_snapshots: Vec<InputSnapshot>,
) -> PpResult<()> {
    AtomicArtifactSetWriter::publish_with_planner_checked(
        out_dir,
        move |locked_root| {
            plan_generation_publication(
                locked_root,
                GenerationPublicationRequest {
                    workflow,
                    artifacts: artifacts
                        .into_iter()
                        .map(|artifact| GeneratedArtifact {
                            relative_path: artifact.relative_path,
                            bytes: artifact.bytes,
                        })
                        .collect(),
                    invalidates,
                    input_snapshots,
                },
            )
        },
        verify_generation_publication,
    )
}

fn managed_relative_path(value: &str) -> PpResult<PathBuf> {
    if value.is_empty() || value.contains('\0') || value.contains('\\') {
        return Err(PpError::InvalidRequest(format!(
            "managed bundle output '{}' is not a safe relative path",
            value
        )));
    }
    let path = PathBuf::from(value);
    if path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(PpError::InvalidRequest(format!(
            "managed bundle output '{}' is not a safe relative path",
            value
        )));
    }
    Ok(path)
}

fn exit_code(error: &PpError) -> i32 {
    match error {
        PpError::InvalidOption(_)
        | PpError::InvalidOptionSource { .. }
        | PpError::InvalidRequest(_)
        | PpError::InvalidRequestSource { .. }
        | PpError::SvgContract(_) => 2,
        PpError::FileIo { .. }
        | PpError::ImageDecode { .. }
        | PpError::ImageEncode { .. }
        | PpError::ImageTooLarge { .. }
        | PpError::SvgRender(_)
        | PpError::Json { .. } => 3,
        PpError::VectorRejected { .. }
        | PpError::VectorQuality { .. }
        | PpError::QualityGate { .. }
        | PpError::UnsupportedVectorContent(_) => 4,
        PpError::CliTransactionFailed { exit_code, .. } => *exit_code,
        _ => 1,
    }
}
fn diagnostic_ownership_manifest(artifacts: &crate::DiagnosticArtifactSet) -> PpResult<Vec<u8>> {
    serde_json::to_vec(&VectorDiagnosticsOwnership {
        schema: VECTOR_DIAGNOSTICS_OWNERSHIP_SCHEMA.to_owned(),
        artifacts: artifacts
            .artifacts()
            .iter()
            .map(|artifact| VectorDiagnosticsOwnershipArtifact {
                path: artifact.relative_path().to_owned(),
                sha256: artifact.digest().to_owned(),
            })
            .collect(),
    })
    .map_err(|source| PpError::Json {
        path: PathBuf::from("<vector-diagnostics-ownership>"),
        message: source.to_string(),
    })
}

fn diagnostic_entries(
    artifacts: &crate::DiagnosticArtifactSet,
) -> PpResult<Vec<AtomicDirectoryEntry<'_>>> {
    artifacts
        .artifacts()
        .iter()
        .map(|artifact| {
            let digest = artifact.digest();
            let mut sha256 = [0u8; 32];
            if digest.len() != 64 {
                return Err(PpError::Vectorizer(
                    "diagnostic artifact digest is not SHA-256".to_owned(),
                ));
            }
            for (index, byte) in sha256.iter_mut().enumerate() {
                *byte =
                    u8::from_str_radix(&digest[index * 2..index * 2 + 2], 16).map_err(|_| {
                        PpError::Vectorizer(
                            "diagnostic artifact digest is not hexadecimal".to_owned(),
                        )
                    })?;
            }
            Ok(AtomicDirectoryEntry {
                relative_path: Path::new(artifact.relative_path()),
                bytes: artifact.exact_bytes(),
                sha256,
            })
        })
        .collect()
}

fn vector_transaction_failure(
    report: &crate::EvaluationReport,
    transaction: CliTransactionOutcome,
    phase: &'static str,
    error: PpError,
) -> PpResult<PpError> {
    let payload = serialize_json(
        &VectorResult {
            schema: "perfectpixel.vector-result/1",
            ok: false,
            decision: report.actual_decision(),
            report,
            transaction,
            failure: Some(CliFailure::from_error(phase, error)),
        },
        "<vector-result>",
    )?;
    Ok(PpError::CliTransactionFailed {
        exit_code: 3,
        payload,
    })
}

fn vector_analysis_transaction_failure(
    analysis: &crate::VectorAnalysis,
    transaction: CliTransactionOutcome,
    phase: &'static str,
    error: PpError,
) -> PpResult<PpError> {
    let payload = serialize_json(
        &VectorAnalysisResult {
            schema: "perfectpixel.vector-analysis-result/1",
            ok: false,
            analysis,
            transaction,
            failure: Some(CliFailure::from_error(phase, error)),
        },
        "<vector-analysis-result>",
    )?;
    Ok(PpError::CliTransactionFailed {
        exit_code: 3,
        payload,
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ErrorPayload {
    ok: bool,
    message: String,
    phase: &'static str,
    path: Option<String>,
    original_error: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CliFailure {
    phase: &'static str,
    path: Option<String>,
    original_error: String,
}

impl CliFailure {
    fn from_error(phase: &'static str, error: PpError) -> Self {
        Self {
            phase,
            path: cli_error_path(&error),
            original_error: cli_original_error(&error),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
enum CommitState {
    NotRequested,
    NotAttempted,
    Committed,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct CliTransactionOutcome {
    report: CommitState,
    diagnostics: CommitState,
    final_commit: CommitState,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CliTransactionEvent {
    ReportCommitted,
    ReportFailed,
    DiagnosticsCommitted,
    DiagnosticsFailed,
    FinalSvgCommitted,
    FinalSvgFailed,
}

impl CliTransactionOutcome {
    fn new(report_requested: bool, diagnostics_requested: bool) -> Self {
        Self {
            report: if report_requested {
                CommitState::NotAttempted
            } else {
                CommitState::NotRequested
            },
            diagnostics: if diagnostics_requested {
                CommitState::NotAttempted
            } else {
                CommitState::NotRequested
            },
            final_commit: CommitState::NotAttempted,
        }
    }
    fn analysis(report_requested: bool) -> Self {
        Self {
            report: if report_requested {
                CommitState::NotAttempted
            } else {
                CommitState::NotRequested
            },
            diagnostics: CommitState::NotRequested,
            final_commit: CommitState::NotRequested,
        }
    }

    /// Pure reducer for persistence facts. Callers run the filesystem effect first,
    /// then record its explicit success or failure event here.
    fn reduce(self, event: CliTransactionEvent) -> PpResult<Self> {
        let mut next = self;
        let (slot, result, artifact) = match event {
            CliTransactionEvent::ReportCommitted => {
                (&mut next.report, CommitState::Committed, "report")
            }
            CliTransactionEvent::ReportFailed => (&mut next.report, CommitState::Failed, "report"),
            CliTransactionEvent::DiagnosticsCommitted => {
                (&mut next.diagnostics, CommitState::Committed, "diagnostics")
            }
            CliTransactionEvent::DiagnosticsFailed => {
                (&mut next.diagnostics, CommitState::Failed, "diagnostics")
            }
            CliTransactionEvent::FinalSvgCommitted => {
                (&mut next.final_commit, CommitState::Committed, "finalSvg")
            }
            CliTransactionEvent::FinalSvgFailed => {
                (&mut next.final_commit, CommitState::Failed, "finalSvg")
            }
        };
        if *slot != CommitState::NotAttempted {
            return Err(PpError::Vectorizer(format!(
                "invalid vector transaction event {event:?}: {artifact} is already {slot:?}"
            )));
        }
        *slot = result;
        Ok(next)
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct VectorResult<'a> {
    schema: &'static str,
    ok: bool,
    decision: crate::EvaluationDecision,
    report: &'a crate::EvaluationReport,
    transaction: CliTransactionOutcome,
    #[serde(skip_serializing_if = "Option::is_none")]
    failure: Option<CliFailure>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct VectorAnalysisResult<'a> {
    schema: &'static str,
    ok: bool,
    analysis: &'a crate::VectorAnalysis,
    transaction: CliTransactionOutcome,
    #[serde(skip_serializing_if = "Option::is_none")]
    failure: Option<CliFailure>,
}

struct VectorCliOptions {
    input: PathBuf,
    output: PathBuf,
    report: Option<PathBuf>,
    diagnostics: Option<PathBuf>,
    preset: crate::VectorPresetSelection,
    profile: crate::SvgProfile,
    detail: Option<u8>,
    minimum_quality: Option<f64>,
    maximum_quality_loss: Option<f64>,
    maximum_paths: Option<usize>,
    policy: VectorPolicy,
}

struct AssetCliOptions {
    input: PathBuf,
    output: PathBuf,
    format: AssetOutputFormat,
    filter: ResampleFilter,
    jpeg_quality: u8,
    background: Option<[u8; 3]>,
    input_sha256: String,
    input_byte_count: u64,
    source_width: u32,
    source_height: u32,
    source: crate::Raster,
}

impl VectorCliOptions {
    fn analysis_request(&self) -> PpResult<VectorAnalysisRequest> {
        VectorAnalysisRequest::new(self.preset, self.profile, self.policy.clone())
            .map_err(|error| PpError::InvalidRequest(error.to_string()))
    }

    fn generation_request(&self) -> PpResult<VectorRequest> {
        VectorRequest::new(
            self.preset,
            self.profile,
            self.detail
                .map(VectorDetail::new)
                .transpose()
                .map_err(|error| PpError::InvalidRequest(error.to_string()))?,
            self.minimum_quality
                .map(UnitScore::new)
                .transpose()
                .map_err(|error| PpError::InvalidRequest(error.to_string()))?,
            self.maximum_quality_loss
                .map(UnitScore::new)
                .transpose()
                .map_err(|error| PpError::InvalidRequest(error.to_string()))?,
            self.maximum_paths
                .map(|value| {
                    std::num::NonZeroUsize::new(value).ok_or_else(|| {
                        PpError::InvalidRequest("--max-paths must be a positive integer".to_owned())
                    })
                })
                .transpose()?,
            self.policy.clone(),
            if self.diagnostics.is_some() {
                crate::DiagnosticsIntent::requested(Vec::new())
                    .map_err(|error| PpError::InvalidRequest(error.to_string()))?
            } else {
                crate::DiagnosticsIntent::none()
            },
        )
        .map_err(|error| PpError::InvalidRequest(error.to_string()))
    }
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
    vector_command: VectorCommandSchema,
    vector_analyze_command: VectorAnalyzeCommandSchema,
    vector_authority: &'static str,
    packing_defaults: PackingDefaultsPayload,
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
    output: &'static [&'static str],
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
struct VectorCommandSchema {
    arguments: &'static [&'static str],
    options: &'static [&'static str],
    defaults: VectorGenerationDefaults,
    publication_order: &'static [&'static str],
    artifact_order: &'static [&'static str],
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct VectorAnalyzeCommandSchema {
    arguments: &'static [&'static str],
    options: &'static [&'static str],
    defaults: VectorAnalysisDefaults,
    publication_order: &'static [&'static str],
    artifact_order: &'static [&'static str],
    publishes_svg: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct VectorAnalysisDefaults {
    preset: &'static str,
    profile: &'static str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct VectorGenerationDefaults {
    preset: &'static str,
    profile: &'static str,
    detail: &'static str,
    min_quality: Option<f64>,
    max_quality_loss: Option<f64>,
    max_paths: Option<usize>,
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct InspectPayload {
    schema: &'static str,
    schema_version: u32,
    ok: bool,
    input: String,
    input_sha256: String,
    input_byte_count: u64,
    #[serde(flatten)]
    inspection: RasterInspection,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AssetTransformSummary {
    schema: &'static str,
    ok: bool,
    command: &'static str,
    input: String,
    output: String,
    input_sha256: String,
    input_byte_count: u64,
    output_sha256: String,
    output_byte_count: u64,
    input_width: u32,
    input_height: u32,
    output_width: u32,
    output_height: u32,
    format: &'static str,
    filter: &'static str,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ChromaPlanRequest {
    schema_version: u32,
    operation: String,
    subject_rgb_colors: Vec<[u8; 3]>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ChromaPlanResponse {
    schema: &'static str,
    schema_version: u32,
    ok: bool,
    operation: &'static str,
    subject_rgb_colors: Vec<[u8; 3]>,
    metric: &'static str,
    selected_rgb: [u8; 3],
    selected_hex: String,
    min_distance: f64,
    candidate_scores: Vec<ChromaCandidateScorePayload>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ChromaCandidateScorePayload {
    rgb: [u8; 3],
    hex: String,
    score: f64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct EditRequest {
    schema_version: u32,
    operation: String,
    input: String,
    output: String,
    steps: Vec<EditStep>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PsdExportRequest {
    schema_version: u32,
    operation: String,
    input: String,
    output: String,
    path: PsdPathRequest,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PsdPathRequest {
    alpha_threshold: u8,
    max_knots: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PsdExportSummary {
    schema: &'static str,
    schema_version: u32,
    ok: bool,
    operation: &'static str,
    input: String,
    output: String,
    width: u32,
    height: u32,
    channels: u16,
    depth: u16,
    color_mode: &'static str,
    alpha_threshold: u8,
    max_knots: usize,
    contour_count: usize,
    knot_count: usize,
    output_sha256: String,
    output_byte_count: usize,
    flattened: bool,
    photoshop_native_open: bool,
}

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum EditStep {
    Crop {
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    },
    Rotate {
        #[serde(rename = "quarterTurns")]
        quarter_turns: u8,
    },
    Flip {
        axis: EditFlipAxis,
    },
    Resize {
        width: u32,
        height: u32,
        filter: EditFilter,
    },
    RemoveBackground {
        keys: Vec<[u8; 3]>,
        tolerance: u8,
        feather: u8,
    },
    RemoveBackgroundAuto {
        #[serde(rename = "maxKeys")]
        max_keys: u8,
        #[serde(rename = "minEdgeCoverageBasisPoints")]
        min_edge_coverage_basis_points: u16,
        tolerance: u8,
        feather: u8,
    },
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum EditFlipAxis {
    Horizontal,
    Vertical,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum EditFilter {
    Nearest,
    Lanczos3,
}

impl EditStep {
    fn to_raster_edit(&self) -> PpResult<crate::RasterEdit> {
        Ok(match self {
            Self::Crop {
                x,
                y,
                width,
                height,
            } => crate::RasterEdit::Crop {
                x: *x,
                y: *y,
                width: *width,
                height: *height,
            },
            Self::Rotate { quarter_turns } => crate::RasterEdit::RotateQuarterTurns {
                quarter_turns: *quarter_turns,
            },
            Self::Flip { axis } => match axis {
                EditFlipAxis::Horizontal => crate::RasterEdit::FlipHorizontal,
                EditFlipAxis::Vertical => crate::RasterEdit::FlipVertical,
            },
            Self::Resize {
                width,
                height,
                filter,
            } => crate::RasterEdit::Resize {
                width: *width,
                height: *height,
                filter: match filter {
                    EditFilter::Nearest => crate::ResampleFilter::Nearest,
                    EditFilter::Lanczos3 => crate::ResampleFilter::Lanczos3,
                },
            },
            Self::RemoveBackground {
                keys,
                tolerance,
                feather,
            } => crate::RasterEdit::RemoveBackground {
                keys: keys.clone(),
                tolerance: *tolerance,
                feather: *feather,
            },
            Self::RemoveBackgroundAuto {
                max_keys,
                min_edge_coverage_basis_points,
                tolerance,
                feather,
            } => crate::RasterEdit::RemoveBackgroundAuto {
                max_keys: *max_keys,
                min_edge_coverage_basis_points: *min_edge_coverage_basis_points,
                tolerance: *tolerance,
                feather: *feather,
            },
        })
    }

    fn evidence_name(&self) -> &'static str {
        match self {
            Self::Crop { .. } => "crop",
            Self::Rotate { .. } => "rotate",
            Self::Flip { .. } => "flip",
            Self::Resize { .. } => "resize",
            Self::RemoveBackground { .. } => "remove_background",
            Self::RemoveBackgroundAuto { .. } => "remove_background_auto",
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct EditSummary {
    schema: &'static str,
    ok: bool,
    operation: &'static str,
    input: String,
    output: String,
    input_width: u32,
    input_height: u32,
    output_width: u32,
    output_height: u32,
    steps: Vec<&'static str>,
    output_sha256: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    auto_background: Vec<AutoBackgroundEvidence>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AutoBackgroundEvidence {
    selected_keys: Vec<[u8; 3]>,
    edge_coverage_basis_points: u16,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct NormalizeSummary {
    ok: bool,
    out_dir: String,
    character: String,
    states: usize,
    frames: usize,
    failed_gates: usize,
    files: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BundleSummary {
    ok: bool,
    out_dir: String,
    character: String,
    animations: usize,
    sheets: usize,
    files: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MotionScaffoldSummary {
    ok: bool,
    out_dir: String,
    paths: usize,
    files: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MotionBuildSummary {
    ok: bool,
    out_dir: String,
    name: String,
    paths: usize,
    parts: usize,
    tracks: usize,
    lottie_shapes: usize,
    dotlottie_archive_created: bool,
    files: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn owned(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn request_frame_path_rejects_unsupported_extension() {
        let result = resolve_request_frame_path(Path::new("."), "frames/idle/frame-00.gif");
        assert!(result.is_err());
    }

    #[test]
    fn schema_rejects_extra_args() {
        let result = run(owned(&["schema", "extra"]));
        assert!(result.is_err());
    }

    #[test]
    fn inspect_rejects_extra_args() {
        let result = run(owned(&["inspect", "a.png", "b.png"]));
        assert!(result.is_err());
    }

    #[test]
    fn pack_views_is_no_longer_public_cli_surface() {
        let result = run(owned(&[
            "pack-views",
            "--request",
            "a.json",
            "--out-dir",
            "out",
        ]));
        assert!(result.is_err());
    }

    #[test]
    fn same_path_rejects_dot_normalized_output() {
        let result = reject_same_path(
            Path::new("input.png"),
            Path::new("./input.png"),
            "same path",
        );
        assert!(result.is_err());
    }

    #[test]
    fn bundle_output_rejects_request_file_collision() -> PpResult<()> {
        let root = unique_test_root("bundle-request-collision");
        fs::create_dir_all(&root).map_err(|source| PpError::FileIo {
            path: root.clone(),
            message: source.to_string(),
        })?;
        let input = root.join("manifest.json");
        fs::write(&input, b"{}").map_err(|source| PpError::FileIo {
            path: input.clone(),
            message: source.to_string(),
        })?;
        let (_, snapshot) = InputSnapshot::capture(&input, 16)?;
        let plan = test_bundle_plan()?;

        let result = reject_bundle_output_collisions(&root, &plan, &[snapshot]);

        assert!(result.is_err());
        let _ = fs::remove_dir_all(&root);
        Ok(())
    }

    #[test]
    fn bundle_output_rejects_frame_file_collision() -> PpResult<()> {
        let root = unique_test_root("bundle-frame-collision");
        let input = root.join("frames/idle/frame-00.png");
        fs::create_dir_all(input.parent().expect("frame parent")).map_err(|source| {
            PpError::FileIo {
                path: input.clone(),
                message: source.to_string(),
            }
        })?;
        fs::write(&input, b"frame").map_err(|source| PpError::FileIo {
            path: input.clone(),
            message: source.to_string(),
        })?;
        let (_, snapshot) = InputSnapshot::capture(&input, 16)?;
        let plan = test_bundle_plan()?;

        let result = reject_bundle_output_collisions(&root, &plan, &[snapshot]);

        assert!(result.is_err());
        let _ = fs::remove_dir_all(&root);
        Ok(())
    }

    #[test]
    fn vector_transaction_reducer_records_each_effect_once() -> PpResult<()> {
        let transaction = CliTransactionOutcome::new(true, true);
        let transaction = transaction.reduce(CliTransactionEvent::ReportCommitted)?;
        assert_eq!(transaction.report, CommitState::Committed);
        assert_eq!(transaction.diagnostics, CommitState::NotAttempted);
        assert_eq!(transaction.final_commit, CommitState::NotAttempted);

        let transaction = transaction.reduce(CliTransactionEvent::DiagnosticsFailed)?;
        assert_eq!(transaction.diagnostics, CommitState::Failed);
        assert!(transaction
            .reduce(CliTransactionEvent::DiagnosticsCommitted)
            .is_err());

        let transaction = transaction.reduce(CliTransactionEvent::FinalSvgCommitted)?;
        assert_eq!(transaction.final_commit, CommitState::Committed);
        Ok(())
    }

    #[test]
    fn analysis_transaction_reducer_rejects_unrequested_effects() {
        let transaction = CliTransactionOutcome::analysis(false);
        assert!(transaction
            .reduce(CliTransactionEvent::ReportCommitted)
            .is_err());
        assert!(transaction
            .reduce(CliTransactionEvent::DiagnosticsFailed)
            .is_err());
        assert!(transaction
            .reduce(CliTransactionEvent::FinalSvgFailed)
            .is_err());
    }

    fn unique_test_root(prefix: &str) -> PathBuf {
        use std::time::{SystemTime, UNIX_EPOCH};

        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "perfectpixel-{prefix}-{}-{stamp}",
            std::process::id()
        ))
    }

    fn test_bundle_plan() -> PpResult<BundlePlan> {
        let frame = crate::Raster::new(1, 1, vec![255, 0, 0, 255])?;
        compose_bundle_with_packing(
            "hero",
            "sprite-sheet.png",
            vec![StateFrames {
                name: "idle".to_string(),
                fps: 8,
                looped: true,
                frames: vec![frame],
            }],
            1,
            1,
            Default::default(),
        )
    }
}
