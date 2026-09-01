# PerfectPixel Roadmap Completion — 2026-09-01

Basis: `PerfectPixel_Improvement_Roadmap_2026-09-01(1).md`.

This file records the implemented clean-break target and the deliberate dependency-minimal deviations requested for the final package. It is not runtime evidence.

## Final product boundary

PerfectPixel is a deterministic image asset compiler and verification engine. External frameworks/executables may return candidate data, but artifact semantics, verification, and publication have one authority inside PerfectPixel.

## Implemented authority map

| Responsibility | Authority |
| --- | --- |
| operation semantics / risk metadata | typed `Operation` + `OperationRegistry` |
| CLI/MCP presentation | transport adapters |
| artifact identity | `ArtifactRef` digest |
| pixel/color/alpha | core pixel/color/alpha |
| geometry/composite | core geometry/composite |
| verification | core verify |
| document semantics | `DocumentIR` |
| PSD serialization | isolated PSD adapter |
| texture semantics | texture operation + KTX2 verifier |
| vector acceptance | PerfectPixel verified vector policy/gates |
| Vision inference | Apple Vision Effect adapter |
| publication | checked atomic file/artifact-set runtime |

## Completed roadmap slices

### Operation authority

- MCP/programmatic DTO no longer reconstructs argv or enters the CLI parser.
- CLI syntax and MCP DTOs converge on typed `Operation`.
- Registry names, risk, capabilities, and timeout metadata are unique and transport-independent.
- new compiler operations are reachable: `document.compile_psd`, `texture.compile`, `vision.apple.foreground_instances`.

### Pixel / color / verification

- content-addressed `ArtifactRef` and strict SHA-256 digest type.
- explicit `PixelSpec`, `AlphaMode`, and `ColorSpec`.
- codec retains embedded ICC bytes/digest and never silently promotes missing color provenance to sRGB.
- verification reports exact, alpha, mask-geometry, region, and CIEDE2000 evidence as typed machine-readable data.

### Dependency-minimal color/perceptual decision

The roadmap originally recommended an ICC engine and SSIMULACRA2. The final requested scope explicitly excludes adding `moxcms` and `fast-ssim2`.

Therefore:

- ICC provenance remains supported.
- ICC **conversion** is explicit `Unsupported`; no fallback to sRGB is allowed.
- CIEDE2000 is the retained perceptual color metric.
- SSIMULACRA2 is not advertised, emulated, or silently replaced by a differently named metric.
- exact/alpha/geometry/region gates remain independent and machine-readable.

This is a deliberate product-scope reduction rather than a weakened success gate.

### Alpha / geometry / composite

- one deterministic mask/alpha kernel is shared instead of per-feature implementations.
- geometry and linear-light compositing live in core.
- Document/Vision/Sprite verification consumes these semantics rather than owning replacements.

### Document / PSD

- compatibility flattened `psd` command remains as an explicit separate public contract.
- canonical layered flow is `document-psd` → `document.compile_psd` → `DocumentIR` → deterministic composite → PSD serializer → structural readback → checked publication.
- serializer is not document/composite authority.

### Sprite / texture

- sprite normalization/bundle behavior remains under the existing deterministic generation authority.
- `texture-compile` runs a SHA-256-pinned KTX executable as an external Effect.
- external completion is accepted only for the current `EffectIdentity`.
- KTX2 header/level/transfer/supercompression invariants are checked before decode roundtrip.
- sRGB texture quality uses CIEDE2000 thresholds; linear/normal/mask texture quality uses explicit maximum channel error.

### Vector

- existing verified PerfectPixel vector policy/gate remains the only acceptance authority.
- an external VTracer candidate path is not exposed because no verified route currently selects it.
- disconnected optional backend code was removed rather than presenting an unwired feature.

### Apple Vision

- only `vision.apple.foreground_instances` is exposed.
- Swift helper is a pinned external Effect.
- generation + operation digest + input digest form current-work identity.
- helper provenance records provider/adapter/OS/request revision.
- candidate masks are copied into PerfectPixel-owned memory, helper staging is removed, then deterministic cleanup/verification occurs.
- failed/cancelled/stale/unverified results cannot enter the checked directory publisher.

## Publication invariant

For managed mutations the intended flow is:

```text
capture precondition
→ immutable input snapshot
→ deterministic work / external candidate Effect
→ Result/Event identity check
→ verification
→ recheck input/request preconditions
→ checked atomic publication
```

No generic force overwrite or silent fallback is part of the target.

## Deliberately absent

These are not hidden partial features:

- built-in ICC transform engine
- SSIMULACRA2
- external VTracer route until the verified vector authority selects it
- SAM / ViTMatte / large-image optional adapters
- OCR / contour / optical-flow Vision expansions
- OpenRaster secondary backend

They should be added only when a concrete product requirement creates a real boundary.

## Local verification gate

Run on the target MacBook:

```bash
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo test --locked --workspace --all-targets --all-features
```

Then separately exercise tools that require the local environment:

- KTX executable encode/extract roundtrip
- Apple Vision helper on supported macOS
- layered PSD open/readback in the desired independent/native application

Until those runs are recorded, their runtime status is `UNKNOWN` rather than PASS.
