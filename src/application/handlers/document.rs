use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{AtomicFileWriter, FilePrecondition, PpError, PpResult, PSD_EXPORT_SCHEMA};

use super::super::{
    path::{reject_same_path, validate_file_extension, validate_raster_input_path},
    shared::{decode_raster_snapshot, read_json_request_snapshot, serialize_json},
};

/// Compatibility v1 export. This remains intentionally flattened while the canonical layered
/// compiler is expressed separately through DocumentIR.
pub(super) fn export_flattened_psd(request_path: PathBuf) -> PpResult<String> {
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

    let output_precondition = FilePrecondition::capture(&output)?;
    let (source, _input_snapshot) = decode_raster_snapshot(&input)?;
    let encoded = crate::encode_psd(
        &source,
        crate::PsdPathOptions {
            alpha_threshold: request.path.alpha_threshold,
            max_knots: request.path.max_knots,
        },
    )?;
    let output_bytes = encoded.bytes();
    let output_artifact = crate::ArtifactRef::from_bytes("image/vnd.adobe.photoshop", output_bytes)?;
    let output_byte_count = output_bytes.len();
    let contour_count = encoded.contour_count();
    let knot_count = encoded.knot_count();
    AtomicFileWriter::write_bytes_checked(&output, &output_precondition, output_bytes)?;

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
            output_artifact,
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
    output_artifact: crate::ArtifactRef,
    output_byte_count: usize,
    flattened: bool,
    photoshop_native_open: bool,
}
