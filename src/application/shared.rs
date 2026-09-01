use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

use serde::{de::DeserializeOwned, Serialize};

use crate::{AtomicArtifactSetWriter, ImageCodec, PpError, PpResult, Raster};

use super::{
    generation::GenerationWorkflow,
    generation_adapter::{
        plan_generation_publication, verify_generation_publication, GeneratedArtifact,
        GenerationPublicationRequest, InputSnapshot,
    },
    path::{managed_relative_path, reject_same_path},
};

pub(super) const MAX_CONTROL_READ_BYTES: usize = 8 * 1024 * 1024;
pub(super) const MAX_SVG_READ_BYTES: usize = 16 * 1024 * 1024;
pub(super) const MAX_ARTIFACT_FILE_BYTES: usize = 64 * 1024 * 1024;
pub(super) const MAX_RASTER_READ_BYTES: usize = 320 * 1024 * 1024;
pub(super) const MAX_GENERATION_DECODED_RASTER_BYTES: usize = 512 * 1024 * 1024;

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

pub(super) fn render_result(result: PpResult<String>, phase: &'static str) -> ApplicationOutput {
    match result {
        Ok(text) => ApplicationOutput::from_text(text, 0),
        Err(PpError::VectorRejected { payload }) => ApplicationOutput::from_text(payload, 4),
        Err(PpError::CliTransactionFailed { exit_code, payload }) => {
            ApplicationOutput::from_text(payload, exit_code)
        }
        Err(error) => {
            let payload = ErrorPayload {
                ok: false,
                message: error_message(&error),
                phase,
                path: error_path(&error),
                original_error: original_error(&error),
            };
            let text = serde_json::to_string(&payload).unwrap_or_else(|source| source.to_string());
            ApplicationOutput::from_text(text, exit_code(&error))
        }
    }
}

pub(super) fn operation_phase(name: &str) -> &'static str {
    match name {
        "image.inspect" | "image.convert" | "image.upscale" | "image.edit" | "image.chroma_plan" => "asset",
        "document.export_psd" => "psd",
        "sprite.normalize" | "sprite.compile" => "sprite",
        "vector.compile" => "vector",
        "vector.analyze" => "vectorAnalyze",
        "motion.scaffold" | "motion.compile" => "motion",
        _ => "application",
    }
}

fn error_message(error: &PpError) -> String {
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

pub(super) fn original_error(error: &PpError) -> String {
    match error {
        PpError::InvalidOptionSource { original_error, .. }
        | PpError::InvalidRequestSource { original_error, .. } => original_error.clone(),
        _ => error.to_string(),
    }
}

pub(super) fn error_path(error: &PpError) -> Option<String> {
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ErrorPayload {
    ok: bool,
    message: String,
    phase: &'static str,
    path: Option<String>,
    original_error: String,
}

pub(super) fn serialize_json<T: Serialize>(value: &T, path: &str) -> PpResult<String> {
    serde_json::to_string_pretty(value).map_err(|source| PpError::Json {
        path: PathBuf::from(path),
        message: source.to_string(),
    })
}

pub(super) fn read_bytes_limited(path: &Path, limit: usize) -> PpResult<Vec<u8>> {
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

pub(super) fn read_utf8_limited(path: &Path, limit: usize) -> PpResult<String> {
    String::from_utf8(read_bytes_limited(path, limit)?).map_err(|source| PpError::FileIo {
        path: path.to_path_buf(),
        message: source.to_string(),
    })
}

pub(super) fn read_utf8_snapshot(path: &Path, limit: usize) -> PpResult<(String, InputSnapshot)> {
    let (bytes, snapshot) = InputSnapshot::capture(path, limit)?;
    let text = String::from_utf8(bytes).map_err(|source| PpError::FileIo {
        path: path.to_path_buf(),
        message: source.to_string(),
    })?;
    Ok((text, snapshot))
}

pub(super) fn read_json_request_snapshot<T: DeserializeOwned>(
    path: &Path,
) -> PpResult<(T, InputSnapshot)> {
    let (text, snapshot) = read_utf8_snapshot(path, MAX_CONTROL_READ_BYTES)?;
    let request = serde_json::from_str(&text).map_err(|source| PpError::InvalidRequestSource {
        message: format!("invalid JSON request at '{}': {source}", path.display()),
        path: path.to_path_buf(),
        original_error: source.to_string(),
    })?;
    Ok((request, snapshot))
}

pub(super) fn decode_raster_snapshot(path: &Path) -> PpResult<(Raster, InputSnapshot)> {
    let (bytes, snapshot) = InputSnapshot::capture(path, MAX_RASTER_READ_BYTES)?;
    let raster = ImageCodec::decode_rgba_bytes(path, &bytes, Default::default())?;
    Ok((raster, snapshot))
}

pub(super) fn account_decoded_raster_bytes(total: &mut usize, raster: &Raster) -> PpResult<()> {
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

pub(super) struct BundleArtifact {
    pub relative_path: String,
    pub bytes: Vec<u8>,
}

pub(super) fn text_artifact(relative_path: &str, text: String) -> BundleArtifact {
    BundleArtifact {
        relative_path: relative_path.to_string(),
        bytes: text.into_bytes(),
    }
}

pub(super) fn publish_generated_artifacts(
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

pub(super) fn reject_generated_output_collisions(
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
