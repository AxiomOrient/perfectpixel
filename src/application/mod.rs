mod asset_codec;
mod cli;
mod generation;
mod generation_adapter;
mod handlers;
mod path;
mod request;
mod shared;

pub use request::ApplicationRequest;
pub use shared::ApplicationOutput;
pub use crate::{PpError, PpResult};

use crate::Operation;
use cli::CliInput;
use shared::{operation_phase, render_result};

const HELP: &str = r#"perfectpixel 0.3.1

PRODUCT
  deterministic asset compiler for authored or generated local assets.
  It transforms and verifies assets; model inference is never a core success authority.
  Successful managed outputs are evaluated before atomic publication.

USAGE
  perfectpixel schema
  perfectpixel inspect <input.png|jpg|jpeg|webp>
  perfectpixel convert <input> --out <output> [--width <n>] [--height <n>] [--filter nearest|lanczos3] [--jpeg-quality <1..100>] [--background <#RRGGBB>]
  perfectpixel upscale <input> --out <output> --scale <n>=2 [--filter nearest|lanczos3] [--jpeg-quality <1..100>] [--background <#RRGGBB>]
  perfectpixel edit --request <edit-request.json>
  perfectpixel psd --request <psd-export-request.json>
  perfectpixel document-psd --request <document-psd-request.json>
  perfectpixel chroma-plan --request <chroma-plan-request.json>
  perfectpixel normalize --request <normalize-request.json> --out-dir <dir>
  perfectpixel bundle --request <sprite-request.json> --out-dir <dir>
  perfectpixel texture-compile --request <texture-request.json>
  perfectpixel vector <input> --out <output.svg> [--preset <preset>] [--profile <profile>] [--detail auto|1..5] [--min-quality <0..1>] [--max-quality-loss <0..1>] [--max-paths <n>] [--policy <json>] [--report <json>] [--diagnostics <dir>]
  perfectpixel vector-analyze <input> [--preset <preset>] [--profile <profile>] [--policy <json>] [--report <json>]
  perfectpixel vision-foreground-instances --request <vision-request.json>
  perfectpixel motion-scaffold <input.svg> --out-dir <dir>
  perfectpixel motion-build --request <motion-request.json> --out-dir <dir>
"#;

/// MCP/programmatic compatibility entry. The transport DTO is converted once into the same
/// canonical Operation used by the CLI; no argv reconstruction or CLI reparsing is involved.
pub fn execute(request: ApplicationRequest) -> ApplicationOutput {
    match Operation::try_from(request) {
        Ok(operation) => execute_operation(operation),
        Err(error) => render_result(Err(error), "application.request", "application"),
    }
}

/// Human CLI adapter. Parsing owns syntax only; all semantics and execution converge on Operation.
pub fn execute_cli(args: Vec<String>) -> ApplicationOutput {
    match cli::parse(&args) {
        Ok(CliInput::Help) => ApplicationOutput::from_text(HELP.to_string(), 0),
        Ok(CliInput::Operation(operation)) => execute_operation(operation),
        Err(error) => render_result(Err(error), "cli.parse", "cli"),
    }
}

/// Single application dispatcher shared by every transport.
pub(crate) fn execute_operation(operation: Operation) -> ApplicationOutput {
    let spec = operation.spec();
    let phase = operation_phase(spec.name);
    render_result(dispatch(operation), spec.name, phase)
}

fn dispatch(operation: Operation) -> PpResult<String> {
    match operation {
        Operation::Schema => handlers::schema(),
        Operation::Inspect { input } => handlers::inspect(input),
        Operation::Convert {
            input,
            output,
            width,
            height,
            filter,
            jpeg_quality,
            background,
        } => handlers::convert(
            input,
            output,
            width,
            height,
            filter,
            jpeg_quality,
            background,
        ),
        Operation::Upscale {
            input,
            output,
            scale,
            filter,
            jpeg_quality,
            background,
        } => handlers::upscale(input, output, scale, filter, jpeg_quality, background),
        Operation::Edit { request } => handlers::edit(request),
        Operation::ExportPsd { request } => handlers::export_psd(request),
        Operation::CompileDocumentPsd { request } => handlers::compile_document_psd(request),
        Operation::ChromaPlan { request } => handlers::chroma_plan(request),
        Operation::NormalizeSprite {
            request,
            output_dir,
        } => handlers::normalize(request, output_dir),
        Operation::CompileSprite {
            request,
            output_dir,
        } => handlers::bundle(request, output_dir),
        Operation::CompileTexture { request } => handlers::texture_compile(request),
        Operation::CompileVector {
            input,
            output,
            preset,
            profile,
            detail,
            minimum_quality,
            maximum_quality_loss,
            maximum_paths,
            policy,
            report,
            diagnostics,
        } => handlers::vector_compile(
            input,
            output,
            preset,
            profile,
            detail,
            minimum_quality,
            maximum_quality_loss,
            maximum_paths,
            policy,
            report,
            diagnostics,
        ),
        Operation::AnalyzeVector {
            input,
            preset,
            profile,
            policy,
            report,
        } => handlers::vector_analyze(input, preset, profile, policy, report),
        Operation::AppleVisionForegroundInstances { request } => {
            handlers::vision_foreground_instances(request)
        }
        Operation::ScaffoldMotion { input, output_dir } => {
            handlers::motion_scaffold(input, output_dir)
        }
        Operation::CompileMotion {
            request,
            output_dir,
        } => handlers::motion_build(request, output_dir),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    #[test]
    fn schema_rejects_extra_cli_args() {
        assert_ne!(execute_cli(args(&["schema", "extra"])).exit_code, 0);
    }

    #[test]
    fn unknown_legacy_command_is_not_reachable() {
        assert_ne!(
            execute_cli(args(&[
                "pack-views",
                "--request",
                "a.json",
                "--out-dir",
                "out"
            ]))
            .exit_code,
            0
        );
    }

    #[test]
    fn application_request_does_not_roundtrip_through_cli() {
        let output = execute(ApplicationRequest::Schema);
        assert_eq!(output.exit_code, 0);
        assert!(output.stdout.contains("deterministic-asset-compiler"));
    }

    #[test]
    fn cli_parse_failure_has_structured_operation_failure() {
        let output = execute_cli(args(&["convert"]));
        assert_ne!(output.exit_code, 0);
        let value: serde_json::Value = serde_json::from_str(output.stdout.trim()).expect("json");
        assert_eq!(value["failure"]["operation"], "cli.parse");
        assert_eq!(value["failure"]["context"][0]["key"], "phase");
    }
}
