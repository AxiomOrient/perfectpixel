# perfectpixel-mcp Contract

**Status: normative MCP adapter contract, v1.** The existing `perfectpixel` CLI contract in
[CONTRACT.md](CONTRACT.md) remains unchanged. This document defines only the local stdio
adapter and its typed tool boundary.

The platform contract is Rust `cfg(unix)` only: Windows and every non-Unix target fail at
compilation. The complete runtime gate is verified on macOS arm64.

## Boundary and installation

`perfectpixel-mcp` is a local Rust MCP server built on `rmcp = 3.1.4`. It uses the MCP protocol
version `2026-07-28` over Tokio stdio. It calls the same synchronous application execution
boundary as the CLI; it does not launch a CLI subprocess, call a generic runner, or duplicate a
publication writer.

The adapter is strict stateless-only. `server/discover` advertises exactly `2026-07-28`, every
subsequent request carries protocol version, client identity, and client capabilities in `_meta`,
and the retired `initialize` handshake is rejected with JSON-RPC `-32601`. No earlier protocol
version or lifecycle fallback is provided.

Build or install it with Cargo:

```bash
cargo build --release --locked --bin perfectpixel-mcp
cargo install --path . --locked
```

The startup command accepts exactly one of these forms:

```text
perfectpixel-mcp --root <ABSOLUTE_EXISTING_DIRECTORY>
perfectpixel-mcp --help
```

`--root` must name an existing, absolute, non-symlink directory. The server canonicalizes it at
startup and never changes it. The process CWD and MCP Roots notifications cannot enlarge or
replace this authority. Startup help and diagnostics use stdout/stderr respectively; after a
server starts, stdout is reserved for MCP frames and diagnostics remain on stderr.

The fixed root is a trusted-local application boundary. It is canonicalized at startup, remains
fixed for the process, and bounds every typed path and nested request source.

A client configuration uses an absolute executable and an absolute fixed root, for example:

```json
{
  "mcpServers": {
    "perfectpixel": {
      "command": "/absolute/path/to/perfectpixel-mcp",
      "args": ["--root", "/absolute/path/to/asset-root"]
    }
  }
}
```

There is no HTTP, WebSocket, service, runtime network, authentication, prompt, resource,
sampling, or remote filesystem transport in this adapter.

## Tool inventory

The server exposes exactly these ten tools. Names are stable and are not aliases for a generic
command runner.

| Tool | Typed parameters | Effect and annotations |
| --- | --- | --- |
| `perfectpixel_schema` | `{}` | CLI schema JSON; `readOnlyHint=true`, `destructiveHint=false` |
| `perfectpixel_inspect` | `inputPath` | Reads one raster; `readOnlyHint=true`, `destructiveHint=false` |
| `perfectpixel_convert` | `inputPath`, `outputPath`, optional `width`, `height`, `filter`, `jpegQuality`, `background` | Writes one raster; `readOnlyHint=false`, `destructiveHint=true` |
| `perfectpixel_upscale` | `inputPath`, `outputPath`, `scale`, optional `filter`, `jpegQuality`, `background` | Writes one raster; `readOnlyHint=false`, `destructiveHint=true` |
| `perfectpixel_normalize` | `requestPath`, `outputDir` | Publishes the existing normalize generation; `readOnlyHint=false`, `destructiveHint=true` |
| `perfectpixel_bundle` | `requestPath`, `outputDir` | Publishes the existing bundle generation; `readOnlyHint=false`, `destructiveHint=true` |
| `perfectpixel_vector` | `inputPath`, `outputPath`, optional `preset`, `profile`, `detail`, `minQuality`, `maxQualityLoss`, `maxPaths`, `policyPath`, `reportPath`, `diagnosticsDir` | Publishes approved SVG/evidence; `readOnlyHint=false`, `destructiveHint=true` |
| `perfectpixel_vector_analyze` | `inputPath`, optional `preset`, `profile`, `policyPath`, `reportPath` | Analysis is read-only unless `reportPath` is supplied; it is conservatively annotated `readOnlyHint=false`, `destructiveHint=true` |
| `perfectpixel_motion_scaffold` | `inputPath`, `outputDir` | Publishes the existing motion scaffold; `readOnlyHint=false`, `destructiveHint=true` |
| `perfectpixel_motion_build` | `requestPath`, `outputDir` | Publishes the existing motion build; `readOnlyHint=false`, `destructiveHint=true` |

Every tool has a generated typed input schema with camelCase field names and rejects unknown
top-level fields. Enum values are exact:

- `filter`: `nearest` or `lanczos3`;
- `preset`: `auto`, `pixel-art`, `legacy-lossless`, `flat-icon`, `line-art`, or
  `bounded-illustration`;
- `profile`: `compact` or `motion-structure-ready`;
- `detail`: `auto`, `1`, `2`, `3`, `4`, or `5`.

`width` and `height`, when present, are positive integers. `scale` is an integer in
`2..=u32::MAX`. `jpegQuality` is `1..=100`; `minQuality` and `maxQualityLoss` are finite
numbers in `0..=1`; `maxPaths` is positive. The CLI remains authoritative for the deeper
request schemas and product-specific limits.

## Root and path contract

Every MCP path field is a UTF-8, root-relative slash path. It must be non-empty, no longer than
1024 bytes, and must not contain NUL, backslash, an absolute prefix, `.`, or `..`. A Windows drive
prefix is rejected even when the server runs on another platform.

Input paths must resolve to regular files. Every existing component is inspected with
symlink-aware metadata before use; any symlink component, including a symlinked input itself,
fails closed. The canonical target must remain strictly inside the fixed root. Output file paths
may be new but their existing parent chain must remain inside the root. Output directory targets
must be strict children of the root, never the root itself, and may not be symlinks. Existing
output files and parents are also checked for symlinks before the application runs.

The adapter repeats the applicable input, output, nested-source, symlink, and canonical-root
checks immediately before calling the shared application boundary. For `normalize`, `bundle`, and
`motion-build`, the JSON request file is preparsed with at most 8 MiB plus one byte from one open
regular-file handle, and the handle revision, current path revision, and canonical in-root target
are verified before and after that bounded read. This 8 MiB limit applies only to those JSON request
files; it is not an image or SVG payload limit. Image and SVG limits remain application-specific.
A stable request file over the limit or an I/O/revision mismatch is an explicit preparation error.
Only a request rejected by the typed JSON parser is delegated to the application so its existing
CLI JSON error remains authoritative.

`normalize`, `bundle`, and `motion-build` request files are first bounded and parsed as their
typed product request when possible. Their nested frame, strip, and `sourceSvg` paths are then
resolved relative to the request file and undergo the same UTF-8, regular-file, root, and
symlink checks. A malformed request is passed to the shared application boundary so the CLI's
existing JSON error and exit semantics remain authoritative; it cannot publish a successful
generation.

The MCP adapter does not use CWD, an MCP Roots capability, path normalization fallbacks, or
best-effort symlink handling to expand access. A generated-output root without
`.perfectpixel-generation.json` does not auto-adopt existing generated files; use a clean output
directory for first publication. There is no migration shim.

These checks reject static traversal/symlink escapes and enforce the documented resource limits.
The adapter assumes a trusted local root and cooperating local writers; it does not provide a
second filesystem authority.

## Result and error contract

Every successfully dispatched operation or root-bounded operation error uses this structured
envelope:

```json
{
  "schema": "perfectpixel.mcp-tool-result/1",
  "ok": true,
  "operation": "perfectpixel_inspect",
  "exitCode": 0,
  "result": { "...": "existing CLI JSON" }
}
```

`result` is the existing CLI JSON parsed as JSON; the adapter does not inspect its message text
to decide success or behavior. Only the shared application exit code determines `ok`: exit code
0 is successful, while 1, 2, 3, and 4 are errors. Root/path rejection, busy state, internal
worker failure, and application-output parse failure use a stable error envelope with exit code
2. An application output that is not valid JSON is an internal MCP error rather than a fabricated
success.

Success envelopes use rmcp `CallToolResult::structured`; error envelopes use
`CallToolResult::structured_error`. The text representation, when a client requests or renders
it, is the same JSON envelope. Tool routing and parameter deserialization remain MCP protocol
validation; rmcp may report an unknown top-level field as an `isError` tool result before an
application envelope is created.

The application owns all product publication and exit semantics. MCP does not reinterpret CLI
stdout strings, bypass atomic writers, or expose a second write implementation.

## Concurrency and cancellation

One `perfectpixel-mcp` process permits one active product operation. A concurrent call is not
queued and returns a stable busy error immediately. The synchronous application execution is
isolated in `spawn_blocking`; the active permit remains held through root preparation and the
complete application operation.

Cancellation of the MCP call does not forcibly interrupt the background application operation.
The worker continues to completion so a file publication cannot be cut through an unsafe middle
state, and the permit is released only when that worker finishes. A client must treat a cancelled
call as having unknown publication visibility and inspect the root before retrying.

## Scope exclusions

This release intentionally does not provide HTTP or other non-stdio transports, network access,
authentication, services, runtime configuration changes, MCP Roots-based permission expansion,
prompts, resources, sampling, generic shell/CLI passthrough, or a `perfectpixel_run` tool.
The ten tools only expose the existing product commands through the shared application boundary.

The platform contract is Unix-only. Windows and every non-Unix target are unsupported and rejected
at crate compilation. See [ARCHITECTURE.md](ARCHITECTURE.md#residual-boundaries) and the CLI
[CONTRACT.md](CONTRACT.md) for the underlying product limits.
