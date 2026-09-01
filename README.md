# PerfectPixel

PerfectPixel is a deterministic local image asset compiler for AI-generated or authored assets. It inspects, transforms, normalizes, packs, vectorizes, and builds motion artifacts with explicit contracts, quality gates, and atomic publication.

## Product boundary

PerfectPixel finalizes local assets. It does not generate images, call remote models, or require a network connection. The product exposes a human CLI and a typed local stdio MCP adapter over the same application semantics.

## Core capabilities

- raster inspection with exact input SHA-256 evidence
- deterministic PNG/JPEG/WebP conversion and resizing
- bounded deterministic raster edit requests
- controlled chroma background planning
- PSD export with soft alpha plus bounded vector path resources
- sprite normalization and deterministic MaxRects atlas compilation
- quality-gated raster-to-SVG vectorization and analysis
- SVG motion scaffolding and deterministic motion artifact generation
- atomic single-file, directory, and multi-artifact publication
- local stdio MCP adapter with a fixed root boundary

## CLI

```text
perfectpixel schema
perfectpixel inspect <input.png|jpg|jpeg|webp>
perfectpixel convert <input> --out <output> [--width N] [--height N] [--filter nearest|lanczos3]
perfectpixel upscale <input> --out <output> --scale N [--filter nearest|lanczos3]
perfectpixel edit --request <edit-request.json>
perfectpixel psd --request <psd-export-request.json>
perfectpixel chroma-plan --request <chroma-plan-request.json>
perfectpixel vector <input> --out <output.svg> [options]
perfectpixel vector-analyze <input> [options]
perfectpixel normalize --request <normalize-request.json> --out-dir <dir>
perfectpixel bundle --request <sprite-request.json> --out-dir <dir>
perfectpixel motion-scaffold <input.svg> --out-dir <dir>
perfectpixel motion-build --request <motion-request.json> --out-dir <dir>
```

Run `perfectpixel --help` for the complete request examples and `perfectpixel schema` for the machine-readable capability surface.

## MCP

```text
perfectpixel-mcp --root /absolute/non-symlink/workspace
```

The MCP server exposes the supported typed operations under one fixed root. It does not use MCP Roots notifications to widen that boundary. See [MCP contract](docs/MCP_CONTRACT.md).

## Architecture

The implementation keeps domain computation separate from I/O and publication:

```text
CLI / MCP
   │
   ▼
application boundary
   │
   ├─ bounded immutable input snapshots
   ├─ deterministic domain/compiler work
   ├─ quality / contract evaluation
   └─ atomic publication
        │
        ▼
core / vector / sprite / motion
```

The detailed authority map and residual boundaries are documented in [Architecture](docs/ARCHITECTURE.md).

## Asset conversion

Raster input is PNG, JPEG, or WebP. Output may be PNG, JPEG, or lossless WebP.

- PNG and WebP preserve RGBA.
- JPEG rejects transparency unless an explicit `--background #RRGGBB` matte is supplied.
- `convert` defaults to Lanczos3 when resizing.
- `upscale` defaults to nearest-neighbor and requires an integer scale of at least 2.
- output publication is atomic and refuses input/output collisions.

## Image edit

`edit` accepts one bounded JSON request. Supported operations are crop, quarter-turn rotate, horizontal/vertical flip, resize, exact keyed background removal, and deterministic edge-derived automatic background removal.

```json
{
  "schemaVersion": 1,
  "operation": "edit",
  "input": "source.png",
  "output": "edited.png",
  "steps": [
    { "op": "crop", "x": 0, "y": 0, "width": 512, "height": 512 },
    { "op": "resize", "width": 256, "height": 256, "filter": "lanczos3" }
  ]
}
```

Automatic background removal is intentionally not semantic segmentation. It selects bounded edge RGB candidates deterministically and returns evidence for the chosen keys and edge coverage.

## PSD export

The PSD command exports one flattened RGB document with four raw planar channels, soft alpha, and closed even-odd path resources derived from a caller-selected alpha threshold.

```json
{
  "schemaVersion": 1,
  "operation": "export_psd",
  "input": "source.png",
  "output": "source.psd",
  "path": {
    "alphaThreshold": 128,
    "maxKnots": 4096
  }
}
```

This is a deliberately bounded export contract, not a general Photoshop document editor. See [Contract](docs/CONTRACT.md).

## Sprite workflow

`normalize` converts raw state images or strips into deterministic cell-aligned frame PNGs plus a `sprite-request.json`. `bundle` then packs those frames using deterministic MaxRects placement and emits:

- one or more PNG sheets
- Aseprite-compatible JSON per sheet
- `manifest.json`
- normalized frame PNGs

Input snapshots and generated-set authority are revalidated before atomic publication.

## Vector workflow

`vector` is the sole SVG publication command. Candidate generation does not imply success. Publication requires the embedded route/policy authority and quality gates to approve the exact candidate bytes.

`vector-analyze` emits evidence only and cannot publish SVG or diagnostics.

See [Vector documentation](docs/vectorize/README.md) for route, calibration, quality, and report contracts.

## Motion workflow

`motion-scaffold` converts a raster-free SVG into a generated workspace with stable path identities, layer metadata, a starter request, and a local inspector.

`motion-build` accepts an explicit motion request and emits deterministic animated SVG/JSON/HTML artifacts plus an exploded dotLottie v2 layout. It does not create a packed `.lottie` archive.

## Documentation

1. [Architecture](docs/ARCHITECTURE.md)
2. [Contract](docs/CONTRACT.md)
3. [Function matrix](docs/FUNCTION_MATRIX.md)
4. [MCP contract](docs/MCP_CONTRACT.md)
5. [Motion contract](docs/MOTION_CONTRACT.md)
6. [Normalize review contract](docs/NORMALIZE_REVIEW_CONTRACT.md)
7. [Vector documentation](docs/vectorize/README.md)
8. [Documentation index](docs/README.md)
9. [Third-party notices](THIRD_PARTY_NOTICES.md) — direct dependency versions and source-publication limits.

## Verification

```bash
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo test --locked --workspace --all-targets --all-features
```

That is the whole gate: one toolchain, no external scripts. Alongside the behavioral suites,
`tests/docs_contract.rs` keeps the prose attached to reality — it fails if a documentation link
breaks, if `perfectpixel schema` stops advertising exactly the commands the binary dispatches,
or if a public command is missing from [the capability matrix](docs/FUNCTION_MATRIX.md).

The complete runtime gate is verified on macOS arm64. Windows and every non-Unix target fail at
crate compilation; no non-Unix runtime behavior is supported. See [Residual boundaries](docs/ARCHITECTURE.md#residual-boundaries).

## GitHub 배포 분류

PerfectPixel의 주 제품은 사람이 직접 실행하는 이미지 도구 CLI와 typed local stdio MCP adapter이므로 canonical 조직은 [`AxiomOrient`](https://github.com/AxiomOrient)다. 내부 crate는 실행 제품을 지원하며 별도 SDK가 주 제품은 아니다.
