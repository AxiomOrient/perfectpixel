use std::{
    env, fs,
    path::{Component, Path, PathBuf},
};

use crate::{PpError, PpResult};

pub(super) fn validate_raster_input_path(path: &Path) -> PpResult<()> {
    validate_file_extension(path, &["png", "jpg", "jpeg", "webp"], "raster input")
}

pub(super) fn validate_file_extension(
    path: &Path,
    allowed: &[&str],
    label: &str,
) -> PpResult<()> {
    let Some(extension) = path.extension().and_then(|value| value.to_str()) else {
        return Err(PpError::InvalidRequest(format!(
            "{label} must use one of these extensions: {}",
            allowed.join(", ")
        )));
    };
    if !allowed
        .iter()
        .any(|allowed_extension| extension.eq_ignore_ascii_case(allowed_extension))
    {
        return Err(PpError::InvalidRequest(format!(
            "{label} must use one of these extensions: {}",
            allowed.join(", ")
        )));
    }
    Ok(())
}

pub(super) fn resolve_request_frame_path(base_dir: &Path, value: &str) -> PpResult<PathBuf> {
    if value.trim().is_empty()
        || value != value.trim()
        || value.contains('\0')
        || value.contains('\\')
    {
        return Err(PpError::InvalidRequest(format!(
            "frame path '{value}' is not a safe relative path"
        )));
    }
    let path = PathBuf::from(value);
    if path.is_absolute() {
        return Err(PpError::InvalidRequest(format!(
            "frame path '{value}' must be relative to the request file"
        )));
    }
    validate_file_extension(&path, &["png", "jpg", "jpeg", "webp"], "frame path")?;
    if path
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(PpError::InvalidRequest(format!(
            "frame path '{value}' must not contain '..'"
        )));
    }
    Ok(base_dir.join(path))
}

pub(super) fn reject_same_path(left: &Path, right: &Path, message: &str) -> PpResult<()> {
    if comparable_path(left)? == comparable_path(right)? {
        return Err(PpError::InvalidRequest(message.to_string()));
    }
    Ok(())
}

pub(super) fn reject_path_overlap(left: &Path, right: &Path, message: &str) -> PpResult<()> {
    let left = comparable_path(left)?;
    let right = comparable_path(right)?;
    if left == right || left.starts_with(&right) || right.starts_with(&left) {
        return Err(PpError::InvalidRequest(message.to_string()));
    }
    Ok(())
}

pub(super) fn resolve_safe_relative(
    base_dir: &Path,
    value: &str,
    allowed_extensions: &[&str],
    label: &str,
) -> PpResult<PathBuf> {
    if value.trim().is_empty()
        || value != value.trim()
        || value.contains('\0')
        || value.contains('\\')
    {
        return Err(PpError::InvalidRequest(format!(
            "{label} is not a safe relative path"
        )));
    }
    let path = PathBuf::from(value);
    if path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(PpError::InvalidRequest(format!(
            "{label} must be a child path without '.' or '..'"
        )));
    }
    if !allowed_extensions.is_empty() {
        validate_file_extension(&path, allowed_extensions, label)?;
    }
    Ok(base_dir.join(path))
}

pub(super) fn managed_relative_path(value: &str) -> PpResult<PathBuf> {
    if value.is_empty() || value.contains('\0') || value.contains('\\') {
        return Err(PpError::InvalidRequest(format!(
            "managed output '{value}' is not a safe relative path"
        )));
    }
    let path = PathBuf::from(value);
    if path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(PpError::InvalidRequest(format!(
            "managed output '{value}' is not a safe relative path"
        )));
    }
    Ok(path)
}

pub(super) fn destination_error(path: &Path, message: &str) -> PpError {
    PpError::InvalidRequest(format!("destination '{}': {message}", path.display()))
}

pub(super) fn comparable_path(path: &Path) -> PpResult<PathBuf> {
    if let Ok(real_path) = fs::canonicalize(path) {
        return Ok(real_path);
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
    if let (Some(parent), Some(file_name)) = (absolute.parent(), absolute.file_name()) {
        if let Ok(real_parent) = fs::canonicalize(parent) {
            return Ok(real_parent.join(file_name));
        }
    }
    Ok(normalize_lexical(&absolute))
}

fn normalize_lexical(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
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
    normalized
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_relative_rejects_parent_and_backslash() {
        assert!(resolve_safe_relative(Path::new("/tmp"), "../a.png", &["png"], "frame").is_err());
        assert!(resolve_safe_relative(Path::new("/tmp"), "dir\\a.png", &["png"], "frame").is_err());
    }

    #[test]
    fn request_frame_path_rejects_parent_and_unsupported_extension() {
        assert!(resolve_request_frame_path(Path::new("."), "../a.png").is_err());
        assert!(resolve_request_frame_path(Path::new("."), "frames/a.gif").is_err());
    }

    #[test]
    fn managed_output_rejects_dot_components() {
        assert!(managed_relative_path("a/../b.png").is_err());
        assert!(managed_relative_path("./b.png").is_err());
    }
}
