use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use perfectpixel::{
    reject_blocked_managed_parents, ArtifactSetConditionPhase, AtomicArtifactSetOwnedEntry,
    AtomicArtifactSetOwnedPlan, Manifest, MotionCompiler, MotionLayersDocument, MotionReport,
    MotionRequest, NormalizeReport, SpriteBundleRequest, MOTION_SCHEMA,
};
use serde::de::DeserializeOwned;
use serde_json::Value;

use super::generation::{
    reduce_generation_authority, GenerationArtifact, GenerationAuthority, GenerationAuthorityEvent,
    GenerationAuthorityState, GenerationRecord, GenerationTransition, GenerationWorkflow,
    GENERATION_AUTHORITY_FILE, MAX_GENERATION_ARTIFACTS, MAX_GENERATION_BYTES,
};
use super::{
    aseprite_json_name, destination_error, managed_relative_path, reject_same_path, PpError,
    PpResult, MAX_CONTROL_READ_BYTES,
};
use perfectpixel::sha256_hex;

const MAX_AUTHORITY_BYTES: u64 = 1024 * 1024;
const MAX_LEGACY_MOTION_ANIMATIONS: usize = 256;
const MAX_INPUT_SNAPSHOTS: usize = MAX_GENERATION_ARTIFACTS + 1;
const MAX_INPUT_SNAPSHOT_BYTES: u64 = 512 * 1024 * 1024;
use perfectpixel::NORMALIZE_SCHEMA as NORMALIZE_REPORT_SCHEMA;
const MOTION_LAYERS_SCHEMA: &str = "perfectpixel.motion-layers/1";
use perfectpixel::MOTION_REPORT_SCHEMA;

/// Filesystem revision captured from the same open file handle as the input
/// bytes. Unix identity and change timestamps detect path replacement and
/// in-place mutation; other platforms retain the strongest portable metadata
/// available from `std` and always verify canonical path, length, and SHA-256.
#[cfg(unix)]
#[derive(Clone, Debug, Eq, PartialEq)]
struct SnapshotFileRevision {
    device: u64,
    inode: u64,
    byte_count: u64,
    modified_seconds: i64,
    modified_nanoseconds: i64,
    changed_seconds: i64,
    changed_nanoseconds: i64,
}

#[cfg(not(unix))]
#[derive(Clone, Debug, Eq, PartialEq)]
struct SnapshotFileRevision {
    byte_count: u64,
    modified: Option<std::time::SystemTime>,
}

#[cfg(unix)]
fn snapshot_file_revision(metadata: &fs::Metadata) -> SnapshotFileRevision {
    use std::os::unix::fs::MetadataExt;

    SnapshotFileRevision {
        device: metadata.dev(),
        inode: metadata.ino(),
        byte_count: metadata.len(),
        modified_seconds: metadata.mtime(),
        modified_nanoseconds: metadata.mtime_nsec(),
        changed_seconds: metadata.ctime(),
        changed_nanoseconds: metadata.ctime_nsec(),
    }
}

#[cfg(not(unix))]
fn snapshot_file_revision(metadata: &fs::Metadata) -> SnapshotFileRevision {
    SnapshotFileRevision {
        byte_count: metadata.len(),
        modified: metadata.modified().ok(),
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct InputRevision {
    sha256: String,
    byte_count: u64,
    file_revision: SnapshotFileRevision,
}

/// Immutable evidence for one input revision used to compute a generation.
/// Exact bytes and this evidence are captured from one open file handle before
/// domain work. Publication verifies the same canonical target, filesystem
/// revision, size, and digest before mutation and again before durable commit.
#[derive(Clone, Debug)]
pub(super) struct InputSnapshot {
    source_path: PathBuf,
    canonical_path: PathBuf,
    sha256: String,
    byte_count: u64,
    file_revision: SnapshotFileRevision,
}

impl InputSnapshot {
    pub(super) fn capture(path: &Path, limit: usize) -> PpResult<(Vec<u8>, Self)> {
        let canonical_path = fs::canonicalize(path)
            .map_err(|source| input_snapshot_error(path, source.to_string()))?;
        let mut file = fs::File::open(&canonical_path)
            .map_err(|source| input_snapshot_error(path, source.to_string()))?;
        let before = file
            .metadata()
            .map_err(|source| input_snapshot_error(path, source.to_string()))?;
        if !before.is_file() {
            return Err(input_snapshot_error(
                path,
                "generation input must resolve to a regular file",
            ));
        }
        let before_revision = snapshot_file_revision(&before);
        let current_before = fs::metadata(&canonical_path)
            .map_err(|source| input_snapshot_error(path, source.to_string()))?;
        if snapshot_file_revision(&current_before) != before_revision {
            return Err(input_snapshot_error(
                path,
                "generation input changed while its byte snapshot was opened",
            ));
        }

        let read_limit = u64::try_from(limit)
            .ok()
            .and_then(|value| value.checked_add(1))
            .ok_or_else(|| input_snapshot_error(path, "generation input read limit overflow"))?;
        let mut bytes = Vec::new();
        file.by_ref()
            .take(read_limit)
            .read_to_end(&mut bytes)
            .map_err(|source| input_snapshot_error(path, source.to_string()))?;
        if bytes.len() > limit {
            return Err(input_snapshot_error(
                path,
                format!("file exceeds {limit}-byte read limit"),
            ));
        }

        let byte_count = u64::try_from(bytes.len())
            .map_err(|_| input_snapshot_error(path, "generation input byte count overflow"))?;
        let after = file
            .metadata()
            .map_err(|source| input_snapshot_error(path, source.to_string()))?;
        let after_revision = snapshot_file_revision(&after);
        let canonical_after = fs::canonicalize(path)
            .map_err(|source| input_snapshot_error(path, source.to_string()))?;
        let current_after = fs::metadata(&canonical_after)
            .map_err(|source| input_snapshot_error(path, source.to_string()))?;
        if before_revision != after_revision
            || canonical_after != canonical_path
            || snapshot_file_revision(&current_after) != after_revision
            || after.len() != byte_count
        {
            return Err(input_snapshot_error(
                path,
                "generation input changed while its byte snapshot was captured",
            ));
        }

        let sha256 = sha256_hex(&bytes);
        Ok((
            bytes,
            Self {
                source_path: path.to_path_buf(),
                canonical_path,
                sha256,
                byte_count,
                file_revision: after_revision,
            },
        ))
    }

    pub(super) fn source_path(&self) -> &Path {
        &self.source_path
    }

    fn revision(&self) -> InputRevision {
        InputRevision {
            sha256: self.sha256.clone(),
            byte_count: self.byte_count,
            file_revision: self.file_revision.clone(),
        }
    }
}

/// Stable evidence for a product-managed file participating in a publication
/// precondition. Bytes and revision are captured from one open file handle.
#[derive(Clone, Debug, Eq, PartialEq)]
struct ManagedFileSnapshot {
    sha256: String,
    byte_count: u64,
    file_revision: SnapshotFileRevision,
}

#[derive(Clone, Debug)]
enum GenerationAuthorityPrecondition {
    Absent,
    Published(ManagedFileSnapshot),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MissingArtifactPolicy {
    Allowed,
    Required,
}

#[derive(Clone, Debug)]
struct ManagedFileExpectation {
    relative_path: PathBuf,
    sha256: String,
    byte_count: u64,
}

impl ManagedFileExpectation {
    fn from_generation_artifact(artifact: &GenerationArtifact) -> PpResult<Self> {
        Ok(Self {
            relative_path: managed_relative_path(&artifact.relative_path)?,
            sha256: artifact.sha256.clone(),
            byte_count: artifact.byte_count,
        })
    }

    fn authority(bytes: &[u8]) -> PpResult<Self> {
        let byte_count = u64::try_from(bytes.len()).map_err(|_| {
            PpError::InvalidRequest("generation authority byte count overflow".to_string())
        })?;
        Ok(Self {
            relative_path: PathBuf::from(GENERATION_AUTHORITY_FILE),
            sha256: sha256_hex(bytes),
            byte_count,
        })
    }
}

/// One generated file before it is converted to the generic artifact-set port.
pub(super) struct GeneratedArtifact {
    pub(super) relative_path: String,
    pub(super) bytes: Vec<u8>,
}

/// Complete application-level publication intent. The artifact-set writer calls
/// this adapter only after it owns the output-root publisher lock and has
/// recovered any interrupted transaction.
pub(super) struct GenerationPublicationRequest {
    pub(super) workflow: GenerationWorkflow,
    pub(super) artifacts: Vec<GeneratedArtifact>,
    pub(super) invalidates: Vec<GenerationWorkflow>,
    pub(super) input_snapshots: Vec<InputSnapshot>,
}

/// Application-owned evidence that must remain true while the generic
/// artifact-set transaction publishes one generation. The transaction owns
/// durability and rollback; this guard owns product-level input, authority,
/// ownership, and complete-generation preconditions.
#[derive(Clone, Debug)]
pub(super) struct GenerationPublicationGuard {
    input_snapshots: Vec<InputSnapshot>,
    authority_precondition: GenerationAuthorityPrecondition,
    retained_artifacts: Vec<GenerationArtifact>,
    superseded_artifacts: Vec<GenerationArtifact>,
    unowned_output_paths: Vec<PathBuf>,
    next_files: Vec<ManagedFileExpectation>,
    stale_paths: Vec<PathBuf>,
}

/// Resolves current generation authority into a write/remove plan without
/// mutating the filesystem. Filesystem mutation remains in `artifact_set`.
pub(super) fn plan_generation_publication(
    root: &Path,
    request: GenerationPublicationRequest,
) -> PpResult<(AtomicArtifactSetOwnedPlan, GenerationPublicationGuard)> {
    let GenerationPublicationRequest {
        workflow,
        artifacts,
        invalidates,
        input_snapshots,
    } = request;
    validate_generated_artifacts(&artifacts)?;
    validate_input_snapshot_set(&input_snapshots)?;
    reject_input_path_collisions(root, &artifacts, &input_snapshots)?;

    let current_artifacts = generation_artifacts_for(&artifacts)?;
    let (observed_state, authority_precondition) = read_generation_authority(root)?;
    let state = match observed_state {
        GenerationAuthorityState::Absent => seed_legacy_generation_authority(root)?,
        published @ GenerationAuthorityState::Published(_) => published,
    };
    let event = GenerationAuthorityEvent::replace(workflow, current_artifacts.clone(), invalidates)
        .map_err(|message| generation_authority_error(root, message))?;
    let mut transition = reduce_generation_authority(state, event)
        .map_err(|message| generation_authority_error(root, message))?;
    refresh_authored_input_artifacts(root, &mut transition, &input_snapshots)?;

    let previous_owned_paths = transition
        .retained_artifacts
        .iter()
        .chain(transition.superseded_artifacts.iter())
        .map(|artifact| artifact.relative_path.as_str())
        .collect::<BTreeSet<_>>();
    let unowned_output_paths = current_artifacts
        .iter()
        .filter(|artifact| !previous_owned_paths.contains(artifact.relative_path.as_str()))
        .map(|artifact| managed_relative_path(&artifact.relative_path))
        .collect::<PpResult<Vec<_>>>()?;

    let current_output_paths = current_artifacts
        .iter()
        .map(|artifact| managed_relative_path(&artifact.relative_path))
        .collect::<PpResult<Vec<_>>>()?;
    reject_blocked_managed_parents(root, &current_output_paths)?;
    verify_authority_precondition(root, &authority_precondition)?;
    verify_generation_artifacts(
        root,
        &transition.retained_artifacts,
        MissingArtifactPolicy::Required,
        None,
    )?;
    verify_generation_artifacts(
        root,
        &transition.superseded_artifacts,
        MissingArtifactPolicy::Allowed,
        Some(&input_snapshots),
    )?;
    verify_paths_absent(
        root,
        &unowned_output_paths,
        "generated output is not owned by generation authority",
    )?;

    let authority_bytes = transition
        .next_authority
        .to_pretty_json()
        .map_err(|message| generation_authority_error(root, message))?
        .into_bytes();
    if authority_bytes.len() as u64 > MAX_AUTHORITY_BYTES {
        return Err(generation_authority_error(
            root,
            format!("serialized authority exceeds {MAX_AUTHORITY_BYTES}-byte limit"),
        ));
    }

    let mut next_files = transition
        .next_authority
        .generations
        .iter()
        .flat_map(|record| record.artifacts.iter())
        .map(ManagedFileExpectation::from_generation_artifact)
        .collect::<PpResult<Vec<_>>>()?;
    next_files.push(ManagedFileExpectation::authority(&authority_bytes)?);
    next_files.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));

    let stale_paths = transition
        .stale_artifacts
        .iter()
        .map(|artifact| managed_relative_path(&artifact.relative_path))
        .collect::<PpResult<Vec<_>>>()?;
    let removals = stale_paths.clone();

    let mut entries = artifacts
        .into_iter()
        .map(|artifact| AtomicArtifactSetOwnedEntry {
            relative_path: PathBuf::from(artifact.relative_path),
            bytes: artifact.bytes,
        })
        .collect::<Vec<_>>();
    entries.push(AtomicArtifactSetOwnedEntry {
        relative_path: PathBuf::from(GENERATION_AUTHORITY_FILE),
        bytes: authority_bytes,
    });

    let guard = GenerationPublicationGuard {
        input_snapshots,
        authority_precondition,
        retained_artifacts: transition.retained_artifacts,
        superseded_artifacts: transition.superseded_artifacts,
        unowned_output_paths,
        next_files,
        stale_paths,
    };
    Ok((AtomicArtifactSetOwnedPlan { entries, removals }, guard))
}

/// Verifies product-level conditions at explicit transaction checkpoints.
/// Before mutation, the observed authority and all ownership preconditions must
/// still hold. Before commit, the complete next generation must match the new
/// authority and every stale path must remain absent.
pub(super) fn verify_generation_publication(
    root: &Path,
    phase: ArtifactSetConditionPhase,
    guard: &GenerationPublicationGuard,
) -> PpResult<()> {
    verify_input_snapshots(&guard.input_snapshots)?;
    match phase {
        ArtifactSetConditionPhase::BeforeMutation => {
            verify_authority_precondition(root, &guard.authority_precondition)?;
            verify_generation_artifacts(
                root,
                &guard.retained_artifacts,
                MissingArtifactPolicy::Required,
                None,
            )?;
            verify_generation_artifacts(
                root,
                &guard.superseded_artifacts,
                MissingArtifactPolicy::Allowed,
                Some(&guard.input_snapshots),
            )?;
            verify_paths_absent(
                root,
                &guard.unowned_output_paths,
                "generated output appeared without generation authority",
            )
        }
        ArtifactSetConditionPhase::BeforeCommit => {
            verify_expected_files(root, &guard.next_files)?;
            verify_paths_absent(
                root,
                &guard.stale_paths,
                "stale generated output reappeared before publication commit",
            )
        }
    }
}

pub(super) fn validate_generation_artifact_count(count: usize) -> PpResult<()> {
    if count == 0 {
        return Err(PpError::InvalidRequest(
            "generation must contain at least one artifact".to_string(),
        ));
    }
    if count > MAX_GENERATION_ARTIFACTS {
        return Err(PpError::InvalidRequest(format!(
            "generation has more than {MAX_GENERATION_ARTIFACTS} artifacts"
        )));
    }
    Ok(())
}

fn validate_generated_artifacts(artifacts: &[GeneratedArtifact]) -> PpResult<()> {
    validate_generation_artifact_count(artifacts.len())?;

    // Hashing is the expensive per-artifact work, so it runs in parallel; the
    // running-total/duplicate-path bookkeeping below stays sequential because it is
    // order-dependent (first-offender error selection, overflow-safe accumulation).
    let records = perfectpixel::parallel_map(artifacts, |artifact| {
        let byte_count = u64::try_from(artifact.bytes.len()).map_err(|_| {
            PpError::InvalidRequest("generation artifact byte count overflow".to_string())
        })?;
        GenerationArtifact::new(
            artifact.relative_path.clone(),
            sha256_hex(&artifact.bytes),
            byte_count,
        )
        .map_err(PpError::InvalidRequest)
    })?;

    let mut total_bytes = 0u64;
    let mut paths = BTreeSet::new();
    for (artifact, record) in artifacts.iter().zip(records) {
        let byte_count = record.byte_count;
        if !paths.insert(record.relative_path) {
            return Err(PpError::InvalidRequest(format!(
                "generation repeats artifact path '{}'",
                artifact.relative_path
            )));
        }
        total_bytes = total_bytes
            .checked_add(byte_count)
            .ok_or_else(|| PpError::InvalidRequest("generation byte count overflow".to_string()))?;
        if total_bytes > MAX_GENERATION_BYTES {
            return Err(PpError::InvalidRequest(
                "generation exceeds byte limit".to_string(),
            ));
        }
    }
    Ok(())
}

fn reject_input_path_collisions(
    root: &Path,
    artifacts: &[GeneratedArtifact],
    input_snapshots: &[InputSnapshot],
) -> PpResult<()> {
    for relative_path in artifacts
        .iter()
        .map(|artifact| artifact.relative_path.as_str())
        .chain(std::iter::once(GENERATION_AUTHORITY_FILE))
    {
        let output = root.join(managed_relative_path(relative_path)?);
        for input in input_snapshots {
            reject_input_output_collision(
                input,
                &output,
                "generated output must not overwrite a current request or input",
            )?;
        }
    }
    Ok(())
}

pub(super) fn validate_input_snapshot_count(count: usize) -> PpResult<()> {
    if count > MAX_INPUT_SNAPSHOTS {
        return Err(PpError::InvalidRequest(format!(
            "generation has more than {MAX_INPUT_SNAPSHOTS} input snapshots"
        )));
    }
    Ok(())
}

pub(super) fn validate_input_snapshot_set(snapshots: &[InputSnapshot]) -> PpResult<()> {
    validate_input_snapshot_count(snapshots.len())?;

    let mut source_revisions = BTreeMap::new();
    let mut canonical_revisions = BTreeMap::new();
    let mut total_bytes = 0u64;
    for snapshot in snapshots {
        total_bytes = total_bytes
            .checked_add(snapshot.byte_count)
            .ok_or_else(|| {
                PpError::InvalidRequest("generation input byte count overflow".to_string())
            })?;
        if total_bytes > MAX_INPUT_SNAPSHOT_BYTES {
            return Err(PpError::InvalidRequest(format!(
                "generation inputs exceed {MAX_INPUT_SNAPSHOT_BYTES}-byte snapshot limit"
            )));
        }
        let revision = snapshot.revision();
        let source_revision = (snapshot.canonical_path.clone(), revision.clone());
        if let Some(previous) =
            source_revisions.insert(snapshot.source_path.clone(), source_revision.clone())
        {
            if previous != source_revision {
                return Err(input_snapshot_error(
                    &snapshot.source_path,
                    "generation captured multiple revisions of the same input path",
                ));
            }
        }
        if let Some(previous) =
            canonical_revisions.insert(snapshot.canonical_path.clone(), revision.clone())
        {
            if previous != revision {
                return Err(input_snapshot_error(
                    &snapshot.source_path,
                    "generation captured conflicting revisions through input path aliases",
                ));
            }
        }
    }
    Ok(())
}

pub(super) fn verify_input_snapshots(snapshots: &[InputSnapshot]) -> PpResult<()> {
    validate_input_snapshot_set(snapshots)?;
    for snapshot in snapshots {
        verify_input_snapshot(snapshot)?;
    }
    Ok(())
}

fn verify_input_snapshot(snapshot: &InputSnapshot) -> PpResult<()> {
    let canonical_before = fs::canonicalize(&snapshot.source_path)
        .map_err(|source| input_snapshot_error(&snapshot.source_path, source.to_string()))?;
    if canonical_before != snapshot.canonical_path {
        return Err(input_snapshot_error(
            &snapshot.source_path,
            "generation input now resolves to a different file",
        ));
    }

    let mut file = fs::File::open(&snapshot.canonical_path)
        .map_err(|source| input_snapshot_error(&snapshot.source_path, source.to_string()))?;
    let before = file
        .metadata()
        .map_err(|source| input_snapshot_error(&snapshot.source_path, source.to_string()))?;
    if !before.is_file() || snapshot_file_revision(&before) != snapshot.file_revision {
        return Err(input_snapshot_error(
            &snapshot.source_path,
            "generation input changed after computation",
        ));
    }

    let read_limit = snapshot.byte_count.checked_add(1).ok_or_else(|| {
        input_snapshot_error(
            &snapshot.source_path,
            "generation input byte count overflow",
        )
    })?;
    let mut bytes = Vec::new();
    file.by_ref()
        .take(read_limit)
        .read_to_end(&mut bytes)
        .map_err(|source| input_snapshot_error(&snapshot.source_path, source.to_string()))?;
    let after = file
        .metadata()
        .map_err(|source| input_snapshot_error(&snapshot.source_path, source.to_string()))?;
    let canonical_after = fs::canonicalize(&snapshot.source_path)
        .map_err(|source| input_snapshot_error(&snapshot.source_path, source.to_string()))?;
    let current_after = fs::metadata(&canonical_after)
        .map_err(|source| input_snapshot_error(&snapshot.source_path, source.to_string()))?;
    let bytes_len = u64::try_from(bytes.len()).map_err(|_| {
        input_snapshot_error(
            &snapshot.source_path,
            "generation input byte count overflow",
        )
    })?;
    if canonical_after != snapshot.canonical_path
        || snapshot_file_revision(&after) != snapshot.file_revision
        || snapshot_file_revision(&current_after) != snapshot.file_revision
        || bytes_len != snapshot.byte_count
        || sha256_hex(&bytes) != snapshot.sha256
    {
        return Err(input_snapshot_error(
            &snapshot.source_path,
            "generation input changed after computation",
        ));
    }
    Ok(())
}

fn reject_input_output_collision(
    input: &InputSnapshot,
    output: &Path,
    message: &str,
) -> PpResult<()> {
    reject_same_path(&input.source_path, output, message)?;
    if input.canonical_path != input.source_path {
        reject_same_path(&input.canonical_path, output, message)?;
    }
    Ok(())
}

fn input_snapshot_error(path: &Path, message: impl ToString) -> PpError {
    PpError::FileIo {
        path: path.to_path_buf(),
        message: message.to_string(),
    }
}

fn generation_artifacts_for(artifacts: &[GeneratedArtifact]) -> PpResult<Vec<GenerationArtifact>> {
    artifacts
        .iter()
        .map(|artifact| {
            let byte_count = u64::try_from(artifact.bytes.len()).map_err(|_| {
                PpError::InvalidRequest("generation artifact byte count overflow".to_string())
            })?;
            GenerationArtifact::new(
                artifact.relative_path.clone(),
                sha256_hex(&artifact.bytes),
                byte_count,
            )
            .map_err(PpError::InvalidRequest)
        })
        .collect()
}

fn capture_stable_managed_file(
    path: &Path,
    limit: u64,
    allow_missing: bool,
    role: &str,
) -> PpResult<Option<(Vec<u8>, ManagedFileSnapshot)>> {
    let initial = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(source) if allow_missing && source.kind() == std::io::ErrorKind::NotFound => {
            return Ok(None);
        }
        Err(source) => {
            return Err(PpError::FileIo {
                path: path.to_path_buf(),
                message: source.to_string(),
            });
        }
    };
    if initial.file_type().is_symlink() || !initial.is_file() {
        return Err(destination_error(
            path,
            &format!("{role} must be a regular non-symlink file"),
        ));
    }
    if initial.len() > limit {
        return Err(PpError::FileIo {
            path: path.to_path_buf(),
            message: format!("{role} exceeds {limit}-byte read limit"),
        });
    }

    let mut file = fs::File::open(path).map_err(|source| PpError::FileIo {
        path: path.to_path_buf(),
        message: source.to_string(),
    })?;
    let opened = file.metadata().map_err(|source| PpError::FileIo {
        path: path.to_path_buf(),
        message: source.to_string(),
    })?;
    let opened_revision = snapshot_file_revision(&opened);
    if !opened.is_file() || snapshot_file_revision(&initial) != opened_revision {
        return Err(destination_error(
            path,
            &format!("{role} changed while it was opened"),
        ));
    }

    let read_limit = limit.checked_add(1).ok_or_else(|| PpError::FileIo {
        path: path.to_path_buf(),
        message: format!("{role} read limit overflow"),
    })?;
    let mut bytes = Vec::new();
    file.by_ref()
        .take(read_limit)
        .read_to_end(&mut bytes)
        .map_err(|source| PpError::FileIo {
            path: path.to_path_buf(),
            message: source.to_string(),
        })?;
    let byte_count = u64::try_from(bytes.len()).map_err(|_| PpError::FileIo {
        path: path.to_path_buf(),
        message: format!("{role} byte count overflow"),
    })?;
    if byte_count > limit {
        return Err(PpError::FileIo {
            path: path.to_path_buf(),
            message: format!("{role} exceeds {limit}-byte read limit"),
        });
    }

    let after = file.metadata().map_err(|source| PpError::FileIo {
        path: path.to_path_buf(),
        message: source.to_string(),
    })?;
    let current = fs::symlink_metadata(path).map_err(|source| PpError::FileIo {
        path: path.to_path_buf(),
        message: source.to_string(),
    })?;
    let after_revision = snapshot_file_revision(&after);
    if current.file_type().is_symlink()
        || !current.is_file()
        || after_revision != opened_revision
        || snapshot_file_revision(&current) != after_revision
        || after.len() != byte_count
    {
        return Err(destination_error(
            path,
            &format!("{role} changed while it was being read"),
        ));
    }

    let sha256 = sha256_hex(&bytes);
    Ok(Some((
        bytes,
        ManagedFileSnapshot {
            sha256,
            byte_count,
            file_revision: after_revision,
        },
    )))
}

fn read_generation_authority(
    root: &Path,
) -> PpResult<(GenerationAuthorityState, GenerationAuthorityPrecondition)> {
    let path = root.join(GENERATION_AUTHORITY_FILE);
    let Some((bytes, snapshot)) =
        capture_stable_managed_file(&path, MAX_AUTHORITY_BYTES, true, "generation authority")?
    else {
        return Ok((
            GenerationAuthorityState::Absent,
            GenerationAuthorityPrecondition::Absent,
        ));
    };
    let authority = GenerationAuthority::from_slice(&bytes)
        .map_err(|message| generation_authority_error(root, message))?;
    Ok((
        GenerationAuthorityState::Published(authority),
        GenerationAuthorityPrecondition::Published(snapshot),
    ))
}

fn verify_authority_precondition(
    root: &Path,
    precondition: &GenerationAuthorityPrecondition,
) -> PpResult<()> {
    let path = root.join(GENERATION_AUTHORITY_FILE);
    match precondition {
        GenerationAuthorityPrecondition::Absent => verify_paths_absent(
            root,
            &[PathBuf::from(GENERATION_AUTHORITY_FILE)],
            "generation authority appeared after publication planning",
        ),
        GenerationAuthorityPrecondition::Published(expected) => {
            let Some((_, observed)) = capture_stable_managed_file(
                &path,
                MAX_AUTHORITY_BYTES,
                true,
                "generation authority",
            )?
            else {
                return Err(destination_error(
                    &path,
                    "generation authority disappeared after publication planning",
                ));
            };
            if observed != *expected {
                return Err(destination_error(
                    &path,
                    "generation authority changed after publication planning",
                ));
            }
            Ok(())
        }
    }
}

/// Imports pre-authority output only when a product-owned control artifact has
/// a recognized schema and the complete declared generation is present. This
/// avoids taking ownership of coincidentally named files.
fn seed_legacy_generation_authority(root: &Path) -> PpResult<GenerationAuthorityState> {
    let records = [
        legacy_normalize_record(root)?,
        legacy_bundle_record(root)?,
        legacy_motion_scaffold_record(root)?,
        legacy_motion_build_record(root)?,
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>();

    if records.is_empty() {
        Ok(GenerationAuthorityState::Absent)
    } else {
        GenerationAuthority::new(records)
            .map(GenerationAuthorityState::Published)
            .map_err(|message| generation_authority_error(root, message))
    }
}

fn legacy_normalize_record(root: &Path) -> PpResult<Option<GenerationRecord>> {
    let report_path = root.join("normalize-report.json");
    let Some(report) = read_optional_versioned_json::<NormalizeReport>(
        &report_path,
        NORMALIZE_REPORT_SCHEMA,
        None,
    )?
    else {
        return Ok(None);
    };

    let mut paths = BTreeSet::from(["normalize-report.json".to_string()]);
    let request_path = root.join("sprite-request.json");
    let request = read_optional_json_strict::<SpriteBundleRequest>(&request_path)?;
    if report.ok && request.is_none() {
        return Err(destination_error(
            &request_path,
            "legacy successful normalize generation is missing sprite-request.json",
        ));
    }
    // A failed legacy normalize may still contain outputs from an earlier
    // successful run. A valid request is strong provenance for those stale
    // frames, so import the complete set and let the next event remove it.
    if let Some(request) = request {
        paths.insert("sprite-request.json".to_string());
        for state in request.states {
            paths.extend(state.frames);
        }
    }

    required_generation_record(root, GenerationWorkflow::Normalize, paths).map(Some)
}

fn legacy_bundle_record(root: &Path) -> PpResult<Option<GenerationRecord>> {
    let path = root.join("manifest.json");
    let Some(manifest) = read_optional_versioned_json::<Manifest>(
        &path,
        perfectpixel::SPRITE_SCHEMA,
        Some(("app", env!("CARGO_PKG_NAME"))),
    )?
    else {
        return Ok(None);
    };

    let mut paths = BTreeSet::from(["manifest.json".to_string()]);
    for sheet in manifest.sheets {
        paths.insert(sheet.image.clone());
        paths.insert(aseprite_json_name(&sheet.image));
    }
    for animation in manifest.animations.into_values() {
        for item in animation.items {
            paths.insert(item.output);
        }
    }
    required_generation_record(root, GenerationWorkflow::Bundle, paths).map(Some)
}

fn legacy_motion_scaffold_record(root: &Path) -> PpResult<Option<GenerationRecord>> {
    let layers_path = root.join("layers.json");
    let Some(layers) = read_optional_versioned_json::<MotionLayersDocument>(
        &layers_path,
        MOTION_LAYERS_SCHEMA,
        None,
    )?
    else {
        return Ok(None);
    };

    let request_path = root.join("motion-request.json");
    let Some(request) =
        read_optional_versioned_json::<MotionRequest>(&request_path, MOTION_SCHEMA, None)?
    else {
        return Err(destination_error(
            &request_path,
            "legacy motion scaffold generation is missing a valid motion-request.json",
        ));
    };
    if request.source_svg != "scene.svg" {
        return Err(destination_error(
            &request_path,
            "legacy motion scaffold request must bind sourceSvg to scene.svg",
        ));
    }

    let scene_path = root.join("scene.svg");
    let Some(scene_svg) = read_optional_control_text(&scene_path)? else {
        return Err(destination_error(
            &scene_path,
            "legacy motion scaffold generation is missing scene.svg",
        ));
    };
    if MotionCompiler::scene_sha256(&scene_svg) != request.source_svg_sha256 {
        return Err(destination_error(
            &request_path,
            "legacy motion scaffold request hash does not match scene.svg",
        ));
    }

    let regenerated =
        MotionCompiler::scaffold(&scene_svg).map_err(|source| PpError::InvalidRequestSource {
            path: scene_path.clone(),
            message: "legacy motion scaffold scene violates the scaffold contract".to_string(),
            original_error: source.to_string(),
        })?;
    if regenerated.scene_svg != scene_svg {
        return Err(destination_error(
            &scene_path,
            "legacy motion scaffold scene is not the canonical scaffold output",
        ));
    }
    if regenerated.layers != layers {
        return Err(destination_error(
            &layers_path,
            "legacy layers.json does not match the canonical scene scaffold",
        ));
    }

    let inspector_path = root.join("layer-inspector.html");
    let Some(inspector_html) = read_optional_control_text(&inspector_path)? else {
        return Err(destination_error(
            &inspector_path,
            "legacy motion scaffold generation is missing layer-inspector.html",
        ));
    };
    if regenerated.inspector_html != inspector_html {
        return Err(destination_error(
            &inspector_path,
            "legacy layer-inspector.html does not match the canonical scene scaffold",
        ));
    }

    required_generation_record(
        root,
        GenerationWorkflow::MotionScaffold,
        [
            "scene.svg".to_string(),
            "layers.json".to_string(),
            "motion-request.json".to_string(),
            "layer-inspector.html".to_string(),
        ]
        .into_iter()
        .collect(),
    )
    .map(Some)
}

fn legacy_motion_build_record(root: &Path) -> PpResult<Option<GenerationRecord>> {
    let report_path = root.join("motion-report.json");
    let Some(report) =
        read_optional_versioned_json::<MotionReport>(&report_path, MOTION_REPORT_SCHEMA, None)?
    else {
        return Ok(None);
    };
    if !report.animated_svg
        || !report.lottie_json
        || !report.dotlottie_v2_layout
        || report.dotlottie_archive_created
    {
        return Err(destination_error(
            &report_path,
            "legacy motion report does not describe the exploded perfectpixel motion output contract",
        ));
    }

    let manifest_path = root.join("dotlottie/manifest.json");
    let Some(manifest_text) = read_optional_control_text(&manifest_path)? else {
        return Err(destination_error(
            &manifest_path,
            "legacy motion generation is missing dotlottie/manifest.json",
        ));
    };
    let manifest: Value =
        serde_json::from_str(&manifest_text).map_err(|source| PpError::InvalidRequestSource {
            path: manifest_path.clone(),
            message: "legacy dotLottie manifest is not valid JSON".to_string(),
            original_error: source.to_string(),
        })?;
    let generated_by_perfectpixel = manifest
        .get("generator")
        .and_then(Value::as_str)
        .is_some_and(|generator| generator.starts_with(perfectpixel::GENERATOR_PREFIX));
    let animations = manifest.get("animations").and_then(Value::as_array);
    let exact_animation_binding = animations.is_some_and(|animations| {
        animations.len() == 1
            && animations[0].get("id").and_then(Value::as_str) == Some(report.name.as_str())
    });
    if manifest.get("version").and_then(Value::as_str) != Some("2")
        || !generated_by_perfectpixel
        || manifest
            .pointer("/initial/animation")
            .and_then(Value::as_str)
            != Some(report.name.as_str())
        || !exact_animation_binding
    {
        return Err(destination_error(
            &manifest_path,
            "legacy dotLottie manifest does not bind exactly one perfectpixel animation to the motion report",
        ));
    }

    let mut paths = BTreeSet::from([
        "animated.svg".to_string(),
        "animation.json".to_string(),
        "motion-report.json".to_string(),
        "preview.html".to_string(),
        "dotlottie/manifest.json".to_string(),
    ]);
    let expected_animation = format!("dotlottie/a/{}.json", report.name);
    let expected_animation_path = root.join(&expected_animation);
    if !is_perfectpixel_lottie_animation(&expected_animation_path, &report.name)? {
        return Err(destination_error(
            &expected_animation_path,
            "legacy dotLottie animation is not a perfectpixel animation matching the motion report",
        ));
    }
    require_equal_legacy_files(
        &root.join("animation.json"),
        &expected_animation_path,
        "legacy animation.json must exactly match its dotLottie animation",
    )?;
    paths.insert(expected_animation);
    paths.extend(legacy_motion_animation_paths(root)?);
    required_generation_record(root, GenerationWorkflow::MotionBuild, paths).map(Some)
}

fn require_equal_legacy_files(left: &Path, right: &Path, message: &str) -> PpResult<()> {
    let read = |path: &Path| -> PpResult<Vec<u8>> {
        let Some((bytes, _)) = capture_stable_managed_file(
            path,
            MAX_CONTROL_READ_BYTES as u64,
            true,
            "legacy generated artifact",
        )?
        else {
            return Err(destination_error(
                path,
                "legacy generated artifact is missing",
            ));
        };
        Ok(bytes)
    };
    if read(left)? != read(right)? {
        return Err(destination_error(left, message));
    }
    Ok(())
}

fn legacy_motion_animation_paths(root: &Path) -> PpResult<BTreeSet<String>> {
    let directory = root.join("dotlottie").join("a");
    let metadata = match fs::symlink_metadata(&directory) {
        Ok(metadata) => metadata,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeSet::new()),
        Err(source) => {
            return Err(PpError::FileIo {
                path: directory,
                message: source.to_string(),
            });
        }
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(destination_error(
            &directory,
            "legacy motion animation directory must be a real directory",
        ));
    }

    let mut paths = BTreeSet::new();
    for entry in fs::read_dir(&directory).map_err(|source| PpError::FileIo {
        path: directory.clone(),
        message: source.to_string(),
    })? {
        let entry = entry.map_err(|source| PpError::FileIo {
            path: directory.clone(),
            message: source.to_string(),
        })?;
        let file_name = entry.file_name();
        let Some(file_name) = file_name.to_str() else {
            continue;
        };
        let Some(expected_name) = file_name.strip_suffix(".json") else {
            continue;
        };
        if expected_name.is_empty() {
            continue;
        }
        let file_type = entry.file_type().map_err(|source| PpError::FileIo {
            path: entry.path(),
            message: source.to_string(),
        })?;
        if file_type.is_symlink() || !file_type.is_file() {
            return Err(destination_error(
                &entry.path(),
                "legacy motion animation must be a regular non-symlink file",
            ));
        }
        if !is_perfectpixel_lottie_animation(&entry.path(), expected_name)? {
            continue;
        }
        if paths.len() >= MAX_LEGACY_MOTION_ANIMATIONS {
            return Err(destination_error(
                &directory,
                "legacy motion animation directory has too many managed JSON files",
            ));
        }
        paths.insert(format!("dotlottie/a/{file_name}"));
    }
    Ok(paths)
}

fn is_perfectpixel_lottie_animation(path: &Path, expected_name: &str) -> PpResult<bool> {
    let Some((bytes, _)) = capture_stable_managed_file(
        path,
        MAX_CONTROL_READ_BYTES as u64,
        true,
        "legacy motion animation",
    )?
    else {
        return Ok(false);
    };
    let Ok(value) = serde_json::from_slice::<Value>(&bytes) else {
        return Ok(false);
    };
    let generated_by_perfectpixel = value
        .pointer("/meta/g")
        .and_then(Value::as_str)
        .is_some_and(|generator| generator.starts_with(perfectpixel::GENERATOR_PREFIX));
    Ok(generated_by_perfectpixel
        && value.get("v").and_then(Value::as_str).is_some()
        && value.get("nm").and_then(Value::as_str) == Some(expected_name)
        && value.get("layers").and_then(Value::as_array).is_some()
        && value.get("assets").and_then(Value::as_array).is_some())
}

fn required_generation_record(
    root: &Path,
    workflow: GenerationWorkflow,
    paths: BTreeSet<String>,
) -> PpResult<GenerationRecord> {
    let artifacts = required_existing_generation_artifacts(root, paths)?;
    GenerationRecord::new(workflow, artifacts)
        .map_err(|message| generation_authority_error(root, message))
}

fn required_existing_generation_artifacts(
    root: &Path,
    paths: BTreeSet<String>,
) -> PpResult<Vec<GenerationArtifact>> {
    // BTreeSet iteration is already sorted, so collecting it keeps the exact order the
    // sequential budget check below relies on. File read + hash is the expensive part and
    // runs in parallel; running-total accumulation stays sequential (order-dependent overflow
    // and budget checks).
    let paths = paths.into_iter().collect::<Vec<_>>();
    let snapshots = perfectpixel::parallel_map(&paths, |relative_path| {
        let relative = managed_relative_path(relative_path)?;
        let path = root.join(&relative);
        let Some((_, snapshot)) = capture_stable_managed_file(
            &path,
            MAX_GENERATION_BYTES,
            true,
            "legacy generated artifact",
        )?
        else {
            return Err(destination_error(
                &path,
                "recognized legacy generation is incomplete",
            ));
        };
        Ok((path, snapshot))
    })?;

    let mut artifacts = Vec::with_capacity(paths.len());
    let mut total_bytes = 0u64;
    for (relative_path, (path, snapshot)) in paths.into_iter().zip(snapshots) {
        total_bytes = total_bytes
            .checked_add(snapshot.byte_count)
            .ok_or_else(|| {
                PpError::InvalidRequest("legacy generation byte count overflow".into())
            })?;
        if total_bytes > MAX_GENERATION_BYTES {
            return Err(PpError::FileIo {
                path,
                message: format!(
                    "legacy generated artifact set exceeds {MAX_GENERATION_BYTES}-byte limit"
                ),
            });
        }
        artifacts.push(
            GenerationArtifact::new(relative_path, snapshot.sha256, snapshot.byte_count)
                .map_err(PpError::InvalidRequest)?,
        );
    }
    Ok(artifacts)
}

fn verify_generation_artifacts(
    root: &Path,
    artifacts: &[GenerationArtifact],
    missing_policy: MissingArtifactPolicy,
    protected_inputs: Option<&[InputSnapshot]>,
) -> PpResult<()> {
    // The running byte total is inherently sequential (order-dependent overflow), so it is
    // computed first as a cheap, I/O-free pass. Everything after — collision checks and the
    // per-artifact file read/hash — is independent per artifact and runs in parallel.
    let mut running_totals = Vec::with_capacity(artifacts.len());
    let mut total_bytes = 0u64;
    for artifact in artifacts {
        total_bytes = total_bytes
            .checked_add(artifact.byte_count)
            .ok_or_else(|| PpError::InvalidRequest("generation byte count overflow".to_string()))?;
        running_totals.push(total_bytes);
    }
    let indexed = artifacts.iter().zip(running_totals).collect::<Vec<_>>();

    perfectpixel::parallel_map(&indexed, |&(artifact, running_total)| {
        if artifact.byte_count > MAX_GENERATION_BYTES || running_total > MAX_GENERATION_BYTES {
            return Err(PpError::InvalidRequest(
                "generation authority exceeds byte limit".to_string(),
            ));
        }

        let path = root.join(managed_relative_path(&artifact.relative_path)?);
        if let Some(inputs) = protected_inputs {
            for input in inputs {
                reject_input_output_collision(
                    input,
                    &path,
                    "managed generated output must not replace or remove a current request or input",
                )?;
            }
        }

        let observed = match fs::symlink_metadata(&path) {
            Ok(_) => capture_stable_managed_file(
                &path,
                MAX_GENERATION_BYTES,
                false,
                "managed generated artifact",
            )?,
            Err(source)
                if missing_policy == MissingArtifactPolicy::Allowed
                    && source.kind() == std::io::ErrorKind::NotFound =>
            {
                None
            }
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
                return Err(destination_error(
                    &path,
                    "generation authority claims a missing managed artifact",
                ));
            }
            Err(source) => {
                return Err(PpError::FileIo {
                    path,
                    message: source.to_string(),
                });
            }
        };
        let Some((_, snapshot)) = observed else {
            return Ok(());
        };
        if snapshot.byte_count != artifact.byte_count || snapshot.sha256 != artifact.sha256 {
            return Err(modified_generated_artifact_error(&path));
        }
        Ok(())
    })?;
    Ok(())
}

/// A retained managed file that this run declares as an immutable input is
/// authored by the caller between generations: `motion-scaffold` writes the
/// starter `motion-request.json` and `motion-build` consumes the filled-in one.
/// Its owning record is refreshed from the captured input snapshot so the
/// byte-identity guard verifies the bytes this run actually consumed instead of
/// the superseded starter. Every other retained file keeps its recorded digest.
fn refresh_authored_input_artifacts(
    root: &Path,
    transition: &mut GenerationTransition,
    input_snapshots: &[InputSnapshot],
) -> PpResult<()> {
    let mut authored = BTreeMap::<String, (String, u64)>::new();
    for artifact in &transition.retained_artifacts {
        let path = root.join(managed_relative_path(&artifact.relative_path)?);
        let Ok(canonical) = fs::canonicalize(&path) else {
            continue;
        };
        if let Some(snapshot) = input_snapshots
            .iter()
            .find(|snapshot| snapshot.canonical_path == canonical)
        {
            authored.insert(
                artifact.relative_path.clone(),
                (snapshot.sha256.clone(), snapshot.byte_count),
            );
        }
    }
    if authored.is_empty() {
        return Ok(());
    }

    let refresh = |artifact: &mut GenerationArtifact| {
        if let Some((sha256, byte_count)) = authored.get(&artifact.relative_path) {
            artifact.sha256.clone_from(sha256);
            artifact.byte_count = *byte_count;
        }
    };
    transition.retained_artifacts.iter_mut().for_each(refresh);
    transition
        .next_authority
        .generations
        .iter_mut()
        .flat_map(|record| record.artifacts.iter_mut())
        .for_each(refresh);
    Ok(())
}

fn verify_expected_files(root: &Path, expected_files: &[ManagedFileExpectation]) -> PpResult<()> {
    for expected in expected_files {
        let path = root.join(&expected.relative_path);
        let Some((_, observed)) = capture_stable_managed_file(
            &path,
            MAX_GENERATION_BYTES,
            true,
            "authority-declared file",
        )?
        else {
            return Err(destination_error(
                &path,
                "next generation is missing an authority-declared file",
            ));
        };
        if observed.byte_count != expected.byte_count || observed.sha256 != expected.sha256 {
            return Err(destination_error(
                &path,
                "published file does not match the next generation authority",
            ));
        }
    }
    Ok(())
}

fn verify_paths_absent(root: &Path, relative_paths: &[PathBuf], message: &str) -> PpResult<()> {
    for relative_path in relative_paths {
        let path = root.join(relative_path);
        match fs::symlink_metadata(&path) {
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(PpError::FileIo {
                    path,
                    message: source.to_string(),
                });
            }
            Ok(_) => return Err(destination_error(&path, message)),
        }
    }
    Ok(())
}

fn modified_generated_artifact_error(path: &Path) -> PpError {
    destination_error(
        path,
        "managed generated artifact changed since authority was written; refusing to replace or remove it",
    )
}

/// A malformed or foreign control file is not provenance. Once the advertised
/// schema/discriminator matches, deserialization failure is a product-contract
/// violation and publication stops instead of guessing.
fn read_optional_versioned_json<T: DeserializeOwned>(
    path: &Path,
    expected_schema: &str,
    discriminator: Option<(&str, &str)>,
) -> PpResult<Option<T>> {
    let Some(text) = read_optional_control_text(path)? else {
        return Ok(None);
    };
    let value: Value = match serde_json::from_str(&text) {
        Ok(value) => value,
        Err(_) => return Ok(None),
    };
    if value.get("schema").and_then(Value::as_str) != Some(expected_schema) {
        return Ok(None);
    }
    if let Some((field, expected)) = discriminator {
        if value.get(field).and_then(Value::as_str) != Some(expected) {
            return Ok(None);
        }
    }
    serde_json::from_value(value)
        .map(Some)
        .map_err(|source| PpError::InvalidRequestSource {
            path: path.to_path_buf(),
            message: format!(
                "legacy control artifact claims schema '{expected_schema}' but violates its contract"
            ),
            original_error: source.to_string(),
        })
}

fn read_optional_json_strict<T: DeserializeOwned>(path: &Path) -> PpResult<Option<T>> {
    let Some(text) = read_optional_control_text(path)? else {
        return Ok(None);
    };
    serde_json::from_str(&text)
        .map(Some)
        .map_err(|source| PpError::InvalidRequestSource {
            path: path.to_path_buf(),
            message: "legacy control artifact is not valid JSON".to_string(),
            original_error: source.to_string(),
        })
}

fn read_optional_control_text(path: &Path) -> PpResult<Option<String>> {
    let Some((bytes, _)) = capture_stable_managed_file(
        path,
        MAX_CONTROL_READ_BYTES as u64,
        true,
        "legacy control artifact",
    )?
    else {
        return Ok(None);
    };
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|source| PpError::InvalidRequestSource {
            path: path.to_path_buf(),
            message: "legacy control artifact must be UTF-8".to_string(),
            original_error: source.to_string(),
        })
}

fn generation_authority_error(root: &Path, message: String) -> PpError {
    PpError::InvalidRequest(format!(
        "generation authority '{}': {message}",
        root.join(GENERATION_AUTHORITY_FILE).display()
    ))
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    #[test]
    fn input_snapshot_accepts_only_the_captured_revision() {
        let root = temp_root("input-snapshot-revision");
        let input = root.join("input.json");
        fs::write(&input, b"first").expect("input");
        let (_, snapshot) = InputSnapshot::capture(&input, 1024).expect("snapshot");

        verify_input_snapshots(std::slice::from_ref(&snapshot)).expect("unchanged snapshot");
        fs::write(&input, b"other").expect("changed input");
        let error = verify_input_snapshots(std::slice::from_ref(&snapshot))
            .expect_err("same-sized content change must fail");

        assert!(error.to_string().contains("changed after computation"));
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[cfg(unix)]
    #[test]
    fn input_snapshot_rejects_symlink_retarget_even_with_identical_bytes() {
        use std::os::unix::fs::symlink;

        let root = temp_root("input-snapshot-symlink");
        let first = root.join("first.svg");
        let second = root.join("second.svg");
        let input = root.join("input.svg");
        fs::write(&first, b"<svg/>").expect("first");
        fs::write(&second, b"<svg/>").expect("second");
        symlink(&first, &input).expect("input symlink");
        let (_, snapshot) = InputSnapshot::capture(&input, 1024).expect("snapshot");

        fs::remove_file(&input).expect("remove symlink");
        symlink(&second, &input).expect("retarget symlink");
        let error = verify_input_snapshots(std::slice::from_ref(&snapshot))
            .expect_err("retargeted input must fail");

        assert!(error.to_string().contains("different file"));
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn input_snapshot_set_rejects_conflicting_alias_revisions() {
        let root = temp_root("input-snapshot-alias");
        let input = root.join("input.json");
        fs::write(&input, b"revision").expect("input");
        let (_, snapshot) = InputSnapshot::capture(&input, 1024).expect("snapshot");
        let mut conflicting_alias = snapshot.clone();
        conflicting_alias.source_path = root.join("alias.json");
        conflicting_alias.sha256 = "00".repeat(32);

        let error = validate_input_snapshot_set(&[snapshot, conflicting_alias])
            .expect_err("one canonical input cannot represent competing revisions");

        assert!(error.to_string().contains("conflicting revisions"));
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn input_snapshot_count_is_bounded_before_publication() {
        assert!(validate_input_snapshot_count(MAX_INPUT_SNAPSHOTS).is_ok());
        let error = validate_input_snapshot_count(MAX_INPUT_SNAPSHOTS + 1)
            .expect_err("snapshot count above the publication bound must fail");
        assert!(error.to_string().contains("input snapshots"));
    }

    #[test]
    fn generation_artifact_count_is_bounded_before_encoding_or_publication() {
        assert!(validate_generation_artifact_count(1).is_ok());
        assert!(validate_generation_artifact_count(MAX_GENERATION_ARTIFACTS).is_ok());
        assert!(validate_generation_artifact_count(0)
            .expect_err("empty generation must fail")
            .to_string()
            .contains("at least one artifact"));
        assert!(
            validate_generation_artifact_count(MAX_GENERATION_ARTIFACTS + 1)
                .expect_err("oversized generation must fail")
                .to_string()
                .contains("artifacts")
        );
    }

    #[test]
    fn generation_input_snapshot_bytes_are_bounded() {
        let root = temp_root("input-snapshot-byte-bound");
        let input = root.join("input.bin");
        fs::write(&input, b"x").expect("input");
        let (_, mut snapshot) = InputSnapshot::capture(&input, 1024).expect("snapshot");
        snapshot.byte_count = MAX_INPUT_SNAPSHOT_BYTES + 1;

        let error = validate_input_snapshot_set(&[snapshot])
            .expect_err("oversized captured input set must fail");
        assert!(error.to_string().contains("snapshot limit"));
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn input_snapshot_capture_enforces_read_limit() {
        let root = temp_root("input-snapshot-limit");
        let input = root.join("input.bin");
        fs::write(&input, b"12345").expect("input");

        let error = InputSnapshot::capture(&input, 4).expect_err("oversized input must fail");

        assert!(error.to_string().contains("4-byte read limit"));
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn unrelated_or_malformed_legacy_manifest_is_not_claimed() {
        let root = temp_root("legacy-unrelated");
        fs::write(root.join("manifest.json"), b"{").expect("manifest");
        assert!(legacy_bundle_record(&root)
            .expect("legacy lookup")
            .is_none());

        fs::write(
            root.join("manifest.json"),
            br#"{"app":"other","schema":"perfectpixel.sprite/3"}"#,
        )
        .expect("other manifest");
        assert!(legacy_bundle_record(&root)
            .expect("legacy lookup")
            .is_none());
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn recognized_but_invalid_legacy_manifest_fails_closed() {
        let root = temp_root("legacy-invalid");
        fs::write(
            root.join("manifest.json"),
            br#"{"app":"perfectpixel","schema":"perfectpixel.sprite/3"}"#,
        )
        .expect("manifest");

        let error = legacy_bundle_record(&root).expect_err("invalid claimed manifest");
        assert!(error.to_string().contains("violates its contract"));
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn recognized_but_incomplete_legacy_generation_fails_closed() {
        let root = temp_root("legacy-incomplete");
        fs::write(
            root.join("manifest.json"),
            br#"{
                "app": "perfectpixel",
                "generator": "perfectpixel",
                "schema": "perfectpixel.sprite/3",
                "version": 3,
                "character": "legacy",
                "packing": {
                    "algorithm": "maxrects",
                    "trim": true,
                    "padding": 2,
                    "allowRotation": false,
                    "multipack": true,
                    "maxWidth": 64,
                    "maxHeight": 64
                },
                "sheets": [{"index": 0, "image": "sheet.png", "width": 1, "height": 1}],
                "animations": {}
            }"#,
        )
        .expect("manifest");

        let error = legacy_bundle_record(&root).expect_err("missing declared sheet");
        assert!(error.to_string().contains("incomplete"));
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn published_authority_is_the_only_state_source() {
        let root = temp_root("published-authority");
        let authority = GenerationAuthority::new(Vec::new()).expect("authority");
        fs::write(
            root.join(GENERATION_AUTHORITY_FILE),
            authority.to_pretty_json().expect("authority json"),
        )
        .expect("authority file");
        fs::write(
            root.join("manifest.json"),
            br#"{"app":"perfectpixel","schema":"perfectpixel.sprite/3"}"#,
        )
        .expect("invalid legacy manifest");

        let (plan, _) = plan_generation_publication(
            &root,
            GenerationPublicationRequest {
                workflow: GenerationWorkflow::Normalize,
                artifacts: vec![GeneratedArtifact {
                    relative_path: "normalize-report.json".to_string(),
                    bytes: b"report".to_vec(),
                }],
                invalidates: Vec::new(),
                input_snapshots: Vec::new(),
            },
        )
        .expect("published authority must prevent legacy rediscovery");

        let authority_entry = plan
            .entries
            .iter()
            .find(|entry| entry.relative_path == Path::new(GENERATION_AUTHORITY_FILE))
            .expect("authority entry");
        let next = GenerationAuthority::from_slice(&authority_entry.bytes).expect("next authority");
        assert_eq!(next.generations.len(), 1);
        assert_eq!(next.generations[0].workflow, GenerationWorkflow::Normalize);
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn failed_legacy_normalize_imports_complete_stale_success_set() {
        let root = temp_root("legacy-normalize-stale");
        fs::create_dir_all(root.join("frames/idle")).expect("frames");
        fs::write(
            root.join("normalize-report.json"),
            br#"{
                "ok": false,
                "schema": "perfectpixel.normalize/1",
                "character": "legacy",
                "cellWidth": 1,
                "cellHeight": 1,
                "states": [],
                "gates": [],
                "errors": ["legacy gate failure"],
                "warnings": []
            }"#,
        )
        .expect("normalize report");
        fs::write(
            root.join("sprite-request.json"),
            br#"{
                "character": "legacy",
                "sheetImage": "sheet.png",
                "cellWidth": 1,
                "cellHeight": 1,
                "states": [{
                    "name": "idle",
                    "fps": 1,
                    "loop": true,
                    "frames": ["frames/idle/frame-00.png"]
                }]
            }"#,
        )
        .expect("sprite request");
        fs::write(root.join("frames/idle/frame-00.png"), b"legacy frame").expect("frame");

        let record = legacy_normalize_record(&root)
            .expect("legacy normalize")
            .expect("record");
        assert_eq!(
            record
                .artifacts
                .iter()
                .map(|artifact| artifact.relative_path.as_str())
                .collect::<Vec<_>>(),
            vec![
                "frames/idle/frame-00.png",
                "normalize-report.json",
                "sprite-request.json"
            ]
        );
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn legacy_motion_build_claims_every_animation_json_for_stale_pruning() {
        let root = temp_root("legacy-motion-animations");
        fs::create_dir_all(root.join("dotlottie/a")).expect("animation directory");
        fs::write(root.join("animated.svg"), b"animated").expect("animated SVG");
        fs::write(root.join("preview.html"), b"preview").expect("preview");
        let lottie = |name: &str| {
            format!(
                r#"{{"v":"5.12.2","nm":"{name}","meta":{{"g":"perfectpixel@0.1.0"}},"assets":[],"layers":[]}}"#
            )
        };
        let current = lottie("current");
        fs::write(root.join("animation.json"), &current).expect("animation JSON");
        fs::write(root.join("dotlottie/a/current.json"), &current).expect("current");
        fs::write(root.join("dotlottie/a/previous.json"), lottie("previous")).expect("previous");
        fs::write(
            root.join("dotlottie/a/unrelated.json"),
            r#"{"v":"5.12.2","nm":"unrelated","assets":[],"layers":[]}"#,
        )
        .expect("unrelated JSON");
        fs::write(root.join("dotlottie/a/notes.txt"), b"unmanaged").expect("unmanaged note");
        fs::write(
            root.join("dotlottie/manifest.json"),
            br#"{
                "version": "2",
                "generator": "perfectpixel@0.1.0",
                "initial": {"animation": "current"},
                "animations": [{"id": "current"}]
            }"#,
        )
        .expect("manifest");
        fs::write(
            root.join("motion-report.json"),
            br#"{
                "schema": "perfectpixel.motion-report/1",
                "name": "current",
                "width": 1,
                "height": 1,
                "fps": 1,
                "durationMs": 1,
                "totalFrames": 1.0,
                "pathCount": 1,
                "authoredPathCount": 0,
                "partCount": 0,
                "trackCount": 0,
                "markerCount": 0,
                "assignedPathCount": 0,
                "unassignedPathCount": 1,
                "lottieShapeCount": 1,
                "animatedSvg": true,
                "lottieJson": true,
                "dotlottieV2Layout": true,
                "dotlottieArchiveCreated": false
            }"#,
        )
        .expect("motion report");

        let record = legacy_motion_build_record(&root)
            .expect("legacy motion lookup")
            .expect("legacy motion record");
        let paths = record
            .artifacts
            .iter()
            .map(|artifact| artifact.relative_path.as_str())
            .collect::<BTreeSet<_>>();

        assert!(paths.contains("dotlottie/a/current.json"));
        assert!(paths.contains("dotlottie/a/previous.json"));
        assert!(!paths.contains("dotlottie/a/unrelated.json"));
        assert!(!paths.contains("dotlottie/a/notes.txt"));
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn legacy_motion_scaffold_requires_canonical_cross_file_provenance() {
        let root = temp_root("legacy-motion-scaffold-provenance");
        let source = r##"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8" viewBox="0 0 8 8"><path d="M1 1L7 1L7 7L1 7Z" fill="#112233"/></svg>"##;
        let scaffold = MotionCompiler::scaffold(source).expect("scaffold");
        let mut request = MotionRequest {
            schema: MOTION_SCHEMA.to_string(),
            name: "scene".to_string(),
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
        fs::write(root.join("scene.svg"), &scaffold.scene_svg).expect("scene");
        fs::write(
            root.join("layers.json"),
            serde_json::to_string_pretty(&scaffold.layers).expect("layers JSON"),
        )
        .expect("layers");
        fs::write(
            root.join("motion-request.json"),
            serde_json::to_string_pretty(&request).expect("request JSON"),
        )
        .expect("request");
        fs::write(root.join("layer-inspector.html"), &scaffold.inspector_html).expect("inspector");

        assert!(legacy_motion_scaffold_record(&root)
            .expect("legacy scaffold lookup")
            .is_some());

        request.source_svg_sha256 = "00".repeat(32);
        fs::write(
            root.join("motion-request.json"),
            serde_json::to_string_pretty(&request).expect("request JSON"),
        )
        .expect("tampered request");
        let error = legacy_motion_scaffold_record(&root)
            .expect_err("mismatched cross-file provenance must fail");
        assert!(error.to_string().contains("hash does not match"));
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn legacy_motion_build_rejects_mismatched_primary_animation() {
        let root = temp_root("legacy-motion-animation-binding");
        fs::create_dir_all(root.join("dotlottie/a")).expect("animation directory");
        let current = r#"{"v":"5.12.2","nm":"current","meta":{"g":"perfectpixel@0.1.0"},"assets":[],"layers":[]}"#;
        fs::write(root.join("animated.svg"), b"animated").expect("animated SVG");
        fs::write(root.join("animation.json"), b"different").expect("primary animation");
        fs::write(root.join("preview.html"), b"preview").expect("preview");
        fs::write(root.join("dotlottie/a/current.json"), current).expect("dotLottie animation");
        fs::write(
            root.join("dotlottie/manifest.json"),
            br#"{
                "version": "2",
                "generator": "perfectpixel@0.1.0",
                "initial": {"animation": "current"},
                "animations": [{"id": "current"}]
            }"#,
        )
        .expect("manifest");
        fs::write(
            root.join("motion-report.json"),
            br#"{
                "schema": "perfectpixel.motion-report/1",
                "name": "current",
                "width": 1,
                "height": 1,
                "fps": 1,
                "durationMs": 1,
                "totalFrames": 1.0,
                "pathCount": 1,
                "authoredPathCount": 0,
                "partCount": 0,
                "trackCount": 0,
                "markerCount": 0,
                "assignedPathCount": 0,
                "unassignedPathCount": 1,
                "lottieShapeCount": 1,
                "animatedSvg": true,
                "lottieJson": true,
                "dotlottieV2Layout": true,
                "dotlottieArchiveCreated": false
            }"#,
        )
        .expect("motion report");

        let error = legacy_motion_build_record(&root)
            .expect_err("mismatched primary animation must fail closed");
        assert!(error.to_string().contains("must exactly match"));
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn first_publication_migrates_every_recognized_legacy_workflow() {
        let root = temp_root("initial-migration");
        fs::write(
            root.join("normalize-report.json"),
            br#"{
                "ok": false,
                "schema": "perfectpixel.normalize/1",
                "character": "legacy",
                "cellWidth": 1,
                "cellHeight": 1,
                "states": [],
                "gates": [],
                "errors": ["legacy gate failure"],
                "warnings": []
            }"#,
        )
        .expect("legacy normalize report");

        let (plan, _) = plan_generation_publication(
            &root,
            GenerationPublicationRequest {
                workflow: GenerationWorkflow::Bundle,
                artifacts: vec![GeneratedArtifact {
                    relative_path: "manifest.json".to_string(),
                    bytes: b"new bundle".to_vec(),
                }],
                invalidates: Vec::new(),
                input_snapshots: Vec::new(),
            },
        )
        .expect("initial migration");

        let authority_entry = plan
            .entries
            .iter()
            .find(|entry| entry.relative_path == Path::new(GENERATION_AUTHORITY_FILE))
            .expect("authority entry");
        let next = GenerationAuthority::from_slice(&authority_entry.bytes).expect("next authority");
        assert_eq!(
            next.generations
                .iter()
                .map(|record| record.workflow)
                .collect::<Vec<_>>(),
            vec![GenerationWorkflow::Normalize, GenerationWorkflow::Bundle]
        );
        assert!(plan.removals.is_empty());
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn modified_stale_artifact_aborts_before_a_plan_is_returned() {
        let root = temp_root("modified-stale");
        let original = b"original";
        let stale = GenerationArtifact::new(
            "old.txt".to_string(),
            sha256_hex(original),
            original.len() as u64,
        )
        .expect("stale record");
        let authority = GenerationAuthority::new(vec![GenerationRecord::new(
            GenerationWorkflow::Normalize,
            vec![stale],
        )
        .expect("record")])
        .expect("authority");
        fs::write(
            root.join(GENERATION_AUTHORITY_FILE),
            authority.to_pretty_json().expect("authority json"),
        )
        .expect("authority file");
        fs::write(root.join("old.txt"), b"modified").expect("modified stale");

        let error = plan_generation_publication(
            &root,
            GenerationPublicationRequest {
                workflow: GenerationWorkflow::Normalize,
                artifacts: vec![GeneratedArtifact {
                    relative_path: "normalize-report.json".to_string(),
                    bytes: b"report".to_vec(),
                }],
                invalidates: Vec::new(),
                input_snapshots: Vec::new(),
            },
        )
        .expect_err("modified stale must fail");

        assert!(error
            .to_string()
            .contains("changed since authority was written"));
        assert!(!root.join("normalize-report.json").exists());
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn authority_appearance_after_planning_aborts_before_mutation() {
        let root = temp_root("authority-appeared");
        let (_, guard) = plan_generation_publication(
            &root,
            GenerationPublicationRequest {
                workflow: GenerationWorkflow::Normalize,
                artifacts: vec![GeneratedArtifact {
                    relative_path: "normalize-report.json".to_string(),
                    bytes: b"report".to_vec(),
                }],
                invalidates: Vec::new(),
                input_snapshots: Vec::new(),
            },
        )
        .expect("publication plan");
        let foreign_authority = GenerationAuthority::new(Vec::new())
            .expect("empty authority")
            .to_pretty_json()
            .expect("authority JSON");
        fs::write(root.join(GENERATION_AUTHORITY_FILE), foreign_authority)
            .expect("foreign authority");

        let error =
            verify_generation_publication(&root, ArtifactSetConditionPhase::BeforeMutation, &guard)
                .expect_err("authority appearance must abort");

        assert!(error.to_string().contains("authority appeared"));
        assert!(!root.join("normalize-report.json").exists());
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn unowned_current_output_is_never_adopted_by_name() {
        let root = temp_root("unowned-current-output");
        fs::write(root.join("normalize-report.json"), b"foreign").expect("foreign output");

        let error = plan_generation_publication(
            &root,
            GenerationPublicationRequest {
                workflow: GenerationWorkflow::Normalize,
                artifacts: vec![GeneratedArtifact {
                    relative_path: "normalize-report.json".to_string(),
                    bytes: b"replacement".to_vec(),
                }],
                invalidates: Vec::new(),
                input_snapshots: Vec::new(),
            },
        )
        .expect_err("unowned output must block publication");

        assert!(error
            .to_string()
            .contains("not owned by generation authority"));
        assert_eq!(
            fs::read(root.join("normalize-report.json")).expect("foreign output remains"),
            b"foreign"
        );
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn missing_retained_generation_file_blocks_replacement() {
        let root = temp_root("missing-retained-output");
        let bundle = GenerationRecord::new(
            GenerationWorkflow::Bundle,
            vec![
                GenerationArtifact::new("manifest.json".to_string(), sha256_hex(b"manifest"), 8)
                    .expect("bundle artifact"),
            ],
        )
        .expect("bundle record");
        let authority = GenerationAuthority::new(vec![bundle]).expect("authority");
        fs::write(
            root.join(GENERATION_AUTHORITY_FILE),
            authority.to_pretty_json().expect("authority JSON"),
        )
        .expect("authority file");

        let error = plan_generation_publication(
            &root,
            GenerationPublicationRequest {
                workflow: GenerationWorkflow::Normalize,
                artifacts: vec![GeneratedArtifact {
                    relative_path: "normalize-report.json".to_string(),
                    bytes: b"report".to_vec(),
                }],
                invalidates: Vec::new(),
                input_snapshots: Vec::new(),
            },
        )
        .expect_err("missing retained artifact must fail closed");

        assert!(error
            .to_string()
            .contains("authority claims a missing managed artifact"));
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn before_commit_detects_modified_next_file_and_reappeared_stale_file() {
        let root = temp_root("commit-preconditions");
        let old = b"old";
        let old_record = GenerationRecord::new(
            GenerationWorkflow::Normalize,
            vec![
                GenerationArtifact::new("old.txt".to_string(), sha256_hex(old), old.len() as u64)
                    .expect("old artifact"),
            ],
        )
        .expect("old record");
        let authority = GenerationAuthority::new(vec![old_record]).expect("authority");
        fs::write(root.join("old.txt"), old).expect("old output");
        fs::write(
            root.join(GENERATION_AUTHORITY_FILE),
            authority.to_pretty_json().expect("authority JSON"),
        )
        .expect("authority file");

        let (plan, guard) = plan_generation_publication(
            &root,
            GenerationPublicationRequest {
                workflow: GenerationWorkflow::Normalize,
                artifacts: vec![GeneratedArtifact {
                    relative_path: "normalize-report.json".to_string(),
                    bytes: b"next".to_vec(),
                }],
                invalidates: Vec::new(),
                input_snapshots: Vec::new(),
            },
        )
        .expect("publication plan");
        for entry in &plan.entries {
            let destination = root.join(&entry.relative_path);
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent).expect("entry parent");
            }
            fs::write(destination, &entry.bytes).expect("planned entry");
        }
        for removal in &plan.removals {
            let path = root.join(removal);
            if path.exists() {
                fs::remove_file(path).expect("planned removal");
            }
        }

        fs::write(root.join("normalize-report.json"), b"tampered").expect("tampered next output");
        let modified_error =
            verify_generation_publication(&root, ArtifactSetConditionPhase::BeforeCommit, &guard)
                .expect_err("modified next file must block commit");
        assert!(modified_error
            .to_string()
            .contains("does not match the next generation authority"));

        fs::write(root.join("normalize-report.json"), b"next").expect("restore next output");
        fs::write(root.join("old.txt"), b"reappeared").expect("reappeared stale output");
        let stale_error =
            verify_generation_publication(&root, ArtifactSetConditionPhase::BeforeCommit, &guard)
                .expect_err("reappeared stale file must block commit");
        assert!(stale_error
            .to_string()
            .contains("stale generated output reappeared"));
        fs::remove_dir_all(root).expect("cleanup");
    }

    fn temp_root(label: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "perfectpixel-generation-adapter-{label}-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir(&root).expect("temp root");
        root
    }
}
