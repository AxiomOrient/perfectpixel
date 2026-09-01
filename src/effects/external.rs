use std::{
    fs,
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

const MAX_CANDIDATE_BYTES: u64 = 128 * 1024 * 1024;
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
            Self::Uastc => "uastc",
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
        "ktx",
        &request.executable,
        &request.input,
        &request.staging_output,
        ktx_arguments(request),
        request.timeout,
        "image/ktx2",
        cancellation,
    )
}

fn ktx_arguments(request: &Ktx2EffectRequest) -> Vec<String> {
    let mut args = vec![
        "create".to_string(),
        "--format".to_string(),
        if request.srgb {
            "R8G8B8A8_SRGB".to_string()
        } else {
            "R8G8B8A8_UNORM".to_string()
        },
        "--encode".to_string(),
        request.encoding.cli_value().to_string(),
        "--threads".to_string(),
        "1".to_string(),
        "--fail-on-color-conversions".to_string(),
    ];
    if request.generate_mipmaps {
        args.push("--generate-mipmap".to_string());
        args.push("--mipmap-filter".to_string());
        args.push("lanczos4".to_string());
        args.push("--mipmap-wrap".to_string());
        args.push("clamp".to_string());
    }
    args.push(request.input.to_string_lossy().into_owned());
    args.push(request.staging_output.to_string_lossy().into_owned());
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
            EffectFailureCode::InvalidRequest,
            error.to_string(),
            false,
        );
    }
    execute_candidate_effect(
        &request.identity,
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
        args.push("--preset".to_string());
        args.push(preset.cli_value().to_string());
    }
    if let Some(value) = request.simplify {
        args.push("--simplify".to_string());
        args.push(value.to_string());
    }
    if let Some(value) = request.path_precision {
        args.push("--path-precision".to_string());
        args.push(value.to_string());
    }
    if let Some(value) = request.max_colors {
        args.push("--max-colors".to_string());
        args.push(value.to_string());
    }
    if let Some(value) = request.optimize {
        args.push("--optimize".to_string());
        args.push(value.to_string());
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
        "apple-vision-foreground-mask",
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
    tool: &str,
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
            EffectFailureCode::InvalidRequest,
            error.to_string(),
            false,
        );
    }
    let spec = ProcessSpec {
        executable: executable.clone(),
        args: args.clone(),
        current_dir: None,
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
        Err(error) => {
            return process_error(identity.clone(), error);
        }
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
                EffectFailureCode::Timeout,
                format!("{tool} exceeded timeout"),
                true,
            );
        }
        ProcessTermination::Signaled => {
            return failure(
                identity.clone(),
                EffectFailureCode::DependencyFailed,
                format!("{tool} terminated by signal"),
                true,
            );
        }
        ProcessTermination::Exited(code) if code != 0 => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return failure(
                identity.clone(),
                EffectFailureCode::DependencyFailed,
                format!("{tool} exited with {code}: {}", stderr.trim()),
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
                EffectFailureCode::Internal,
                error.to_string(),
                false,
            );
        }
    };
    let arguments_sha256 = Sha256Digest::from_bytes(canonical_arguments(&args).as_bytes());
    EffectResult {
        identity: identity.clone(),
        completion: EffectCompletion::Succeeded(ExternalArtifactCandidate {
            artifact,
            bytes,
            receipt: ExternalToolReceipt {
                tool: tool.to_string(),
                executable_sha256: executable.sha256().clone(),
                arguments_sha256,
                stdout_sha256: Sha256Digest::from_bytes(&output.stdout),
                stderr_sha256: Sha256Digest::from_bytes(&output.stderr),
                stdout_truncated: output.stdout_truncated,
                stderr_truncated: output.stderr_truncated,
            },
        }),
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
    let input_metadata = fs::symlink_metadata(input).map_err(|error| PpError::FileIo {
        path: input.to_path_buf(),
        message: error.to_string(),
    })?;
    if input_metadata.file_type().is_symlink() || !input_metadata.is_file() {
        return Err(PpError::InvalidRequest(
            "external Effect input must be a regular non-symlink file".to_string(),
        ));
    }
    match fs::symlink_metadata(staging_output) {
        Ok(_) => {
            return Err(PpError::InvalidRequest(
                "external Effect staging output must not already exist".to_string(),
            ));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(PpError::FileIo {
                path: staging_output.to_path_buf(),
                message: error.to_string(),
            });
        }
    }
    let parent = staging_output.parent().ok_or_else(|| {
        PpError::InvalidRequest("external Effect staging output has no parent".to_string())
    })?;
    let parent_metadata = fs::symlink_metadata(parent).map_err(|error| PpError::FileIo {
        path: parent.to_path_buf(),
        message: error.to_string(),
    })?;
    if parent_metadata.file_type().is_symlink() || !parent_metadata.is_dir() {
        return Err(PpError::InvalidRequest(
            "external Effect staging parent must be a regular non-symlink directory".to_string(),
        ));
    }
    if staging_output.components().any(|component| matches!(component, Component::ParentDir)) {
        return Err(PpError::InvalidRequest(
            "external Effect staging output must not contain '..'".to_string(),
        ));
    }
    Ok(())
}

fn read_candidate(path: &Path) -> PpResult<Vec<u8>> {
    let metadata = fs::symlink_metadata(path).map_err(|error| PpError::FileIo {
        path: path.to_path_buf(),
        message: error.to_string(),
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(PpError::InvalidRequest(
            "external Effect candidate must be a regular non-symlink file".to_string(),
        ));
    }
    if metadata.len() == 0 || metadata.len() > MAX_CANDIDATE_BYTES {
        return Err(PpError::InvalidRequest(format!(
            "external Effect candidate size {} is outside 1..={MAX_CANDIDATE_BYTES}",
            metadata.len()
        )));
    }
    let bytes = fs::read(path).map_err(|error| PpError::FileIo {
        path: path.to_path_buf(),
        message: error.to_string(),
    })?;
    if bytes.len() as u64 != metadata.len() {
        return Err(PpError::InvalidRequest(
            "external Effect candidate changed while being read".to_string(),
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

fn process_error<T>(identity: EffectIdentity, error: ProcessRunError) -> EffectResult<T> {
    let code = match error {
        ProcessRunError::InvalidRequest(_) | ProcessRunError::InvalidExecutable(_) => {
            EffectFailureCode::InvalidRequest
        }
        ProcessRunError::ExecutableDigestMismatch { .. }
        | ProcessRunError::ExecutableChanged(_) => EffectFailureCode::PreconditionFailed,
        ProcessRunError::Io(_) | ProcessRunError::Reader(_) => EffectFailureCode::DependencyFailed,
    };
    failure(identity, code, error.to_string(), false)
}

fn failure<T>(
    identity: EffectIdentity,
    code: EffectFailureCode,
    cause: String,
    retryable: bool,
) -> EffectResult<T> {
    EffectResult {
        identity,
        completion: EffectCompletion::Failed(EffectFailure {
            code,
            cause,
            context: Vec::new(),
            retryable,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ktx_effect_forces_single_thread_and_fail_closed_color_conversion() -> PpResult<()> {
        let executable_digest = Sha256Digest::from_bytes(b"tool");
        let identity = EffectIdentity::new(1, Sha256Digest::from_bytes(b"op"), Sha256Digest::from_bytes(b"input"))?;
        let request = Ktx2EffectRequest {
            identity,
            executable: PinnedExecutable::new("/definitely/not/a/tool", executable_digest)
                .expect_err("fixture path is intentionally absent")
                .into_pinned_for_test(),
            input: "/tmp/in.png".into(),
            staging_output: "/tmp/out.ktx2".into(),
            encoding: KtxEncoding::Uastc,
            generate_mipmaps: true,
            srgb: true,
            timeout: Duration::from_secs(1),
        };
        let args = ktx_arguments(&request);
        assert!(args.windows(2).any(|pair| pair == ["--threads", "1"]));
        assert!(args.iter().any(|value| value == "--fail-on-color-conversions"));
        Ok(())
    }

    trait TestPinnedExecutable {
        fn into_pinned_for_test(self) -> PinnedExecutable;
    }

    impl TestPinnedExecutable for ProcessRunError {
        fn into_pinned_for_test(self) -> PinnedExecutable {
            panic!("test fixture should never need a real executable: {self}")
        }
    }
}
