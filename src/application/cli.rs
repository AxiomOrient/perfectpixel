use std::path::PathBuf;

use crate::{Operation, PpError, PpResult};

use super::ApplicationRequest;

pub(super) enum CliInput {
    Help,
    Operation(Operation),
}

pub(super) fn parse(args: &[String]) -> PpResult<CliInput> {
    let Some(command) = args.first().map(String::as_str) else {
        return Ok(CliInput::Help);
    };
    match command {
        "--help" | "-h" => {
            reject_extra(&args[1..], "help")?;
            Ok(CliInput::Help)
        }
        "schema" => {
            reject_extra(&args[1..], "schema")?;
            operation(ApplicationRequest::Schema)
        }
        "inspect" => {
            if args.len() != 2 || args[1].starts_with("--") {
                return Err(PpError::InvalidOption(
                    "inspect requires exactly one <input>".to_string(),
                ));
            }
            operation(ApplicationRequest::Inspect {
                input: PathBuf::from(&args[1]),
            })
        }
        "convert" => parse_convert(&args[1..]),
        "upscale" => parse_upscale(&args[1..]),
        "edit" => one_request(&args[1..], |request| Operation::Edit { request }),
        "psd" => one_request(&args[1..], |request| Operation::ExportPsd { request }),
        "document-psd" => one_request(&args[1..], |request| Operation::CompileDocumentPsd { request }),
        "chroma-plan" => one_request(&args[1..], |request| Operation::ChromaPlan { request }),
        "normalize" => parse_request_directory(&args[1..], true),
        "bundle" => parse_request_directory(&args[1..], false),
        "texture-compile" => one_request(&args[1..], |request| Operation::CompileTexture { request }),
        "vector" => parse_vector(&args[1..]),
        "vector-analyze" => parse_vector_analyze(&args[1..]),
        "vision-foreground-instances" => one_request(&args[1..], |request| {
            Operation::AppleVisionForegroundInstances { request }
        }),
        "motion-scaffold" => parse_motion_scaffold(&args[1..]),
        "motion-build" => parse_motion_build(&args[1..]),
        other => Err(PpError::InvalidOption(format!(
            "unknown command '{other}'; use schema, inspect, convert, upscale, edit, psd, document-psd, chroma-plan, vector, vector-analyze, normalize, bundle, texture-compile, vision-foreground-instances, motion-scaffold, or motion-build"
        ))),
    }
}

fn parse_convert(args: &[String]) -> PpResult<CliInput> {
    let input = positional_input(args, "convert")?;
    let options = options(
        &args[1..],
        &["--out", "--width", "--height", "--filter", "--jpeg-quality", "--background"],
    )?;
    operation(ApplicationRequest::Convert {
        input,
        output: required_path(&options, "--out")?,
        width: optional_parse(&options, "--width", "--width must be a positive integer")?,
        height: optional_parse(&options, "--height", "--height must be a positive integer")?,
        filter: optional_string(&options, "--filter"),
        jpeg_quality: optional_parse(&options, "--jpeg-quality", "--jpeg-quality must be from 1 through 100")?,
        background: optional_string(&options, "--background"),
    })
}

fn parse_upscale(args: &[String]) -> PpResult<CliInput> {
    let input = positional_input(args, "upscale")?;
    let options = options(
        &args[1..],
        &["--out", "--scale", "--filter", "--jpeg-quality", "--background"],
    )?;
    let scale = required_parse(
        &options,
        "--scale",
        "--scale must be an integer greater than or equal to 2",
    )?;
    operation(ApplicationRequest::Upscale {
        input,
        output: required_path(&options, "--out")?,
        scale,
        filter: optional_string(&options, "--filter"),
        jpeg_quality: optional_parse(&options, "--jpeg-quality", "--jpeg-quality must be from 1 through 100")?,
        background: optional_string(&options, "--background"),
    })
}

fn parse_request_directory(args: &[String], normalize: bool) -> PpResult<CliInput> {
    let values = options(args, &["--request", "--out-dir"])?;
    let request = required_path(&values, "--request")?;
    let output_dir = required_path(&values, "--out-dir")?;
    if normalize {
        operation(ApplicationRequest::Normalize { request, output_dir })
    } else {
        operation(ApplicationRequest::Bundle { request, output_dir })
    }
}

fn parse_vector(args: &[String]) -> PpResult<CliInput> {
    let input = positional_input(args, "vector")?;
    let values = options(
        &args[1..],
        &[
            "--out", "--preset", "--profile", "--detail", "--min-quality",
            "--max-quality-loss", "--max-paths", "--policy", "--report", "--diagnostics",
        ],
    )?;
    let detail = match value(&values, "--detail") {
        None | Some("auto") => None,
        Some(raw) => Some(raw.parse::<u8>().map_err(|_| {
            PpError::InvalidOption("--detail must be auto or an integer from 1 through 5".to_string())
        })?),
    };
    operation(ApplicationRequest::Vector {
        input,
        output: required_path(&values, "--out")?,
        preset: optional_string(&values, "--preset"),
        profile: optional_string(&values, "--profile"),
        detail,
        min_quality: optional_parse(
            &values,
            "--min-quality",
            "--min-quality and --max-quality-loss must be finite numbers from 0 through 1",
        )?,
        max_quality_loss: optional_parse(
            &values,
            "--max-quality-loss",
            "--min-quality and --max-quality-loss must be finite numbers from 0 through 1",
        )?,
        max_paths: optional_parse(&values, "--max-paths", "--max-paths must be a positive integer")?,
        policy: optional_path(&values, "--policy"),
        report: optional_path(&values, "--report"),
        diagnostics: optional_path(&values, "--diagnostics"),
    })
}

fn parse_vector_analyze(args: &[String]) -> PpResult<CliInput> {
    let input = positional_input(args, "vector-analyze")?;
    let values = options(&args[1..], &["--preset", "--profile", "--policy", "--report"])?;
    operation(ApplicationRequest::VectorAnalyze {
        input,
        preset: optional_string(&values, "--preset"),
        profile: optional_string(&values, "--profile"),
        policy: optional_path(&values, "--policy"),
        report: optional_path(&values, "--report"),
    })
}

fn parse_motion_scaffold(args: &[String]) -> PpResult<CliInput> {
    let input = positional_input(args, "motion-scaffold")?;
    let values = options(&args[1..], &["--out-dir"])?;
    operation(ApplicationRequest::MotionScaffold {
        input,
        output_dir: required_path(&values, "--out-dir")?,
    })
}

fn parse_motion_build(args: &[String]) -> PpResult<CliInput> {
    let values = options(args, &["--request", "--out-dir"])?;
    operation(ApplicationRequest::MotionBuild {
        request: required_path(&values, "--request")?,
        output_dir: required_path(&values, "--out-dir")?,
    })
}

fn one_request(
    args: &[String],
    make: impl FnOnce(PathBuf) -> Operation,
) -> PpResult<CliInput> {
    let values = options(args, &["--request"])?;
    Ok(CliInput::Operation(make(required_path(&values, "--request")?)))
}

fn positional_input(args: &[String], command: &str) -> PpResult<PathBuf> {
    let Some(input) = args.first() else {
        return Err(PpError::InvalidOption(format!(
            "{command} requires <input>"
        )));
    };
    if input.starts_with("--") {
        return Err(PpError::InvalidOption(format!(
            "{command} requires <input>"
        )));
    }
    Ok(PathBuf::from(input))
}

#[derive(Debug)]
struct OptionValue {
    key: String,
    value: String,
}

fn options(args: &[String], allowed: &[&str]) -> PpResult<Vec<OptionValue>> {
    if args.len() % 2 != 0 {
        let key = args.last().map(String::as_str).unwrap_or("option");
        return Err(PpError::InvalidOption(format!(
            "missing value for option '{key}'"
        )));
    }
    let mut result = Vec::with_capacity(args.len() / 2);
    for pair in args.chunks_exact(2) {
        let key = pair[0].as_str();
        if !key.starts_with("--") {
            return Err(PpError::InvalidOption(format!(
                "unexpected positional argument '{key}'"
            )));
        }
        if !allowed.contains(&key) {
            return Err(PpError::InvalidOption(format!("unknown option '{key}'")));
        }
        if result.iter().any(|entry: &OptionValue| entry.key == key) {
            return Err(PpError::InvalidOption(format!("duplicate option '{key}'")));
        }
        if pair[1].starts_with("--") {
            return Err(PpError::InvalidOption(format!(
                "missing value for option '{key}'"
            )));
        }
        result.push(OptionValue {
            key: key.to_string(),
            value: pair[1].clone(),
        });
    }
    Ok(result)
}

fn value<'a>(values: &'a [OptionValue], key: &str) -> Option<&'a str> {
    values
        .iter()
        .find(|entry| entry.key == key)
        .map(|entry| entry.value.as_str())
}

fn required_path(values: &[OptionValue], key: &str) -> PpResult<PathBuf> {
    value(values, key)
        .map(PathBuf::from)
        .ok_or_else(|| PpError::InvalidOption(format!("{key} is required")))
}

fn optional_path(values: &[OptionValue], key: &str) -> Option<PathBuf> {
    value(values, key).map(PathBuf::from)
}

fn optional_string(values: &[OptionValue], key: &str) -> Option<String> {
    value(values, key).map(str::to_string)
}

fn optional_parse<T: std::str::FromStr>(
    values: &[OptionValue],
    key: &str,
    message: &str,
) -> PpResult<Option<T>> {
    value(values, key)
        .map(|raw| raw.parse::<T>().map_err(|_| PpError::InvalidOption(message.to_string())))
        .transpose()
}

fn required_parse<T: std::str::FromStr>(
    values: &[OptionValue],
    key: &str,
    message: &str,
) -> PpResult<T> {
    optional_parse(values, key, message)?
        .ok_or_else(|| PpError::InvalidOption(format!("{key} is required")))
}

fn reject_extra(args: &[String], command: &str) -> PpResult<()> {
    if args.is_empty() {
        Ok(())
    } else {
        Err(PpError::InvalidOption(format!(
            "{command} does not accept extra arguments"
        )))
    }
}

fn operation(request: ApplicationRequest) -> PpResult<CliInput> {
    Ok(CliInput::Operation(Operation::try_from(request)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    #[test]
    fn duplicate_options_fail_before_operation() {
        assert!(parse(&args(&[
            "convert", "a.png", "--out", "b.png", "--out", "c.png"
        ]))
        .is_err());
    }

    #[test]
    fn semantic_scale_validation_comes_from_operation_conversion() {
        assert!(parse(&args(&[
            "upscale", "a.png", "--out", "b.png", "--scale", "1"
        ]))
        .is_err());
    }

    #[test]
    fn request_driven_compilers_are_thin_operation_adapters() -> PpResult<()> {
        let cases = [
            ("document-psd", "document.compile_psd"),
            ("texture-compile", "texture.compile"),
            ("vision-foreground-instances", "vision.apple.foreground_instances"),
        ];
        for (command, expected) in cases {
            let parsed = parse(&args(&[command, "--request", "request.json"]))?;
            let CliInput::Operation(operation) = parsed else {
                panic!("expected operation");
            };
            assert_eq!(operation.spec().name, expected);
        }
        Ok(())
    }
}
