# Architecture

This document describes the implemented `perfectpixel` 0.3.1 architecture. Public behavior is defined by [CONTRACT.md](CONTRACT.md), capability reachability by [FUNCTION_MATRIX.md](FUNCTION_MATRIX.md), and the roadmap closure by [ROADMAP_COMPLETION_2026-09-01.md](ROADMAP_COMPLETION_2026-09-01.md).

## Product invariant

PerfectPixel has one semantic authority per responsibility. Transport, codecs, platform frameworks, and external executables may carry input or produce candidates; they do not independently decide product success.

```text
Input / transport
      │
      ▼
 typed Operation
      │
      ▼
pure/bounded domain work
      │
      ├──────────── no external Effect ───────────┐
      │                                           │
      └─ Effect request -> Effect completion ─────┤
                                                  ▼
                                            verification
                                                  │
                                                  ▼
                                      checked publication
```

State mutation is visible only at the publication boundary. External success, helper output, or candidate encoding alone never changes artifact authority.

## Dependency direction

```text
core
  artifact / pixel / color / alpha
  geometry / composite / document
  raster / transform / verify / svg
         │
         ├─────────────┐
         ▼             ▼
      vector        adapters
  policy/gates     PSD / sprite / motion
         │             │
         └──────┬──────┘
                ▼
          application
       typed Operation dispatch
                │
        ┌───────┼────────┐
        ▼       ▼        ▼
       I/O    effects   MCP
                │
                ▼
             runtime
        bounded processes
```

`core` contains format-independent deterministic semantics. It does not parse command-line syntax, own MCP routing, invoke Apple Vision, invoke KTX tooling, or publish files.

`adapters` exist where a real external/interchange representation is required. `adapters::psd` serializes `DocumentIR`; it does not own document semantics or merged appearance. Sprite and motion adapters consume core values and retain their existing bounded packaging contracts.

`application` resolves transport-neutral operations into domain work, Effect requests, verification, and publication. It is orchestration, not a second semantic domain.

`effects` and `runtime` isolate external execution. They cannot publish final artifacts.

`io` owns byte/path capabilities and atomic publication mechanics. It does not decide domain acceptance.

## Operation authority

`OperationRegistry` is the only operation metadata authority. Every operation name is unique and binds summary, risk, side-effect class, capabilities, and optional timeout.

The CLI parses syntax directly into `Operation`. MCP retains typed DTOs for its existing protocol surface; `ApplicationRequest` converts directly into the same `Operation` and never reconstructs argv.

```text
CLI syntax -> Operation ─┐
                         ├-> application dispatch
MCP typed DTO -> Operation┘
```

CLI and MCP inventory parity means semantic parity for operations exposed through both transports, not necessarily identical command counts. New compiler CLI operations do not silently expand the fixed MCP compatibility contract.

## Authoritative owners

| Responsibility | Owner |
| --- | --- |
| operation identity / metadata | `OperationRegistry` |
| artifact identity | `ArtifactRef` digest |
| raster pixel/color/alpha semantics | core pixel/color/alpha |
| mask cleanup | core alpha |
| geometry / compositing | core geometry/composite |
| verification policy / evidence | core verify |
| layered document semantics | `DocumentIR` |
| PSD byte serialization | PSD adapter |
| sprite semantics / generation | sprite plan + generation authority |
| texture acceptance | `texture.compile` verification path |
| vector route / acceptance | embedded PerfectPixel vector authority |
| Apple Vision inference | Apple Vision Effect adapter |
| KTX encoding/extraction | KTX process Effects |
| visible mutation | atomic publication runtime |
| CLI/MCP presentation | transport adapters |

No filename convention, helper result, README statement, or backend name may override these owners.

## Domain purity

Pure or deterministic modules operate on values and return values/errors. Examples include raster transforms, alpha operations, connected components, geometry, compositing, `DocumentIR` validation/composition, KTX2 inspection, verification calculations, vector policy/gate decisions, and generation ownership reducers.

Actors, locks, channels, and state machines are not added to pure computation. They exist only where there is actual concurrency or recoverable state.

## Real state machines

### Generated output ownership

Generated sprite/motion workflows use one pure generation authority state and explicit replacement events. The reducer decides retained, superseded, and stale managed artifacts; I/O adapters implement the resulting plan.

### Recoverable artifact-set publication

The artifact-set writer owns the durable transaction lifecycle. It stages new bytes, records exact backup evidence, verifies preconditions, mutates the managed set, verifies the installed set, records commit evidence, and performs rollback/recovery when required.

This state machine exists because multi-file replacement can be interrupted. Single-file transformations do not use it.

### Vector publication outcome

Vector generation retains its explicit ordered transaction outcome because report, diagnostics, and final SVG have distinct publication truth. Only an approved vector outcome can publish the final SVG.

## External Effect boundary

External Effects are candidate producers with explicit work identity.

`EffectIdentity` binds:

```text
generation
+ operation digest
+ input artifact digest
```

Completion is explicit:

```text
Succeeded(value)
Failed(error)
Cancelled
```

A result whose identity is not current is stale and rejected. Timeout is represented as a typed failure rather than a generic I/O error.

### Process runtime

Pinned executables are opened through the no-follow capability boundary, hashed, canonicalized, and revalidated around execution. Child execution receives an explicit environment after `env_clear`.

One Effect owns one process group. Cancellation and timeout terminate the complete group and reap the direct child before the Effect returns. Stdout/stderr are drained with bounded retained evidence so a verbose child cannot deadlock on a full pipe.

The remaining pathname check-to-exec window is a platform residual of Darwin/`std::process`; PerfectPixel does not claim an executable-fd primitive it does not have.

## Texture flow

```text
strict texture request
-> request/input/output preconditions
-> exact input ArtifactRef verification
-> explicit PixelSpec/color normalization policy
-> canonical PNG carrier
-> pinned KTX encode Effect
-> current EffectIdentity check
-> KTX2 structural/semantic verification
-> pinned KTX extract Effect
-> current EffectIdentity check
-> deterministic decode-roundtrip verification
-> precondition recheck
-> atomic KTX2 publication
```

The external KTX executable owns encoding bytes only. PerfectPixel owns texture semantics and acceptance. Color-sRGB verification uses CIEDE2000; linear/normal/mask verification uses explicit channel-error policy. Normal maps are not judged with an unrelated sRGB metric.

## Apple Vision flow

Only `vision.apple.foreground_instances` is included.

```text
strict Vision request
-> request/input/output preconditions
-> EffectIdentity
-> pinned Swift helper
-> Apple Vision framework
-> EffectResult
-> current identity check
-> candidate mask bytes copied into PerfectPixel memory
-> helper staging removed
-> core Mask cleanup
-> coverage/components/bounds verification
-> request/input precondition recheck
-> checked directory publication
```

The helper receipt records provider, adapter version, OS version, request type, request revision, and instance mapping. PerfectPixel does not require opaque Vision output itself to be bit-exact across OS/model revisions; it gates the deterministic postconditions it owns.

## Document flow

Compatibility `psd` and canonical layered `document-psd` are separate contracts.

The layered path is:

```text
strict request
-> DocumentIR validation
-> exact ArtifactRef bindings
-> explicit pixel/color semantics
-> PerfectPixel merged composite
-> PSD adapter serialization
-> structural readback
-> precondition recheck
-> atomic publication
```

The merged composite is computed before serialization and remains PerfectPixel authority. The PSD writer is never allowed to become the renderer-of-record.

## Vector flow

```text
raster snapshot
-> profile/request normalization
-> verified embedded route
-> compiled native candidate backend
-> bounded SVG parse/safety checks
-> topology/protected geometry gates
-> resvg render-back
-> quality/resource gates
-> Approved | Rejected
-> explicit publication outcome
```

External VTracer is deliberately absent from this release. Adding it before the verified route registry can select and identity-pin it would create a disconnected second path. If later adopted, its bytes must enter the exact same acceptance flow shown above.

## Color management boundary

Codec decode preserves embedded ICC bytes and binds them to `ColorSpec::Icc { digest }`. No-profile input remains `ColorSpec::Unknown`.

The current minimal release does not embed an ICC transform engine. A required ICC transform returns explicit `Unsupported`; there is no silent sRGB fallback. This keeps color correctness strict while avoiding a dependency whose behavior would otherwise become a hidden semantic authority.

## MCP boundary

MCP is local stdio with one configured fixed root. Root/path validation, one-active-operation admission, typed request parsing, and MCP envelope rendering belong to this adapter only.

Synchronous product work runs in `spawn_blocking`. Cancelling the MCP future does not force-stop an already-running in-process publication worker, and its permit is not released early. This existing compatibility behavior is separate from external process Effect cancellation; conflating them would make publication interruptible at an unsafe point.

## Publication boundary

### Single file

A single-file mutation captures destination/input preconditions before expensive work, creates and verifies bytes, rechecks preconditions, then commits through `AtomicFileWriter`. Failure before the rename preserves the previous destination. A durability failure after visibility is reported explicitly rather than rewritten as success.

### Managed set

Generated sets and Vision directory output use checked artifact-set publication. The writer coordinates cooperating publishers with an OS lock, exact stage/backup evidence, journaled transition, rollback/recovery, and verification before commit evidence.

A stale or unowned file is not silently deleted. A force flag is not part of the architecture.

## Bounded concurrency

MCP admission uses a semaphore because there is a real shared active-operation boundary. CPU-heavy independent per-item work may use the existing bounded scoped-thread helper; it joins synchronously and preserves deterministic lowest-input-index error selection.

No generic actor system, async domain layer, or channel graph is introduced.

## Deliberate release exclusions

The following are not partially wired features:

- embedded ICC conversion engine;
- SSIMULACRA2;
- external VTracer route;
- SAM / ViTMatte / libvips optional adapters;
- OCR / contour / feature-distance / optical-flow Vision operations;
- OpenRaster backend.

A future implementation must first establish a concrete product requirement and a single authority/effect boundary.

## Residual / environment boundaries

Source-level implementation cannot establish target-host runtime evidence for:

- Cargo fmt/clippy/test under the requested Rust toolchain;
- Apple Vision SDK/runtime behavior;
- the exact installed KTX executable behavior;
- Photoshop/native layered PSD interoperability.

Those remain UNKNOWN until executed on the target host. See [ROADMAP_COMPLETION_2026-09-01.md](ROADMAP_COMPLETION_2026-09-01.md).
