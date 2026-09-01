# PerfectPixel documentation

PerfectPixel is documented as one deterministic image-asset compiler and verification engine. Executable behavior has priority over prose: runtime result > tests > implementation > configuration > documentation.

## Read by task

| Need | Read |
| --- | --- |
| Public CLI/request/failure/publication contract | [CONTRACT.md](CONTRACT.md) |
| Implemented authority/effect/module boundaries | [ARCHITECTURE.md](ARCHITECTURE.md) |
| Trace every public command to its owner | [FUNCTION_MATRIX.md](FUNCTION_MATRIX.md) |
| See the 2026-09-01 roadmap closure and deliberate exclusions | [ROADMAP_COMPLETION_2026-09-01.md](ROADMAP_COMPLETION_2026-09-01.md) |
| Run the final target-Mac verification | [LOCAL_VERIFICATION_HANDOFF.md](LOCAL_VERIFICATION_HANDOFF.md) |
| Configure the fixed-root local MCP adapter | [MCP_CONTRACT.md](MCP_CONTRACT.md) |
| Author or compile motion | [MOTION_CONTRACT.md](MOTION_CONTRACT.md) |
| Maintain normalization review invariants | [NORMALIZE_REVIEW_CONTRACT.md](NORMALIZE_REVIEW_CONTRACT.md) |
| Work with vector route/policy/evidence | [vectorize/README.md](vectorize/README.md) |
| Review direct dependencies | [../THIRD_PARTY_NOTICES.md](../THIRD_PARTY_NOTICES.md) |

Start with the root [README](../README.md). `perfectpixel schema` is the machine-readable capability surface; documentation does not make unreachable code a feature.

## Authority summary

- `OperationRegistry` / typed `Operation` owns operation semantics and metadata.
- `core` owns artifact identity, raster/pixel/color/alpha, mask cleanup, geometry, compositing, `DocumentIR`, SVG parsing, and verification semantics.
- `vector` owns route selection and SVG acceptance gates.
- `adapters::psd` owns PSD byte serialization only; document/composite semantics remain in core.
- `adapters::sprite` and `adapters::motion` own their bounded packaging/compiler concerns.
- `effects` owns external candidate adapters for KTX and Apple Vision; external completion is not product success.
- `runtime` owns pinned bounded process execution and process-group cancellation/timeout.
- `io` owns bounded decode/encode capabilities and checked atomic publication mechanics.
- `application` orchestrates typed operations; CLI and MCP remain presentation adapters.

Generated-output roots without explicit generation authority do not auto-adopt existing files. Stale, failed, cancelled, or unverified external candidates cannot gain artifact authority.

## Release scope

The final 0.3.1 roadmap slice includes the existing raster/sprite/vector/motion product plus three useful compiler operations: layered DocumentIR PSD, verified KTX2 texture compilation, and Apple Vision foreground instances.

Optional speculative expansions such as embedded ICC conversion, SSIMULACRA2, external VTracer routing, SAM/ViTMatte, libvips, extra Vision operations, and OpenRaster are deliberately absent until a concrete product requirement justifies their boundary.
