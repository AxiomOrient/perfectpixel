# PerfectPixel Function Matrix

This matrix tracks observable product capability by contract, state authority,
side-effect boundary, failure model, and regression evidence. A row is a capability or
cross-cutting contract, not a file count or test count.

| ID | Capability / public trigger | Pure decision or authoritative state | Explicit event / result | Side-effect boundary | Failure and recovery contract | Source evidence |
| --- | --- | --- | --- | --- | --- | --- |
| C1 | Raster inspection — `perfectpixel inspect` | Decode limits, one bounded source-byte snapshot, exact SHA-256/byte count, immutable alpha/color facts, and deterministic top-16 edge RGB counts | `perfectpixel.asset-inspection/1` strict JSON evidence | `ImageCodec` reads; stdout publishes numeric metadata only | Invalid input/limit returns JSON error; no file mutation or pixel exposure | `src/application/mod.rs`, `src/core/inspect.rs`, `src/io/codec.rs`, `tests/asset_adapter_contract.rs`, `tests/inspect_contract.rs`, `tests/edit_contract.rs`, `tests/codec_limits.rs` |
| C2 | Raster transform — `convert`, `upscale` | Positive-dimension `Raster`, pure target dimensions/filter, exact input/output bytes and digests | `perfectpixel.asset-transform/1` strict JSON evidence | One bounded input read/decode; encode, hash, and count exact output bytes before `AtomicFileWriter` commit | Zero dimensions fail at the core boundary. Failure before rename preserves the previous destination; rename is the visible commit point, a later parent-sync failure is explicit, and a writer failure emits no success digest evidence or output | `src/application/mod.rs`, `src/core/raster.rs`, `src/core/transform.rs`, `src/io/atomic.rs`, `tests/asset_adapter_contract.rs` |
| C2a | Agent-readable post-generation edit — `edit` | Sequential `RasterEdit` pipeline: crop, quarter-turn rotate, horizontal/vertical flip, nearest/Lanczos3 resize, explicit edge-connected RGB-key background removal | `perfectpixel.image-edit/1` strict JSON evidence | `ImageCodec` snapshot + `PngEncoder` + `AtomicFileWriter` | Unknown/missing fields, invalid geometry/key sets, unsupported filters, path collisions, and cumulative key-weighted work fail before publication | `src/core/transform.rs`, `src/application/mod.rs`, `tests/edit_contract.rs` |
| C2b | Controlled chroma planning — `chroma-plan` | Fixed eight-color high-saturation palette; OKLab Euclidean maximin score over 1..=32 unique subject RGB colors with lexicographic tie-break | `perfectpixel.chroma-plan/1` strict JSON evidence | Reads bounded JSON only; publishes no files | Unknown/missing fields, empty/duplicate/excess colors, and non-finite/unbounded results fail closed; this is not semantic segmentation or photo matting | `src/core/chroma.rs`, `src/application/mod.rs`, `tests/chroma_plan_contract.rs` |
| C2c | Controlled automatic background edit — `remove_background_auto` inside `edit` | Complete edge RGB histogram, count-desc/RGB ordering, max-16 key selection, integer basis-point threshold, then the existing four-connected keyed flood | Optional bounded `autoBackground` evidence in `perfectpixel.image-edit/1` | `ImageCodec` snapshot + `PngEncoder` + `AtomicFileWriter` | Coverage below threshold or cumulative work above cap fails before publication; successful output preserves interior keyed pixels unless edge-connected | `src/core/transform.rs`, `src/application/mod.rs`, `tests/edit_contract.rs`, `src/core/inspect.rs` |
| C2d | Photoshop export — `psd` | Pure bounded PSD v1 encoder: exact RGBA raw planes plus deterministic alpha-threshold contours, holes, and disconnected components | `perfectpixel.photoshop-export/1` strict JSON evidence | `ImageCodec` snapshot + pure PSD encoder + `AtomicFileWriter` | Unknown/missing fields, unsupported extensions, path collisions, empty/ambiguous foreground, over-complex paths, malformed resources, and >64 MiB outputs fail before publication; existing output remains unchanged | `src/core/psd.rs`, `src/application/mod.rs`, `tests/psd_contract.rs` |
| C2e | Agent execution plane — `agent-schema`, `agent-inspect`, `agent-extract`, `agent-render`, `agent-compare` | Content-addressed artifact descriptors, bounded extraction, ordered raster composition, deterministic text rasterization, and explicit compare assertions | `perfectpixel.agent-image/2` capability/result/receipt schemas | `src/agent.rs` and `src/agent_image.rs` own wire identity and pure image decisions; `src/text.rs` owns font-byte-bound text; `src/application/mod.rs` owns CLI snapshot and atomic output directories | Unknown fields, path traversal, content-address mismatch, unsupported pixel specs, duplicate IDs/assertions, budget overflow, and failed required assertions fail closed; committed outputs carry dependency closure and operation receipt | `src/agent.rs`, `src/agent_image.rs`, `src/text.rs`, `src/application/mod.rs`, `tests/agent_protocol_contract.rs`, `docs/AGENT_PROTOCOL_V2.md` |
| C3 | Vector generation/analysis — `vector`, `vector-analyze` | Embedded vector authority, route/policy resolution, quality gates | `VectorOutcome::{Approved, Rejected}` and CLI transaction events | Report and final SVG use single-file publication; diagnostics use exact-file-set `AtomicDirectoryWriter` over shared journal `/2` | Rejection never publishes final SVG; diagnostic/report failure is explicit and ordered. Diagnostics remove stale files transactionally but reject unrepresented empty directories and file↔directory shape changes | `src/vector/`, `src/application/mod.rs`, `src/io/atomic.rs`, `tests/vector_*`, `tests/io_contract.rs` |
| C4 | Normalize success — `normalize` | Pure `NormalizePlan`; `.perfectpixel-generation.json` owns the published generation | `GenerationAuthorityEvent::Replace { workflow: Normalize, ... }` | Shared recoverable artifact-set publisher | Full report/request/frame set replaces the prior normalize generation; stale files require exact authority evidence | `src/adapters/sprite/normalize.rs`, `src/application/generation*.rs`, `tests/normalize_contract.rs`, `tests/cli_output_contract.rs` |
| C5 | Normalize gate failure — `normalize` with `report.ok=false` | Report-only generation is authoritative failure state | `Replace { workflow: Normalize, artifacts: [normalize-report.json] }` | Same publisher as C4 | Previous success request/frames are removed as verified stale files; unrelated files remain | `src/application/mod.rs`, `tests/cli_output_contract.rs` |
| C6 | Sprite bundle — `bundle` | Pure packing/manifest plan; bundle workflow authority | `Replace { workflow: Bundle, ... }` | Shared recoverable artifact-set publisher | Replaces the complete bundle generation; no manifest-name-based deletion without provenance | `src/adapters/sprite/atlas.rs`, `src/adapters/sprite/manifest.rs`, `src/application/generation_adapter.rs`, `tests/sprite_contract.rs` |
| C7 | Motion scaffold — `motion-scaffold` | Deterministic scene/layers/request scaffold | `Replace { workflow: MotionScaffold, invalidates: [MotionBuild] }` | Shared recoverable artifact-set publisher | New scaffold invalidates all prior build artifacts bound to the old scene | `src/adapters/motion/`, `src/application/mod.rs`, `tests/cli_workflow.rs` |
| C8 | Motion build — `motion-build` | Validated request/source hash and deterministic motion compiler | `Replace { workflow: MotionBuild, ... }` | Shared recoverable artifact-set publisher | Renamed dotLottie animation and all other prior build-only files become verified stale outputs | `src/adapters/motion/`, `src/application/generation_adapter.rs`, `tests/vector_motion_acceptance.rs` |
| C9 | Generation ownership | `GenerationAuthorityState::{Absent, Published}` and pure reducer | `GenerationAuthorityEvent::Replace` | `generation_adapter` maps state transition to I/O plan | Corrupt authority, competing ownership, missing retained files, modified stale files, and unowned output collisions fail closed | `src/application/generation.rs`, `src/application/generation_adapter.rs` unit tests |
| C10 | Recoverable artifact-set publication | `ArtifactTransactionState::{Prepared, BackupsReady, Installed, Aborted, Restored, RecoveryRequired}`; `perfectpixel.artifact-set-transaction/2` owns sorted `(path, byteCount, sha256)` backup evidence | `BackupsRecorded`, `InstallRecorded`, `PublicationAborted`, `RollbackRestored`, `RecoveryRequired`; install-marker outcome is durable or visible-with-unconfirmed-durability | One library `AtomicArtifactSetWriter` + OS lock + durable stage/backup/restore adapters, shared by generated workflows and exact diagnostics | Before output mutation, the exact backup set is verified, fully restaged, reverified, and the immutable backup set is rechecked; each touched path is then restored or removed once. A visible installed marker is never rolled back when its parent sync fails, and committed cleanup failure is explicit. Transaction/root-parent durability failure, corrupt/incomplete evidence, and schema `/1` fail closed; interrupted recovery retries from unconsumed backups | `src/io/artifact_set.rs` unit/fault tests, `src/io/atomic.rs`, `tests/io_contract.rs` |
| C11 | Input revision binding for generated workflows | Immutable input bytes + canonical path + revision + size + SHA-256 | Publication checkpoints `BeforeMutation`, `BeforeCommit` | `InputSnapshot` capture/verification adapter | Retargeted or changed input aborts before mutation or rolls back before commit | `src/application/generation_adapter.rs`, `src/io/codec.rs`, `tests/codec_limits.rs` |
| C12 | Request and bounded-work contract | Strict Serde structures and deterministic validation | `Result<ValidatedRequest, PpError>` | Parsing at CLI/application boundary only | Unknown fields, non-finite values, zero raster dimensions, unsafe/NUL paths, symlinks, unrepresentable directory-only state, and work above caps are rejected; no fallback defaults for invalid values or failed durability confirmation | `src/core/raster.rs`, `src/adapters/sprite/manifest.rs`, `src/adapters/sprite/normalize.rs`, `src/adapters/sprite/normalize/validation.rs`, `src/io/atomic.rs`, `tests/request_contract.rs`, `tests/normalize_contract.rs` |
| C13 | Shared application execution boundary | Typed `ApplicationRequest` and exact `ApplicationOutput { stdout, exit_code }` | One synchronous result for CLI and MCP adapters | `src/application::execute` invokes existing orchestration and publication authority | CLI stdout/help/exit behavior remains unchanged; MCP classifies success from exit code and treats invalid JSON output as an internal error | `src/application/mod.rs`, `src/application/request.rs`, `src/bin/perfectpixel.rs`, `tests/mcp_contract.rs` |
| C14 | Fixed-root MCP stdio adapter | rmcp 3.1.4 typed router, root-bounded paths, one-operation semaphore | `perfectpixel.mcp-tool-result/1` envelope and structured tool result | Tokio stdio transport plus `spawn_blocking`; no subprocess or duplicate writer | Exact ten tools; unsafe paths, symlink escape, root output directory, busy calls, and cancellation lifetime fail closed | `src/mcp/mod.rs`, `src/mcp/root.rs`, `src/mcp/params.rs`, `src/bin/perfectpixel-mcp.rs`, `tests/mcp_contract.rs` |

## Boundary decisions

- `src/core/` holds only capability the product treats as unconditional: raster decode/encode,
  raster transform/inspection, and shared SVG IR parsing (`svg/`). `src/vector/` (SVG generation)
  is also core-tier — it is a first-class capability, not a feature built on top of the other two.
  Neither depends on `src/adapters/`.
- `src/adapters/sprite/` (normalize, atlas packing, manifest, Aseprite export) and
  `src/adapters/motion/` (scaffold/build, Lottie/dotLottie) are downstream, optional packaging
  features built from core capability. `sprite` consumes `core::Raster` and `core` geometry
  primitives (`FrameRect`/`Point`/`Size`/`content_bbox`); `motion` consumes SVG bytes that
  `vector` (or any raster-free SVG source) already produced. Neither adapter depends on the other.
  `Raster::copy_region_rotated_cw` (only `adapters::sprite`'s atlas packer calls it today) stays in
  `core` beside `copy_region`/`copy_from` rather than moving out: all three share the same
  validation/index math over `Raster`'s private pixel buffer, so splitting one out would trade a
  single-caller cosmetic asymmetry for a real one.
- One narrow, documented exception to "adapters never get called from core": `vector`'s opt-in
  `motion-structure-ready` profile calls `adapters::motion`'s scaffold compiler directly
  (`assess_authorized_motion_structure` → `MotionCompiler::scaffold`) as its readiness check —
  the strongest available proof that a candidate SVG will actually scaffold is running the real
  scaffolder. This is not a layering mistake: it is acyclic (`adapters::motion` never imports
  `vector`), it only activates behind an explicit non-default profile flag, and it was evaluated
  against introducing a `trait`-based port instead — rejected because there is exactly one
  implementation and one caller, and a port with no substitutability need is indirection for its
  own sake, not a real boundary.
- `src/application/generation.rs` contains pure ownership state, event, validation, and
  deterministic transition logic. It performs no I/O.
- `src/application/generation_adapter.rs` captures input and managed-file evidence,
  resolves only the published generation authority, and converts a transition into a
  publication plan and checkpoint guard. Roots without that authority are not auto-adopted.
- `src/io/artifact_set.rs` is the single library implementation that owns lock, stage,
  journal, backup, install, rollback, recovery, and directory durability mechanics. It does not
  decide product ownership.
- `src/io/atomic.rs` keeps single-file rename semantics separate and adapts exact diagnostic
  directories to the shared artifact-set protocol; it does not duplicate transaction state.
- `src/application/mod.rs` is the shared synchronous application adapter: CLI parsing, path
  resolution, conversion from domain results to stdout text, and invocation of the correct
  publisher. `src/bin/perfectpixel.rs` is only the thin process adapter.
- `src/mcp/mod.rs` is the only Tokio-backed adapter. It exposes no product-domain async API:
  synchronous application work runs in `spawn_blocking`, and cancellation does not force-stop
  that worker or release its active-operation permit early. The CLI and application layers remain
  free of Tokio dependencies.
- `src/io/parallel.rs` (`parallel_map`, `parallel_map_owned`) is the one place `std::thread::scope`
  is used, and only there: bounded-worker, disjoint-output fan-out/join for CPU-bound,
  per-item-independent work (frame resize/crop/pitch-detection, atlas bbox scan and page
  composition, artifact hashing/verification) identified in normalize, atlas packing, and
  generation-artifact verification. Each caller keeps genuinely sequential state (running byte
  totals, duplicate-path bookkeeping) in a cheap pre/post pass outside the parallel section, so
  which error is reported never depends on thread scheduling — `parallel_map` always returns the
  lowest-original-index error, verified by a dedicated multi-failure determinism test run 20
  times per case. Inputs below `MIN_PARALLEL_ITEMS` run inline, so the small sets several call
  sites routinely pass (a transaction's removals, a single atlas page) never pay thread-spawn
  cost to do less work than the spawn itself; both paths are asserted to produce identical
  results. A panicking closure propagates the original panic from either helper, matching the
  sequential loops they replaced. `mpsc`/channels are not used for this pattern: it is a
  synchronous fan-out with a join, not a producer/consumer stream, so disjoint-slice writes
  joined by `thread::scope` are the smaller, sufficient mechanism.

## Coverage rule

A capability is preserved only when its public trigger, input contract, domain decision,
output contract, side effects, and failure path remain represented in this matrix. New
implementation work must update the affected row before packaging.
