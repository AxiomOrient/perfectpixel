use std::path::PathBuf;

/// A typed application command shared by the CLI and MCP adapters.
#[derive(Debug, Clone)]
pub enum ApplicationRequest {
    Schema,
    Inspect {
        input: PathBuf,
    },
    Convert {
        input: PathBuf,
        output: PathBuf,
        width: Option<u32>,
        height: Option<u32>,
        filter: Option<String>,
        jpeg_quality: Option<u8>,
        background: Option<String>,
    },
    Upscale {
        input: PathBuf,
        output: PathBuf,
        scale: u32,
        filter: Option<String>,
        jpeg_quality: Option<u8>,
        background: Option<String>,
    },
    Normalize {
        request: PathBuf,
        output_dir: PathBuf,
    },
    Bundle {
        request: PathBuf,
        output_dir: PathBuf,
    },
    Vector {
        input: PathBuf,
        output: PathBuf,
        preset: Option<String>,
        profile: Option<String>,
        detail: Option<u8>,
        min_quality: Option<f64>,
        max_quality_loss: Option<f64>,
        max_paths: Option<usize>,
        policy: Option<PathBuf>,
        report: Option<PathBuf>,
        diagnostics: Option<PathBuf>,
    },
    VectorAnalyze {
        input: PathBuf,
        preset: Option<String>,
        profile: Option<String>,
        policy: Option<PathBuf>,
        report: Option<PathBuf>,
    },
    MotionScaffold {
        input: PathBuf,
        output_dir: PathBuf,
    },
    MotionBuild {
        request: PathBuf,
        output_dir: PathBuf,
    },
}

impl ApplicationRequest {
    pub(crate) fn into_cli_args(self) -> Vec<String> {
        let mut args = Vec::new();
        match self {
            Self::Schema => args.push("schema".to_string()),
            Self::Inspect { input } => args.extend(["inspect".to_string(), path_arg(input)]),
            Self::Convert {
                input,
                output,
                width,
                height,
                filter,
                jpeg_quality,
                background,
            } => {
                args.extend([
                    "convert".to_string(),
                    path_arg(input),
                    "--out".to_string(),
                    path_arg(output),
                ]);
                push_option(&mut args, "--width", width);
                push_option(&mut args, "--height", height);
                push_option(&mut args, "--filter", filter);
                push_option(&mut args, "--jpeg-quality", jpeg_quality);
                push_option(&mut args, "--background", background);
            }
            Self::Upscale {
                input,
                output,
                scale,
                filter,
                jpeg_quality,
                background,
            } => {
                args.extend([
                    "upscale".to_string(),
                    path_arg(input),
                    "--out".to_string(),
                    path_arg(output),
                ]);
                push_option(&mut args, "--scale", Some(scale));
                push_option(&mut args, "--filter", filter);
                push_option(&mut args, "--jpeg-quality", jpeg_quality);
                push_option(&mut args, "--background", background);
            }
            Self::Normalize {
                request,
                output_dir,
            } => args.extend([
                "normalize".to_string(),
                "--request".to_string(),
                path_arg(request),
                "--out-dir".to_string(),
                path_arg(output_dir),
            ]),
            Self::Bundle {
                request,
                output_dir,
            } => args.extend([
                "bundle".to_string(),
                "--request".to_string(),
                path_arg(request),
                "--out-dir".to_string(),
                path_arg(output_dir),
            ]),
            Self::Vector {
                input,
                output,
                preset,
                profile,
                detail,
                min_quality,
                max_quality_loss,
                max_paths,
                policy,
                report,
                diagnostics,
            } => {
                args.extend([
                    "vector".to_string(),
                    path_arg(input),
                    "--out".to_string(),
                    path_arg(output),
                ]);
                push_option(&mut args, "--preset", preset);
                push_option(&mut args, "--profile", profile);
                push_option(&mut args, "--detail", detail);
                push_option(&mut args, "--min-quality", min_quality);
                push_option(&mut args, "--max-quality-loss", max_quality_loss);
                push_option(&mut args, "--max-paths", max_paths);
                push_option(&mut args, "--policy", policy.map(path_arg));
                push_option(&mut args, "--report", report.map(path_arg));
                push_option(&mut args, "--diagnostics", diagnostics.map(path_arg));
            }
            Self::VectorAnalyze {
                input,
                preset,
                profile,
                policy,
                report,
            } => {
                args.extend(["vector-analyze".to_string(), path_arg(input)]);
                push_option(&mut args, "--preset", preset);
                push_option(&mut args, "--profile", profile);
                push_option(&mut args, "--policy", policy.map(path_arg));
                push_option(&mut args, "--report", report.map(path_arg));
            }
            Self::MotionScaffold { input, output_dir } => args.extend([
                "motion-scaffold".to_string(),
                path_arg(input),
                "--out-dir".to_string(),
                path_arg(output_dir),
            ]),
            Self::MotionBuild {
                request,
                output_dir,
            } => args.extend([
                "motion-build".to_string(),
                "--request".to_string(),
                path_arg(request),
                "--out-dir".to_string(),
                path_arg(output_dir),
            ]),
        }
        args
    }
}

fn path_arg(path: PathBuf) -> String {
    path.into_os_string()
        .into_string()
        .expect("application paths must be valid UTF-8")
}

fn push_option<T: ToString>(args: &mut Vec<String>, flag: &str, value: Option<T>) {
    if let Some(value) = value {
        args.push(flag.to_string());
        args.push(value.to_string());
    }
}
