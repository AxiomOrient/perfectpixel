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

One responsibility has one authoritative owner. CLI and MCP are presentation/admission adapters; they do not independently own product validation, success policy, timeout policy, or publication semantics.

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

`perfectpixel schema` is the machine-readable capability surface. `--help` is presentation only.

## Core boundaries

- **Operation authority** — typed `Operation` plus `OperationRegistry`; transports do not reconstruct argv.
- **Artifact identity** — content SHA-256, not pathname.
- **Inspection** — exact source `ArtifactRef`, codec-observed `PixelSpec`, ICC byte evidence, and deterministic raster facts. A bare `Raster` does not invent color provenance.
- **Pixel semantics** — RGBA8 plus explicit alpha/color provenance; missing color remains `Unknown`.
- **ICC** — embedded profile bytes/digest are preserved. This release has no ICC conversion engine and fails explicitly instead of assuming sRGB.
- **Verification** — exact dimensions/pixel/alpha/artifact, mask geometry, region assertions, and CIEDE2000 evidence.
- **Alpha / geometry / composite** — deterministic core operations shared by documents, masks, sprites, and verification.
- **Document** — `DocumentIR` owns layered semantics; the PSD adapter serializes them and does not own merged appearance.
- **Texture** — a SHA-pinned KTX executable is an external Effect; candidate bytes must pass structure and decode-roundtrip verification before publication.
- **Vector** — PerfectPixel's verified route/policy and raster roundtrip gates decide acceptance. Disconnected optional backends are not exposed.
- **Apple Vision** — inference is an external Effect. Work identity, helper provenance, deterministic mask cleanup, and verification precede checked publication.
- **Publication** — destination/input preconditions are captured before expensive work; only checked atomic publication owns visible mutation.

## MCP

```text
perfectpixel-mcp --root /absolute/non-symlink/workspace
```

The MCP adapter is local stdio with a fixed root. Protocol-local typed `*Params` resolve directly into the same `Operation` authority used by the CLI. It is not a generic command runner.

## Failure semantics

Failure, cancellation, timeout, dependency failure, verification failure, precondition conflict, and stale Effect results remain distinct. External process completion never means product success by itself. A failed, cancelled, stale, or unverified candidate cannot acquire artifact authority.

## Documentation

- [Architecture](docs/ARCHITECTURE.md)
- [Product contract](docs/CONTRACT.md)
- [Function matrix](docs/FUNCTION_MATRIX.md)
- [MCP contract](docs/MCP_CONTRACT.md)
- [Motion contract](docs/MOTION_CONTRACT.md)
- [Normalize review contract](docs/NORMALIZE_REVIEW_CONTRACT.md)
- [Vector documentation](docs/vectorize/README.md)
- [Roadmap completion](docs/ROADMAP_COMPLETION_2026-09-01.md)
- [Research decisions](docs/RESEARCH_DECISIONS_2026-09-01.md)
- [Final handoff](docs/FINAL_HANDOFF_2026-09-01.md)
- [Documentation index](docs/README.md)
- [Third-party notices](THIRD_PARTY_NOTICES.md)

## Verification

Run on the target macOS checkout:

```bash
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo test --locked --workspace --all-targets --all-features
```

Apple Vision, KTX executable integration, and native layered-PSD interoperability require the corresponding local tools/frameworks. They are not inferred from source inspection.

PerfectPixel intentionally supports Unix targets only; non-Unix targets fail at crate compilation.
