use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{
    apply_raster_edits_with_evidence, AtomicFileWriter, PngEncoder, PpError, PpResult,
    PSD_EXPORT_SCHEMA,
};

use super::super::{
    path::{reject_same_path, validate_file_extension, validate_raster_input_path},
    shared::{decode_raster_snapshot, read_json_request_snapshot, serialize_json},
};

const EDIT_SCHEMA: &str = "perfectpixel.image-edit/1";
const CHROMA_PLAN_OPERATION: &str = "chroma_plan";

pub(super) fn chroma_plan(request_path: PathBuf) -> PpResult<String> {
    validate_file_extension(&request_path, &["json"], "chroma plan request")?;
    let (request, _snapshot): (ChromaPlanRequest, _) =
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
            schema: crate::CHROMA_PLAN_SCHEMA,
            schema_version: 1,
            ok: true,
            operation: CHROMA_PLAN_OPERATION,
            subject_rgb_colors: request.subject_rgb_colors,
            metric: crate::CHROMA_PLAN_METRIC,
            selected_rgb: plan.selected_rgb,
            selected_hex: rgb_hex(plan.selected_rgb),
            min_distance: plan.min_distance,
            candidate_scores,
        },
        "<chroma-plan>",
    )
}

pub(super) fn edit(request_path: PathBuf) -> PpResult<String> {
    validate_file_extension(&request_path, &["json"], "edit request")?;
    let (request, _request_snapshot): (EditRequest, _) =
        read_json_request_snapshot(&request_path)?;
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

    let base_dir = request_path.parent().unwrap_or_else(|| Path::new("."));
    let input = resolve_edit_path(base_dir, &request.input, "input")?;
    let output = resolve_edit_path(base_dir, &request.output, "output")?;
    validate_raster_input_path(&input)?;
    validate_file_extension(&output, &["png"], "edit output")?;
    reject_same_path(&input, &output, "edit input and output must not collide")?;
    reject_same_path(
        &request_path,
        &output,
        "edit output must not overwrite its request",
    )?;

    let (source, _input_snapshot) = decode_raster_snapshot(&input)?;
    let edits = request
        .steps
        .iter()
        .map(EditStep::to_raster_edit)
        .collect::<Vec<_>>();
    let (result, auto_plans) = apply_raster_edits_with_evidence(&source, &edits)?;
    let encoded = PngEncoder::encode_rgba(&result)?;
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

pub(super) fn export_psd(request_path: PathBuf) -> PpResult<String> {
    validate_file_extension(&request_path, &["json"], "PSD export request")?;
    let (request, _request_snapshot): (PsdExportRequest, _) =
        read_json_request_snapshot(&request_path)?;
    if request.schema_version != 1 || request.operation != "export_psd" {
        return Err(PpError::InvalidRequest(
            "PSD export request schemaVersion must be 1 and operation must be 'export_psd'"
                .to_string(),
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

fn resolve_edit_path(base_dir: &Path, value: &str, label: &str) -> PpResult<PathBuf> {
    if value.trim().is_empty() || value != value.trim() || value.contains('\0') {
        return Err(PpError::InvalidRequest(format!(
            "edit {label} path must be a non-empty printable path"
        )));
    }
    let path = Path::new(value);
    Ok(if path.is_absolute() {
        path.to_path_buf()
    } else {
        base_dir.join(path)
    })
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

fn rgb_hex(rgb: [u8; 3]) -> String {
    format!("#{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2])
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
    fn to_raster_edit(&self) -> crate::RasterEdit {
        match self {
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
        }
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
