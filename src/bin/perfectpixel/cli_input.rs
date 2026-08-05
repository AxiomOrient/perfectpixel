use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::path::{Component, Path, PathBuf};

use perfectpixel::{PpError, PpResult, SvgProfile, VectorPresetSelection};

pub(super) fn reject_extra_args(args: &[String], command: &str) -> PpResult<()> {
    if args.is_empty() {
        return Ok(());
    }
    Err(PpError::InvalidOption(format!(
        "{} does not accept extra arguments",
        command
    )))
}

pub(super) fn validate_options(args: &[String], allowed: &[&str]) -> PpResult<()> {
    let mut seen = BTreeSet::new();
    let mut index = 0usize;
    while index < args.len() {
        let key = args[index].as_str();
        if !key.starts_with("--") {
            return Err(PpError::InvalidOption(format!(
                "unexpected positional argument '{}'",
                key
            )));
        }
        if !allowed.contains(&key) {
            return Err(PpError::InvalidOption(format!("unknown option '{}'", key)));
        }
        if !seen.insert(key.to_string()) {
            return Err(PpError::InvalidOption(format!(
                "duplicate option '{}'",
                key
            )));
        }
        let Some(value) = args.get(index + 1) else {
            return Err(PpError::InvalidOption(format!(
                "missing value for option '{}'",
                key
            )));
        };
        if value.starts_with("--") {
            return Err(PpError::InvalidOption(format!(
                "missing value for option '{}'",
                key
            )));
        }
        index += 2;
    }
    Ok(())
}

pub(super) fn option_path(args: &[String], key: &str) -> PpResult<PathBuf> {
    option_value(args, key)
        .map(PathBuf::from)
        .ok_or_else(|| PpError::InvalidOption(format!("{} is required", key)))
}

pub(super) fn option_path_optional(args: &[String], key: &str) -> Option<PathBuf> {
    option_value(args, key).map(PathBuf::from)
}

pub(super) fn option_value<'a>(args: &'a [String], key: &str) -> Option<&'a str> {
    args.windows(2)
        .find(|pair| pair[0] == key)
        .map(|pair| pair[1].as_str())
}

pub(super) fn parse_vector_preset(raw: &str) -> PpResult<VectorPresetSelection> {
    match raw {
        "auto" => Ok(VectorPresetSelection::Auto),
        "pixel-art" => Ok(VectorPresetSelection::PixelArt),
        "legacy-lossless" => Ok(VectorPresetSelection::LegacyLossless),
        "flat-icon" => Ok(VectorPresetSelection::FlatIcon),
        "line-art" => Ok(VectorPresetSelection::LineArt),
        "bounded-illustration" => Ok(VectorPresetSelection::BoundedIllustration),
        _ => Err(PpError::InvalidOption(
            "--preset must be auto, pixel-art, legacy-lossless, flat-icon, line-art, or bounded-illustration".to_string(),
        )),
    }
}

pub(super) fn parse_vector_profile(raw: &str) -> PpResult<SvgProfile> {
    match raw {
        "compact" => Ok(SvgProfile::Compact),
        "motion-structure-ready" => Ok(SvgProfile::MotionStructureReady),
        _ => Err(PpError::InvalidOption(
            "--profile must be compact or motion-structure-ready".to_string(),
        )),
    }
}

pub(super) fn parse_vector_detail(raw: &str) -> PpResult<Option<u8>> {
    if raw == "auto" {
        return Ok(None);
    }
    let detail = raw.parse::<u8>().map_err(|_| {
        PpError::InvalidOption("--detail must be auto or an integer from 1 through 5".to_string())
    })?;
    if !(1..=5).contains(&detail) {
        return Err(PpError::InvalidOption(
            "--detail must be auto or an integer from 1 through 5".to_string(),
        ));
    }
    Ok(Some(detail))
}

pub(super) fn parse_unit_float(raw: &str, key: &str) -> PpResult<f64> {
    let message = if matches!(key, "--min-quality" | "--max-quality-loss") {
        "--min-quality and --max-quality-loss must be finite numbers from 0 through 1".to_string()
    } else {
        format!("{key} must be a finite number from 0 through 1")
    };
    let value = raw
        .parse::<f64>()
        .map_err(|_| PpError::InvalidOption(message.clone()))?;
    if !value.is_finite() || !(0.0..=1.0).contains(&value) {
        return Err(PpError::InvalidOption(message));
    }
    Ok(value)
}

pub(super) fn parse_positive_usize(raw: &str, key: &str) -> PpResult<usize> {
    let value = raw
        .parse::<usize>()
        .map_err(|_| PpError::InvalidOption(format!("{key} must be a positive integer")))?;
    if value == 0 {
        return Err(PpError::InvalidOption(format!(
            "{key} must be a positive integer"
        )));
    }
    Ok(value)
}

pub(super) fn validate_raster_input_path(path: &Path) -> PpResult<()> {
    validate_file_extension(path, &["png", "jpg", "jpeg", "webp"], "raster input")
}

pub(super) fn validate_file_extension(path: &Path, allowed: &[&str], label: &str) -> PpResult<()> {
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
            "frame path '{}' is not a safe relative path",
            value
        )));
    }
    let path = PathBuf::from(value);
    if path.is_absolute() {
        return Err(PpError::InvalidRequest(format!(
            "frame path '{}' must be relative to the request file",
            value
        )));
    }
    validate_file_extension(&path, &["png", "jpg", "jpeg", "webp"], "frame path")?;
    if path
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(PpError::InvalidRequest(format!(
            "frame path '{}' must not contain '..'",
            value
        )));
    }
    Ok(base_dir.join(path))
}

pub(super) fn reject_same_path(left: &Path, right: &Path, message: &str) -> PpResult<()> {
    let left = comparable_path(left)?;
    let right = comparable_path(right)?;
    if left == right {
        return Err(PpError::InvalidRequest(message.to_string()));
    }
    Ok(())
}

fn comparable_path(path: &Path) -> PpResult<PathBuf> {
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
