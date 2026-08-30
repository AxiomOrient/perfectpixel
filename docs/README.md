# PerfectPixel documentation

This directory documents the `perfectpixel` Rust image-processing library and its
`perfectpixel` asset-adapter CLI. The authoritative sources for public behavior are the CLI
surface reported by `perfectpixel schema`, the implementation, and the tests; the documents
below explain those sources but do not supersede them.

## Read by task

| Need | Read |
| --- | --- |
| Run the CLI or validate a request/output | [CONTRACT.md](CONTRACT.md) |
| Configure or integrate the local MCP server | [MCP_CONTRACT.md](MCP_CONTRACT.md) |
| Understand modules and ownership | [ARCHITECTURE.md](ARCHITECTURE.md) |
| Author or compile motion | [MOTION_CONTRACT.md](MOTION_CONTRACT.md) |
| Maintain normalization invariants | [NORMALIZE_REVIEW_CONTRACT.md](NORMALIZE_REVIEW_CONTRACT.md) |
| Work with SVG vectorization | [vectorize/README.md](vectorize/README.md) |
| Trace capability coverage and boundaries | [FUNCTION_MATRIX.md](FUNCTION_MATRIX.md) |

Start with the root [README](../README.md) for scope and primary commands, then use
`CONTRACT.md` for exact field and failure rules.

## Ownership map

- `core` owns format-independent raster transforms, deterministic PSD encoding, shared SVG parsing, and content digests.
- `vector` owns route selection, candidate evaluation, and SVG quality gates.
- `adapters::sprite` owns normalization, atlas packing, manifest, and Aseprite export.
- `adapters::motion` owns SVG parts and transform/opacity animation compilation.
- `io` owns bounded image decoding, artifact publication, and bounded parallel execution.
- `src/application/generation.rs` owns pure generated-output authority state and events.
- `src/application/generation_adapter.rs` owns input snapshots and publication guards.
- `src/application/mod.rs` owns argument parsing, path resolution, domain-to-byte orchestration,
  and the shared stdout/exit-code application result.
- `src/bin/perfectpixel.rs` is the thin CLI process adapter; `src/mcp/` and
  `src/bin/perfectpixel-mcp.rs` own only typed fixed-root MCP routing and stdio transport.

The root crate is one Rust package, not a collection of independently runnable child projects.
Generated directories such as `target/` are not documentation sources.

Generated-output roots without `.perfectpixel-generation.json` do not auto-adopt existing files;
use a clean output directory for first publication. There is no migration shim.

Vector JSON under [`vectorize/examples/`](vectorize/examples/) is recorded request/evaluation
evidence, not a template that grants approval or proves publication.
