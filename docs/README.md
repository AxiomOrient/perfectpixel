# PerfectPixel documentation

This directory documents the `perfectpixel` Rust image-processing library and its
`perfectpixel` asset-adapter CLI. The authoritative sources for public behavior are the CLI
surface reported by `perfectpixel schema`, the implementation, and the tests; the documents
below explain those sources but do not supersede them.

## Read by task

| Need | Read |
| --- | --- |
| Run the CLI or validate a request/output | [CONTRACT.md](CONTRACT.md) |
| Understand modules and ownership | [ARCHITECTURE.md](ARCHITECTURE.md) |
| Author or compile motion | [MOTION_CONTRACT.md](MOTION_CONTRACT.md) |
| Maintain normalization invariants | [NORMALIZE_REVIEW_CONTRACT.md](NORMALIZE_REVIEW_CONTRACT.md) |
| Work with SVG vectorization | [vectorize/README.md](vectorize/README.md) |
| Trace capability coverage and boundaries | [FUNCTION_MATRIX.md](FUNCTION_MATRIX.md) |

Start with the root [README](../README.md) for scope and primary commands, then use
`CONTRACT.md` for exact field and failure rules.

## Ownership map

- `core` owns format-independent raster transforms, shared SVG parsing, and content digests.
- `vector` owns route selection, candidate evaluation, and SVG quality gates.
- `adapters::sprite` owns normalization, atlas packing, manifest, and Aseprite export.
- `adapters::motion` owns SVG parts and transform/opacity animation compilation.
- `io` owns bounded image decoding, artifact publication, and bounded parallel execution.
- `src/bin/perfectpixel/generation.rs` owns pure generated-output authority state and events.
- `src/bin/perfectpixel/generation_adapter.rs` owns input snapshots, legacy provenance import,
  and publication guards.
- `src/bin/perfectpixel.rs` owns argument parsing, path resolution, domain-to-byte
  orchestration, and process exit codes.

The root crate is one Rust package, not a collection of independently runnable child projects.
Generated directories such as `target/` are not documentation sources.

Vector JSON under [`vectorize/examples/`](vectorize/examples/) is recorded request/evaluation
evidence, not a template that grants approval or proves publication.
