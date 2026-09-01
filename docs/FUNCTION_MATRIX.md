# PerfectPixel Function Matrix

This matrix traces each public trigger to its authoritative semantic owner, Effect boundary, verification, and publication rule. A capability is working only when its trigger reaches the listed owner; disconnected code is not a feature.

| Public trigger | Canonical semantic owner | Effect / I/O boundary | Acceptance / publication |
| --- | --- | --- | --- |
| `schema` | `OperationRegistry` + CLI presentation metadata | stdout only | no mutation |
| `inspect` | raster inspection core | bounded immutable file snapshot | machine-readable facts only |
| `convert` | raster transform core | decode/encode + checked single-file writer | publish encoded bytes after successful transform |
| `upscale` | raster transform core | decode/encode + checked single-file writer | publish encoded bytes after successful transform |
| `edit` | deterministic `RasterEdit` transition | bounded source snapshot + PNG writer | invalid geometry/key/work fails before publication |
| `psd` | compatibility flattened PSD v1 contract | PSD serializer + checked file writer | legacy public compatibility surface; no layered authority |
| `document-psd` | `DocumentIR` (`document.compile_psd`) | raster artifact resolution + isolated PSD serializer | PerfectPixel merged composite/structure first, checked PSD publication last |
| `chroma-plan` | deterministic color-planning core | request read/stdout | no mutation; not semantic segmentation |
| `normalize` | normalize plan / generation authority | recoverable artifact-set publisher | complete managed generation replaces previous normalize generation |
| `bundle` | sprite semantics / atlas plan | deterministic packing + artifact-set publisher | complete verified sprite generation only |
| `texture-compile` | texture operation + KTX2 contract | pinned `ktx` process Effect | Effect candidate → KTX2 validation → decode roundtrip → checked file publication |
| `vector` | PerfectPixel vector policy/verified route | native candidate + resvg verification + checked writers | backend name never decides success; rejection cannot publish final SVG |
| `vector-analyze` | PerfectPixel vector policy/route analysis | bounded source snapshot/stdout or report | candidate-free; cannot publish SVG |
| `vision-foreground-instances` | mask/alpha/geometry verification core | pinned Apple Vision helper Effect | current Effect identity only → deterministic mask cleanup/verification → checked directory publication |
| `motion-scaffold` | deterministic motion scaffold compiler | recoverable artifact-set publisher | new scaffold invalidates prior build artifacts bound to old scene |
| `motion-build` | deterministic motion compiler | recoverable artifact-set publisher | verified request/source identity before commit |

## Authoritative owners

| Responsibility | Owner |
| --- | --- |
| operation name / semantic policy | `OperationRegistry` / typed `Operation` |
| CLI syntax / MCP presentation | transport adapters only |
| artifact identity | `ArtifactRef` SHA-256 digest |
| pixel / color / alpha semantics | core pixel/color/alpha |
| geometry / compositing | core geometry/composite |
| verification policy / evidence | core verify |
| document semantics | `DocumentIR` |
| PSD bytes | PSD adapter only |
| sprite semantics | sprite plan/IR |
| KTX bytes | external KTX Effect only |
| vector acceptance | PerfectPixel vector policy/verification |
| Apple Vision inference | Apple Vision Effect adapter only |
| durable publication | checked atomic file/artifact-set runtime |

## Failure invariants

- No transport reconstructs argv and reparses it to reach product semantics.
- Unknown color/profile is never silently promoted to sRGB. Embedded ICC is preserved as provenance and is explicitly unsupported by the dependency-minimal transform boundary.
- Perceptual raster verification is CIEDE2000 plus explicit alpha support; exact/region/geometry gates remain independent.
- External process success is only a candidate event. `EffectIdentity` prevents stale results from becoming current state.
- Cancelled, timed-out, failed, stale, or unverified external Effects cannot publish artifacts.
- Mutation captures a destination/input precondition before expensive work and commits only through the atomic publication authority.
- VTracer is not exposed until it is selected by the same verified vector route/acceptance pipeline; disconnected optional backend code is not retained.

## Verification trace

The local host is expected to run:

```bash
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo test --locked --workspace --all-targets --all-features
```

Environment-specific Apple Vision/KTX/Photoshop behavior remains UNKNOWN until exercised on the corresponding local host/toolchain.
