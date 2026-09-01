# PerfectPixel

PerfectPixel is a deterministic local image asset compiler and verification engine. It transforms raster, document, sprite, vector, texture, and bounded motion assets; external Vision/tool processes may produce candidates, but PerfectPixel owns semantics, verification, and publication.

## Architecture

```text
CLI / MCP
    │
    ▼
typed Operation
    │
    ▼
deterministic domain transition
    │
    ├─ no Effect required ───────────────┐
    │                                    │
    └─ Effect request → Result/Event ────┤
                                         ▼
                                  verification
                                         │
                                         ▼
                                checked atomic publish
```

The important invariant is one authoritative owner per responsibility. CLI and MCP are presentation adapters; they do not own validation, success policy, or publication semantics independently.

## CLI

```text
perfectpixel schema
perfectpixel inspect <input.png|jpg|jpeg|webp>
perfectpixel convert <input> --out <output> [options]
perfectpixel upscale <input> --out <output> --scale <n> [options]
perfectpixel edit --request <edit-request.json>
perfectpixel psd --request <psd-export-request.json>
perfectpixel document-psd --request <document-psd-request.json>
perfectpixel chroma-plan --request <chroma-plan-request.json>
perfectpixel normalize --request <normalize-request.json> --out-dir <dir>
perfectpixel bundle --request <sprite-request.json> --out-dir <dir>
perfectpixel texture-compile --request <texture-request.json>
perfectpixel vector <input> --out <output.svg> [options]
perfectpixel vector-analyze <input> [options]
perfectpixel vision-foreground-instances --request <vision-request.json>
perfectpixel motion-scaffold <input.svg> --out-dir <dir>
perfectpixel motion-build --request <motion-request.json> --out-dir <dir>
```

`perfectpixel schema` is the machine-readable capability surface. `--help` is the human syntax surface.

## Core boundaries

- **Artifact identity** — content SHA-256, not pathname.
- **Pixel semantics** — RGBA8 plus explicit alpha/color provenance; unknown color remains unknown.
- **ICC** — embedded profile bytes/digest are preserved. This dependency-minimal build does not perform ICC conversion and fails explicitly instead of assuming sRGB.
- **Verification** — exact dimensions/pixel/alpha/artifact, mask geometry, region assertions, and in-repository CIEDE2000 evidence.
- **Alpha / geometry / composite** — deterministic core operations shared by documents, masks, sprites, and verification.
- **Document** — `DocumentIR` owns layered semantics; the PSD adapter serializes those semantics and does not own the merged appearance.
- **Texture** — pinned KTX executable is an external Effect; candidate bytes must pass KTX2 structure and decode-roundtrip verification before publication.
- **Vector** — PerfectPixel's verified route/policy and raster roundtrip gates decide acceptance. Optional disconnected backends are not exposed.
- **Apple Vision** — inference is an external Effect. generation/operation/input identity, helper provenance, deterministic mask cleanup, and verification precede checked publication.
- **Publication** — destination/input preconditions are captured before expensive work; only checked atomic publication owns the visible mutation.

## MCP

```text
perfectpixel-mcp --root /absolute/non-symlink/workspace
```

The MCP adapter is local stdio with a fixed root. Its typed DTOs convert directly to the same `Operation` authority used by the CLI. It is not a generic command runner.

## Failure semantics

Failure, cancellation, timeout, dependency failure, verification failure, precondition conflict, and stale Effect results are distinct. External process completion never means product success by itself. A failed or stale candidate cannot acquire artifact authority.

## Documentation

- [Architecture](docs/ARCHITECTURE.md)
- [Contract](docs/CONTRACT.md)
- [Function matrix](docs/FUNCTION_MATRIX.md)
- [MCP contract](docs/MCP_CONTRACT.md)
- [Motion contract](docs/MOTION_CONTRACT.md)
- [Normalize review contract](docs/NORMALIZE_REVIEW_CONTRACT.md)
- [Vector documentation](docs/vectorize/README.md)
- [Documentation index](docs/README.md)
- [Third-party notices](THIRD_PARTY_NOTICES.md)
- [Roadmap completion](docs/ROADMAP_COMPLETION_2026-09-01.md)

## Verification

Run on the target macOS checkout:

```bash
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo test --locked --workspace --all-targets --all-features
```

Apple Vision, KTX executable integration, and native Photoshop interoperability require the corresponding local tools/frameworks and are not inferred from source inspection.

PerfectPixel intentionally supports Unix targets only; non-Unix targets fail at crate compilation.
