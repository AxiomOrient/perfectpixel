use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::core::{PpError, PpResult};

use super::artifact_set::{
    validate_relative_path, ArtifactSetConditionPhase, AtomicArtifactSetOwnedEntry,
    AtomicArtifactSetOwnedPlan, AtomicArtifactSetWriter,
};
use super::capability;

const MAX_TEMP_ATTEMPTS: usize = 64;
const MAX_DIRECTORY_ENTRIES: usize = 64;
const MAX_DIRECTORY_BYTES: usize = 64 * 1024 * 1024;
const MAX_RELATIVE_PATH_DEPTH: usize = 16;
const MAX_DIRECTORY_TREE_ENTRIES: usize = MAX_DIRECTORY_ENTRIES * (MAX_RELATIVE_PATH_DEPTH + 1);

/// Publishes one file through a same-directory temporary file.
///
/// Every path component is resolved with descriptor-relative no-follow I/O.
/// Temporary bytes are durably synchronized before `renameat`; the containing
/// directory is synchronized as part of the rename commit. An error before the
/// rename preserves the previous destination. A durability error after rename
/// is reported as failure and never converted into success.
pub struct AtomicFileWriter;

/// A single verified member of an atomically replaced artifact directory.
///
/// `sha256` is the expected SHA-256 digest of `bytes`; entries are staged only
/// when their bytes match it exactly.
pub struct AtomicDirectoryEntry<'a> {
    pub relative_path: &'a Path,
    pub bytes: &'a [u8],
    pub sha256: [u8; 32],
}

pub struct AtomicDirectoryWriter;

struct DirectoryPublicationGuard {
    observed_paths: Vec<PathBuf>,
    next_paths: Vec<PathBuf>,
}

impl AtomicFileWriter {
    pub fn write_bytes(path: impl AsRef<Path>, bytes: &[u8]) -> PpResult<()> {
        let path = prepare_file_path(path.as_ref())?;
        let (tmp, file) = create_temp_file(&path)?;
        match write_temp_and_commit(&path, &tmp, file, bytes) {
            Ok(()) => Ok(()),
            Err(error) => cleanup_temp_after_failure(&tmp, error),
        }
    }

    pub fn write_text(path: impl AsRef<Path>, text: &str) -> PpResult<()> {
        Self::write_bytes(path, text.as_bytes())
    }
}

impl AtomicDirectoryWriter {
    /// Publishes the supplied files as the complete managed file set below
    /// `target`. The writer delegates locking, staging, journal `/2`, rollback,
    /// and crash recovery to the shared artifact-set protocol. Existing regular
    /// files not present in `entries` are removed transactionally; unrelated
    /// empty directory-only state is rejected because it cannot be represented
    /// by the file-set journal without inventing directory ownership.
    pub fn replace(target: impl AsRef<Path>, entries: &[AtomicDirectoryEntry<'_>]) -> PpResult<()> {
        let requested_target = target.as_ref();
        validate_target(requested_target)?;
        validate_entries(entries)?;
        let owned_entries = entries
            .iter()
            .map(|entry| AtomicArtifactSetOwnedEntry {
                relative_path: entry.relative_path.to_path_buf(),
                bytes: entry.bytes.to_vec(),
            })
            .collect::<Vec<_>>();
        let desired_paths = owned_entries
            .iter()
            .map(|entry| entry.relative_path.clone())
            .collect::<BTreeSet<_>>();

        let next_paths = desired_paths.iter().cloned().collect::<Vec<_>>();
        let ensure_empty_root = entries.is_empty();
        AtomicArtifactSetWriter::publish_with_planner_checked_exact(
            requested_target,
            move |locked_root| {
                let existing_paths = collect_existing_directory_files(locked_root, &desired_paths)?;
                let removals = existing_paths
                    .iter()
                    .filter(|path| !desired_paths.contains(*path))
                    .cloned()
                    .collect();
                Ok((
                    AtomicArtifactSetOwnedPlan {
                        entries: owned_entries,
                        removals,
                    },
                    DirectoryPublicationGuard {
                        observed_paths: existing_paths,
                        next_paths,
                    },
                ))
            },
            verify_directory_publication,
            ensure_empty_root,
        )
    }
}

fn verify_directory_publication(
    root: &Path,
    phase: ArtifactSetConditionPhase,
    guard: &DirectoryPublicationGuard,
) -> PpResult<()> {
    let desired = guard.next_paths.iter().cloned().collect::<BTreeSet<_>>();
    let actual = collect_existing_directory_files(root, &desired)?;
    let expected = match phase {
        ArtifactSetConditionPhase::BeforeMutation => &guard.observed_paths,
        ArtifactSetConditionPhase::BeforeCommit => &guard.next_paths,
    };
    if actual.as_slice() != expected.as_slice() {
        let message = match phase {
            ArtifactSetConditionPhase::BeforeMutation => {
                "managed directory file set changed before publication mutation"
            }
            ArtifactSetConditionPhase::BeforeCommit => {
                "managed directory file set changed before publication commit"
            }
        };
        return Err(file_error(root, message));
    }
    Ok(())
}

fn validate_target(target: &Path) -> PpResult<()> {
    if target.as_os_str().is_empty() || target.file_name().is_none() {
        return Err(file_error(target, "directory target must name a directory"));
    }
    match fs::symlink_metadata(target) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            return Err(invalid_destination_error(
                target,
                "directory target must be a non-symlink directory",
            ));
        }
        Ok(_) => {}
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {}
        Err(source) => return Err(file_error(target, source)),
    }
    Ok(())
}

fn prepare_file_path(requested: &Path) -> PpResult<PathBuf> {
    if requested.as_os_str().is_empty() || requested.file_name().is_none() {
        return Err(invalid_destination_error(
            requested,
            "file target must name a file",
        ));
    }
    let parent = requested
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    capability::create_dir_all(parent).map_err(|source| file_error(parent, source))?;
    match capability::open_read(requested) {
        Ok(file) if !file.metadata().map_err(|source| file_error(requested, source))?.is_file() => {
            Err(invalid_destination_error(
                requested,
                "file target must be a regular non-symlink file",
            ))
        }
        Ok(_) => Ok(requested.to_path_buf()),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(requested.to_path_buf()),
        Err(source) if source.raw_os_error() == Some(libc::ELOOP) => Err(invalid_destination_error(
            requested,
            "file target must be a regular non-symlink file",
        )),
        Err(source) => Err(file_error(requested, source)),
    }
}

fn validate_entries(entries: &[AtomicDirectoryEntry<'_>]) -> PpResult<()> {
    if entries.len() > MAX_DIRECTORY_ENTRIES {
        return Err(file_error(
            Path::new("<diagnostics>"),
            "too many directory entries",
        ));
    }
    let mut total_bytes = 0usize;
    let mut paths = BTreeSet::new();
    for entry in entries {
        validate_relative_path(entry.relative_path)?;
        if !paths.insert(entry.relative_path.to_path_buf()) {
            return Err(file_error(entry.relative_path, "duplicate directory entry"));
        }
        total_bytes = total_bytes
            .checked_add(entry.bytes.len())
            .ok_or_else(|| file_error(entry.relative_path, "directory artifact bytes overflow"))?;
        if total_bytes > MAX_DIRECTORY_BYTES {
            return Err(file_error(
                entry.relative_path,
                "directory artifact bytes exceed limit",
            ));
        }
        if crate::core::sha256::sha256(entry.bytes) != entry.sha256 {
            return Err(file_error(
                entry.relative_path,
                "directory artifact digest mismatch",
            ));
        }
    }
    for path in &paths {
        for ancestor in path.ancestors().skip(1) {
            if paths.contains(ancestor) {
                return Err(file_error(
                    path,
                    "directory entry collides with another entry",
                ));
            }
        }
    }
    Ok(())
}

fn collect_existing_directory_files(
    root: &Path,
    desired_paths: &BTreeSet<PathBuf>,
) -> PpResult<Vec<PathBuf>> {
    match fs::symlink_metadata(root) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            return Err(invalid_destination_error(
                root,
                "managed directory root must be a non-symlink directory",
            ));
        }
        Ok(_) => {}
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(source) => return Err(file_error(root, source)),
    }

    let mut files = Vec::new();
    let mut directories = Vec::new();
    let mut tree_entries = 0usize;
    let mut total_bytes = 0u64;
    collect_directory_tree(
        root,
        root,
        &mut files,
        &mut directories,
        &mut tree_entries,
        &mut total_bytes,
    )?;

    files.sort();
    directories.sort();
    for directory in directories {
        let represented_by_file = files.iter().any(|path| path.starts_with(&directory));
        let represented_by_next_set = desired_paths
            .iter()
            .any(|path| path.starts_with(&directory));
        if !represented_by_file && !represented_by_next_set {
            return Err(file_error(
                &root.join(&directory),
                "directory replacement cannot own an empty directory without a managed file",
            ));
        }
    }
    Ok(files)
}

fn collect_directory_tree(
    root: &Path,
    directory: &Path,
    files: &mut Vec<PathBuf>,
    directories: &mut Vec<PathBuf>,
    tree_entries: &mut usize,
    total_bytes: &mut u64,
) -> PpResult<()> {
    let mut entries = fs::read_dir(directory)
        .map_err(|source| file_error(directory, source))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|source| file_error(directory, source))?;
    entries.sort_by_key(|entry| entry.file_name());

    for entry in entries {
        *tree_entries = (*tree_entries)
            .checked_add(1)
            .ok_or_else(|| file_error(directory, "directory tree entry count overflow"))?;
        if *tree_entries > MAX_DIRECTORY_TREE_ENTRIES {
            return Err(file_error(
                directory,
                "existing directory tree exceeds entry limit",
            ));
        }

        let path = entry.path();
        let relative = path
            .strip_prefix(root)
            .map_err(|source| file_error(&path, source))?
            .to_path_buf();
        validate_relative_path(&relative)?;
        let file_type = entry
            .file_type()
            .map_err(|source| file_error(&path, source))?;
        if file_type.is_symlink() {
            return Err(invalid_destination_error(
                &path,
                "managed directory tree must not contain symlinks",
            ));
        }
        if file_type.is_dir() {
            directories.push(relative);
            collect_directory_tree(root, &path, files, directories, tree_entries, total_bytes)?;
            continue;
        }
        if !file_type.is_file() {
            return Err(invalid_destination_error(
                &path,
                "managed directory tree must contain only regular files and directories",
            ));
        }

        let metadata = entry
            .metadata()
            .map_err(|source| file_error(&path, source))?;
        *total_bytes = (*total_bytes)
            .checked_add(metadata.len())
            .ok_or_else(|| file_error(&path, "existing directory byte count overflow"))?;
        if *total_bytes > MAX_DIRECTORY_BYTES as u64 {
            return Err(file_error(
                &path,
                "existing directory artifacts exceed byte limit",
            ));
        }
        files.push(relative);
        if files.len() > MAX_DIRECTORY_ENTRIES {
            return Err(file_error(
                &path,
                "existing directory artifacts exceed entry limit",
            ));
        }
    }
    Ok(())
}

fn write_temp_and_commit(path: &Path, tmp: &Path, mut file: File, bytes: &[u8]) -> PpResult<()> {
    file.write_all(bytes).map_err(|source| PpError::FileIo {
        path: tmp.to_path_buf(),
        message: source.to_string(),
    })?;
    capability::sync_file_durable(&file).map_err(|source| PpError::FileIo {
        path: tmp.to_path_buf(),
        message: source.to_string(),
    })?;
    drop(file);
    capability::rename(tmp, path).map_err(|source| PpError::FileIo {
        path: path.to_path_buf(),
        message: format!("replacement/durability commit failed: {source}"),
    })
}

fn create_temp_file(path: &Path) -> PpResult<(PathBuf, File)> {
    for attempt in 0..MAX_TEMP_ATTEMPTS {
        let tmp = temp_path(path, "tmp", attempt);
        match capability::create_new(&tmp) {
            Ok(file) => return Ok((tmp, file)),
            Err(source) if source.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(source) => return Err(file_error(&tmp, source)),
        }
    }
    Err(file_error(
        path,
        format!("could not create unique temp file after {MAX_TEMP_ATTEMPTS} attempts"),
    ))
}

fn cleanup_temp_after_failure(tmp: &Path, primary: PpError) -> PpResult<()> {
    match capability::remove_file(tmp) {
        Ok(()) => Err(primary),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Err(primary),
        Err(source) => Err(PpError::FileIo {
            path: tmp.to_path_buf(),
            message: format!("{primary}; failed to durably remove temporary file: {source}"),
        }),
    }
}

fn temp_path(path: &Path, kind: &str, attempt: usize) -> PathBuf {
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("output");
    path.with_file_name(format!(
        ".{file_name}.{kind}.{}.{}",
        std::process::id(),
        attempt
    ))
}

fn file_error(path: &Path, source: impl ToString) -> PpError {
    PpError::FileIo {
        path: path.to_path_buf(),
        message: source.to_string(),
    }
}

fn invalid_destination_error(path: &Path, message: &str) -> PpError {
    PpError::InvalidRequest(format!("destination '{}': {message}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_target_rejects_an_existing_directory() {
        let root = std::env::temp_dir().join(format!(
            "perfectpixel-file-target-directory-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("directory target");

        let error = AtomicFileWriter::write_bytes(&root, b"bytes")
            .expect_err("directory must not be replaced as a file");

        assert!(matches!(error, PpError::InvalidRequest(_)));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn directory_replacement_rejects_unrepresented_empty_subdirectories() {
        let root = std::env::temp_dir().join(format!(
            "perfectpixel-empty-directory-{}",
            std::process::id()
        ));
        let target = root.join("diagnostics");
        let empty = target.join("unmanaged-empty");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&empty).expect("empty directory");

        let error = AtomicDirectoryWriter::replace(&target, &[])
            .expect_err("empty directory ownership must fail closed");

        assert!(error.to_string().contains("cannot own an empty directory"));
        assert!(empty.is_dir());
        let _ = fs::remove_dir_all(root);
    }
}
