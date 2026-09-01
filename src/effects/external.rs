use std::{
    path::{Component, Path, PathBuf},
    time::Duration,
};

use serde::{Deserialize, Serialize};

use crate::{
    runtime::{
        run_process, CancellationFlag, PinnedExecutable, ProcessRunError, ProcessSpec,
        ProcessTermination,
    },
    ArtifactRef, EffectCompletion, EffectFailure, EffectFailureCode, EffectIdentity, EffectResult,
    PpError, PpResult, Sha256Digest,
};

const MAX_CANDIDATE_BYTES: usize = 128 * 1024 * 1024;
const PROCESS_EVIDENCE_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExternalToolReceipt {
    pub tool: String,
    pub executable_sha256: Sha256Digest,
    pub arguments_sha256: Sha256Digest,
    pub stdout_sha256: Sha256Digest,
    pub stderr_sha256: Sha256Digest,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalArtifactCandidate {
    pub artifact: ArtifactRef,
    pub bytes: Vec<u8>,
    pub receipt: ExternalToolReceipt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KtxEncoding {
    BasisLz,
    Uastc,
}

impl KtxEncoding {
    fn cli_value(self) -> &'static str {
        match self {
            Self::BasisLz => "basis-lz",
            // `uastc` remains accepted by KTX-Software but is a deprecated alias.
            // Receipts digest arguments, so use the canonical stable spelling.
            Self::Uastc => "uastc-ldr-4x4",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Ktx2EffectRequest {
    pub identity: EffectIdentity,
    pub executable: PinnedExecutable,
    pub input: PathBuf,
    pub staging_output: PathBuf,
    pub encoding: KtxEncoding,
    pub generate_mipmaps: bool,
    pub srgb: bool,
    pub timeout: Duration,
}

pub fn run_ktx2_effect(
    request: &Ktx2EffectRequest,
    cancellation: &CancellationFlag,
) -> EffectResult<ExternalArtifactCandidate> {
    execute_candidate_effect(
        &request.identity,
        "texture.encode_ktx2",
        "ktx",
        &request.executable,
        &request.input,
        &request.staging_output,
        ktx_arguments(
            &request.input,
            &request.staging_output,
            request.encoding,
            request.generate_mipmaps,
            request.srgb,
        ),
        request.timeout,
        "image/ktx2",
        cancellation,
    )
}

fn ktx_arguments(
    input: &Path,
    output: &Path,
    encoding: KtxEncoding,
    generate_mipmaps: bool,
    srgb: bool,
) -> Vec<String> {
    let mut args = vec![
        "create".to_string(),
        "--format".to_string(),
        if srgb {
            "R8G8B8A8_SRGB".to_string()
        } else {
            "R8G8B8A8_UNORM".to_string()
        },
        "--encode".to_string(),
        encoding.cli_value().to_string(),
        "--threads".to_string(),
        "1".to_string(),
        "--fail-on-color-conversions".to_string(),
    ];
    if generate_mipmaps {
        args.extend([
            "--generate-mipmap".to_string(),
            "--mipmap-filter".to_string(),
            "lanczos4".to_string(),
            "--mipmap-wrap".to_string(),
            "clamp".to_string(),
        ]);
    }
    args.push(input.to_string_lossy().into_owned());
    args.push(output.to_string_lossy().into_owned());
    args
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VTracerPreset {
    Bw,
    Poster,
    Photo,
}

impl VTracerPreset {
    fn cli_value(self) -> &'static str {
        match self {
            Self::Bw => "bw",
            Self::Poster => "poster",
            Self::Photo => "photo",
        }
    }
}

#[derive(Debug, Clone)]
pub struct VTracerEffectRequest {
    pub identity: EffectIdentity,
    pub executable: PinnedExecutable,
    pub input: PathBuf,
    pub staging_output: PathBuf,
    pub preset: Option<VTracerPreset>,
    pub simplify: Option<f64>,
    pub path_precision: Option<u32>,
    pub max_colors: Option<usize>,
    pub optimize: Option<u8>,
    pub timeout: Duration,
}

impl VTracerEffectRequest {
    pub fn validate(&self) -> PpResult<()> {
        if self
            .simplify
            .is_some_and(|value| !value.is_finite() || value <= 0.0)
        {
            return Err(PpError::InvalidRequest(
                "VTracer simplify tolerance must be positive and finite".to_string(),
            ));
        }
        if self.max_colors == Some(0) {
            return Err(PpError::InvalidRequest(
                "VTracer maxColors must be positive".to_string(),
            ));
        }
        if self.optimize.is_some_and(|value| value > 2) {
            return Err(PpError::InvalidRequest(
                "VTracer optimize must be 0, 1, or 2".to_string(),
            ));
        }
        Ok(())
    }
}

pub fn run_vtracer_effect(
    request: &VTracerEffectRequest,
    cancellation: &CancellationFlag,
) -> EffectResult<ExternalArtifactCandidate> {
    if let Err(error) = request.validate() {
        return failure(
            request.identity.clone(),
            "vector.vtracer_candidate",
            "vtracer",
            EffectFailureCode::InvalidArgument,
            error.to_string(),
            false,
        );
    }
    execute_candidate_effect(
        &request.identity,
        "vector.vtracer_candidate",
        "vtracer",
        &request.executable,
        &request.input,
        &request.staging_output,
        vtracer_arguments(request),
        request.timeout,
        "image/svg+xml",
        cancellation,
    )
}

fn vtracer_arguments(request: &VTracerEffectRequest) -> Vec<String> {
    let mut args = vec![
        request.input.to_string_lossy().into_owned(),
        request.staging_output.to_string_lossy().into_owned(),
    ];
    if let Some(preset) = request.preset {
        args.extend(["--preset".to_string(), preset.cli_value().to_string()]);
    }
    if let Some(value) = request.simplify {
        args.extend(["--simplify".to_string(), value.to_string()]);
    }
    if let Some(value) = request.path_precision {
        args.extend(["--path-precision".to_string(), value.to_string()]);
    }
    if let Some(value) = request.max_colors {
        args.extend(["--max-colors".to_string(), value.to_string()]);
    }
    if let Some(value) = request.optimize {
        args.extend(["--optimize".to_string(), value.to_string()]);
    }
    args
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppleVisionRevision {
    Revision1,
}

impl AppleVisionRevision {
    fn cli_value(self) -> &'static str {
        match self {
            Self::Revision1 => "1",
        }
    }
}

#[derive(Debug, Clone)]
pub struct AppleVisionForegroundMaskRequest {
    pub identity: EffectIdentity,
    pub executable: PinnedExecutable,
    pub input: PathBuf,
    pub staging_output: PathBuf,
    pub revision: AppleVisionRevision,
    pub timeout: Duration,
}

pub fn run_apple_vision_foreground_mask_effect(
    request: &AppleVisionForegroundMaskRequest,
    cancellation: &CancellationFlag,
) -> EffectResult<ExternalArtifactCandidate> {
    let args = vec![
        "foreground-mask".to_string(),
        "--input".to_string(),
        request.input.to_string_lossy().into_owned(),
        "--output".to_string(),
        request.staging_output.to_string_lossy().into_owned(),
        "--revision".to_string(),
        request.revision.cli_value().to_string(),
    ];
    execute_candidate_effect(
        &request.identity,
        "mask.apple_vision_foreground",
        "perfectpixel-vision-helper",
        &request.executable,
        &request.input,
        &request.staging_output,
        args,
        request.timeout,
        "image/png",
        cancellation,
    )
}

#[allow(clippy::too_many_arguments)]
fn execute_candidate_effect(
    identity: &EffectIdentity,
    operation: &str,
    dependency: &str,
    executable: &PinnedExecutable,
    input: &Path,
    staging_output: &Path,
    args: Vec<String>,
    timeout: Duration,
    media_type: &str,
    cancellation: &CancellationFlag,
) -> EffectResult<ExternalArtifactCandidate> {
    if let Err(error) = validate_effect_paths(input, staging_output) {
        return failure(
            identity.clone(),
            operation,
            dependency,
            EffectFailureCode::InvalidArgument,
            error.to_string(),
            false,
        );
    }
    let spec = ProcessSpec {
        executable: executable.clone(),
        args: args.clone(),
        environment: vec![
            ("LANG".to_string(), "C".to_string()),
            ("LC_ALL".to_string(), "C".to_string()),
            ("TZ".to_string(), "UTC".to_string()),
        ],
        timeout,
        max_stdout_bytes: PROCESS_EVIDENCE_BYTES,
        max_stderr_bytes: PROCESS_EVIDENCE_BYTES,
    };
    let output = match run_process(&spec, cancellation) {
        Ok(output) => output,
        Err(error) => return process_error(identity.clone(), operation, dependency, error),
    };
    match output.termination {
        ProcessTermination::Cancelled => {
            return EffectResult {
                identity: identity.clone(),
                completion: EffectCompletion::Cancelled,
            };
        }
        ProcessTermination::TimedOut => {
            return failure(
                identity.clone(),
                operation,
                dependency,
                EffectFailureCode::Timeout,
                format!("{dependency} exceeded timeout"),
                true,
            );
        }
        ProcessTermination::Signaled => {
            return failure(
                identity.clone(),
                operation,
                dependency,
                EffectFailureCode::DependencyFailed,
                format!("{dependency} terminated by signal"),
                true,
            );
        }
        ProcessTermination::Exited(code) if code != 0 => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return failure(
                identity.clone(),
                operation,
                dependency,
                EffectFailureCode::DependencyFailed,
                format!("{dependency} exited with {code}: {}", stderr.trim()),
                false,
            );
        }
        ProcessTermination::Exited(_) => {}
    }
    let bytes = match read_candidate(staging_output) {
        Ok(bytes) => bytes,
        Err(error) => {
            return failure(
                identity.clone(),
                operation,
                dependency,
                EffectFailureCode::DependencyFailed,
                error.to_string(),
                false,
            );
        }
    };
    let artifact = match ArtifactRef::from_bytes(media_type, &bytes) {
        Ok(artifact) => artifact,
        Err(error) => {
            return failure(
                identity.clone(),
                operation,
                dependency,
                EffectFailureCode::Internal,
                error.to_string(),
                false,
            );
        }
    };
    EffectResult {
        identity: identity.clone(),
        completion: EffectCompletion::Succeeded {
            value: ExternalArtifactCandidate {
                artifact,
                bytes,
                receipt: ExternalToolReceipt {
                    tool: dependency.to_string(),
                    executable_sha256: executable.sha256().clone(),
                    arguments_sha256: Sha256Digest::from_bytes(
                        canonical_arguments(&args).as_bytes(),
                    ),
                    stdout_sha256: Sha256Digest::from_bytes(&output.stdout),
                    stderr_sha256: Sha256Digest::from_bytes(&output.stderr),
                    stdout_truncated: output.stdout_truncated,
                    stderr_truncated: output.stderr_truncated,
                },
            },
        },
    }
}

fn validate_effect_paths(input: &Path, staging_output: &Path) -> PpResult<()> {
    if !input.is_absolute() || !staging_output.is_absolute() {
        return Err(PpError::InvalidRequest(
            "external Effect input and staging output paths must be absolute".to_string(),
        ));
    }
    if input == staging_output {
        return Err(PpError::InvalidRequest(
            "external Effect staging output must not overwrite input".to_string(),
        ));
    }
    if staging_output
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(PpError::InvalidRequest(
            "external Effect staging output must not contain '..'".to_string(),
        ));
    }

    let input_file = crate::io::capability::open_read(input).map_err(|error| PpError::FileIo {
        path: input.to_path_buf(),
        message: error.to_string(),
    })?;
    if !input_file
        .metadata()
        .map_err(|error| PpError::FileIo {
            path: input.to_path_buf(),
            message: error.to_string(),
        })?
        .is_file()
    {
        return Err(PpError::InvalidRequest(
            "external Effect input must be a regular no-follow file".to_string(),
        ));
    }

    let parent = staging_output.parent().ok_or_else(|| {
        PpError::InvalidRequest("external Effect staging output has no parent".to_string())
    })?;
    crate::io::capability::open_directory(parent).map_err(|error| PpError::FileIo {
        path: parent.to_path_buf(),
        message: error.to_string(),
    })?;

    // Fail closed for any existing entry. The external tool itself is a pathname
    // Effect, so PerfectPixel never treats its output as trusted; the candidate is
    // reopened through the capability boundary and verified before publication.
    match crate::io::capability::open_read(staging_output) {
        Ok(_) => {
            return Err(PpError::InvalidRequest(
                "external Effect staging output must not already exist".to_string(),
            ))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            if crate::io::capability::open_directory(staging_output).is_ok() {
                return Err(PpError::InvalidRequest(
                    "external Effect staging output must not already exist".to_string(),
                ));
            }
            return Err(PpError::InvalidRequest(format!(
                "external Effect staging output is not an absent regular path: {error}"
            )));
        }
    }
    Ok(())
}

fn read_candidate(path: &Path) -> PpResult<Vec<u8>> {
    let bytes = crate::io::capability::read_bounded(path, MAX_CANDIDATE_BYTES).map_err(|error| {
        PpError::FileIo {
            path: path.to_path_buf(),
            message: error.to_string(),
        }
    })?;
    if bytes.is_empty() {
        return Err(PpError::InvalidRequest(
            "external Effect candidate must not be empty".to_string(),
        ));
    }
    Ok(bytes)
}

fn canonical_arguments(args: &[String]) -> String {
    let mut canonical = String::new();
    for argument in args {
        canonical.push_str(&argument.len().to_string());
        canonical.push(':');
        canonical.push_str(argument);
        canonical.push('\n');
    }
    canonical
}

fn process_error<T>(
    identity: EffectIdentity,
    operation: &str,
    dependency: &str,
    error: ProcessRunError,
) -> EffectResult<T> {
    let code = match error {
        ProcessRunError::InvalidRequest(_) | ProcessRunError::InvalidExecutable(_) => {
            EffectFailureCode::InvalidArgument
        }
        ProcessRunError::ExecutableDigestMismatch { .. }
        | ProcessRunError::ExecutableChanged(_) => EffectFailureCode::PreconditionFailed,
        ProcessRunError::Io(_) | ProcessRunError::Reader(_) => EffectFailureCode::DependencyFailed,
    };
    failure(
        identity,
        operation,
        dependency,
        code,
        error.to_string(),
        false,
    )
}

fn failure<T>(
    identity: EffectIdentity,
    operation: &str,
    dependency: &str,
    code: EffectFailureCode,
    cause: String,
    retryable: bool,
) -> EffectResult<T> {
    EffectResult {
        identity,
        completion: EffectCompletion::Failed {
            error: EffectFailure {
                code,
                operation: operation.to_string(),
                dependency: dependency.to_string(),
                retryable,
                cause,
                context: Vec::new(),
            },
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ktx_arguments_force_determinism_and_fail_closed_color() {
        let args = ktx_arguments(
            Path::new("/tmp/in.png"),
            Path::new("/tmp/out.ktx2"),
            KtxEncoding::Uastc,
            true,
            true,
        );
        assert!(args.windows(2).any(|pair| pair == ["--threads", "1"]));
        assert!(args
            .iter()
            .any(|value| value == "--fail-on-color-conversions"));
        assert!(args.iter().any(|value| value == "--generate-mipmap"));
        assert!(args.iter().any(|value| value == "uastc-ldr-4x4"));
    }

    #[test]
    fn canonical_argument_digest_is_boundary_unambiguous() {
        assert_ne!(
            canonical_arguments(&["ab".to_string(), "c".to_string()]),
            canonical_arguments(&["a".to_string(), "bc".to_string()])
        );
    }
}
