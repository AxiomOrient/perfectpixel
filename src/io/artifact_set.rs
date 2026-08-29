use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{PpError, PpResult};

// Schema /2 records path, byte count, and SHA-256 for every preexisting
// touched file. Unsupported schema /1 cannot be upgraded automatically because
// its backup bytes have no historical integrity evidence.
const JOURNAL_SCHEMA: &str = "perfectpixel.artifact-set-transaction/2";
const PREPARED_MARKER: &str = "prepared.json";
const BACKUPS_READY_MARKER: &str = "backups-ready.json";
const INSTALLED_MARKER: &str = "installed.json";
const ABORTED_MARKER: &str = "aborted.json";
const MAX_TEMP_ATTEMPTS: usize = 64;
const MAX_ARTIFACT_ENTRIES: usize = 4_096;
const MAX_ARTIFACT_BYTES: usize = 512 * 1024 * 1024;
const MAX_RELATIVE_PATH_BYTES: usize = 1_024;
const MAX_RELATIVE_PATH_DEPTH: usize = 16;
const MAX_TRANSACTION_TREE_ENTRIES: usize = MAX_ARTIFACT_ENTRIES * (MAX_RELATIVE_PATH_DEPTH + 1);
const MAX_JOURNAL_BYTES: usize = 1024 * 1024;
const FILE_COMPARE_BUFFER_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
struct ArtifactFileRevision {
    device: u64,
    inode: u64,
    byte_count: u64,
    modified_seconds: i64,
    modified_nanoseconds: i64,
    changed_seconds: i64,
    changed_nanoseconds: i64,
}

fn artifact_file_revision(metadata: &fs::Metadata) -> ArtifactFileRevision {
    use std::os::unix::fs::MetadataExt;

    ArtifactFileRevision {
        device: metadata.dev(),
        inode: metadata.ino(),
        byte_count: metadata.len(),
        modified_seconds: metadata.mtime(),
        modified_nanoseconds: metadata.mtime_nsec(),
        changed_seconds: metadata.ctime(),
        changed_nanoseconds: metadata.ctime_nsec(),
    }
}

/// One managed file to publish as part of an artifact-set transaction.
pub struct AtomicArtifactSetEntry<'a> {
    pub relative_path: &'a Path,
    pub bytes: &'a [u8],
}

/// One owned managed file to publish as part of a planned artifact-set transaction.
#[derive(Debug)]
pub struct AtomicArtifactSetOwnedEntry {
    pub relative_path: PathBuf,
    pub bytes: Vec<u8>,
}

/// Owned transaction plan created after the output-root publisher lock is held.
#[derive(Debug)]
pub struct AtomicArtifactSetOwnedPlan {
    pub entries: Vec<AtomicArtifactSetOwnedEntry>,
    pub removals: Vec<PathBuf>,
}

/// Explicit publication checkpoints for application-owned preconditions.
///
/// `BeforeMutation` runs after durable backups exist but before any managed
/// output is changed. `BeforeCommit` runs after installation and filesystem
/// synchronization but before the durable installed marker. A failed check at
/// the first checkpoint aborts without output mutation; a failed check at the
/// second checkpoint rolls the complete managed set back.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArtifactSetConditionPhase {
    BeforeMutation,
    BeforeCommit,
}

/// Publishes a recoverable managed file set while preserving unrelated files
/// below the output root.
///
/// The writer serializes publishers for the same root, stages every byte before
/// mutation, records a durable recovery journal, snapshots every existing
/// managed file, and rolls the complete set back when installation fails. It
/// does not provide generation-atomic visibility to readers that do not
/// participate in the publisher lock.
pub struct AtomicArtifactSetWriter;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ArtifactTransactionState {
    Prepared,
    BackupsReady,
    Installed,
    Aborted,
    Restored,
    RecoveryRequired,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ArtifactTransactionEvent {
    BackupsRecorded,
    InstallRecorded,
    PublicationAborted,
    RollbackRestored,
    RecoveryRequired,
}

#[derive(Debug)]
enum InstallMarkerOutcome {
    Durable,
    VisibleDurabilityUnconfirmed(PpError),
}

/// Pure state transition for the durable artifact-set protocol. Filesystem effects
/// write the corresponding journal marker before the caller emits its event.
fn reduce_artifact_transaction(
    state: ArtifactTransactionState,
    event: ArtifactTransactionEvent,
) -> Result<ArtifactTransactionState, &'static str> {
    match (state, event) {
        (ArtifactTransactionState::Prepared, ArtifactTransactionEvent::BackupsRecorded) => {
            Ok(ArtifactTransactionState::BackupsReady)
        }
        (ArtifactTransactionState::BackupsReady, ArtifactTransactionEvent::InstallRecorded) => {
            Ok(ArtifactTransactionState::Installed)
        }
        (
            ArtifactTransactionState::Prepared | ArtifactTransactionState::BackupsReady,
            ArtifactTransactionEvent::PublicationAborted,
        ) => Ok(ArtifactTransactionState::Aborted),
        (ArtifactTransactionState::BackupsReady, ArtifactTransactionEvent::RollbackRestored) => {
            Ok(ArtifactTransactionState::Restored)
        }
        (_, ArtifactTransactionEvent::RecoveryRequired) => {
            Ok(ArtifactTransactionState::RecoveryRequired)
        }
        _ => Err("event is not valid for the current artifact transaction state"),
    }
}

#[derive(Clone, Debug)]
struct ArtifactSetPlan {
    ordered_entry_indexes: Vec<usize>,
    writes: Vec<PathBuf>,
    removals: Vec<PathBuf>,
    touched: Vec<PathBuf>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ArtifactSetJournal {
    schema: String,
    root_name: String,
    root_existed: bool,
    writes: Vec<String>,
    removals: Vec<String>,
    backups: Vec<ArtifactSetBackupJournal>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ArtifactSetBackupJournal {
    path: String,
    byte_count: u64,
    sha256: String,
}

impl ArtifactSetJournal {
    fn prepared(root: &Path, root_existed: bool, plan: &ArtifactSetPlan) -> PpResult<Self> {
        Ok(Self {
            schema: JOURNAL_SCHEMA.to_owned(),
            root_name: root_file_name(root)?,
            root_existed,
            writes: journal_paths(&plan.writes)?,
            removals: journal_paths(&plan.removals)?,
            backups: Vec::new(),
        })
    }

    fn with_backups(&self, backups: Vec<ArtifactSetBackupJournal>) -> Self {
        let mut journal = self.clone();
        journal.backups = backups;
        journal
            .backups
            .sort_by(|left, right| left.path.cmp(&right.path));
        journal
    }

    fn validate(&self, root: &Path) -> PpResult<ArtifactSetRecoveryPlan> {
        if self.schema != JOURNAL_SCHEMA {
            return Err(transaction_error(
                root,
                ArtifactTransactionState::RecoveryRequired,
                format!("unsupported transaction journal schema '{}'", self.schema),
            ));
        }
        if self.root_name != root_file_name(root)? {
            return Err(transaction_error(
                root,
                ArtifactTransactionState::RecoveryRequired,
                "transaction journal belongs to a different output root",
            ));
        }

        let managed_count = self
            .writes
            .len()
            .checked_add(self.removals.len())
            .ok_or_else(|| {
                transaction_error(
                    root,
                    ArtifactTransactionState::RecoveryRequired,
                    "transaction journal artifact count overflow",
                )
            })?;
        if managed_count > MAX_ARTIFACT_ENTRIES || self.backups.len() > managed_count {
            return Err(transaction_error(
                root,
                ArtifactTransactionState::RecoveryRequired,
                "transaction journal exceeds artifact entry limit",
            ));
        }

        let writes = parse_journal_paths(root, "writes", &self.writes)?;
        let removals = parse_journal_paths(root, "removals", &self.removals)?;
        let write_set = writes.iter().cloned().collect::<BTreeSet<_>>();
        let removal_set = removals.iter().cloned().collect::<BTreeSet<_>>();
        let touched = write_set
            .union(&removal_set)
            .cloned()
            .collect::<BTreeSet<_>>();

        let mut backups = Vec::with_capacity(self.backups.len());
        let mut backup_paths = BTreeSet::new();
        let mut total_backup_bytes = 0u64;
        let mut previous_path: Option<&str> = None;
        for backup in &self.backups {
            if previous_path.is_some_and(|previous| previous >= backup.path.as_str()) {
                return Err(transaction_error(
                    root,
                    ArtifactTransactionState::RecoveryRequired,
                    "transaction journal backup records are not strictly sorted by path",
                ));
            }
            previous_path = Some(&backup.path);
            let relative_path = parse_journal_path(root, "backups", &backup.path)?;
            if !touched.contains(&relative_path) || !backup_paths.insert(relative_path.clone()) {
                return Err(transaction_error(
                    root,
                    ArtifactTransactionState::RecoveryRequired,
                    "transaction journal contains duplicate or foreign backup paths",
                ));
            }
            if !crate::core::sha256::is_sha256_hex(&backup.sha256) {
                return Err(transaction_error(
                    root,
                    ArtifactTransactionState::RecoveryRequired,
                    format!(
                        "transaction journal backup '{}' has an invalid SHA-256",
                        backup.path
                    ),
                ));
            }
            total_backup_bytes = total_backup_bytes
                .checked_add(backup.byte_count)
                .ok_or_else(|| {
                    transaction_error(
                        root,
                        ArtifactTransactionState::RecoveryRequired,
                        "transaction journal backup byte count overflow",
                    )
                })?;
            if total_backup_bytes > MAX_ARTIFACT_BYTES as u64 {
                return Err(transaction_error(
                    root,
                    ArtifactTransactionState::RecoveryRequired,
                    "transaction journal backups exceed byte limit",
                ));
            }
            backups.push(ArtifactSetBackupRecord {
                relative_path,
                byte_count: backup.byte_count,
                sha256: backup.sha256.clone(),
            });
        }

        if write_set.len() != writes.len()
            || removal_set.len() != removals.len()
            || !write_set.is_disjoint(&removal_set)
        {
            return Err(transaction_error(
                root,
                ArtifactTransactionState::RecoveryRequired,
                "transaction journal contains duplicate or overlapping paths",
            ));
        }
        for path in &touched {
            if path
                .ancestors()
                .skip(1)
                .any(|ancestor| touched.contains(ancestor))
            {
                return Err(transaction_error(
                    root,
                    ArtifactTransactionState::RecoveryRequired,
                    "transaction journal contains colliding ancestor paths",
                ));
            }
        }

        if !self.root_existed && !backups.is_empty() {
            return Err(transaction_error(
                root,
                ArtifactTransactionState::RecoveryRequired,
                "transaction journal records backups for an output root that did not exist",
            ));
        }

        Ok(ArtifactSetRecoveryPlan {
            root_existed: self.root_existed,
            writes,
            backups,
            touched: touched.into_iter().collect(),
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ArtifactSetBackupRecord {
    relative_path: PathBuf,
    byte_count: u64,
    sha256: String,
}

#[derive(Clone, Debug)]
struct ArtifactSetRecoveryPlan {
    root_existed: bool,
    writes: Vec<PathBuf>,
    backups: Vec<ArtifactSetBackupRecord>,
    touched: Vec<PathBuf>,
}

impl AtomicArtifactSetWriter {
    pub fn publish(
        root: impl AsRef<Path>,
        entries: &[AtomicArtifactSetEntry<'_>],
        removals: &[PathBuf],
    ) -> PpResult<()> {
        let mut ops = StdArtifactSetOps;
        publish_with_ops(root.as_ref(), entries, removals, &mut ops)
    }

    /// Builds the write/remove plan only after acquiring the output-root publisher
    /// lock and completing stale transaction recovery. Use this when the removal
    /// set depends on currently published authority state.
    pub fn publish_with_planner<F>(root: impl AsRef<Path>, planner: F) -> PpResult<()>
    where
        F: FnOnce(&Path) -> PpResult<AtomicArtifactSetOwnedPlan>,
    {
        let mut ops = StdArtifactSetOps;
        let mut condition = |_root: &Path, _phase: ArtifactSetConditionPhase, _context: &()| Ok(());
        publish_with_planner_ops(
            root.as_ref(),
            |locked_root| planner(locked_root).map(|plan| (plan, ())),
            &mut condition,
            false,
            &mut ops,
        )
    }

    /// Publishes a planned artifact set only while application-owned conditions
    /// remain true. The planner returns an opaque context retained by the
    /// transaction. The condition receives that context after durable backups
    /// and again after installation but before the durable commit marker. A
    /// failure before mutation aborts cleanly; a failure after mutation rolls the
    /// complete set back. This binds generated output to product-level state
    /// without moving domain policy into the filesystem transaction.
    pub fn publish_with_planner_checked<F, V, C>(
        root: impl AsRef<Path>,
        planner: F,
        mut condition: V,
    ) -> PpResult<()>
    where
        F: FnOnce(&Path) -> PpResult<(AtomicArtifactSetOwnedPlan, C)>,
        V: FnMut(&Path, ArtifactSetConditionPhase, &C) -> PpResult<()>,
    {
        let mut ops = StdArtifactSetOps;
        publish_with_planner_ops(root.as_ref(), planner, &mut condition, false, &mut ops)
    }

    /// Publishes an exact-directory file set and guarantees that the managed
    /// directory exists when the requested set is empty. Directory creation is
    /// completed under the same publisher lock as recovery, planning, mutation,
    /// and commit, so a zero-file generation has no second-lock race window.
    pub(super) fn publish_with_planner_checked_exact<F, V, C>(
        root: impl AsRef<Path>,
        planner: F,
        mut condition: V,
        ensure_empty_root: bool,
    ) -> PpResult<()>
    where
        F: FnOnce(&Path) -> PpResult<(AtomicArtifactSetOwnedPlan, C)>,
        V: FnMut(&Path, ArtifactSetConditionPhase, &C) -> PpResult<()>,
    {
        let mut ops = StdArtifactSetOps;
        publish_with_planner_ops(
            root.as_ref(),
            planner,
            &mut condition,
            ensure_empty_root,
            &mut ops,
        )
    }
}

trait ArtifactSetOps {
    fn copy(&mut self, from: &Path, to: &Path) -> std::io::Result<u64>;
    fn rename(&mut self, from: &Path, to: &Path) -> std::io::Result<()>;
    fn remove_file(&mut self, path: &Path) -> std::io::Result<()>;
    fn remove_dir_all(&mut self, path: &Path) -> std::io::Result<()>;
}

struct StdArtifactSetOps;

impl ArtifactSetOps for StdArtifactSetOps {
    fn copy(&mut self, from: &Path, to: &Path) -> std::io::Result<u64> {
        fs::copy(from, to)
    }

    fn rename(&mut self, from: &Path, to: &Path) -> std::io::Result<()> {
        fs::rename(from, to)
    }

    fn remove_file(&mut self, path: &Path) -> std::io::Result<()> {
        fs::remove_file(path)
    }

    fn remove_dir_all(&mut self, path: &Path) -> std::io::Result<()> {
        fs::remove_dir_all(path)
    }
}

struct ArtifactSetLock {
    _file: File,
}

fn publish_with_ops<O: ArtifactSetOps>(
    requested_root: &Path,
    entries: &[AtomicArtifactSetEntry<'_>],
    removals: &[PathBuf],
    ops: &mut O,
) -> PpResult<()> {
    validate_root(requested_root)?;
    let root = prepare_root_path(requested_root)?;
    let _lock = acquire_lock(&root)?;
    recover_stale_transactions(&root, ops)?;
    let mut condition = |_root: &Path, _phase: ArtifactSetConditionPhase, _context: &()| Ok(());
    publish_after_lock(&root, entries, removals, &(), &mut condition, ops)
}

fn publish_with_planner_ops<O, F, V, C>(
    requested_root: &Path,
    planner: F,
    condition: &mut V,
    ensure_root: bool,
    ops: &mut O,
) -> PpResult<()>
where
    O: ArtifactSetOps,
    F: FnOnce(&Path) -> PpResult<(AtomicArtifactSetOwnedPlan, C)>,
    V: FnMut(&Path, ArtifactSetConditionPhase, &C) -> PpResult<()>,
{
    validate_root(requested_root)?;
    let root = prepare_root_path(requested_root)?;
    let _lock = acquire_lock(&root)?;
    recover_stale_transactions(&root, ops)?;
    preflight_root(&root)?;

    let (owned, context) = planner(&root)?;
    let entries = owned
        .entries
        .iter()
        .map(|entry| AtomicArtifactSetEntry {
            relative_path: entry.relative_path.as_path(),
            bytes: entry.bytes.as_slice(),
        })
        .collect::<Vec<_>>();

    if entries.is_empty() && owned.removals.is_empty() {
        condition(&root, ArtifactSetConditionPhase::BeforeMutation, &context)?;
        let created_root = if ensure_root {
            ensure_root_exists(&root)?
        } else {
            false
        };
        if let Err(primary) = condition(&root, ArtifactSetConditionPhase::BeforeCommit, &context) {
            if created_root {
                return match rollback_empty_root_creation(&root) {
                    Ok(()) => Err(primary),
                    Err(rollback) => Err(transaction_error(
                        &root,
                        ArtifactTransactionState::RecoveryRequired,
                        format!(
                            "{primary}; failed to roll back empty output-root creation: {rollback}"
                        ),
                    )),
                };
            }
            return Err(primary);
        }
        return Ok(());
    }

    publish_after_lock(&root, &entries, &owned.removals, &context, condition, ops)?;
    if ensure_root {
        ensure_root_exists(&root)?;
    }
    Ok(())
}

fn publish_after_lock<O, V, C>(
    root: &Path,
    entries: &[AtomicArtifactSetEntry<'_>],
    removals: &[PathBuf],
    context: &C,
    condition: &mut V,
    ops: &mut O,
) -> PpResult<()>
where
    O: ArtifactSetOps,
    V: FnMut(&Path, ArtifactSetConditionPhase, &C) -> PpResult<()>,
{
    let plan = validate_artifact_set(entries, removals)?;
    preflight_targets(root, entries, removals)?;
    if entries.is_empty() && removals.is_empty() {
        return Ok(());
    }

    let root_existed =
        existing_directory(root, "artifact output root must be a non-symlink directory")?;
    preflight_journal_capacity(root, root_existed, &plan)?;
    let transaction_dir = create_transaction_dir(root)?;
    let mut transaction = ArtifactSetTransaction::prepare(
        root.to_path_buf(),
        transaction_dir,
        root_existed,
        plan,
        ops,
    )?;

    if let Err(error) = transaction.stage(entries) {
        return Err(transaction.abort_without_mutation(error));
    }
    if let Err(error) = transaction.snapshot_existing() {
        return Err(transaction.abort_without_mutation(error));
    }
    if let Err(error) = condition(root, ArtifactSetConditionPhase::BeforeMutation, context) {
        return Err(transaction.abort_before_install(error));
    }
    if let Err(error) = transaction.verify_snapshot_unchanged() {
        return Err(transaction.abort_before_install(error));
    }
    if let Err(error) = transaction.install_files(entries) {
        return Err(transaction.rollback(error));
    }
    if let Err(error) = condition(root, ArtifactSetConditionPhase::BeforeCommit, context) {
        return Err(transaction.rollback(error));
    }
    if let Err(error) = transaction.verify_installed_set(entries) {
        return Err(transaction.rollback(error));
    }
    match transaction.record_install() {
        Ok(InstallMarkerOutcome::Durable) => transaction.commit(),
        Ok(InstallMarkerOutcome::VisibleDurabilityUnconfirmed(error)) => {
            Err(transaction.retain_visible_commit(error))
        }
        Err(error) => Err(transaction.rollback(error)),
    }
}

struct ArtifactSetTransaction<'a, O: ArtifactSetOps> {
    state: ArtifactTransactionState,
    root: PathBuf,
    transaction_dir: PathBuf,
    stage_dir: PathBuf,
    backup_dir: PathBuf,
    plan: ArtifactSetPlan,
    journal: ArtifactSetJournal,
    preexisting: BTreeSet<PathBuf>,
    ops: &'a mut O,
}

impl<'a, O: ArtifactSetOps> ArtifactSetTransaction<'a, O> {
    fn prepare(
        root: PathBuf,
        transaction_dir: PathBuf,
        root_existed: bool,
        plan: ArtifactSetPlan,
        ops: &'a mut O,
    ) -> PpResult<Self> {
        let stage_dir = transaction_dir.join("stage");
        let backup_dir = transaction_dir.join("backup");
        let journal = ArtifactSetJournal::prepared(&root, root_existed, &plan)?;

        let result = (|| {
            fs::create_dir(&stage_dir).map_err(|source| file_error(&stage_dir, source))?;
            fs::create_dir(&backup_dir).map_err(|source| file_error(&backup_dir, source))?;
            write_marker(&transaction_dir, PREPARED_MARKER, &journal)?;
            let parent = transaction_dir
                .parent()
                .expect("artifact transaction directory has a parent");
            // The journal cannot be recovery authority after a power loss until
            // the transaction directory entry itself is durable in its parent.
            // No managed output mutation is permitted before this succeeds.
            sync_directory(parent)
        })();
        if let Err(primary) = result {
            return match remove_transaction_directory(ops, &transaction_dir) {
                Ok(()) => Err(primary),
                Err(cleanup) => Err(transaction_error(
                    &root,
                    ArtifactTransactionState::RecoveryRequired,
                    format!(
                        "{primary}; failed to durably clean incomplete transaction '{}': {cleanup}",
                        transaction_dir.display()
                    ),
                )),
            };
        }

        Ok(Self {
            state: ArtifactTransactionState::Prepared,
            root,
            transaction_dir,
            stage_dir,
            backup_dir,
            plan,
            journal,
            preexisting: BTreeSet::new(),
            ops,
        })
    }

    fn stage(&mut self, entries: &[AtomicArtifactSetEntry<'_>]) -> PpResult<()> {
        debug_assert_eq!(self.state, ArtifactTransactionState::Prepared);
        let stage_dir = &self.stage_dir;
        crate::io::parallel_map(&self.plan.ordered_entry_indexes, |&index| {
            let entry = &entries[index];
            let destination = stage_dir.join(entry.relative_path);
            let parent = destination
                .parent()
                .expect("validated artifact path has a parent");
            fs::create_dir_all(parent).map_err(|source| file_error(parent, source))?;
            write_new_file(&destination, entry.bytes)
        })?;
        sync_directory_tree(&self.stage_dir)?;
        sync_directory(&self.stage_dir)
    }

    fn snapshot_existing(&mut self) -> PpResult<()> {
        debug_assert_eq!(self.state, ArtifactTransactionState::Prepared);
        reject_blocked_managed_parents(&self.root, &self.plan.touched)?;
        let mut preexisting = Vec::new();
        let mut backups = Vec::new();
        let mut total_backup_bytes = 0u64;
        for relative_path in &self.plan.touched {
            let target = self.root.join(relative_path);
            match fs::symlink_metadata(&target) {
                Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
                    return Err(invalid_destination_error(
                        &target,
                        "managed artifact must be a non-symlink file",
                    ));
                }
                Ok(metadata) => {
                    let projected_backup_bytes = total_backup_bytes
                        .checked_add(metadata.len())
                        .ok_or_else(|| {
                            file_error(&target, "rollback backup byte count overflow")
                        })?;
                    if projected_backup_bytes > MAX_ARTIFACT_BYTES as u64 {
                        return Err(file_error(
                            &target,
                            "rollback backup set exceeds byte limit",
                        ));
                    }
                    let backup = self.backup_dir.join(relative_path);
                    let parent = backup
                        .parent()
                        .expect("validated artifact path has a parent");
                    fs::create_dir_all(parent).map_err(|source| file_error(parent, source))?;
                    durable_copy(self.ops, &target, &backup)?;
                    verify_regular_files_equal(
                        &target,
                        &backup,
                        "managed artifact changed while its rollback snapshot was captured",
                    )?;
                    let integrity = measure_regular_file(
                        &backup,
                        "rollback backup changed while its integrity was measured",
                    )?;
                    total_backup_bytes = total_backup_bytes
                        .checked_add(integrity.byte_count)
                        .ok_or_else(|| {
                            file_error(&backup, "rollback backup byte count overflow")
                        })?;
                    if total_backup_bytes > MAX_ARTIFACT_BYTES as u64 {
                        return Err(file_error(
                            &backup,
                            "rollback backup set exceeds byte limit",
                        ));
                    }
                    backups.push(ArtifactSetBackupJournal {
                        path: journal_path(relative_path)?,
                        byte_count: integrity.byte_count,
                        sha256: integrity.sha256,
                    });
                    preexisting.push(relative_path.clone());
                }
                Err(source) if source.kind() == std::io::ErrorKind::NotFound => {}
                Err(source) => return Err(file_error(&target, source)),
            }
        }
        sync_directory_tree(&self.backup_dir)?;
        sync_directory(&self.backup_dir)?;
        self.preexisting = preexisting.iter().cloned().collect();
        self.journal = self.journal.with_backups(backups);
        write_marker(&self.transaction_dir, BACKUPS_READY_MARKER, &self.journal)?;
        self.transition(ArtifactTransactionEvent::BackupsRecorded)?;
        Ok(())
    }

    fn install_files(&mut self, entries: &[AtomicArtifactSetEntry<'_>]) -> PpResult<()> {
        debug_assert_eq!(self.state, ArtifactTransactionState::BackupsReady);
        fs::create_dir_all(&self.root).map_err(|source| file_error(&self.root, source))?;
        reject_symlink_ancestors(&self.root)?;

        // `verify_snapshot_unchanged` already re-checked every touched path immediately
        // before this call with no intervening mutation (see `publish_after_lock`), so
        // re-verifying here would only read and hash the same bytes a second time for no
        // added safety.
        for index in &self.plan.ordered_entry_indexes {
            let entry = &entries[*index];
            let staged = self.stage_dir.join(entry.relative_path);
            let target = self.root.join(entry.relative_path);
            let parent = target
                .parent()
                .expect("validated artifact path has a parent");
            fs::create_dir_all(parent).map_err(|source| file_error(parent, source))?;
            reject_symlink_ancestors(parent)?;
            remove_existing_file(self.ops, &target)?;
            self.ops
                .rename(&staged, &target)
                .map_err(|source| file_error(&target, source))?;
        }

        for relative_path in &self.plan.removals {
            remove_existing_file(self.ops, &self.root.join(relative_path))?;
        }

        let mut directory_errors = Vec::new();
        for relative_path in self.plan.removals.iter().rev() {
            // An absent removal path does not grant ownership of a preexisting
            // empty parent directory. Only a file captured in the rollback
            // snapshot proves that the transaction actually removed content
            // from this branch; rollback can recreate that branch from backup.
            if !self.preexisting.contains(relative_path) {
                continue;
            }
            let target = self.root.join(relative_path);
            remove_empty_directories(
                target.parent(),
                Some(self.root.as_path()),
                &mut directory_errors,
            );
        }
        if !directory_errors.is_empty() {
            return Err(file_error(&self.root, directory_errors.join("; ")));
        }

        sync_managed_directories(&self.root, &self.plan.touched)
    }

    fn verify_snapshot_unchanged(&self) -> PpResult<()> {
        debug_assert_eq!(self.state, ArtifactTransactionState::BackupsReady);
        let recovery = self.journal.validate(&self.root)?;
        verify_backup_set(&self.transaction_dir, &recovery)?;
        // `snapshot_existing` already rejected any blocked managed-artifact parent, but that
        // was before the caller-supplied `BeforeMutation` condition ran; re-check here, the
        // last read-only checkpoint before `install_files` mutates, so a parent shape blocked
        // by that callback still surfaces as a clean rejection instead of a raw IO error.
        reject_blocked_managed_parents(&self.root, &self.plan.touched)?;
        let root = &self.root;
        let preexisting = &self.preexisting;
        let backup_dir = &self.backup_dir;
        crate::io::parallel_map(&self.plan.touched, |relative_path| {
            verify_target_unchanged(root, preexisting, backup_dir, relative_path)
        })?;
        Ok(())
    }

    fn verify_installed_set(&self, entries: &[AtomicArtifactSetEntry<'_>]) -> PpResult<()> {
        debug_assert_eq!(self.state, ArtifactTransactionState::BackupsReady);
        let root = &self.root;
        crate::io::parallel_map(&self.plan.ordered_entry_indexes, |&index| {
            let entry = &entries[index];
            verify_regular_file_bytes(
                &root.join(entry.relative_path),
                entry.bytes,
                "installed artifact bytes do not match the staged publication",
            )
        })?;
        crate::io::parallel_map(&self.plan.removals, |relative_path| {
            verify_path_absent(
                &root.join(relative_path),
                "removed artifact reappeared before publication commit",
            )
        })?;
        Ok(())
    }

    fn record_install(&mut self) -> PpResult<InstallMarkerOutcome> {
        debug_assert_eq!(self.state, ArtifactTransactionState::BackupsReady);
        let next_state =
            reduce_artifact_transaction(self.state, ArtifactTransactionEvent::InstallRecorded)
                .map_err(|reason| {
                    transaction_error(
                        &self.root,
                        self.state,
                        format!(
                            "invalid artifact transaction event {:?}: {reason}",
                            ArtifactTransactionEvent::InstallRecorded
                        ),
                    )
                })?;
        let outcome = write_install_marker(&self.transaction_dir, &self.journal)?;
        self.state = next_state;
        Ok(outcome)
    }

    fn retain_visible_commit(self, durability_error: PpError) -> PpError {
        debug_assert_eq!(self.state, ArtifactTransactionState::Installed);
        transaction_error(
            &self.root,
            self.state,
            format!(
                "publication files and installed marker are visible, but commit-marker durability confirmation failed: {durability_error}; the transaction was retained so the next locked publisher can resolve the terminal marker before retry"
            ),
        )
    }

    fn commit(self) -> PpResult<()> {
        debug_assert_eq!(self.state, ArtifactTransactionState::Installed);
        // `installed.json` is the durable commit point. Cleanup cannot roll the
        // committed generation back, but its failure must remain observable. If
        // the directory is still visible, the next locked publisher resolves it;
        // if only the parent sync failed, a crash may reveal the terminal journal
        // again and recovery will clean it idempotently.
        remove_transaction_directory(&mut *self.ops, &self.transaction_dir).map_err(|cleanup| {
            transaction_error(
                &self.root,
                self.state,
                format!(
                    "publication committed, but transaction cleanup durability was not confirmed: {cleanup}"
                ),
            )
        })
    }

    fn abort_without_mutation(self, primary: PpError) -> PpError {
        debug_assert_eq!(self.state, ArtifactTransactionState::Prepared);
        self.record_abort_and_cleanup(primary)
    }

    fn abort_before_install(self, primary: PpError) -> PpError {
        debug_assert_eq!(self.state, ArtifactTransactionState::BackupsReady);
        self.record_abort_and_cleanup(primary)
    }

    fn record_abort_and_cleanup(mut self, primary: PpError) -> PpError {
        let durable_abort = write_marker(&self.transaction_dir, ABORTED_MARKER, &self.journal);
        let transition = if durable_abort.is_ok() {
            self.transition(ArtifactTransactionEvent::PublicationAborted)
                .map_err(|error| error.to_string())
        } else {
            Ok(())
        };
        let cleanup = remove_transaction_directory(self.ops, &self.transaction_dir);

        match (durable_abort, transition, cleanup) {
            (Ok(()), Ok(()), Ok(())) => primary,
            (marker, state, cleanup) => {
                let mut failures = Vec::new();
                if let Err(error) = marker {
                    failures.push(format!("failed to record durable abort: {error}"));
                }
                if let Err(error) = state {
                    failures.push(format!("abort state transition failed: {error}"));
                }
                if let Err(error) = cleanup {
                    failures.push(format!("transaction cleanup failed: {error}"));
                }
                transaction_error(
                    &self.root,
                    ArtifactTransactionState::RecoveryRequired,
                    format!("{primary}; {}", failures.join("; ")),
                )
            }
        }
    }

    fn rollback(mut self, primary: PpError) -> PpError {
        let recovery = match self.journal.validate(&self.root) {
            Ok(recovery) => recovery,
            Err(error) => {
                if let Err(transition) = self.transition(ArtifactTransactionEvent::RecoveryRequired)
                {
                    return transaction_error(
                        &self.root,
                        self.state,
                        format!(
                            "{primary}; rollback journal validation failed: {error}; recovery transition failed: {transition}"
                        ),
                    );
                }
                return transaction_error(
                    &self.root,
                    self.state,
                    format!("{primary}; rollback journal validation failed: {error}"),
                );
            }
        };

        match rollback_from_journal(&self.root, &self.transaction_dir, &recovery, self.ops) {
            Ok(()) => {
                match self.transition(ArtifactTransactionEvent::RollbackRestored) {
                    Ok(()) => primary,
                    Err(transition) => transaction_error(
                        &self.root,
                        self.state,
                        format!("{primary}; rollback state transition failed: {transition}"),
                    ),
                }
            }
            Err(rollback) => {
                match self.transition(ArtifactTransactionEvent::RecoveryRequired) {
                    Ok(()) => transaction_error(
                        &self.root,
                        self.state,
                        format!("{primary}; rollback failed: {rollback}"),
                    ),
                    Err(transition) => transaction_error(
                        &self.root,
                        self.state,
                        format!(
                            "{primary}; rollback failed: {rollback}; recovery transition failed: {transition}"
                        ),
                    ),
                }
            }
        }
    }

    fn transition(&mut self, event: ArtifactTransactionEvent) -> PpResult<()> {
        let state = self.state;
        self.state = reduce_artifact_transaction(state, event).map_err(|reason| {
            transaction_error(
                &self.root,
                state,
                format!("invalid artifact transaction event {event:?}: {reason}"),
            )
        })?;
        Ok(())
    }
}

fn acquire_lock(root: &Path) -> PpResult<ArtifactSetLock> {
    let path = lock_path(root)?;
    match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            return Err(invalid_destination_error(
                &path,
                "artifact transaction lock must be a non-symlink file",
            ));
        }
        Ok(_) => {}
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {}
        Err(source) => return Err(file_error(&path, source)),
    }

    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .map_err(|source| file_error(&path, source))?;
    file.try_lock().map_err(|source| {
        let message = match source {
            std::fs::TryLockError::WouldBlock => {
                "another artifact publication is already in progress".to_owned()
            }
            std::fs::TryLockError::Error(source) => source.to_string(),
        };
        file_error(&path, message)
    })?;
    Ok(ArtifactSetLock { _file: file })
}

fn recover_stale_transactions<O: ArtifactSetOps>(root: &Path, ops: &mut O) -> PpResult<()> {
    let parent = root.parent().expect("prepared artifact root has a parent");
    let prefix = transaction_prefix(root)?;
    let mut transactions = Vec::new();
    for entry in fs::read_dir(parent).map_err(|source| file_error(parent, source))? {
        let entry = entry.map_err(|source| file_error(parent, source))?;
        if !is_transaction_name(&entry.file_name(), &prefix) {
            continue;
        }
        let file_type = entry
            .file_type()
            .map_err(|source| file_error(&entry.path(), source))?;
        if file_type.is_symlink() || !file_type.is_dir() {
            return Err(transaction_error(
                root,
                ArtifactTransactionState::RecoveryRequired,
                format!(
                    "transaction path '{}' is not a real directory",
                    entry.path().display()
                ),
            ));
        }
        transactions.push(entry.path());
    }
    transactions.sort();

    for transaction_dir in transactions {
        recover_transaction(root, &transaction_dir, ops)?;
    }
    Ok(())
}

fn recover_transaction<O: ArtifactSetOps>(
    root: &Path,
    transaction_dir: &Path,
    ops: &mut O,
) -> PpResult<()> {
    let installed = transaction_dir.join(INSTALLED_MARKER);
    let aborted = transaction_dir.join(ABORTED_MARKER);
    let backups_ready = transaction_dir.join(BACKUPS_READY_MARKER);
    let prepared = transaction_dir.join(PREPARED_MARKER);
    let installed_present = marker_present(&installed)?;
    let aborted_present = marker_present(&aborted)?;

    if installed_present && aborted_present {
        return Err(transaction_error(
            root,
            ArtifactTransactionState::RecoveryRequired,
            "transaction has conflicting installed and aborted terminal markers",
        ));
    }

    if installed_present {
        let journal = read_marker(root, &installed)?;
        journal.validate(root)?;
        return remove_transaction_directory(ops, transaction_dir).map_err(|error| {
            transaction_error(
                root,
                ArtifactTransactionState::RecoveryRequired,
                format!("failed to clean committed transaction: {error}"),
            )
        });
    }

    if aborted_present {
        let journal = read_marker(root, &aborted)?;
        journal.validate(root)?;
        return remove_transaction_directory(ops, transaction_dir).map_err(|error| {
            transaction_error(
                root,
                ArtifactTransactionState::RecoveryRequired,
                format!("failed to clean aborted transaction: {error}"),
            )
        });
    }

    if marker_present(&backups_ready)? {
        let journal = read_marker(root, &backups_ready)?;
        let recovery = journal.validate(root)?;
        return rollback_from_journal(root, transaction_dir, &recovery, ops).map_err(|error| {
            transaction_error(
                root,
                ArtifactTransactionState::RecoveryRequired,
                format!("failed to recover interrupted transaction: {error}"),
            )
        });
    }

    if marker_present(&prepared)? {
        let journal = read_marker(root, &prepared)?;
        journal.validate(root)?;
        return remove_transaction_directory(ops, transaction_dir).map_err(|error| {
            transaction_error(
                root,
                ArtifactTransactionState::RecoveryRequired,
                format!("failed to clean unmutated transaction: {error}"),
            )
        });
    }

    Err(transaction_error(
        root,
        ArtifactTransactionState::RecoveryRequired,
        format!(
            "transaction '{}' has no valid recovery marker",
            transaction_dir.display()
        ),
    ))
}

fn rollback_from_journal<O: ArtifactSetOps>(
    root: &Path,
    transaction_dir: &Path,
    recovery: &ArtifactSetRecoveryPlan,
    ops: &mut O,
) -> PpResult<()> {
    // Prove and durably restage the complete rollback source set before one
    // visible output path is changed. Missing or corrupt backup evidence leaves
    // the current generation untouched and the journal available for recovery.
    let restore_dir = restage_verified_backups(root, transaction_dir, recovery, ops)?;
    let backups_by_path = recovery
        .backups
        .iter()
        .map(|backup| (backup.relative_path.as_path(), backup))
        .collect::<BTreeMap<_, _>>();
    let write_paths = recovery
        .writes
        .iter()
        .map(PathBuf::as_path)
        .collect::<BTreeSet<_>>();
    let mut errors = Vec::new();

    // Mutate each managed path once. A path with a backup is restored; a write
    // that had no prior file is removed; an absent removal needs no action.
    // This preserves the smallest possible visible blast radius if the process
    // is interrupted during recovery, while the immutable backup set remains
    // available for the next recovery attempt.
    for relative_path in &recovery.touched {
        let target = root.join(relative_path);
        let Some(backup_record) = backups_by_path.get(relative_path.as_path()).copied() else {
            if write_paths.contains(relative_path.as_path()) {
                if let Err(error) = remove_existing_file(ops, &target) {
                    errors.push(error.to_string());
                    break;
                }
            }
            continue;
        };

        let staged_restore = restore_dir.join(relative_path);
        let target_parent = target
            .parent()
            .expect("validated artifact path has a parent");
        if let Err(error) = verify_regular_file_integrity(
            &staged_restore,
            backup_record,
            "restaged rollback backup changed before installation",
        ) {
            errors.push(error.to_string());
            break;
        }
        if let Err(source) = fs::create_dir_all(target_parent) {
            errors.push(file_error(target_parent, source).to_string());
            break;
        }
        if let Err(error) = reject_symlink_ancestors(target_parent) {
            errors.push(error.to_string());
            break;
        }
        if let Err(error) = remove_existing_file(ops, &target) {
            errors.push(error.to_string());
            break;
        }
        if let Err(source) = ops.rename(&staged_restore, &target) {
            errors.push(file_error(&target, source).to_string());
            break;
        }
        if let Err(error) = verify_regular_file_integrity(
            &target,
            backup_record,
            "restored artifact does not match its journaled rollback backup",
        ) {
            errors.push(error.to_string());
            break;
        }
    }

    if errors.is_empty() {
        if let Err(error) = sync_managed_directories(root, &recovery.touched) {
            errors.push(error.to_string());
        }
    }

    // A write with no backup proves the *file* did not exist before this
    // transaction, but the journal records no evidence about its *parent
    // directory* — an empty directory at that path could have predated the
    // transaction for unrelated reasons. Recovery runs from journal evidence
    // alone (possibly in a new process after a crash), so unlike
    // install_files' preexisting-backed removal pruning, there is no proof
    // available here that this recovery attempt owns the directory. Leaving a
    // stray empty directory behind is a cosmetic imperfection; deleting one
    // this attempt cannot prove it created is not an acceptable trade, so
    // recovery intentionally does not prune empty parent directories.

    if errors.is_empty() && !recovery.root_existed {
        match fs::remove_dir(root) {
            Ok(()) => {}
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => {}
            Err(source) if source.kind() == std::io::ErrorKind::DirectoryNotEmpty => {}
            Err(source) => errors.push(file_error(root, source).to_string()),
        }
    }

    if errors.is_empty() {
        if let Err(error) = remove_transaction_directory(ops, transaction_dir) {
            errors.push(error.to_string());
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(transaction_error(
            root,
            ArtifactTransactionState::RecoveryRequired,
            errors.join("; "),
        ))
    }
}

fn restage_verified_backups<O: ArtifactSetOps>(
    root: &Path,
    transaction_dir: &Path,
    recovery: &ArtifactSetRecoveryPlan,
    ops: &mut O,
) -> PpResult<PathBuf> {
    verify_backup_set(transaction_dir, recovery)?;

    let restore_dir = transaction_dir.join("restore");
    remove_directory_if_present(ops, &restore_dir)?;
    fs::create_dir(&restore_dir).map_err(|source| file_error(&restore_dir, source))?;

    for backup_record in &recovery.backups {
        let backup = transaction_dir
            .join("backup")
            .join(&backup_record.relative_path);
        let staged_restore = restore_dir.join(&backup_record.relative_path);
        let restore_parent = staged_restore
            .parent()
            .expect("validated artifact path has a parent");
        fs::create_dir_all(restore_parent).map_err(|source| file_error(restore_parent, source))?;

        verify_regular_file_integrity(
            &backup,
            backup_record,
            "rollback backup does not match its journaled integrity evidence",
        )?;
        durable_copy(ops, &backup, &staged_restore)?;
        verify_regular_file_integrity(
            &backup,
            backup_record,
            "rollback backup changed while it was restaged",
        )?;
        verify_regular_file_integrity(
            &staged_restore,
            backup_record,
            "restaged rollback backup does not match its journaled integrity evidence",
        )?;
    }

    sync_directory_tree(&restore_dir)?;
    sync_directory(&restore_dir)?;
    sync_directory(transaction_dir)?;
    verify_recorded_file_set(
        &restore_dir,
        recovery,
        "restaged rollback backup does not match its journaled integrity evidence",
    )?;
    verify_backup_set(transaction_dir, recovery).map_err(|error| {
        transaction_error(
            root,
            ArtifactTransactionState::RecoveryRequired,
            format!("rollback backup set changed after restaging: {error}"),
        )
    })?;
    Ok(restore_dir)
}

fn validate_root(root: &Path) -> PpResult<()> {
    if root.as_os_str().is_empty() || root.file_name().is_none() {
        return Err(file_error(
            root,
            "artifact output root must name a directory",
        ));
    }
    root_file_name(root).map(|_| ())
}

fn validate_artifact_set(
    entries: &[AtomicArtifactSetEntry<'_>],
    removals: &[PathBuf],
) -> PpResult<ArtifactSetPlan> {
    let managed_count = entries
        .len()
        .checked_add(removals.len())
        .ok_or_else(|| file_error(Path::new("<artifact-set>"), "artifact entry count overflow"))?;
    if managed_count > MAX_ARTIFACT_ENTRIES {
        return Err(file_error(
            Path::new("<artifact-set>"),
            "artifact set exceeds entry limit",
        ));
    }

    let mut write_set = BTreeSet::new();
    let mut total_bytes = 0usize;
    for entry in entries {
        validate_relative_path(entry.relative_path)?;
        if !write_set.insert(entry.relative_path.to_path_buf()) {
            return Err(file_error(entry.relative_path, "duplicate artifact entry"));
        }
        total_bytes = total_bytes
            .checked_add(entry.bytes.len())
            .ok_or_else(|| file_error(entry.relative_path, "artifact bytes overflow"))?;
        if total_bytes > MAX_ARTIFACT_BYTES {
            return Err(file_error(
                entry.relative_path,
                "artifact set exceeds byte limit",
            ));
        }
    }

    let mut removal_set = BTreeSet::new();
    for path in removals {
        validate_relative_path(path)?;
        if write_set.contains(path) {
            return Err(file_error(path, "artifact cannot be written and removed"));
        }
        if !removal_set.insert(path.clone()) {
            return Err(file_error(path, "duplicate artifact removal"));
        }
    }

    let touched = write_set.union(&removal_set).cloned().collect::<Vec<_>>();
    for path in &touched {
        if path
            .ancestors()
            .skip(1)
            .any(|ancestor| write_set.contains(ancestor) || removal_set.contains(ancestor))
        {
            return Err(file_error(
                path,
                "managed artifact path collides with another artifact",
            ));
        }
    }

    let mut ordered_entry_indexes = (0..entries.len()).collect::<Vec<_>>();
    ordered_entry_indexes.sort_by(|left, right| {
        entries[*left]
            .relative_path
            .cmp(entries[*right].relative_path)
    });

    Ok(ArtifactSetPlan {
        ordered_entry_indexes,
        writes: write_set.into_iter().collect(),
        removals: removal_set.into_iter().collect(),
        touched,
    })
}

fn prepare_root_path(requested_root: &Path) -> PpResult<PathBuf> {
    let parent = requested_root
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    reject_symlink_ancestors(parent)?;
    fs::create_dir_all(parent).map_err(|source| file_error(parent, source))?;
    reject_symlink_ancestors(parent)?;
    let canonical_parent = fs::canonicalize(parent).map_err(|source| file_error(parent, source))?;
    Ok(canonical_parent.join(
        requested_root
            .file_name()
            .expect("validated output root has a file name"),
    ))
}

fn ensure_root_exists(root: &Path) -> PpResult<bool> {
    match fs::symlink_metadata(root) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            return Err(invalid_destination_error(
                root,
                "artifact output root must be a non-symlink directory",
            ));
        }
        Ok(_) => return Ok(false),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {}
        Err(source) => return Err(file_error(root, source)),
    }

    fs::create_dir(root).map_err(|source| file_error(root, source))?;
    let durability: PpResult<()> = (|| {
        sync_directory(root)?;
        if let Some(parent) = root.parent() {
            sync_directory(parent)?;
        }
        Ok(())
    })();
    if let Err(error) = durability {
        return match rollback_empty_root_creation(root) {
            Ok(()) => Err(error),
            Err(rollback) => Err(PpError::FileIo {
                path: root.to_path_buf(),
                message: format!(
                    "empty directory creation failed durability confirmation ({error}), and rollback failed: {rollback}"
                ),
            }),
        };
    }
    Ok(true)
}

fn rollback_empty_root_creation(root: &Path) -> PpResult<()> {
    fs::remove_dir(root).map_err(|source| file_error(root, source))?;
    if let Some(parent) = root.parent() {
        sync_directory(parent)?;
    }
    Ok(())
}

fn preflight_root(root: &Path) -> PpResult<()> {
    match fs::symlink_metadata(root) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => Err(
            invalid_destination_error(root, "artifact output root must be a non-symlink directory"),
        ),
        Ok(_) => Ok(()),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(file_error(root, source)),
    }
}

fn preflight_targets(
    root: &Path,
    entries: &[AtomicArtifactSetEntry<'_>],
    removals: &[PathBuf],
) -> PpResult<()> {
    preflight_root(root)?;

    for relative_path in entries
        .iter()
        .map(|entry| entry.relative_path)
        .chain(removals.iter().map(PathBuf::as_path))
    {
        preflight_target(root, relative_path)?;
    }
    Ok(())
}

fn preflight_target(root: &Path, relative_path: &Path) -> PpResult<()> {
    let mut cursor = root.to_path_buf();
    let parent = relative_path.parent().unwrap_or_else(|| Path::new(""));
    for component in parent.components() {
        cursor.push(component.as_os_str());
        match fs::symlink_metadata(&cursor) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                return Err(invalid_destination_error(
                    &cursor,
                    "artifact parent must be a non-symlink directory",
                ));
            }
            Ok(_) => {}
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => {}
            Err(source) => return Err(file_error(&cursor, source)),
        }
    }

    let target = root.join(relative_path);
    match fs::symlink_metadata(&target) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => Err(
            invalid_destination_error(&target, "managed artifact must be a non-symlink file"),
        ),
        Ok(_) => Ok(()),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(file_error(&target, source)),
    }
}

pub(super) fn validate_relative_path(path: &Path) -> PpResult<()> {
    let value = path.to_str().ok_or_else(|| {
        file_error(
            path,
            "artifact path must be non-empty UTF-8 for recovery journaling",
        )
    })?;
    if value.is_empty() {
        return Err(file_error(
            path,
            "artifact path must be non-empty UTF-8 for recovery journaling",
        ));
    }
    if value.contains('\0') {
        return Err(file_error(path, "artifact path contains a NUL byte"));
    }
    if path.as_os_str().len() > MAX_RELATIVE_PATH_BYTES {
        return Err(file_error(path, "artifact path exceeds byte limit"));
    }
    let mut depth = 0usize;
    for component in path.components() {
        match component {
            Component::Normal(_) => depth += 1,
            _ => {
                return Err(file_error(
                    path,
                    "artifact path must be relative and normalized",
                ));
            }
        }
    }
    if depth == 0 || depth > MAX_RELATIVE_PATH_DEPTH {
        return Err(file_error(path, "artifact path exceeds depth limit"));
    }
    Ok(())
}

fn create_transaction_dir(root: &Path) -> PpResult<PathBuf> {
    for attempt in 0..MAX_TEMP_ATTEMPTS {
        let path = transaction_path(root, attempt)?;
        match fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(source) if source.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(source) => return Err(file_error(&path, source)),
        }
    }
    Err(file_error(
        root,
        format!("could not create artifact transaction after {MAX_TEMP_ATTEMPTS} attempts"),
    ))
}

fn transaction_path(root: &Path, attempt: usize) -> PpResult<PathBuf> {
    Ok(root.with_file_name(format!(
        "{}{}.{}",
        transaction_prefix(root)?,
        std::process::id(),
        attempt
    )))
}

fn transaction_prefix(root: &Path) -> PpResult<String> {
    Ok(format!(".{}.artifact-set.", root_file_name(root)?))
}

fn is_transaction_name(name: &std::ffi::OsStr, prefix: &str) -> bool {
    let Some(name) = name.to_str() else {
        return false;
    };
    let Some(suffix) = name.strip_prefix(prefix) else {
        return false;
    };
    let mut components = suffix.split('.');
    let (Some(pid), Some(attempt), None) =
        (components.next(), components.next(), components.next())
    else {
        return false;
    };
    !pid.is_empty()
        && pid.bytes().all(|byte| byte.is_ascii_digit())
        && !attempt.is_empty()
        && attempt.bytes().all(|byte| byte.is_ascii_digit())
}

fn lock_path(root: &Path) -> PpResult<PathBuf> {
    Ok(root.with_file_name(format!(".{}.artifact-set-lock", root_file_name(root)?)))
}

fn root_file_name(root: &Path) -> PpResult<String> {
    root.file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| file_error(root, "artifact output root must have a UTF-8 file name"))
}

fn preflight_journal_capacity(
    root: &Path,
    root_existed: bool,
    plan: &ArtifactSetPlan,
) -> PpResult<()> {
    let prepared = ArtifactSetJournal::prepared(root, root_existed, plan)?;
    let prepared_marker = root.with_file_name(format!(
        ".{}.artifact-set-preflight-{PREPARED_MARKER}",
        root_file_name(root)?
    ));
    serialize_marker(&prepared_marker, &prepared)?;

    // Reserve the largest journal shape before creating a transaction. Every
    // touched file may need a path, byte count, and complete SHA-256 record.
    let backups = plan
        .touched
        .iter()
        .map(|path| {
            Ok(ArtifactSetBackupJournal {
                path: journal_path(path)?,
                byte_count: u64::MAX,
                sha256: "f".repeat(64),
            })
        })
        .collect::<PpResult<Vec<_>>>()?;
    let backups_ready = prepared.with_backups(backups);
    let backups_marker = root.with_file_name(format!(
        ".{}.artifact-set-preflight-{BACKUPS_READY_MARKER}",
        root_file_name(root)?
    ));
    serialize_marker(&backups_marker, &backups_ready)?;
    Ok(())
}

fn serialize_marker(marker: &Path, journal: &ArtifactSetJournal) -> PpResult<Vec<u8>> {
    let bytes = serde_json::to_vec_pretty(journal).map_err(|source| PpError::Json {
        path: marker.to_path_buf(),
        message: source.to_string(),
    })?;
    if bytes.len() > MAX_JOURNAL_BYTES {
        return Err(file_error(marker, "transaction marker exceeds byte limit"));
    }
    Ok(bytes)
}

fn write_install_marker(
    transaction_dir: &Path,
    journal: &ArtifactSetJournal,
) -> PpResult<InstallMarkerOutcome> {
    write_install_marker_with_sync(transaction_dir, journal, sync_directory)
}

fn write_install_marker_with_sync<F>(
    transaction_dir: &Path,
    journal: &ArtifactSetJournal,
    mut synchronize: F,
) -> PpResult<InstallMarkerOutcome>
where
    F: FnMut(&Path) -> PpResult<()>,
{
    let marker = transaction_dir.join(INSTALLED_MARKER);
    if marker_present(&marker)? {
        return Err(file_error(&marker, "transaction marker already exists"));
    }
    let bytes = serialize_marker(&marker, journal)?;
    let temporary = transaction_dir.join(format!(".{INSTALLED_MARKER}.tmp"));
    write_new_file(&temporary, &bytes)?;
    fs::rename(&temporary, &marker).map_err(|source| file_error(&marker, source))?;
    match synchronize(transaction_dir) {
        Ok(()) => Ok(InstallMarkerOutcome::Durable),
        Err(error) => Ok(InstallMarkerOutcome::VisibleDurabilityUnconfirmed(error)),
    }
}

fn write_marker(
    transaction_dir: &Path,
    marker_name: &str,
    journal: &ArtifactSetJournal,
) -> PpResult<()> {
    let marker = transaction_dir.join(marker_name);
    if marker_present(&marker)? {
        return Err(file_error(&marker, "transaction marker already exists"));
    }
    let bytes = serialize_marker(&marker, journal)?;
    let temporary = transaction_dir.join(format!(".{marker_name}.tmp"));
    write_new_file(&temporary, &bytes)?;
    fs::rename(&temporary, &marker).map_err(|source| file_error(&marker, source))?;
    sync_directory(transaction_dir)
}

fn marker_present(path: &Path) -> PpResult<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(source) => Err(file_error(path, source)),
    }
}

fn read_marker(root: &Path, path: &Path) -> PpResult<ArtifactSetJournal> {
    let (mut file, revision) = open_regular_file_checked(
        path,
        "transaction marker must be a non-symlink file",
        "transaction marker changed while it was opened",
    )?;
    let mut bytes = Vec::new();
    (&mut file)
        .take(MAX_JOURNAL_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| file_error(path, source))?;
    if bytes.len() > MAX_JOURNAL_BYTES {
        return Err(file_error(path, "transaction marker exceeds byte limit"));
    }
    verify_open_file_unchanged(
        path,
        &file,
        &revision,
        "transaction marker changed while it was read",
    )?;
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct JournalSchemaProbe {
        schema: String,
    }
    let probe: JournalSchemaProbe =
        serde_json::from_slice(&bytes).map_err(|source| PpError::Json {
            path: path.to_path_buf(),
            message: source.to_string(),
        })?;
    if probe.schema != JOURNAL_SCHEMA {
        return Err(transaction_error(
            root,
            ArtifactTransactionState::RecoveryRequired,
            format!(
                "transaction marker '{}' uses unsupported schema '{}'; schema '{}' with backup integrity evidence is required, and an unverified legacy backup must not be auto-upgraded",
                path.display(), probe.schema, JOURNAL_SCHEMA
            ),
        ));
    }
    serde_json::from_slice(&bytes).map_err(|source| PpError::Json {
        path: path.to_path_buf(),
        message: source.to_string(),
    })
}

fn journal_paths(paths: &[PathBuf]) -> PpResult<Vec<String>> {
    paths.iter().map(|path| journal_path(path)).collect()
}

fn journal_path(path: &Path) -> PpResult<String> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| file_error(path, "artifact journal path must be UTF-8"))
}

fn parse_journal_paths(root: &Path, field: &str, values: &[String]) -> PpResult<Vec<PathBuf>> {
    let mut paths = Vec::with_capacity(values.len());
    for value in values {
        paths.push(parse_journal_path(root, field, value)?);
    }
    paths.sort();
    Ok(paths)
}

fn parse_journal_path(root: &Path, field: &str, value: &str) -> PpResult<PathBuf> {
    let path = PathBuf::from(value);
    validate_relative_path(&path).map_err(|error| {
        transaction_error(
            root,
            ArtifactTransactionState::RecoveryRequired,
            format!("invalid {field} path '{value}': {error}"),
        )
    })?;
    Ok(path)
}

fn write_new_file(path: &Path, bytes: &[u8]) -> PpResult<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|source| file_error(path, source))?;
    file.write_all(bytes)
        .map_err(|source| file_error(path, source))?;
    file.sync_all().map_err(|source| file_error(path, source))
}

fn durable_copy<O: ArtifactSetOps>(ops: &mut O, from: &Path, to: &Path) -> PpResult<()> {
    ops.copy(from, to)
        .map_err(|source| file_error(to, source))?;
    File::open(to)
        .and_then(|file| file.sync_all())
        .map_err(|source| file_error(to, source))
}

fn open_regular_file(path: &Path) -> PpResult<(File, ArtifactFileRevision)> {
    open_regular_file_checked(
        path,
        "managed artifact must be a non-symlink file",
        "managed artifact changed while it was opened",
    )
}

fn open_regular_file_checked(
    path: &Path,
    invalid_type_message: &str,
    changed_message: &str,
) -> PpResult<(File, ArtifactFileRevision)> {
    let path_metadata = fs::symlink_metadata(path).map_err(|source| file_error(path, source))?;
    if path_metadata.file_type().is_symlink() || !path_metadata.is_file() {
        return Err(invalid_destination_error(path, invalid_type_message));
    }

    let file = File::open(path).map_err(|source| file_error(path, source))?;
    let handle_metadata = file.metadata().map_err(|source| file_error(path, source))?;
    let revision = artifact_file_revision(&handle_metadata);
    if !handle_metadata.is_file() || artifact_file_revision(&path_metadata) != revision {
        return Err(file_error(path, changed_message));
    }
    Ok((file, revision))
}

fn verify_open_file_unchanged(
    path: &Path,
    file: &File,
    expected: &ArtifactFileRevision,
    message: &str,
) -> PpResult<()> {
    let handle_metadata = file.metadata().map_err(|source| file_error(path, source))?;
    let path_metadata = fs::symlink_metadata(path).map_err(|source| file_error(path, source))?;
    if path_metadata.file_type().is_symlink()
        || !path_metadata.is_file()
        || artifact_file_revision(&handle_metadata) != *expected
        || artifact_file_revision(&path_metadata) != *expected
    {
        return Err(file_error(path, message));
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RegularFileIntegrity {
    byte_count: u64,
    sha256: String,
}

fn measure_regular_file(path: &Path, message: &str) -> PpResult<RegularFileIntegrity> {
    let (mut file, revision) = open_regular_file(path)?;
    let mut digest = crate::core::sha256::Sha256State::new();
    let mut buffer = [0u8; FILE_COMPARE_BUFFER_BYTES];
    let mut byte_count = 0u64;
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|source| file_error(path, source))?;
        if count == 0 {
            break;
        }
        byte_count = byte_count
            .checked_add(count as u64)
            .ok_or_else(|| file_error(path, "file byte count overflow"))?;
        if byte_count > revision.byte_count {
            return Err(file_error(path, message));
        }
        digest.update(&buffer[..count]);
    }
    if byte_count != revision.byte_count {
        return Err(file_error(path, message));
    }
    verify_open_file_unchanged(path, &file, &revision, message)?;
    Ok(RegularFileIntegrity {
        byte_count,
        sha256: crate::core::sha256::hex_encode(digest.finalize()),
    })
}

fn verify_regular_file_integrity(
    path: &Path,
    expected: &ArtifactSetBackupRecord,
    message: &str,
) -> PpResult<()> {
    let observed = measure_regular_file(path, message)?;
    if observed.byte_count != expected.byte_count || observed.sha256 != expected.sha256 {
        return Err(file_error(path, message));
    }
    Ok(())
}

fn verify_backup_set(transaction_dir: &Path, recovery: &ArtifactSetRecoveryPlan) -> PpResult<()> {
    let backup_dir = transaction_dir.join("backup");
    verify_recorded_file_set(
        &backup_dir,
        recovery,
        "rollback backup does not match its journaled integrity evidence",
    )
}

fn verify_recorded_file_set(
    directory: &Path,
    recovery: &ArtifactSetRecoveryPlan,
    integrity_message: &str,
) -> PpResult<()> {
    let observed = collect_transaction_files(directory)?;
    let expected = recovery
        .backups
        .iter()
        .map(|backup| backup.relative_path.clone())
        .collect::<BTreeSet<_>>();
    if observed != expected {
        return Err(file_error(
            directory,
            "rollback file tree differs from the journaled backup set",
        ));
    }
    for backup_record in &recovery.backups {
        verify_regular_file_integrity(
            &directory.join(&backup_record.relative_path),
            backup_record,
            integrity_message,
        )?;
    }
    Ok(())
}

fn collect_transaction_files(root: &Path) -> PpResult<BTreeSet<PathBuf>> {
    let metadata = fs::symlink_metadata(root).map_err(|source| file_error(root, source))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(invalid_destination_error(
            root,
            "transaction backup root must be a non-symlink directory",
        ));
    }
    let mut files = BTreeSet::new();
    let mut pending = vec![root.to_path_buf()];
    let mut tree_entries = 0usize;
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory).map_err(|source| file_error(&directory, source))? {
            let entry = entry.map_err(|source| file_error(&directory, source))?;
            let path = entry.path();
            tree_entries = tree_entries
                .checked_add(1)
                .ok_or_else(|| file_error(root, "rollback backup tree entry count overflow"))?;
            if tree_entries > MAX_TRANSACTION_TREE_ENTRIES {
                return Err(file_error(root, "rollback backup tree exceeds entry limit"));
            }
            let relative = path
                .strip_prefix(root)
                .expect("transaction backup entry is below its traversal root")
                .to_path_buf();
            validate_relative_path(&relative)?;
            let file_type = entry
                .file_type()
                .map_err(|source| file_error(&path, source))?;
            if file_type.is_symlink() {
                return Err(invalid_destination_error(
                    &path,
                    "symlinks are not permitted in rollback backups",
                ));
            }
            if file_type.is_dir() {
                pending.push(path);
                continue;
            }
            if !file_type.is_file() {
                return Err(invalid_destination_error(
                    &path,
                    "rollback backup must contain only regular files and directories",
                ));
            }
            if !files.insert(relative) || files.len() > MAX_ARTIFACT_ENTRIES {
                return Err(file_error(root, "rollback backup tree exceeds entry limit"));
            }
        }
    }
    Ok(files)
}

fn verify_target_unchanged(
    root: &Path,
    preexisting: &BTreeSet<PathBuf>,
    backup_dir: &Path,
    relative_path: &Path,
) -> PpResult<()> {
    let target = root.join(relative_path);
    if preexisting.contains(relative_path) {
        let backup = backup_dir.join(relative_path);
        verify_regular_files_equal(
            &target,
            &backup,
            "managed artifact changed after its rollback snapshot was captured",
        )
    } else {
        verify_path_absent(
            &target,
            "managed artifact appeared after the transaction snapshot was captured",
        )
    }
}

fn verify_regular_files_equal(left: &Path, right: &Path, message: &str) -> PpResult<()> {
    let (mut left_file, left_revision) = open_regular_file(left)?;
    let (mut right_file, right_revision) = open_regular_file(right)?;
    if left_revision.byte_count != right_revision.byte_count {
        return Err(file_error(left, message));
    }

    let mut left_buffer = [0u8; FILE_COMPARE_BUFFER_BYTES];
    let mut right_buffer = [0u8; FILE_COMPARE_BUFFER_BYTES];
    let mut remaining = left_revision.byte_count;
    while remaining > 0 {
        let chunk = usize::try_from(remaining.min(FILE_COMPARE_BUFFER_BYTES as u64))
            .expect("bounded file comparison chunk fits usize");
        left_file
            .read_exact(&mut left_buffer[..chunk])
            .map_err(|source| file_error(left, source))?;
        right_file
            .read_exact(&mut right_buffer[..chunk])
            .map_err(|source| file_error(right, source))?;
        if left_buffer[..chunk] != right_buffer[..chunk] {
            return Err(file_error(left, message));
        }
        remaining -= chunk as u64;
    }

    verify_open_file_unchanged(left, &left_file, &left_revision, message)?;
    verify_open_file_unchanged(right, &right_file, &right_revision, message)
}

fn verify_regular_file_bytes(path: &Path, expected: &[u8], message: &str) -> PpResult<()> {
    let (mut file, revision) = open_regular_file(path)?;
    let expected_len = u64::try_from(expected.len())
        .map_err(|_| file_error(path, "expected artifact byte count overflow"))?;
    if revision.byte_count != expected_len {
        return Err(file_error(path, message));
    }

    let mut buffer = [0u8; FILE_COMPARE_BUFFER_BYTES];
    let mut offset = 0usize;
    while offset < expected.len() {
        let end = offset
            .checked_add(FILE_COMPARE_BUFFER_BYTES)
            .map(|value| value.min(expected.len()))
            .ok_or_else(|| file_error(path, "artifact comparison offset overflow"))?;
        file.read_exact(&mut buffer[..end - offset])
            .map_err(|source| file_error(path, source))?;
        if buffer[..end - offset] != expected[offset..end] {
            return Err(file_error(path, message));
        }
        offset = end;
    }

    verify_open_file_unchanged(path, &file, &revision, message)
}

fn verify_path_absent(path: &Path, message: &str) -> PpResult<()> {
    match fs::symlink_metadata(path) {
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(file_error(path, source)),
        Ok(_) => Err(file_error(path, message)),
    }
}

fn remove_existing_file<O: ArtifactSetOps>(ops: &mut O, path: &Path) -> PpResult<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => Err(
            invalid_destination_error(path, "managed artifact must be a non-symlink file"),
        ),
        Ok(_) => ops
            .remove_file(path)
            .map_err(|source| file_error(path, source)),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(file_error(path, source)),
    }
}

fn remove_directory_if_present<O: ArtifactSetOps>(ops: &mut O, path: &Path) -> PpResult<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => Err(
            invalid_destination_error(path, "transaction path must be a non-symlink directory"),
        ),
        Ok(_) => ops
            .remove_dir_all(path)
            .map_err(|source| file_error(path, source)),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(file_error(path, source)),
    }
}

fn remove_transaction_directory<O: ArtifactSetOps>(
    ops: &mut O,
    transaction_dir: &Path,
) -> PpResult<()> {
    remove_directory_if_present(ops, transaction_dir)?;
    if let Some(parent) = transaction_dir.parent() {
        sync_directory(parent)?;
    }
    Ok(())
}

pub(super) fn reject_symlink_ancestors(path: &Path) -> PpResult<()> {
    // Stop at the nearest existing directory: every missing descendant will be
    // created beneath that already-verified trust boundary.
    for ancestor in path.ancestors() {
        if ancestor.as_os_str().is_empty() {
            break;
        }
        match fs::symlink_metadata(ancestor) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                return Err(invalid_destination_error(
                    ancestor,
                    "destination parent must be a non-symlink directory",
                ));
            }
            Ok(_) => return Ok(()),
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => {}
            Err(source) => return Err(file_error(ancestor, source)),
        }
    }
    Ok(())
}

fn sync_directory_tree(path: &Path) -> PpResult<()> {
    for child in fs::read_dir(path).map_err(|source| file_error(path, source))? {
        let child = child.map_err(|source| file_error(path, source))?;
        let child_path = child.path();
        let file_type = child
            .file_type()
            .map_err(|source| file_error(&child_path, source))?;
        if file_type.is_symlink() {
            return Err(invalid_destination_error(
                &child_path,
                "symlinks are not permitted in artifact transactions",
            ));
        }
        if file_type.is_dir() {
            sync_directory_tree(&child_path)?;
            sync_directory(&child_path)?;
        }
    }
    Ok(())
}

fn sync_managed_directories(root: &Path, touched: &[PathBuf]) -> PpResult<()> {
    let mut directories = BTreeSet::new();
    if let Some(parent) = root.parent() {
        directories.insert(parent.to_path_buf());
    }
    if existing_directory(root, "artifact output root must be a non-symlink directory")? {
        directories.insert(root.to_path_buf());
    }
    for relative_path in touched {
        let target = root.join(relative_path);
        let mut current = target.parent();
        while let Some(directory) = current {
            if !directory.starts_with(root) {
                break;
            }
            if existing_directory(
                directory,
                "managed artifact parent must be a non-symlink directory",
            )? {
                directories.insert(directory.to_path_buf());
            }
            if directory == root {
                break;
            }
            current = directory.parent();
        }
    }
    let mut directories = directories.into_iter().collect::<Vec<_>>();
    directories.sort_by_key(|path| std::cmp::Reverse(path.components().count()));
    for directory in directories {
        sync_directory(&directory)?;
    }
    Ok(())
}

fn existing_directory(path: &Path, invalid_message: &str) -> PpResult<bool> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            Err(invalid_destination_error(path, invalid_message))
        }
        Ok(_) => Ok(true),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(source) => Err(file_error(path, source)),
    }
}

pub(super) fn sync_directory(path: &Path) -> PpResult<()> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|source| file_error(path, source))
}

fn remove_empty_directories(
    start: Option<&Path>,
    stop: Option<&Path>,
    rollback_errors: &mut Vec<String>,
) {
    let mut current = start;
    while let Some(path) = current {
        if stop.is_some_and(|boundary| path == boundary) {
            break;
        }
        match fs::remove_dir(path) {
            Ok(()) => current = path.parent(),
            Err(source)
                if source.kind() == std::io::ErrorKind::NotFound
                    || source.kind() == std::io::ErrorKind::DirectoryNotEmpty =>
            {
                break;
            }
            Err(source) => {
                rollback_errors.push(format!(
                    "remove empty directory '{}': {source}",
                    path.display()
                ));
                break;
            }
        }
    }
}

fn file_error(path: &Path, source: impl ToString) -> PpError {
    PpError::FileIo {
        path: path.to_path_buf(),
        message: source.to_string(),
    }
}

/// Every managed artifact parent that already exists must already be a
/// directory. Converting a regular file or a symlink into a managed directory
/// would assign implicit ownership of a path the product never wrote, so the
/// shape collision is rejected before any output is mutated instead of
/// surfacing as a raw create-directory failure at install time.
pub fn reject_blocked_managed_parents(root: &Path, touched: &[PathBuf]) -> PpResult<()> {
    let mut checked = BTreeSet::new();
    for relative_path in touched {
        // Shallowest first, so a blocking file is reported as a path-shape
        // rejection before any deeper probe can fail with a raw `ENOTDIR`.
        let ancestors = relative_path
            .ancestors()
            .skip(1)
            .filter(|ancestor| !ancestor.as_os_str().is_empty())
            .collect::<Vec<_>>();
        for ancestor in ancestors.into_iter().rev() {
            if !checked.insert(ancestor.to_path_buf()) {
                continue;
            }
            let target = root.join(ancestor);
            match fs::symlink_metadata(&target) {
                Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                    return Err(invalid_destination_error(
                        &target,
                        "managed artifact parent must be a non-symlink directory",
                    ));
                }
                Ok(_) => {}
                Err(source) if source.kind() == std::io::ErrorKind::NotFound => {}
                Err(source) => return Err(file_error(&target, source)),
            }
        }
    }
    Ok(())
}

fn invalid_destination_error(path: &Path, message: &str) -> PpError {
    PpError::InvalidRequest(format!("destination '{}': {message}", path.display()))
}

fn transaction_error(
    root: &Path,
    state: ArtifactTransactionState,
    message: impl ToString,
) -> PpError {
    PpError::FileIo {
        path: root.to_path_buf(),
        message: format!("artifact transaction {state:?}: {}", message.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    #[test]
    fn install_marker_sync_failure_retains_a_visible_terminal_marker() {
        let root = unique_temp_dir("artifact-set-visible-install-marker");
        let parent = root.parent().expect("root parent");
        fs::create_dir_all(parent).expect("parent");
        let transaction_dir = parent.join(format!(
            ".{}.artifact-set-test-marker",
            root.file_name().expect("root name").to_string_lossy()
        ));
        let _ = fs::remove_dir_all(&transaction_dir);
        fs::create_dir(&transaction_dir).expect("transaction directory");
        let plan = ArtifactSetPlan {
            ordered_entry_indexes: vec![0],
            writes: vec![PathBuf::from("output.txt")],
            removals: Vec::new(),
            touched: vec![PathBuf::from("output.txt")],
        };
        let journal = ArtifactSetJournal::prepared(&root, false, &plan).expect("journal");

        let outcome = write_install_marker_with_sync(&transaction_dir, &journal, |path| {
            Err(file_error(
                path,
                "simulated directory synchronization failure",
            ))
        })
        .expect("visible marker outcome");

        assert!(matches!(
            outcome,
            InstallMarkerOutcome::VisibleDurabilityUnconfirmed(_)
        ));
        assert!(transaction_dir.join(INSTALLED_MARKER).is_file());
        let _ = fs::remove_dir_all(transaction_dir);
    }

    #[test]
    fn publishes_complete_set_and_preserves_unrelated_files() {
        let root = unique_temp_dir("artifact-set-success");
        fs::create_dir_all(&root).expect("root");
        fs::write(root.join("old.txt"), b"old").expect("old");
        fs::write(root.join("notes.txt"), b"notes").expect("notes");
        let entries = [entry("current.txt", b"current")];
        let removals = [PathBuf::from("old.txt")];

        AtomicArtifactSetWriter::publish(&root, &entries, &removals).expect("publish");

        assert_eq!(
            fs::read(root.join("current.txt")).expect("current"),
            b"current"
        );
        assert!(!root.join("old.txt").exists());
        assert_eq!(fs::read(root.join("notes.txt")).expect("notes"), b"notes");
        assert_no_transaction_directories(&root);
        cleanup_case(&root);
    }

    #[test]
    fn planner_runs_after_recovery_and_lock_before_validation() {
        let root = unique_temp_dir("artifact-set-planner");
        let mut observed_parent = false;

        AtomicArtifactSetWriter::publish_with_planner(&root, |locked_root| {
            observed_parent = locked_root.parent().is_some_and(Path::exists);
            Ok(AtomicArtifactSetOwnedPlan {
                entries: vec![AtomicArtifactSetOwnedEntry {
                    relative_path: PathBuf::from("planned.txt"),
                    bytes: b"planned".to_vec(),
                }],
                removals: Vec::new(),
            })
        })
        .expect("planned publish");

        assert!(observed_parent);
        assert_eq!(
            fs::read(root.join("planned.txt")).expect("planned"),
            b"planned"
        );
        cleanup_case(&root);
    }

    #[test]
    fn checked_noop_publication_runs_both_guards_around_empty_root_creation() {
        let root = unique_temp_dir("artifact-set-checked-noop");
        let mut phases = Vec::new();

        AtomicArtifactSetWriter::publish_with_planner_checked_exact(
            &root,
            |_| {
                Ok((
                    AtomicArtifactSetOwnedPlan {
                        entries: Vec::new(),
                        removals: Vec::new(),
                    },
                    (),
                ))
            },
            |locked_root, phase, _context| {
                phases.push(phase);
                match phase {
                    ArtifactSetConditionPhase::BeforeMutation => {
                        assert!(!locked_root.exists());
                    }
                    ArtifactSetConditionPhase::BeforeCommit => {
                        assert!(locked_root.is_dir());
                    }
                }
                Ok(())
            },
            true,
        )
        .expect("checked empty publication");

        assert_eq!(
            phases,
            vec![
                ArtifactSetConditionPhase::BeforeMutation,
                ArtifactSetConditionPhase::BeforeCommit,
            ]
        );
        assert!(root.is_dir());
        cleanup_case(&root);
    }

    #[test]
    fn before_mutation_callback_creating_a_blocked_parent_still_fails_closed_cleanly() {
        let root = unique_temp_dir("artifact-set-before-mutation-blocked-parent");
        fs::create_dir_all(&root).expect("root");

        let error = AtomicArtifactSetWriter::publish_with_planner_checked_exact(
            &root,
            |_| {
                Ok((
                    AtomicArtifactSetOwnedPlan {
                        entries: vec![AtomicArtifactSetOwnedEntry {
                            relative_path: PathBuf::from("subdir/file.txt"),
                            bytes: b"content".to_vec(),
                        }],
                        removals: Vec::new(),
                    },
                    (),
                ))
            },
            |locked_root, phase, _context| {
                if phase == ArtifactSetConditionPhase::BeforeMutation {
                    // Simulate a caller precondition that creates a regular file at a path
                    // this transaction needs as a managed-artifact parent directory, after
                    // the pre-mutation snapshot's own shape check already passed.
                    fs::write(locked_root.join("subdir"), b"not a directory")
                        .expect("blocking file");
                }
                Ok(())
            },
            true,
        )
        .expect_err("blocked parent created during BeforeMutation must still fail closed");

        assert!(
            error
                .to_string()
                .contains("must be a non-symlink directory"),
            "expected the clean path-shape rejection, got: {error}"
        );
        assert!(!root.join("subdir/file.txt").exists());
        cleanup_case(&root);
    }

    #[test]
    fn checked_noop_failure_after_root_creation_rolls_the_empty_root_back() {
        let root = unique_temp_dir("artifact-set-checked-noop-rollback");

        let error = AtomicArtifactSetWriter::publish_with_planner_checked_exact(
            &root,
            |_| {
                Ok((
                    AtomicArtifactSetOwnedPlan {
                        entries: Vec::new(),
                        removals: Vec::new(),
                    },
                    (),
                ))
            },
            |_locked_root, phase, _context| match phase {
                ArtifactSetConditionPhase::BeforeMutation => Ok(()),
                ArtifactSetConditionPhase::BeforeCommit => Err(PpError::InvalidRequest(
                    "empty directory precondition changed".to_owned(),
                )),
            },
            true,
        )
        .expect_err("failed final guard must roll back a newly created empty root");

        assert!(error
            .to_string()
            .contains("empty directory precondition changed"));
        assert!(!root.exists());
        cleanup_case(&root);
    }

    #[test]
    fn failed_precondition_before_install_preserves_previous_generation() {
        let root = unique_temp_dir("artifact-set-precondition-before");
        fs::create_dir_all(&root).expect("root");
        fs::write(root.join("current.txt"), b"old").expect("old output");
        let mut checks = 0usize;

        let error = AtomicArtifactSetWriter::publish_with_planner_checked(
            &root,
            |_| {
                Ok((
                    AtomicArtifactSetOwnedPlan {
                        entries: vec![AtomicArtifactSetOwnedEntry {
                            relative_path: PathBuf::from("current.txt"),
                            bytes: b"new".to_vec(),
                        }],
                        removals: Vec::new(),
                    },
                    "captured revision",
                ))
            },
            |_root, phase, context| {
                checks += 1;
                assert_eq!(phase, ArtifactSetConditionPhase::BeforeMutation);
                assert_eq!(*context, "captured revision");
                Err(PpError::InvalidRequest(
                    "input revision changed".to_string(),
                ))
            },
        )
        .expect_err("precondition failure must abort publication");

        assert_eq!(checks, 1);
        assert!(error.to_string().contains("input revision changed"));
        assert_eq!(fs::read(root.join("current.txt")).expect("current"), b"old");
        assert_no_transaction_directories(&root);
        cleanup_case(&root);
    }

    #[test]
    fn target_created_after_backup_is_not_silently_overwritten() {
        let root = unique_temp_dir("artifact-set-created-after-backup");
        let root_for_condition = root.clone();

        let error = AtomicArtifactSetWriter::publish_with_planner_checked(
            &root,
            |_| {
                Ok((
                    AtomicArtifactSetOwnedPlan {
                        entries: vec![AtomicArtifactSetOwnedEntry {
                            relative_path: PathBuf::from("current.txt"),
                            bytes: b"generated".to_vec(),
                        }],
                        removals: Vec::new(),
                    },
                    (),
                ))
            },
            move |_root, phase, _context| {
                assert_eq!(phase, ArtifactSetConditionPhase::BeforeMutation);
                fs::create_dir_all(&root_for_condition).expect("root");
                fs::write(root_for_condition.join("current.txt"), b"external")
                    .expect("external output");
                Ok(())
            },
        )
        .expect_err("a target created after backup capture must abort");

        assert!(error
            .to_string()
            .contains("appeared after the transaction snapshot"));
        assert_eq!(
            fs::read(root.join("current.txt")).expect("external output"),
            b"external"
        );
        assert_no_transaction_directories(&root);
        cleanup_case(&root);
    }

    #[test]
    fn target_changed_after_backup_is_not_silently_overwritten() {
        let root = unique_temp_dir("artifact-set-changed-after-backup");
        fs::create_dir_all(&root).expect("root");
        fs::write(root.join("current.txt"), b"old").expect("old output");
        let root_for_condition = root.clone();

        let error = AtomicArtifactSetWriter::publish_with_planner_checked(
            &root,
            |_| {
                Ok((
                    AtomicArtifactSetOwnedPlan {
                        entries: vec![AtomicArtifactSetOwnedEntry {
                            relative_path: PathBuf::from("current.txt"),
                            bytes: b"generated".to_vec(),
                        }],
                        removals: Vec::new(),
                    },
                    (),
                ))
            },
            move |_root, phase, _context| {
                assert_eq!(phase, ArtifactSetConditionPhase::BeforeMutation);
                fs::write(root_for_condition.join("current.txt"), b"external")
                    .expect("external output");
                Ok(())
            },
        )
        .expect_err("a target changed after backup capture must abort");

        assert!(error
            .to_string()
            .contains("changed after its rollback snapshot"));
        assert_eq!(
            fs::read(root.join("current.txt")).expect("external output"),
            b"external"
        );
        assert_no_transaction_directories(&root);
        cleanup_case(&root);
    }

    #[test]
    fn failed_precondition_after_install_rolls_back_complete_generation() {
        let root = unique_temp_dir("artifact-set-precondition-after");
        fs::create_dir_all(&root).expect("root");
        fs::write(root.join("current.txt"), b"old").expect("old output");
        let mut checks = 0usize;

        let error = AtomicArtifactSetWriter::publish_with_planner_checked(
            &root,
            |_| {
                Ok((
                    AtomicArtifactSetOwnedPlan {
                        entries: vec![
                            AtomicArtifactSetOwnedEntry {
                                relative_path: PathBuf::from("current.txt"),
                                bytes: b"new".to_vec(),
                            },
                            AtomicArtifactSetOwnedEntry {
                                relative_path: PathBuf::from("fresh.txt"),
                                bytes: b"fresh".to_vec(),
                            },
                        ],
                        removals: Vec::new(),
                    },
                    (),
                ))
            },
            |_root, phase, _context| {
                checks += 1;
                if checks == 1 {
                    assert_eq!(phase, ArtifactSetConditionPhase::BeforeMutation);
                    Ok(())
                } else {
                    assert_eq!(phase, ArtifactSetConditionPhase::BeforeCommit);
                    Err(PpError::InvalidRequest(
                        "input revision changed".to_string(),
                    ))
                }
            },
        )
        .expect_err("post-install precondition failure must roll back");

        assert_eq!(checks, 2);
        assert!(error.to_string().contains("input revision changed"));
        assert_eq!(fs::read(root.join("current.txt")).expect("current"), b"old");
        assert!(!root.join("fresh.txt").exists());
        assert_no_transaction_directories(&root);
        cleanup_case(&root);
    }

    #[test]
    fn installed_output_changed_before_commit_rolls_back() {
        let root = unique_temp_dir("artifact-set-installed-output-changed");
        fs::create_dir_all(&root).expect("root");
        fs::write(root.join("current.txt"), b"old").expect("old output");
        let mut checks = 0usize;

        let error = AtomicArtifactSetWriter::publish_with_planner_checked(
            &root,
            |_| {
                Ok((
                    AtomicArtifactSetOwnedPlan {
                        entries: vec![AtomicArtifactSetOwnedEntry {
                            relative_path: PathBuf::from("current.txt"),
                            bytes: b"generated".to_vec(),
                        }],
                        removals: Vec::new(),
                    },
                    (),
                ))
            },
            |_root, phase, _context| {
                checks += 1;
                match phase {
                    ArtifactSetConditionPhase::BeforeMutation => Ok(()),
                    ArtifactSetConditionPhase::BeforeCommit => {
                        fs::write(root.join("current.txt"), b"external").expect("external output");
                        Ok(())
                    }
                }
            },
        )
        .expect_err("installed bytes changed before commit must roll back");

        assert_eq!(checks, 2);
        assert!(error.to_string().contains("staged publication"));
        assert_eq!(
            fs::read(root.join("current.txt")).expect("restored"),
            b"old"
        );
        assert_no_transaction_directories(&root);
        cleanup_case(&root);
    }

    #[test]
    fn durable_abort_recovery_cleans_without_restoring_unmodified_outputs() {
        let root = unique_temp_dir("artifact-set-abort-recovery");
        fs::create_dir_all(&root).expect("root");
        fs::write(root.join("current.txt"), b"old").expect("old output");
        let mut ops = FaultingOps::fail_commit_cleanup();
        let mut condition = |_root: &Path, phase: ArtifactSetConditionPhase, _context: &()| {
            assert_eq!(phase, ArtifactSetConditionPhase::BeforeMutation);
            Err(PpError::InvalidRequest(
                "input revision changed".to_string(),
            ))
        };

        let error = publish_with_planner_ops(
            &root,
            |_| {
                Ok((
                    AtomicArtifactSetOwnedPlan {
                        entries: vec![AtomicArtifactSetOwnedEntry {
                            relative_path: PathBuf::from("current.txt"),
                            bytes: b"new".to_vec(),
                        }],
                        removals: Vec::new(),
                    },
                    (),
                ))
            },
            &mut condition,
            false,
            &mut ops,
        )
        .expect_err("failed cleanup must leave a recoverable abort marker");

        assert!(error.to_string().contains("transaction cleanup failed"));
        assert_eq!(fs::read(root.join("current.txt")).expect("current"), b"old");
        let prefix = transaction_prefix(&root).expect("transaction prefix");
        let transaction_dir = fs::read_dir(root.parent().expect("parent"))
            .expect("transaction parent")
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .find(|path| {
                path.file_name()
                    .is_some_and(|name| name.to_string_lossy().starts_with(&prefix))
            })
            .expect("aborted transaction remains");
        assert!(transaction_dir.join(ABORTED_MARKER).is_file());

        // A non-cooperating writer may change an output after this publication
        // has aborted. Recovery must only clean the durable abort journal; it
        // must not replay backups for a transaction that never mutated output.
        fs::write(root.join("current.txt"), b"external").expect("external update");
        AtomicArtifactSetWriter::publish(&root, &[], &[]).expect("recover aborted journal");

        assert_eq!(
            fs::read(root.join("current.txt")).expect("current"),
            b"external"
        );
        assert_no_transaction_directories(&root);
        cleanup_case(&root);
    }

    #[test]
    fn conflicting_terminal_markers_fail_closed() {
        let root = unique_temp_dir("artifact-set-terminal-conflict");
        fs::create_dir_all(&root).expect("root");
        let entries = [entry("current.txt", b"current")];
        let plan = validate_artifact_set(&entries, &[]).expect("plan");
        let transaction_dir = create_transaction_dir(&root).expect("transaction");
        fs::create_dir(transaction_dir.join("stage")).expect("stage");
        fs::create_dir(transaction_dir.join("backup")).expect("backup");
        let journal = ArtifactSetJournal::prepared(&root, true, &plan).expect("journal");
        write_marker(&transaction_dir, PREPARED_MARKER, &journal).expect("prepared");
        write_marker(&transaction_dir, ABORTED_MARKER, &journal).expect("aborted");
        write_marker(&transaction_dir, INSTALLED_MARKER, &journal).expect("installed");

        let error = AtomicArtifactSetWriter::publish(&root, &[], &[])
            .expect_err("conflicting terminal markers must require operator recovery");

        assert!(error
            .to_string()
            .contains("conflicting installed and aborted"));
        assert!(transaction_dir.exists());
        cleanup_case(&root);
    }

    #[test]
    fn unrelated_prefix_directory_is_not_treated_as_a_transaction() {
        let root = unique_temp_dir("artifact-set-unrelated-prefix");
        let parent = root.parent().expect("parent");
        let prefix = transaction_prefix(&root).expect("prefix");
        let unrelated = parent.join(format!("{prefix}operator-notes"));
        fs::create_dir(&unrelated).expect("unrelated directory");

        AtomicArtifactSetWriter::publish(&root, &[], &[])
            .expect("unrelated prefix directory must be ignored");

        assert!(unrelated.is_dir());
        cleanup_case(&root);
        let _ = fs::remove_dir_all(unrelated);
    }

    #[test]
    fn absent_removal_does_not_delete_a_preexisting_empty_parent() {
        let root = unique_temp_dir("artifact-set-absent-removal-parent");
        let empty_parent = root.join("operator-empty");
        fs::create_dir_all(&empty_parent).expect("empty parent");
        let removals = [PathBuf::from("operator-empty/missing.txt")];

        AtomicArtifactSetWriter::publish(&root, &[], &removals)
            .expect("absent removal must not own the parent directory");

        assert!(empty_parent.is_dir());
        assert!(fs::read_dir(&empty_parent)
            .expect("empty parent")
            .next()
            .is_none());
        assert_no_transaction_directories(&root);
        cleanup_case(&root);
    }

    #[test]
    fn transaction_shaped_name_outside_local_attempt_range_fails_closed() {
        let root = unique_temp_dir("artifact-set-transaction-shape");
        let parent = root.parent().expect("parent");
        let prefix = transaction_prefix(&root).expect("prefix");
        let transaction = parent.join(format!("{prefix}123.999"));
        fs::write(&transaction, b"not a directory").expect("transaction-shaped file");

        let error = AtomicArtifactSetWriter::publish(&root, &[], &[])
            .expect_err("transaction-shaped names must not be ignored");

        assert!(error.to_string().contains("is not a real directory"));
        let _ = fs::remove_file(transaction);
        cleanup_case(&root);
    }

    #[test]
    fn transaction_shaped_numeric_overflow_name_fails_closed() {
        let root = unique_temp_dir("artifact-set-transaction-overflow-shape");
        let parent = root.parent().expect("parent");
        let prefix = transaction_prefix(&root).expect("prefix");
        let transaction = parent.join(format!(
            "{prefix}999999999999999999999999999999.999999999999999999999999999999"
        ));
        fs::write(&transaction, b"not a directory").expect("transaction-shaped file");

        let error = AtomicArtifactSetWriter::publish(&root, &[], &[])
            .expect_err("numeric transaction-shaped names must fail closed even when values overflow native integers");

        assert!(error.to_string().contains("is not a real directory"));
        let _ = fs::remove_file(transaction);
        cleanup_case(&root);
    }

    #[test]
    fn artifact_path_rejects_nul_before_creating_a_transaction() {
        let root = unique_temp_dir("artifact-set-nul-path");
        let invalid = PathBuf::from("broken\0name");
        let entries = [AtomicArtifactSetEntry {
            relative_path: &invalid,
            bytes: b"payload",
        }];

        let error = AtomicArtifactSetWriter::publish(&root, &entries, &[])
            .expect_err("NUL path must be rejected");

        assert!(error.to_string().contains("NUL byte"));
        assert_no_transaction_directories(&root);
        cleanup_case(&root);
    }

    #[test]
    fn exact_transaction_name_with_non_directory_type_fails_closed() {
        let root = unique_temp_dir("artifact-set-transaction-type");
        let transaction = transaction_path(&root, 0).expect("transaction path");
        fs::write(&transaction, b"not a directory").expect("transaction-shaped file");

        let error = AtomicArtifactSetWriter::publish(&root, &[], &[])
            .expect_err("transaction-shaped file must block recovery");

        assert!(error.to_string().contains("is not a real directory"));
        let _ = fs::remove_file(transaction);
        cleanup_case(&root);
    }

    #[test]
    fn planner_does_not_run_while_output_root_is_locked() {
        let root = unique_temp_dir("artifact-set-planner-lock");
        let prepared_root = prepare_root_path(&root).expect("prepared root");
        let lock_path = lock_path(&prepared_root).expect("lock path");
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .expect("lock file");
        lock.try_lock().expect("exclusive lock");
        let planner_called = std::cell::Cell::new(false);

        let error = AtomicArtifactSetWriter::publish_with_planner(&root, |_| {
            planner_called.set(true);
            Ok(AtomicArtifactSetOwnedPlan {
                entries: Vec::new(),
                removals: Vec::new(),
            })
        })
        .expect_err("concurrent publication must fail");

        assert!(!planner_called.get());
        assert!(error
            .to_string()
            .contains("another artifact publication is already in progress"));
        drop(lock);
        cleanup_case(&root);
    }

    #[test]
    fn planner_does_not_follow_symlink_output_root() {
        use std::os::unix::fs::symlink;

        let target = unique_temp_dir("artifact-set-planner-target");
        let root = unique_temp_dir("artifact-set-planner-symlink");
        fs::create_dir_all(&target).expect("target root");
        symlink(&target, &root).expect("output-root symlink");
        let planner_called = std::cell::Cell::new(false);

        let error = AtomicArtifactSetWriter::publish_with_planner(&root, |_| {
            planner_called.set(true);
            Ok(AtomicArtifactSetOwnedPlan {
                entries: Vec::new(),
                removals: Vec::new(),
            })
        })
        .expect_err("symlink output root must fail before planning");

        assert!(!planner_called.get());
        assert!(error
            .to_string()
            .contains("artifact output root must be a non-symlink directory"));
        fs::remove_file(&root).expect("remove output-root symlink");
        cleanup_case(&root);
        cleanup_case(&target);
    }

    #[test]
    fn first_publish_failure_removes_every_new_file() {
        let root = unique_temp_dir("artifact-set-first-failure");
        let entries = [entry("a.txt", b"new-a"), entry("b.txt", b"new-b")];
        let mut ops = FaultingOps::fail_rename_at(2);

        publish_with_ops(&root, &entries, &[], &mut ops).expect_err("second install fails");

        assert!(!root.join("a.txt").exists());
        assert!(!root.join("b.txt").exists());
        assert_no_transaction_directories(&root);
        cleanup_case(&root);
    }

    #[test]
    fn rerun_failure_restores_all_previous_bytes() {
        let root = unique_temp_dir("artifact-set-rerun-failure");
        fs::create_dir_all(&root).expect("root");
        fs::write(root.join("a.txt"), b"old-a").expect("old a");
        fs::write(root.join("b.txt"), b"old-b").expect("old b");
        fs::write(root.join("notes.txt"), b"notes").expect("notes");
        let entries = [
            entry("a.txt", b"new-a"),
            entry("b.txt", b"new-b"),
            entry("c.txt", b"new-c"),
        ];
        let mut ops = FaultingOps::fail_rename_at(2);

        publish_with_ops(&root, &entries, &[], &mut ops).expect_err("second install fails");

        assert_eq!(fs::read(root.join("a.txt")).expect("a"), b"old-a");
        assert_eq!(fs::read(root.join("b.txt")).expect("b"), b"old-b");
        assert!(!root.join("c.txt").exists());
        assert_eq!(fs::read(root.join("notes.txt")).expect("notes"), b"notes");
        assert_no_transaction_directories(&root);
        cleanup_case(&root);
    }

    #[test]
    fn backup_failure_leaves_targets_unchanged() {
        let root = unique_temp_dir("artifact-set-backup-failure");
        fs::create_dir_all(&root).expect("root");
        fs::write(root.join("current.txt"), b"old-current").expect("current");
        fs::write(root.join("stale.txt"), b"old-stale").expect("stale");
        let entries = [entry("current.txt", b"new-current")];
        let removals = [PathBuf::from("stale.txt")];
        let mut ops = FaultingOps::fail_copy_at(2);

        publish_with_ops(&root, &entries, &removals, &mut ops).expect_err("second backup fails");

        assert_eq!(
            fs::read(root.join("current.txt")).expect("current"),
            b"old-current"
        );
        assert_eq!(
            fs::read(root.join("stale.txt")).expect("stale"),
            b"old-stale"
        );
        assert_no_transaction_directories(&root);
        cleanup_case(&root);
    }

    #[test]
    fn rollback_cleanup_failure_is_not_hidden() {
        let root = unique_temp_dir("artifact-set-rollback-failure");
        let entries = [entry("a.txt", b"new-a"), entry("b.txt", b"new-b")];
        let mut ops = FaultingOps::fail_install_and_rollback_cleanup();

        let error =
            publish_with_ops(&root, &entries, &[], &mut ops).expect_err("rollback cleanup failure");

        assert!(error.to_string().contains("rollback failed"));
        assert!(error.to_string().contains("injected remove failure"));
        cleanup_case(&root);
    }

    #[test]
    fn commit_cleanup_failure_is_reported_without_rolling_back_installed_outputs() {
        let root = unique_temp_dir("artifact-set-commit-cleanup");
        let entries = [entry("current.txt", b"current")];
        let mut ops = FaultingOps::fail_commit_cleanup();

        let error = publish_with_ops(&root, &entries, &[], &mut ops)
            .expect_err("committed output with failed cleanup must be reported");

        assert!(error.to_string().contains("publication committed"));
        assert!(error.to_string().contains("cleanup durability"));
        assert_eq!(
            fs::read(root.join("current.txt")).expect("current"),
            b"current"
        );
        AtomicArtifactSetWriter::publish(&root, &[], &[]).expect("recover committed journal");
        assert_no_transaction_directories(&root);
        cleanup_case(&root);
    }

    #[test]
    fn interrupted_install_is_recovered_before_next_publication() {
        let root = unique_temp_dir("artifact-set-recovery");
        fs::create_dir_all(&root).expect("root");
        fs::write(root.join("a.txt"), b"old-a").expect("old a");
        let entries = [entry("a.txt", b"new-a"), entry("b.txt", b"new-b")];
        let plan = validate_artifact_set(&entries, &[]).expect("plan");
        let transaction_dir = create_transaction_dir(&root).expect("transaction");
        fs::create_dir(transaction_dir.join("stage")).expect("stage");
        fs::create_dir(transaction_dir.join("backup")).expect("backup");
        let prepared = ArtifactSetJournal::prepared(&root, true, &plan).expect("prepared");
        write_marker(&transaction_dir, PREPARED_MARKER, &prepared).expect("prepared marker");
        let backup = transaction_dir.join("backup/a.txt");
        fs::copy(root.join("a.txt"), &backup).expect("backup");
        let ready = prepared.with_backups(vec![backup_journal("a.txt", &backup)]);
        write_marker(&transaction_dir, BACKUPS_READY_MARKER, &ready).expect("ready marker");
        fs::write(root.join("a.txt"), b"new-a").expect("new a");
        fs::write(root.join("b.txt"), b"new-b").expect("new b");

        AtomicArtifactSetWriter::publish(&root, &[], &[]).expect("recover");

        assert_eq!(fs::read(root.join("a.txt")).expect("a"), b"old-a");
        assert!(!root.join("b.txt").exists());
        assert_no_transaction_directories(&root);
        cleanup_case(&root);
    }

    #[test]
    fn corrupt_backup_blocks_recovery_before_any_output_mutation() {
        let root = unique_temp_dir("artifact-set-corrupt-backup");
        fs::create_dir_all(&root).expect("root");
        fs::write(root.join("a.txt"), b"old-a").expect("old a");
        fs::write(root.join("b.txt"), b"old-b").expect("old b");
        let entries = [entry("a.txt", b"new-a"), entry("b.txt", b"new-b")];
        let plan = validate_artifact_set(&entries, &[]).expect("plan");
        let transaction_dir = create_transaction_dir(&root).expect("transaction");
        fs::create_dir(transaction_dir.join("stage")).expect("stage");
        fs::create_dir(transaction_dir.join("backup")).expect("backup");
        let prepared = ArtifactSetJournal::prepared(&root, true, &plan).expect("prepared");
        write_marker(&transaction_dir, PREPARED_MARKER, &prepared).expect("prepared marker");

        let backup_a = transaction_dir.join("backup/a.txt");
        let backup_b = transaction_dir.join("backup/b.txt");
        fs::copy(root.join("a.txt"), &backup_a).expect("backup a");
        fs::copy(root.join("b.txt"), &backup_b).expect("backup b");
        let ready = prepared.with_backups(vec![
            backup_journal("a.txt", &backup_a),
            backup_journal("b.txt", &backup_b),
        ]);
        write_marker(&transaction_dir, BACKUPS_READY_MARKER, &ready).expect("ready marker");

        fs::write(root.join("a.txt"), b"new-a").expect("new a");
        fs::write(root.join("b.txt"), b"new-b").expect("new b");
        fs::write(&backup_b, b"corrupt").expect("corrupt backup");

        let error = AtomicArtifactSetWriter::publish(&root, &[], &[])
            .expect_err("corrupt rollback source must fail closed");

        assert!(error.to_string().contains("journaled integrity evidence"));
        assert_eq!(fs::read(root.join("a.txt")).expect("a"), b"new-a");
        assert_eq!(fs::read(root.join("b.txt")).expect("b"), b"new-b");
        assert!(transaction_dir.exists());
        cleanup_case(&root);
    }

    #[test]
    fn missing_backup_blocks_recovery_before_any_output_mutation() {
        let root = unique_temp_dir("artifact-set-missing-backup");
        fs::create_dir_all(&root).expect("root");
        fs::write(root.join("a.txt"), b"old-a").expect("old a");
        fs::write(root.join("b.txt"), b"old-b").expect("old b");
        let entries = [entry("a.txt", b"new-a"), entry("b.txt", b"new-b")];
        let plan = validate_artifact_set(&entries, &[]).expect("plan");
        let transaction_dir = create_transaction_dir(&root).expect("transaction");
        fs::create_dir(transaction_dir.join("stage")).expect("stage");
        fs::create_dir(transaction_dir.join("backup")).expect("backup");
        let prepared = ArtifactSetJournal::prepared(&root, true, &plan).expect("prepared");
        write_marker(&transaction_dir, PREPARED_MARKER, &prepared).expect("prepared marker");

        let backup_a = transaction_dir.join("backup/a.txt");
        let backup_b = transaction_dir.join("backup/b.txt");
        fs::copy(root.join("a.txt"), &backup_a).expect("backup a");
        fs::copy(root.join("b.txt"), &backup_b).expect("backup b");
        let ready = prepared.with_backups(vec![
            backup_journal("a.txt", &backup_a),
            backup_journal("b.txt", &backup_b),
        ]);
        write_marker(&transaction_dir, BACKUPS_READY_MARKER, &ready).expect("ready marker");

        fs::write(root.join("a.txt"), b"new-a").expect("new a");
        fs::write(root.join("b.txt"), b"new-b").expect("new b");
        fs::remove_file(&backup_b).expect("remove backup b");

        let error = AtomicArtifactSetWriter::publish(&root, &[], &[])
            .expect_err("incomplete rollback source must fail closed");

        assert!(error.to_string().contains("rollback file tree differs"));
        assert_eq!(fs::read(root.join("a.txt")).expect("a"), b"new-a");
        assert_eq!(fs::read(root.join("b.txt")).expect("b"), b"new-b");
        assert!(transaction_dir.exists());
        cleanup_case(&root);
    }

    #[test]
    fn extra_backup_blocks_recovery_before_any_output_mutation() {
        let root = unique_temp_dir("artifact-set-extra-backup");
        fs::create_dir_all(&root).expect("root");
        fs::write(root.join("a.txt"), b"old-a").expect("old a");
        let entries = [entry("a.txt", b"new-a")];
        let plan = validate_artifact_set(&entries, &[]).expect("plan");
        let transaction_dir = create_transaction_dir(&root).expect("transaction");
        fs::create_dir(transaction_dir.join("stage")).expect("stage");
        fs::create_dir(transaction_dir.join("backup")).expect("backup");
        let prepared = ArtifactSetJournal::prepared(&root, true, &plan).expect("prepared");
        write_marker(&transaction_dir, PREPARED_MARKER, &prepared).expect("prepared marker");

        let backup_a = transaction_dir.join("backup/a.txt");
        fs::copy(root.join("a.txt"), &backup_a).expect("backup a");
        let ready = prepared.with_backups(vec![backup_journal("a.txt", &backup_a)]);
        write_marker(&transaction_dir, BACKUPS_READY_MARKER, &ready).expect("ready marker");

        fs::write(root.join("a.txt"), b"new-a").expect("new a");
        fs::write(transaction_dir.join("backup/foreign.txt"), b"foreign").expect("extra backup");

        let error = AtomicArtifactSetWriter::publish(&root, &[], &[])
            .expect_err("foreign rollback source must fail closed");

        assert!(error.to_string().contains("rollback file tree differs"));
        assert_eq!(fs::read(root.join("a.txt")).expect("a"), b"new-a");
        assert!(transaction_dir.exists());
        cleanup_case(&root);
    }

    #[test]
    fn recovery_restages_all_backups_before_mutating_outputs() {
        let root = unique_temp_dir("artifact-set-restage-before-mutation");
        fs::create_dir_all(&root).expect("root");
        fs::write(root.join("a.txt"), b"old-a").expect("old a");
        fs::write(root.join("b.txt"), b"old-b").expect("old b");
        let entries = [entry("a.txt", b"new-a"), entry("b.txt", b"new-b")];
        let plan = validate_artifact_set(&entries, &[]).expect("plan");
        let transaction_dir = create_transaction_dir(&root).expect("transaction");
        fs::create_dir(transaction_dir.join("stage")).expect("stage");
        fs::create_dir(transaction_dir.join("backup")).expect("backup");
        let prepared = ArtifactSetJournal::prepared(&root, true, &plan).expect("prepared");
        write_marker(&transaction_dir, PREPARED_MARKER, &prepared).expect("prepared marker");

        let backup_a = transaction_dir.join("backup/a.txt");
        let backup_b = transaction_dir.join("backup/b.txt");
        fs::copy(root.join("a.txt"), &backup_a).expect("backup a");
        fs::copy(root.join("b.txt"), &backup_b).expect("backup b");
        let ready = prepared.with_backups(vec![
            backup_journal("a.txt", &backup_a),
            backup_journal("b.txt", &backup_b),
        ]);
        write_marker(&transaction_dir, BACKUPS_READY_MARKER, &ready).expect("ready marker");
        fs::write(root.join("a.txt"), b"new-a").expect("new a");
        fs::write(root.join("b.txt"), b"new-b").expect("new b");

        let recovery = ready.validate(&root).expect("recovery plan");
        let mut ops = FaultingOps::fail_copy_at(2);
        let error = rollback_from_journal(&root, &transaction_dir, &recovery, &mut ops)
            .expect_err("second restage copy fails");

        assert!(error.to_string().contains("injected copy failure"));
        assert_eq!(fs::read(root.join("a.txt")).expect("a"), b"new-a");
        assert_eq!(fs::read(root.join("b.txt")).expect("b"), b"new-b");
        assert!(transaction_dir.exists());
        cleanup_case(&root);
    }

    #[test]
    fn recovery_does_not_delete_a_preexisting_empty_parent_of_a_rolled_back_write() {
        let root = unique_temp_dir("artifact-set-recovery-preexisting-parent");
        fs::create_dir_all(root.join("subdir")).expect("preexisting empty subdir");
        let entries = [entry("subdir/new.txt", b"new-content")];
        let plan = validate_artifact_set(&entries, &[]).expect("plan");
        let transaction_dir = create_transaction_dir(&root).expect("transaction");
        fs::create_dir(transaction_dir.join("stage")).expect("stage");
        fs::create_dir(transaction_dir.join("backup")).expect("backup");
        let prepared = ArtifactSetJournal::prepared(&root, true, &plan).expect("prepared");
        write_marker(&transaction_dir, PREPARED_MARKER, &prepared).expect("prepared marker");
        let ready = prepared.with_backups(vec![]);
        write_marker(&transaction_dir, BACKUPS_READY_MARKER, &ready).expect("ready marker");
        // Simulate install having already renamed the new file into place before the crash
        // that forces this recovery attempt.
        fs::write(root.join("subdir/new.txt"), b"new-content").expect("simulated installed write");

        let recovery = ready.validate(&root).expect("recovery plan");
        let mut ops = StdArtifactSetOps;
        rollback_from_journal(&root, &transaction_dir, &recovery, &mut ops)
            .expect("rollback of an unbacked write succeeds");

        assert!(!root.join("subdir/new.txt").exists());
        assert!(
            root.join("subdir").is_dir(),
            "a directory that predates the transaction must survive rollback of a write inside it"
        );
        cleanup_case(&root);
    }

    #[test]
    fn interrupted_recovery_keeps_unreached_outputs_and_retries_from_immutable_backups() {
        let root = unique_temp_dir("artifact-set-recovery-blast-radius");
        fs::create_dir_all(&root).expect("root");
        fs::write(root.join("a.txt"), b"old-a").expect("old a");
        fs::write(root.join("b.txt"), b"old-b").expect("old b");
        let entries = [entry("a.txt", b"new-a"), entry("b.txt", b"new-b")];
        let plan = validate_artifact_set(&entries, &[]).expect("plan");
        let transaction_dir = create_transaction_dir(&root).expect("transaction");
        fs::create_dir(transaction_dir.join("stage")).expect("stage");
        fs::create_dir(transaction_dir.join("backup")).expect("backup");
        let prepared = ArtifactSetJournal::prepared(&root, true, &plan).expect("prepared");
        write_marker(&transaction_dir, PREPARED_MARKER, &prepared).expect("prepared marker");

        let backup_a = transaction_dir.join("backup/a.txt");
        let backup_b = transaction_dir.join("backup/b.txt");
        fs::copy(root.join("a.txt"), &backup_a).expect("backup a");
        fs::copy(root.join("b.txt"), &backup_b).expect("backup b");
        let ready = prepared.with_backups(vec![
            backup_journal("a.txt", &backup_a),
            backup_journal("b.txt", &backup_b),
        ]);
        write_marker(&transaction_dir, BACKUPS_READY_MARKER, &ready).expect("ready marker");
        fs::write(root.join("a.txt"), b"new-a").expect("new a");
        fs::write(root.join("b.txt"), b"new-b").expect("new b");

        let recovery = ready.validate(&root).expect("recovery plan");
        let mut ops = FaultingOps::fail_remove_file_at(2);
        let error = rollback_from_journal(&root, &transaction_dir, &recovery, &mut ops)
            .expect_err("second output mutation fails");

        assert!(error.to_string().contains("injected remove failure"));
        assert_eq!(fs::read(root.join("a.txt")).expect("a"), b"old-a");
        assert_eq!(fs::read(root.join("b.txt")).expect("b"), b"new-b");
        assert_eq!(fs::read(&backup_a).expect("immutable backup a"), b"old-a");
        assert_eq!(fs::read(&backup_b).expect("immutable backup b"), b"old-b");
        assert!(transaction_dir.exists());

        AtomicArtifactSetWriter::publish(&root, &[], &[]).expect("retry recovery");
        assert_eq!(fs::read(root.join("a.txt")).expect("a"), b"old-a");
        assert_eq!(fs::read(root.join("b.txt")).expect("b"), b"old-b");
        assert_no_transaction_directories(&root);
        cleanup_case(&root);
    }

    #[test]
    fn unsupported_journal_v1_fails_closed() {
        let root = unique_temp_dir("artifact-set-v1-journal");
        fs::create_dir_all(&root).expect("root");
        let transaction_dir = create_transaction_dir(&root).expect("transaction");
        fs::create_dir(transaction_dir.join("stage")).expect("stage");
        fs::create_dir(transaction_dir.join("backup")).expect("backup");
        let root_name = root_file_name(&root).expect("root name");
        let unsupported = format!(
            "{{\n  \"schema\": \"perfectpixel.artifact-set-transaction/1\",\n  \"rootName\": \"{root_name}\",\n  \"rootExisted\": true,\n  \"writes\": [],\n  \"removals\": [],\n  \"preexisting\": []\n}}\n"
        );
        fs::write(transaction_dir.join(PREPARED_MARKER), unsupported).expect("unsupported marker");

        let error = AtomicArtifactSetWriter::publish(&root, &[], &[])
            .expect_err("unsupported journal v1 lacks trusted backup evidence");

        assert!(error
            .to_string()
            .contains("perfectpixel.artifact-set-transaction/1"));
        assert!(error.to_string().contains(JOURNAL_SCHEMA));
        assert!(transaction_dir.exists());
        cleanup_case(&root);
    }

    #[test]
    fn journal_v2_rejects_unordered_foreign_and_invalid_backup_evidence() {
        let root = unique_temp_dir("artifact-set-v2-journal-validation");
        let entries = [entry("a.txt", b"a"), entry("b.txt", b"b")];
        let plan = validate_artifact_set(&entries, &[]).expect("plan");
        let prepared = ArtifactSetJournal::prepared(&root, true, &plan).expect("prepared");
        let valid_sha = "00".repeat(32);

        let mut unordered = prepared.clone();
        unordered.backups = vec![
            ArtifactSetBackupJournal {
                path: "b.txt".to_owned(),
                byte_count: 1,
                sha256: valid_sha.clone(),
            },
            ArtifactSetBackupJournal {
                path: "a.txt".to_owned(),
                byte_count: 1,
                sha256: valid_sha.clone(),
            },
        ];
        assert!(unordered
            .validate(&root)
            .expect_err("unordered backups")
            .to_string()
            .contains("strictly sorted"));

        let mut foreign = prepared.clone();
        foreign.backups = vec![ArtifactSetBackupJournal {
            path: "foreign.txt".to_owned(),
            byte_count: 1,
            sha256: valid_sha.clone(),
        }];
        assert!(foreign
            .validate(&root)
            .expect_err("foreign backup")
            .to_string()
            .contains("foreign backup paths"));

        let mut invalid_digest = prepared;
        invalid_digest.backups = vec![ArtifactSetBackupJournal {
            path: "a.txt".to_owned(),
            byte_count: 1,
            sha256: "A".repeat(64),
        }];
        assert!(invalid_digest
            .validate(&root)
            .expect_err("invalid digest")
            .to_string()
            .contains("invalid SHA-256"));
    }

    #[test]
    fn journal_v2_rejects_colliding_ancestor_paths() {
        let root = unique_temp_dir("artifact-set-v2-journal-path-collision");
        let journal = ArtifactSetJournal {
            schema: JOURNAL_SCHEMA.to_owned(),
            root_name: root_file_name(&root).expect("root name"),
            root_existed: false,
            writes: vec!["artifact".to_owned(), "artifact/child.txt".to_owned()],
            removals: Vec::new(),
            backups: Vec::new(),
        };

        let error = journal
            .validate(&root)
            .expect_err("recovery journal path-shape collision must fail closed");

        assert!(error.to_string().contains("colliding ancestor paths"));
    }

    #[test]
    fn installed_marker_keeps_published_outputs_and_only_cleans_journal() {
        let root = unique_temp_dir("artifact-set-installed-recovery");
        fs::create_dir_all(&root).expect("root");
        fs::write(root.join("a.txt"), b"new-a").expect("new a");
        let entries = [entry("a.txt", b"new-a")];
        let plan = validate_artifact_set(&entries, &[]).expect("plan");
        let transaction_dir = create_transaction_dir(&root).expect("transaction");
        fs::create_dir(transaction_dir.join("stage")).expect("stage");
        fs::create_dir(transaction_dir.join("backup")).expect("backup");
        let journal = ArtifactSetJournal::prepared(&root, true, &plan).expect("journal");
        write_marker(&transaction_dir, PREPARED_MARKER, &journal).expect("prepared");
        write_marker(&transaction_dir, BACKUPS_READY_MARKER, &journal).expect("ready");
        write_marker(&transaction_dir, INSTALLED_MARKER, &journal).expect("installed");

        AtomicArtifactSetWriter::publish(&root, &[], &[]).expect("cleanup installed");

        assert_eq!(fs::read(root.join("a.txt")).expect("a"), b"new-a");
        assert_no_transaction_directories(&root);
        cleanup_case(&root);
    }

    #[test]
    fn rejects_write_remove_overlap_before_mutation() {
        let root = unique_temp_dir("artifact-set-overlap");
        let entries = [entry("same.txt", b"new")];
        let removals = [PathBuf::from("same.txt")];

        let error = AtomicArtifactSetWriter::publish(&root, &entries, &removals)
            .expect_err("overlap must fail");

        assert!(matches!(error, PpError::FileIo { .. }));
        assert!(!root.exists());
        cleanup_case(&root);
    }

    #[test]
    fn journal_capacity_is_rejected_before_transaction_creation() {
        let root = unique_temp_dir("artifact-set-journal-capacity");
        let suffix = "x".repeat(800);
        let paths = (0..700)
            .map(|index| PathBuf::from(format!("artifact-{index:04}-{suffix}")))
            .collect::<Vec<_>>();
        let entries = paths
            .iter()
            .map(|path| AtomicArtifactSetEntry {
                relative_path: path,
                bytes: b"",
            })
            .collect::<Vec<_>>();
        let plan = validate_artifact_set(&entries, &[]).expect("artifact plan");

        let error = preflight_journal_capacity(&root, false, &plan)
            .expect_err("worst-case journal must exceed its capacity");

        assert!(error
            .to_string()
            .contains("transaction marker exceeds byte limit"));
        assert!(!root.exists());
        assert_no_transaction_directories(&root);
        cleanup_case(&root);
    }

    #[test]
    fn writer_and_recovery_share_artifact_entry_and_path_bounds() {
        let paths = (0..=MAX_ARTIFACT_ENTRIES)
            .map(|index| PathBuf::from(format!("artifact-{index}.txt")))
            .collect::<Vec<_>>();
        let entries = paths
            .iter()
            .map(|path| AtomicArtifactSetEntry {
                relative_path: path,
                bytes: b"",
            })
            .collect::<Vec<_>>();

        let writer_error = validate_artifact_set(&entries, &[]).expect_err("writer entry limit");
        assert!(writer_error.to_string().contains("entry limit"));

        let root = unique_temp_dir("artifact-set-recovery-bounds");
        let journal = ArtifactSetJournal {
            schema: JOURNAL_SCHEMA.to_owned(),
            root_name: root_file_name(&root).expect("root name"),
            root_existed: false,
            writes: paths
                .iter()
                .map(|path| path.to_string_lossy().into_owned())
                .collect(),
            removals: Vec::new(),
            backups: Vec::new(),
        };
        let recovery_error = journal.validate(&root).expect_err("recovery entry limit");
        assert!(recovery_error.to_string().contains("entry limit"));

        let too_long = "a".repeat(MAX_RELATIVE_PATH_BYTES + 1);
        let writer_error =
            validate_artifact_set(&[entry(&too_long, b"")], &[]).expect_err("writer path limit");
        assert!(writer_error.to_string().contains("byte limit"));

        let journal = ArtifactSetJournal {
            schema: JOURNAL_SCHEMA.to_owned(),
            root_name: root_file_name(&root).expect("root name"),
            root_existed: false,
            writes: vec![too_long],
            removals: Vec::new(),
            backups: Vec::new(),
        };
        let recovery_error = journal.validate(&root).expect_err("recovery path limit");
        assert!(recovery_error.to_string().contains("invalid writes path"));
    }

    #[test]
    fn artifact_transaction_reducer_allows_only_durable_protocol_events() {
        let state = reduce_artifact_transaction(
            ArtifactTransactionState::Prepared,
            ArtifactTransactionEvent::BackupsRecorded,
        )
        .expect("backups marker advances the transaction");
        assert_eq!(state, ArtifactTransactionState::BackupsReady);

        let state = reduce_artifact_transaction(state, ArtifactTransactionEvent::InstallRecorded)
            .expect("installed marker commits the transaction");
        assert_eq!(state, ArtifactTransactionState::Installed);
        assert!(
            reduce_artifact_transaction(state, ArtifactTransactionEvent::RollbackRestored).is_err()
        );

        let aborted = reduce_artifact_transaction(
            ArtifactTransactionState::BackupsReady,
            ArtifactTransactionEvent::PublicationAborted,
        )
        .expect("a backed-up but unmutated transaction can abort");
        assert_eq!(aborted, ArtifactTransactionState::Aborted);
        assert!(
            reduce_artifact_transaction(aborted, ArtifactTransactionEvent::InstallRecorded)
                .is_err()
        );

        let restored = reduce_artifact_transaction(
            ArtifactTransactionState::BackupsReady,
            ArtifactTransactionEvent::RollbackRestored,
        )
        .expect("only a backed-up transaction can be restored");
        assert_eq!(restored, ArtifactTransactionState::Restored);
        assert_eq!(
            reduce_artifact_transaction(restored, ArtifactTransactionEvent::RecoveryRequired),
            Ok(ArtifactTransactionState::RecoveryRequired)
        );
    }

    #[test]
    fn streaming_sha256_matches_standard_vectors_across_update_boundaries() {
        let mut empty = crate::core::sha256::Sha256State::new();
        empty.update(b"");
        assert_eq!(
            crate::core::sha256::hex_encode(empty.finalize()),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );

        let mut abc = crate::core::sha256::Sha256State::new();
        abc.update(b"a");
        abc.update(b"b");
        abc.update(b"c");
        assert_eq!(
            crate::core::sha256::hex_encode(abc.finalize()),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );

        let mut long = crate::core::sha256::Sha256State::new();
        long.update(&[b'a'; 63]);
        long.update(b"a");
        long.update(&[b'a'; 36]);
        assert_eq!(
            crate::core::sha256::hex_encode(long.finalize()),
            "2816597888e4a0d3a36b82b83316ab32680eb8f00f8cd3b904d681246d285a0e"
        );
    }

    fn entry<'a>(relative_path: &'a str, bytes: &'a [u8]) -> AtomicArtifactSetEntry<'a> {
        AtomicArtifactSetEntry {
            relative_path: Path::new(relative_path),
            bytes,
        }
    }

    fn backup_journal(relative_path: &str, backup: &Path) -> ArtifactSetBackupJournal {
        let integrity = measure_regular_file(backup, "test backup integrity").expect("integrity");
        ArtifactSetBackupJournal {
            path: relative_path.to_owned(),
            byte_count: integrity.byte_count,
            sha256: integrity.sha256,
        }
    }

    struct FaultingOps {
        copy_count: usize,
        fail_copy_at: Option<usize>,
        rename_count: usize,
        fail_rename_at: Option<usize>,
        remove_file_count: usize,
        fail_remove_file_at: Option<usize>,
        remove_dir_all_count: usize,
        fail_remove_dir_all_at: Option<usize>,
    }

    impl FaultingOps {
        fn new() -> Self {
            Self {
                copy_count: 0,
                fail_copy_at: None,
                rename_count: 0,
                fail_rename_at: None,
                remove_file_count: 0,
                fail_remove_file_at: None,
                remove_dir_all_count: 0,
                fail_remove_dir_all_at: None,
            }
        }

        fn fail_copy_at(call: usize) -> Self {
            Self {
                fail_copy_at: Some(call),
                ..Self::new()
            }
        }

        fn fail_rename_at(call: usize) -> Self {
            Self {
                fail_rename_at: Some(call),
                ..Self::new()
            }
        }

        fn fail_remove_file_at(call: usize) -> Self {
            Self {
                fail_remove_file_at: Some(call),
                ..Self::new()
            }
        }

        fn fail_install_and_rollback_cleanup() -> Self {
            Self {
                fail_rename_at: Some(2),
                fail_remove_file_at: Some(1),
                ..Self::new()
            }
        }

        fn fail_commit_cleanup() -> Self {
            Self {
                fail_remove_dir_all_at: Some(1),
                ..Self::new()
            }
        }
    }

    impl ArtifactSetOps for FaultingOps {
        fn copy(&mut self, from: &Path, to: &Path) -> std::io::Result<u64> {
            self.copy_count += 1;
            if self.fail_copy_at == Some(self.copy_count) {
                return Err(std::io::Error::other("injected copy failure"));
            }
            fs::copy(from, to)
        }

        fn rename(&mut self, from: &Path, to: &Path) -> std::io::Result<()> {
            self.rename_count += 1;
            if self.fail_rename_at == Some(self.rename_count) {
                return Err(std::io::Error::other("injected rename failure"));
            }
            fs::rename(from, to)
        }

        fn remove_file(&mut self, path: &Path) -> std::io::Result<()> {
            self.remove_file_count += 1;
            if self.fail_remove_file_at == Some(self.remove_file_count) {
                return Err(std::io::Error::other("injected remove failure"));
            }
            fs::remove_file(path)
        }

        fn remove_dir_all(&mut self, path: &Path) -> std::io::Result<()> {
            self.remove_dir_all_count += 1;
            if self.fail_remove_dir_all_at == Some(self.remove_dir_all_count) {
                return Err(std::io::Error::other(
                    "injected transaction cleanup failure",
                ));
            }
            fs::remove_dir_all(path)
        }
    }

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "perfectpixel-{prefix}-{}-{nanos}",
            std::process::id()
        ))
    }

    fn assert_no_transaction_directories(root: &Path) {
        let Some(parent) = root.parent() else {
            return;
        };
        let prefix = transaction_prefix(root).expect("prefix");
        for entry in fs::read_dir(parent).expect("transaction parent") {
            let entry = entry.expect("entry");
            if entry.file_type().expect("file type").is_dir()
                && is_transaction_name(&entry.file_name(), &prefix)
            {
                panic!(
                    "transaction directory left behind: {}",
                    entry.path().display()
                );
            }
        }
    }

    fn cleanup_case(root: &Path) {
        let _ = fs::remove_dir_all(root);
        if let Ok(path) = lock_path(root) {
            let _ = fs::remove_file(path);
        }
        let Some(parent) = root.parent() else {
            return;
        };
        let Ok(prefix) = transaction_prefix(root) else {
            return;
        };
        let Ok(entries) = fs::read_dir(parent) else {
            return;
        };
        for entry in entries.flatten() {
            if is_transaction_name(&entry.file_name(), &prefix) {
                let _ = fs::remove_dir_all(entry.path());
            }
        }
    }
}
