use std::{
    borrow::Cow,
    path::{Path, PathBuf},
    sync::Arc,
};

use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{
        CallToolResult, ErrorCode, InitializeRequestParams, InitializeResult, ProtocolVersion,
        ServerCapabilities, ServerInfo,
    },
    service::RequestContext,
    tool, tool_handler, tool_router, ErrorData, RoleServer, ServerHandler, ServiceExt,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::{oneshot, Semaphore};

use crate::adapters::motion::MotionRequest;
use crate::adapters::sprite::{NormalizeRequest, SpriteBundleRequest};
use crate::application::{execute, ApplicationRequest};

mod params;
mod root;

use params::{
    ConvertParams, InspectParams, MotionBuildParams, MotionScaffoldParams, RequestDirectoryParams,
    SchemaParams, UpscaleParams, VectorAnalyzeParams, VectorParams,
};
use root::{Root, RootPathError};

pub(crate) const MCP_RESULT_SCHEMA: &str = "perfectpixel.mcp-tool-result/1";
const MAX_REQUEST_PREPARSE_BYTES: usize = 8 * 1024 * 1024;
pub const MCP_HELP: &str = r#"perfectpixel-mcp 0.3.0

USAGE
  perfectpixel-mcp --root <ABSOLUTE_EXISTING_DIRECTORY>
  perfectpixel-mcp --help

The configured root is fixed for the process. MCP transport is stdio only.
"#;

#[derive(Debug, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
struct McpToolResultEnvelope {
    schema: &'static str,
    ok: bool,
    operation: String,
    exit_code: i32,
    result: Value,
}

#[derive(Debug)]
struct OperationResult {
    envelope: McpToolResultEnvelope,
}

#[derive(Debug)]
enum PreparationError {
    Root(RootPathError),
    Invalid(String),
}

impl PreparationError {
    fn message(&self) -> String {
        match self {
            Self::Root(error) => error.message().to_string(),
            Self::Invalid(message) => message.clone(),
        }
    }
}

#[derive(Debug)]
pub enum Startup {
    Help,
    Server(PerfectPixelMcp),
}

#[derive(Debug)]
pub struct StartupError {
    message: String,
}

impl StartupError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for StartupError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for StartupError {}

pub fn startup(args: Vec<String>) -> Result<Startup, StartupError> {
    match args.as_slice() {
        [help] if help == "--help" => Ok(Startup::Help),
        [flag, root] if flag == "--root" => {
            let root = Root::new(PathBuf::from(root)).map_err(|error| {
                StartupError::new(format!("invalid --root: {}", error.message()))
            })?;
            Ok(Startup::Server(PerfectPixelMcp::new(root)))
        }
        _ => Err(StartupError::new(
            "usage: perfectpixel-mcp --root <ABSOLUTE_EXISTING_DIRECTORY> | --help",
        )),
    }
}

#[derive(Clone)]
pub struct PerfectPixelMcp {
    root: Root,
    active_operation: Arc<Semaphore>,
    tool_router: ToolRouter<Self>,
}

impl std::fmt::Debug for PerfectPixelMcp {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PerfectPixelMcp")
            .field("root", &self.root.canonical())
            .finish_non_exhaustive()
    }
}

impl PerfectPixelMcp {
    pub(crate) fn new(root: Root) -> Self {
        Self {
            root,
            active_operation: Arc::new(Semaphore::new(1)),
            tool_router: Self::tool_router(),
        }
    }

    async fn run_operation<F>(&self, operation: &'static str, prepare: F) -> CallToolResult
    where
        F: FnOnce(&Root) -> Result<ApplicationRequest, PreparationError> + Send + 'static,
    {
        let permit = match Arc::clone(&self.active_operation).try_acquire_owned() {
            Ok(permit) => permit,
            Err(_) => {
                return render_result(OperationResult {
                    envelope: error_envelope(
                        operation,
                        2,
                        json!({
                            "ok": false,
                            "code": "busy",
                            "message": "another operation is active; no queue is available",
                        }),
                    ),
                });
            }
        };

        let root = self.root.clone();
        let (sender, receiver) = oneshot::channel();
        tokio::task::spawn_blocking(move || {
            let result = match prepare(&root) {
                Ok(request) => match revalidate_application_request(&root, &request) {
                    Ok(()) => {
                        let output = execute(request);
                        application_result(operation, output.exit_code, &output.stdout)
                    }
                    Err(error) => OperationResult {
                        envelope: error_envelope(
                            operation,
                            2,
                            json!({
                                "ok": false,
                                "code": "rootOrPathRejected",
                                "message": error.message(),
                            }),
                        ),
                    },
                },
                Err(error) => OperationResult {
                    envelope: error_envelope(
                        operation,
                        2,
                        json!({
                            "ok": false,
                            "code": "rootOrPathRejected",
                            "message": error.message(),
                        }),
                    ),
                },
            };
            let _ = sender.send(result);
            drop(permit);
        });

        match receiver.await {
            Ok(result) => render_result(result),
            Err(_) => render_result(OperationResult {
                envelope: error_envelope(
                    operation,
                    2,
                    json!({
                        "ok": false,
                        "code": "internal",
                        "message": "internal MCP error: operation worker terminated",
                    }),
                ),
            }),
        }
    }
}

#[tool_router(router = tool_router)]
impl PerfectPixelMcp {
    #[tool(
        name = "perfectpixel_schema",
        description = "Return the existing perfectpixel CLI schema JSON.",
        output_schema = rmcp::handler::server::common::schema_for_output::<McpToolResultEnvelope>(),
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    pub async fn perfectpixel_schema(
        &self,
        Parameters(params): Parameters<SchemaParams>,
    ) -> CallToolResult {
        let _ = params;
        self.run_operation("perfectpixel_schema", move |root| {
            let _ = root;
            Ok(ApplicationRequest::Schema)
        })
        .await
    }

    #[tool(
        name = "perfectpixel_inspect",
        description = "Inspect one root-bounded raster file without writing.",
        output_schema = rmcp::handler::server::common::schema_for_output::<McpToolResultEnvelope>(),
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    pub async fn perfectpixel_inspect(
        &self,
        Parameters(params): Parameters<InspectParams>,
    ) -> CallToolResult {
        self.run_operation("perfectpixel_inspect", move |root| {
            let input = root
                .input_file(&params.input_path)
                .map_err(PreparationError::Root)?;
            Ok(ApplicationRequest::Inspect { input })
        })
        .await
    }

    #[tool(
        name = "perfectpixel_convert",
        description = "Convert or resize one root-bounded raster file.",
        output_schema = rmcp::handler::server::common::schema_for_output::<McpToolResultEnvelope>(),
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    pub async fn perfectpixel_convert(
        &self,
        Parameters(params): Parameters<ConvertParams>,
    ) -> CallToolResult {
        self.run_operation("perfectpixel_convert", move |root| {
            if params.width == Some(0) || params.height == Some(0) {
                return Err(PreparationError::Invalid(
                    "width and height must be positive when supplied".to_string(),
                ));
            }
            if params
                .jpeg_quality
                .is_some_and(|quality| !(1..=100).contains(&quality))
            {
                return Err(PreparationError::Invalid(
                    "jpegQuality must be in 1..=100".to_string(),
                ));
            }
            let input = root
                .input_file(&params.input_path)
                .map_err(PreparationError::Root)?;
            let output = root
                .output_file(&params.output_path)
                .map_err(PreparationError::Root)?;
            Ok(ApplicationRequest::Convert {
                input,
                output,
                width: params.width,
                height: params.height,
                filter: params.filter.map(|filter| filter.as_str().to_string()),
                jpeg_quality: params.jpeg_quality,
                background: params.background,
            })
        })
        .await
    }

    #[tool(
        name = "perfectpixel_upscale",
        description = "Upscale one root-bounded raster file.",
        output_schema = rmcp::handler::server::common::schema_for_output::<McpToolResultEnvelope>(),
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    pub async fn perfectpixel_upscale(
        &self,
        Parameters(params): Parameters<UpscaleParams>,
    ) -> CallToolResult {
        self.run_operation("perfectpixel_upscale", move |root| {
            if params.scale < 2 {
                return Err(PreparationError::Invalid(
                    "scale must be in 2..=u32::MAX".to_string(),
                ));
            }
            if params
                .jpeg_quality
                .is_some_and(|quality| !(1..=100).contains(&quality))
            {
                return Err(PreparationError::Invalid(
                    "jpegQuality must be in 1..=100".to_string(),
                ));
            }
            let input = root
                .input_file(&params.input_path)
                .map_err(PreparationError::Root)?;
            let output = root
                .output_file(&params.output_path)
                .map_err(PreparationError::Root)?;
            Ok(ApplicationRequest::Upscale {
                input,
                output,
                scale: params.scale,
                filter: params.filter.map(|filter| filter.as_str().to_string()),
                jpeg_quality: params.jpeg_quality,
                background: params.background,
            })
        })
        .await
    }

    #[tool(
        name = "perfectpixel_normalize",
        description = "Normalize a typed sprite request and publish its generated set under root.",
        output_schema = rmcp::handler::server::common::schema_for_output::<McpToolResultEnvelope>(),
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    pub async fn perfectpixel_normalize(
        &self,
        Parameters(params): Parameters<RequestDirectoryParams>,
    ) -> CallToolResult {
        self.run_operation("perfectpixel_normalize", move |root| {
            prepare_normalize(root, params)
        })
        .await
    }

    #[tool(
        name = "perfectpixel_bundle",
        description = "Build a typed sprite bundle and publish its generated set under root.",
        output_schema = rmcp::handler::server::common::schema_for_output::<McpToolResultEnvelope>(),
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    pub async fn perfectpixel_bundle(
        &self,
        Parameters(params): Parameters<RequestDirectoryParams>,
    ) -> CallToolResult {
        self.run_operation("perfectpixel_bundle", move |root| {
            prepare_bundle(root, params)
        })
        .await
    }

    #[tool(
        name = "perfectpixel_vector",
        description = "Run the quality-gated raster-to-SVG vector command under root.",
        output_schema = rmcp::handler::server::common::schema_for_output::<McpToolResultEnvelope>(),
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    pub async fn perfectpixel_vector(
        &self,
        Parameters(params): Parameters<VectorParams>,
    ) -> CallToolResult {
        self.run_operation("perfectpixel_vector", move |root| {
            prepare_vector(root, params)
        })
        .await
    }

    #[tool(
        name = "perfectpixel_vector_analyze",
        description = "Analyze one raster under root; reportPath, when supplied, is a managed write.",
        output_schema = rmcp::handler::server::common::schema_for_output::<McpToolResultEnvelope>(),
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    pub async fn perfectpixel_vector_analyze(
        &self,
        Parameters(params): Parameters<VectorAnalyzeParams>,
    ) -> CallToolResult {
        self.run_operation("perfectpixel_vector_analyze", move |root| {
            prepare_vector_analyze(root, params)
        })
        .await
    }

    #[tool(
        name = "perfectpixel_motion_scaffold",
        description = "Scaffold a raster-free SVG into a generated motion workspace under root.",
        output_schema = rmcp::handler::server::common::schema_for_output::<McpToolResultEnvelope>(),
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    pub async fn perfectpixel_motion_scaffold(
        &self,
        Parameters(params): Parameters<MotionScaffoldParams>,
    ) -> CallToolResult {
        self.run_operation("perfectpixel_motion_scaffold", move |root| {
            let input = root
                .input_file(&params.input_path)
                .map_err(PreparationError::Root)?;
            let output_dir = root
                .output_dir(&params.output_dir)
                .map_err(PreparationError::Root)?;
            Ok(ApplicationRequest::MotionScaffold { input, output_dir })
        })
        .await
    }

    #[tool(
        name = "perfectpixel_motion_build",
        description = "Build an accepted motion request and publish its generated set under root.",
        output_schema = rmcp::handler::server::common::schema_for_output::<McpToolResultEnvelope>(),
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    pub async fn perfectpixel_motion_build(
        &self,
        Parameters(params): Parameters<MotionBuildParams>,
    ) -> CallToolResult {
        self.run_operation("perfectpixel_motion_build", move |root| {
            prepare_motion_build(root, params)
        })
        .await
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for PerfectPixelMcp {
    fn supported_protocol_versions(&self) -> Cow<'static, [ProtocolVersion]> {
        Cow::Borrowed(&[ProtocolVersion::V_2026_07_28])
    }

    async fn initialize(
        &self,
        _request: InitializeRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<InitializeResult, ErrorData> {
        Err(ErrorData::new(
            ErrorCode::METHOD_NOT_FOUND,
            "initialize is not available in MCP 2026-07-28",
            None,
        ))
    }

    fn get_info(&self) -> ServerInfo {
        let mut info = ServerInfo::new(ServerCapabilities::builder().enable_tools().build());
        info.protocol_version = ProtocolVersion::V_2026_07_28;
        info.with_server_info(rmcp::model::Implementation::new(
            "perfectpixel-mcp",
            env!("CARGO_PKG_VERSION"),
        ))
    }
}

fn prepare_normalize(
    root: &Root,
    params: RequestDirectoryParams,
) -> Result<ApplicationRequest, PreparationError> {
    let request = root
        .input_file(&params.request_path)
        .map_err(PreparationError::Root)?;
    let output_dir = root
        .output_dir(&params.output_dir)
        .map_err(PreparationError::Root)?;
    Ok(ApplicationRequest::Normalize {
        request,
        output_dir,
    })
}

fn prepare_bundle(
    root: &Root,
    params: RequestDirectoryParams,
) -> Result<ApplicationRequest, PreparationError> {
    let request = root
        .input_file(&params.request_path)
        .map_err(PreparationError::Root)?;
    let output_dir = root
        .output_dir(&params.output_dir)
        .map_err(PreparationError::Root)?;
    Ok(ApplicationRequest::Bundle {
        request,
        output_dir,
    })
}

fn prepare_vector(
    root: &Root,
    params: VectorParams,
) -> Result<ApplicationRequest, PreparationError> {
    if params
        .min_quality
        .is_some_and(|value| !value.is_finite() || !(0.0..=1.0).contains(&value))
        || params
            .max_quality_loss
            .is_some_and(|value| !value.is_finite() || !(0.0..=1.0).contains(&value))
    {
        return Err(PreparationError::Invalid(
            "minQuality and maxQualityLoss must be finite values in 0..=1".to_string(),
        ));
    }
    if params.max_paths == Some(0) {
        return Err(PreparationError::Invalid(
            "maxPaths must be a positive integer".to_string(),
        ));
    }
    let input = root
        .input_file(&params.input_path)
        .map_err(PreparationError::Root)?;
    let output = root
        .output_file(&params.output_path)
        .map_err(PreparationError::Root)?;
    let policy = params
        .policy_path
        .as_deref()
        .map(|path| root.input_file(path).map_err(PreparationError::Root))
        .transpose()?;
    let report = params
        .report_path
        .as_deref()
        .map(|path| root.output_file(path).map_err(PreparationError::Root))
        .transpose()?;
    let diagnostics = params
        .diagnostics_dir
        .as_deref()
        .map(|path| root.output_dir(path).map_err(PreparationError::Root))
        .transpose()?;
    Ok(ApplicationRequest::Vector {
        input,
        output,
        preset: params.preset.map(|value| value.as_str().to_string()),
        profile: params.profile.map(|value| value.as_str().to_string()),
        detail: params
            .detail
            .and_then(|value| value.as_cli_value())
            .and_then(|value| value.parse().ok()),
        min_quality: params.min_quality,
        max_quality_loss: params.max_quality_loss,
        max_paths: params.max_paths,
        policy,
        report,
        diagnostics,
    })
}

fn prepare_vector_analyze(
    root: &Root,
    params: VectorAnalyzeParams,
) -> Result<ApplicationRequest, PreparationError> {
    let input = root
        .input_file(&params.input_path)
        .map_err(PreparationError::Root)?;
    let policy = params
        .policy_path
        .as_deref()
        .map(|path| root.input_file(path).map_err(PreparationError::Root))
        .transpose()?;
    let report = params
        .report_path
        .as_deref()
        .map(|path| root.output_file(path).map_err(PreparationError::Root))
        .transpose()?;
    Ok(ApplicationRequest::VectorAnalyze {
        input,
        preset: params.preset.map(|value| value.as_str().to_string()),
        profile: params.profile.map(|value| value.as_str().to_string()),
        policy,
        report,
    })
}

fn prepare_motion_build(
    root: &Root,
    params: MotionBuildParams,
) -> Result<ApplicationRequest, PreparationError> {
    let request = root
        .input_file(&params.request_path)
        .map_err(PreparationError::Root)?;
    let output_dir = root
        .output_dir(&params.output_dir)
        .map_err(PreparationError::Root)?;
    Ok(ApplicationRequest::MotionBuild {
        request,
        output_dir,
    })
}

enum RequestPreparse<T> {
    Parsed(T),
    RejectedByTypedParser(serde_json::error::Category),
}

fn preparse_request<T: for<'de> Deserialize<'de>>(
    root: &Root,
    path: &Path,
) -> Result<RequestPreparse<T>, PreparationError> {
    let bytes = root
        .read_bounded_regular_file(path, MAX_REQUEST_PREPARSE_BYTES)
        .map_err(PreparationError::Root)?;
    match serde_json::from_slice(&bytes) {
        Ok(request) => Ok(RequestPreparse::Parsed(request)),
        Err(error) => {
            let category = error.classify();
            if category == serde_json::error::Category::Io {
                return Err(PreparationError::Invalid(format!(
                    "request preparse failed: {error}"
                )));
            }
            Ok(RequestPreparse::RejectedByTypedParser(category))
        }
    }
}

fn revalidate_application_request(
    root: &Root,
    request: &ApplicationRequest,
) -> Result<(), PreparationError> {
    let input = |path: &Path| {
        root.revalidate_input_file(path)
            .map_err(PreparationError::Root)
    };
    let output_file = |path: &Path| {
        root.revalidate_output_file(path)
            .map_err(PreparationError::Root)
    };
    let output_dir = |path: &Path| {
        root.revalidate_output_dir(path)
            .map_err(PreparationError::Root)
    };

    match request {
        ApplicationRequest::Schema => Ok(()),
        ApplicationRequest::Inspect { input: path } => input(path),
        ApplicationRequest::Convert {
            input: source,
            output,
            ..
        }
        | ApplicationRequest::Upscale {
            input: source,
            output,
            ..
        } => {
            input(source)?;
            output_file(output)
        }
        ApplicationRequest::Normalize {
            request,
            output_dir: destination,
        } => {
            revalidate_normalize_request(root, request)?;
            output_dir(destination)
        }
        ApplicationRequest::Bundle {
            request,
            output_dir: destination,
        } => {
            revalidate_bundle_request(root, request)?;
            output_dir(destination)
        }
        ApplicationRequest::Vector {
            input: source,
            output,
            policy,
            report,
            diagnostics,
            ..
        } => {
            input(source)?;
            output_file(output)?;
            if let Some(path) = policy {
                input(path)?;
            }
            if let Some(path) = report {
                output_file(path)?;
            }
            if let Some(path) = diagnostics {
                output_dir(path)?;
            }
            Ok(())
        }
        ApplicationRequest::VectorAnalyze {
            input: source,
            policy,
            report,
            ..
        } => {
            input(source)?;
            if let Some(path) = policy {
                input(path)?;
            }
            if let Some(path) = report {
                output_file(path)?;
            }
            Ok(())
        }
        ApplicationRequest::MotionScaffold {
            input: source,
            output_dir: destination,
        } => {
            input(source)?;
            output_dir(destination)
        }
        ApplicationRequest::MotionBuild {
            request,
            output_dir: destination,
        } => {
            revalidate_motion_request(root, request)?;
            output_dir(destination)
        }
    }
}

fn revalidate_normalize_request(root: &Root, request: &Path) -> Result<(), PreparationError> {
    match preparse_request::<NormalizeRequest>(root, request)? {
        RequestPreparse::Parsed(request_data) => {
            for state in request_data.states {
                for frame in state.frames {
                    root.nested_input(request, &frame)
                        .map_err(PreparationError::Root)?;
                }
                if let Some(strip) = state.strip {
                    root.nested_input(request, &strip)
                        .map_err(PreparationError::Root)?;
                }
            }
        }
        RequestPreparse::RejectedByTypedParser(_category) => {}
    }
    Ok(())
}

fn revalidate_bundle_request(root: &Root, request: &Path) -> Result<(), PreparationError> {
    match preparse_request::<SpriteBundleRequest>(root, request)? {
        RequestPreparse::Parsed(request_data) => {
            for state in request_data.states {
                for frame in state.frames {
                    root.nested_input(request, &frame)
                        .map_err(PreparationError::Root)?;
                }
            }
        }
        RequestPreparse::RejectedByTypedParser(_category) => {}
    }
    Ok(())
}

fn revalidate_motion_request(root: &Root, request: &Path) -> Result<(), PreparationError> {
    match preparse_request::<MotionRequest>(root, request)? {
        RequestPreparse::Parsed(request_data) => {
            root.nested_input(request, &request_data.source_svg)
                .map_err(PreparationError::Root)?;
        }
        RequestPreparse::RejectedByTypedParser(_category) => {}
    }
    Ok(())
}

fn application_result(operation: &str, exit_code: i32, stdout: &str) -> OperationResult {
    let result = match serde_json::from_str::<Value>(stdout.trim()) {
        Ok(result) => result,
        Err(_) => {
            return OperationResult {
                envelope: error_envelope(
                    operation,
                    2,
                    json!({
                        "ok": false,
                        "code": "internal",
                        "message": "internal MCP error: application output was not valid JSON",
                    }),
                ),
            };
        }
    };
    OperationResult {
        envelope: McpToolResultEnvelope {
            schema: MCP_RESULT_SCHEMA,
            ok: exit_code == 0,
            operation: operation.to_string(),
            exit_code,
            result,
        },
    }
}

fn error_envelope(operation: &str, exit_code: i32, result: Value) -> McpToolResultEnvelope {
    McpToolResultEnvelope {
        schema: MCP_RESULT_SCHEMA,
        ok: false,
        operation: operation.to_string(),
        exit_code,
        result,
    }
}

fn render_result(result: OperationResult) -> CallToolResult {
    let value = serde_json::to_value(result.envelope).unwrap_or_else(|_| {
        json!({
            "schema": MCP_RESULT_SCHEMA,
            "ok": false,
            "operation": "internal",
            "exitCode": 2,
            "result": {
                "ok": false,
                "code": "internal",
                "message": "internal MCP error: envelope serialization failed",
            }
        })
    });
    if value.get("ok").and_then(Value::as_bool).unwrap_or(false) {
        CallToolResult::structured(value)
    } else {
        CallToolResult::structured_error(value)
    }
}

pub async fn serve(server: PerfectPixelMcp) -> Result<(), String> {
    let running = server
        .serve(rmcp::transport::stdio())
        .await
        .map_err(|error| error.to_string())?;
    running
        .waiting()
        .await
        .map(|_| ())
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        sync::atomic::{AtomicBool, Ordering},
        time::{Duration, SystemTime, UNIX_EPOCH},
    };

    fn temp_root(label: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "perfectpixel-mcp-server-{label}-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("create root");
        path
    }

    fn envelope(result: CallToolResult) -> Value {
        result.structured_content.expect("structured MCP result")
    }

    #[test]
    fn startup_accepts_only_help_or_an_absolute_root() {
        assert!(matches!(
            startup(vec!["--help".to_string()]),
            Ok(Startup::Help)
        ));
        assert!(startup(vec!["--root".to_string(), "relative".to_string()]).is_err());
        assert!(startup(vec!["--root".to_string()]).is_err());
        assert!(startup(vec!["--help".to_string(), "extra".to_string()]).is_err());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn concurrent_call_is_busy_and_cancellation_keeps_the_permit() {
        let root_path = temp_root("busy");
        let root = Root::new(root_path.clone()).expect("root");
        let server = Arc::new(PerfectPixelMcp::new(root));
        let started = Arc::new(AtomicBool::new(false));
        let finished = Arc::new(AtomicBool::new(false));

        let first_server = Arc::clone(&server);
        let first_started = Arc::clone(&started);
        let first_finished = Arc::clone(&finished);
        let first = tokio::spawn(async move {
            first_server
                .run_operation("test", move |_| {
                    first_started.store(true, Ordering::Release);
                    std::thread::sleep(Duration::from_millis(150));
                    first_finished.store(true, Ordering::Release);
                    Ok(ApplicationRequest::Schema)
                })
                .await
        });

        while !started.load(Ordering::Acquire) {
            tokio::task::yield_now().await;
        }

        let busy = server
            .run_operation("test", |_| Ok(ApplicationRequest::Schema))
            .await;
        let busy_value = envelope(busy);
        assert_eq!(busy_value["result"]["code"], "busy");
        assert_eq!(busy_value["ok"], false);

        first.abort();
        while !finished.load(Ordering::Acquire) {
            tokio::task::yield_now().await;
        }
        while server.active_operation.available_permits() == 0 {
            tokio::task::yield_now().await;
        }

        let after = server
            .run_operation("test", |_| Ok(ApplicationRequest::Schema))
            .await;
        let after_value = envelope(after);
        assert_eq!(after_value["ok"], true);

        let _ = first.await;
        let _ = fs::remove_dir_all(root_path);
    }
}
