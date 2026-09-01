# PerfectPixel Function Matrix

A capability is working only when its public trigger reaches the listed semantic owner. Disconnected code, a backend name, or a successful external process is not a product feature.

| Public trigger | Canonical semantic owner | Effect / I/O boundary | Acceptance / publication |
| --- | --- | --- | --- |
| `schema` | `OperationRegistry` + presentation schema | stdout only | no mutation |
| `inspect` | codec-observed `PixelSpec` + pure raster inspection | bounded immutable encoded-byte snapshot | returns content `ArtifactRef`, explicit pixel/ICC provenance, geometry/alpha facts; no mutation |
| `convert` | raster transform core | decode/encode + checked single-file writer | publish encoded bytes after successful transform |
| `upscale` | raster transform core | decode/encode + checked single-file writer | publish encoded bytes after successful transform |
| `edit` | deterministic `RasterEdit` transition | bounded source snapshot + PNG writer | invalid geometry/key/work fails before publication |
| `psd` | flattened PSD v1 contract | PSD serializer + checked file writer | independent single-raster export; no layered authority |
| `document-psd` | `DocumentIR` (`document.compile_psd`) | raster binding resolution + isolated PSD serializer | PerfectPixel merged composite/structure first, checked PSD publication last |
| `chroma-plan` | deterministic color-planning core | request read/stdout | no mutation; not semantic segmentation |
| `normalize` | normalize plan / generation authority | recoverable artifact-set publisher | complete managed generation replaces previous normalize generation |
| `bundle` | sprite semantics / atlas plan | deterministic packing + artifact-set publisher | complete verified sprite generation only |
| `texture-compile` | texture operation + KTX2 verifier | SHA-pinned KTX process Effects | current candidate → structure + decode roundtrip verification → checked file publication |
| `vector` | PerfectPixel vector policy / verified route | native candidate + resvg verification + checked writers | backend name never decides success; rejection cannot publish final SVG |
| `vector-analyze` | PerfectPixel vector policy / route analysis | bounded source snapshot/stdout or checked report publication | candidate-free and never publishes SVG; operation metadata is conservatively write-capable because `--report` can mutate |
| `vision-foreground-instances` | mask/alpha/geometry verification core | SHA-pinned Apple Vision helper Effect | current identity → deterministic mask cleanup/verification → checked directory publication |
| `motion-scaffold` | deterministic motion scaffold compiler | recoverable artifact-set publisher | new scaffold invalidates prior build artifacts bound to old scene |
| `motion-build` | deterministic motion compiler | recoverable artifact-set publisher | verified request/source identity before commit |

## Authoritative owners

| Responsibility | Owner |
| --- | --- |
| operation name / risk / capabilities / timeout | `OperationRegistry` / typed `Operation` |
| CLI syntax / MCP presentation | transport adapters only |
| artifact identity | `ArtifactRef` SHA-256 digest |
| encoded color provenance | codec boundary → `PixelSpec` |
| pixel / alpha semantics | core pixel/alpha |
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

- CLI and MCP create the same canonical `Operation`; neither transport reconstructs argv to reach product semantics.
- A bare `Raster` does not claim color provenance. `inspect` reports codec-observed `PixelSpec` instead of inventing sRGB.
- Missing profile remains `ColorSpec::Unknown`; embedded ICC is preserved by digest/bytes. Operations that require ICC conversion return explicit `Unsupported` because this release has no ICC transform engine.
- Perceptual raster verification is CIEDE2000 plus explicit alpha support; exact/region/geometry gates remain independent.
- External process success is only a candidate event. `EffectIdentity` prevents stale results from becoming current state.
- Cancelled, timed-out, failed, stale, or unverified external Effects cannot publish artifacts.
- Operation metadata is conservative when an invocation can publish conditionally; a write-capable command is never advertised as read-only.
- Mutation captures destination/input preconditions before expensive work and commits only through the atomic publication authority.
- VTracer remains absent until the same verified vector route can select and identity-pin it; disconnected optional backend code is not retained.

## Verification trace

Run on the target MacBook:

```bash
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo test --locked --workspace --all-targets --all-features
```

Apple Vision, KTX executable behavior, and native PSD interoperability remain `UNKNOWN` until exercised on the corresponding local runtime.
