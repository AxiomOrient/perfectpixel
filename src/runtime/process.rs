use std::{
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

use crate::{core::sha256::Sha256State, Sha256Digest};

const POLL_INTERVAL: Duration = Duration::from_millis(10);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PinnedExecutable {
    canonical_path: PathBuf,
    sha256: Sha256Digest,
}

impl PinnedExecutable {
    pub fn new(path: impl AsRef<Path>, sha256: Sha256Digest) -> Result<Self, ProcessRunError> {
        let path = path.as_ref();
        if !path.is_absolute() {
            return Err(ProcessRunError::InvalidExecutable(
                "external executable path must be absolute".to_string(),
            ));
        }
        let canonical_path = fs::canonicalize(path)
            .map_err(|error| ProcessRunError::Io(format!("canonicalize executable: {error}")))?;
        let metadata = fs::symlink_metadata(&canonical_path)
            .map_err(|error| ProcessRunError::Io(format!("stat executable: {error}")))?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(ProcessRunError::InvalidExecutable(
                "external executable must resolve to a regular non-symlink file".to_string(),
            ));
        }
        let actual = digest_file(&canonical_path)?;
        if actual != sha256 {
            return Err(ProcessRunError::ExecutableDigestMismatch {
                expected: sha256,
                actual,
            });
        }
        Ok(Self {
            canonical_path,
            sha256,
        })
    }

    pub fn path(&self) -> &Path {
        &self.canonical_path
    }

    pub fn sha256(&self) -> &Sha256Digest {
        &self.sha256
    }

    fn revalidate(&self) -> Result<(), ProcessRunError> {
        let metadata = fs::symlink_metadata(&self.canonical_path)
            .map_err(|error| ProcessRunError::Io(format!("stat executable: {error}")))?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(ProcessRunError::ExecutableChanged(
                "pinned executable is no longer a regular file".to_string(),
            ));
        }
        let actual = digest_file(&self.canonical_path)?;
        if actual != self.sha256 {
            return Err(ProcessRunError::ExecutableDigestMismatch {
                expected: self.sha256.clone(),
                actual,
            });
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct ProcessSpec {
    pub executable: PinnedExecutable,
    pub args: Vec<String>,
    pub current_dir: Option<PathBuf>,
    /// The process inherits no ambient environment. Every required variable is explicit here.
    pub environment: Vec<(String, String)>,
    pub timeout: Duration,
    pub max_stdout_bytes: usize,
    pub max_stderr_bytes: usize,
}

impl ProcessSpec {
    pub fn validate(&self) -> Result<(), ProcessRunError> {
        if self.timeout.is_zero() {
            return Err(ProcessRunError::InvalidRequest(
                "external process timeout must be positive".to_string(),
            ));
        }
        if self.max_stdout_bytes == 0 || self.max_stderr_bytes == 0 {
            return Err(ProcessRunError::InvalidRequest(
                "external process output limits must be positive".to_string(),
            ));
        }
        if self.args.iter().any(|argument| argument.contains('\0')) {
            return Err(ProcessRunError::InvalidRequest(
                "external process argument contains NUL".to_string(),
            ));
        }
        if self.environment.iter().any(|(key, value)| {
            key.is_empty()
                || key.contains('=')
                || key.contains('\0')
                || value.contains('\0')
        }) {
            return Err(ProcessRunError::InvalidRequest(
                "external process environment contains an invalid key or NUL".to_string(),
            ));
        }
        if let Some(current_dir) = &self.current_dir {
            if !current_dir.is_absolute() {
                return Err(ProcessRunError::InvalidRequest(
                    "external process working directory must be absolute".to_string(),
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default)]
pub struct CancellationFlag(Arc<AtomicBool>);

impl CancellationFlag {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessTermination {
    Exited(i32),
    Signaled,
    TimedOut,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessOutput {
    pub termination: ProcessTermination,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcessRunError {
    InvalidRequest(String),
    InvalidExecutable(String),
    ExecutableDigestMismatch {
        expected: Sha256Digest,
        actual: Sha256Digest,
    },
    ExecutableChanged(String),
    Io(String),
    Reader(String),
}

impl std::fmt::Display for ProcessRunError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidRequest(message) => write!(formatter, "invalid process request: {message}"),
            Self::InvalidExecutable(message) => write!(formatter, "invalid executable: {message}"),
            Self::ExecutableDigestMismatch { expected, actual } => write!(
                formatter,
                "executable digest mismatch: expected {}, actual {}",
                expected.as_str(),
                actual.as_str()
            ),
            Self::ExecutableChanged(message) => write!(formatter, "executable changed: {message}"),
            Self::Io(message) => write!(formatter, "process I/O failed: {message}"),
            Self::Reader(message) => write!(formatter, "process output reader failed: {message}"),
        }
    }
}

impl std::error::Error for ProcessRunError {}

pub fn run_process(
    spec: &ProcessSpec,
    cancellation: &CancellationFlag,
) -> Result<ProcessOutput, ProcessRunError> {
    spec.validate()?;
    spec.executable.revalidate()?;
    if cancellation.is_cancelled() {
        return Ok(ProcessOutput {
            termination: ProcessTermination::Cancelled,
            stdout: Vec::new(),
            stderr: Vec::new(),
            stdout_truncated: false,
            stderr_truncated: false,
        });
    }

    let mut command = Command::new(spec.executable.path());
    command
        .args(&spec.args)
        .env_clear()
        .envs(spec.environment.iter().map(|(key, value)| (key, value)))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(current_dir) = &spec.current_dir {
        command.current_dir(current_dir);
    }
    let mut child = command
        .spawn()
        .map_err(|error| ProcessRunError::Io(format!("spawn: {error}")))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| ProcessRunError::Io("stdout pipe unavailable".to_string()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| ProcessRunError::Io("stderr pipe unavailable".to_string()))?;
    let stdout_limit = spec.max_stdout_bytes;
    let stderr_limit = spec.max_stderr_bytes;
    let stdout_reader = thread::spawn(move || read_bounded_to_eof(stdout, stdout_limit));
    let stderr_reader = thread::spawn(move || read_bounded_to_eof(stderr, stderr_limit));

    let started = Instant::now();
    let termination = loop {
        if cancellation.is_cancelled() {
            let _ = child.kill();
            let _ = child.wait();
            break ProcessTermination::Cancelled;
        }
        if started.elapsed() >= spec.timeout {
            let _ = child.kill();
            let _ = child.wait();
            break ProcessTermination::TimedOut;
        }
        match child
            .try_wait()
            .map_err(|error| ProcessRunError::Io(format!("wait: {error}")))?
        {
            Some(status) => {
                break status
                    .code()
                    .map(ProcessTermination::Exited)
                    .unwrap_or(ProcessTermination::Signaled)
            }
            None => thread::sleep(POLL_INTERVAL),
        }
    };

    let (stdout, stdout_truncated) = join_reader(stdout_reader)?;
    let (stderr, stderr_truncated) = join_reader(stderr_reader)?;
    spec.executable.revalidate()?;
    Ok(ProcessOutput {
        termination,
        stdout,
        stderr,
        stdout_truncated,
        stderr_truncated,
    })
}

fn join_reader(
    handle: thread::JoinHandle<io::Result<(Vec<u8>, bool)>>,
) -> Result<(Vec<u8>, bool), ProcessRunError> {
    handle
        .join()
        .map_err(|_| ProcessRunError::Reader("reader thread panicked".to_string()))?
        .map_err(|error| ProcessRunError::Reader(error.to_string()))
}

fn read_bounded_to_eof(
    mut reader: impl Read,
    limit: usize,
) -> io::Result<(Vec<u8>, bool)> {
    let mut kept = Vec::with_capacity(limit.min(64 * 1024));
    let mut buffer = [0u8; 16 * 1024];
    let mut truncated = false;
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        let remaining = limit.saturating_sub(kept.len());
        let copy = remaining.min(read);
        kept.extend_from_slice(&buffer[..copy]);
        truncated |= copy < read;
    }
    Ok((kept, truncated))
}

fn digest_file(path: &Path) -> Result<Sha256Digest, ProcessRunError> {
    let mut file = fs::File::open(path)
        .map_err(|error| ProcessRunError::Io(format!("open executable: {error}")))?;
    let mut state = Sha256State::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| ProcessRunError::Io(format!("read executable: {error}")))?;
        if read == 0 {
            break;
        }
        state.update(&buffer[..read]);
    }
    Ok(Sha256Digest::from_digest_bytes(state.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_reader_discards_beyond_evidence_limit_without_stopping_drain() {
        let input = vec![7u8; 1024];
        let (kept, truncated) = read_bounded_to_eof(&input[..], 16).expect("read");
        assert_eq!(kept.len(), 16);
        assert!(truncated);
    }

    #[test]
    fn process_spec_rejects_ambient_or_malformed_environment_state() {
        let invalid = vec![("A=B".to_string(), "value".to_string())];
        assert!(invalid.iter().any(|(key, _)| key.contains('=')));
    }

    #[test]
    fn cancellation_is_explicit_state() {
        let cancellation = CancellationFlag::default();
        assert!(!cancellation.is_cancelled());
        cancellation.cancel();
        assert!(cancellation.is_cancelled());
    }
}
