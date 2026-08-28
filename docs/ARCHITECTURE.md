# Architecture

This document describes the implemented architecture of `perfectpixel` 0.3.0.
The public contract is defined in [CONTRACT.md](CONTRACT.md); capability coverage is
tracked in [FUNCTION_MATRIX.md](FUNCTION_MATRIX.md).

## Design rule

The product keeps deterministic decisions in pure Rust data structures and confines
filesystem mutation to explicit adapters. State machines are used only where state,
concurrency, or recovery is real:

- generation ownership uses `GenerationAuthorityState` and
  `GenerationAuthorityEvent::Replace`;
- recoverable publication uses `ArtifactTransactionState` and
  `ArtifactTransactionEvent`;
- vector CLI publication uses its existing explicit transaction outcome reducer;
- ordinary raster transforms remain direct pure functions and are not wrapped in actors.

There is no Tokio runtime in the product. The current executable is a synchronous local
CLI, and the only shared concurrency boundary is publication to one output root. That
boundary is serialized by an operating-system file lock (a single non-blocking `try_lock`,
not a wait — there is no timeout/cancellation boundary for `select!` to add). Adding tasks,
channels, or an actor there would create a second authority without a concurrent workload
to isolate. Separately, `io::parallel_map`/`parallel_map_owned` apply bounded
`std::thread::scope` fan-out/join to CPU-bound, per-item-independent work identified inside
normalize, atlas packing, and generation-artifact verification (frame resize/crop,
bbox scans, hashing) — a genuine, narrow concurrency boundary, applied only where it exists.
See [FUNCTION_MATRIX.md](FUNCTION_MATRIX.md#boundary-decisions) for the determinism
guarantee this relies on.

## Dependency direction

```text
core (unconditional capability, no dependency on adapters)
    core::{raster, transform, inspect, svg}
    vector::{request, route, evaluation, report, backends, authority}
    bin/perfectpixel/generation.rs

adapters (optional packaging features built from core)
    adapters::sprite::{normalize, atlas, manifest, aseprite}
    adapters::motion::{scaffold/build compiler}

application orchestration
    bin/perfectpixel.rs
    bin/perfectpixel/generation_adapter.rs

I/O adapters
    io::{codec, atomic, artifact_set, parallel}
    bin/perfectpixel/asset_codec.rs
```

Core modules do not read command-line arguments, inspect output directories, acquire
locks, or publish files. The CLI resolves paths and converts bytes into domain inputs.
I/O adapters own decoding, encoding, locking, staging, journaling, installation,
rollback, and recovery.

`adapters::sprite` and `adapters::motion` depend on `core` (raster/geometry primitives and,
for motion, parsed SVG); neither depends on the other, and core never depends on either
adapter — with one narrow, acyclic exception: `vector`'s opt-in `motion-structure-ready`
profile calls into `adapters::motion`'s scaffold compiler as its readiness check. See
[FUNCTION_MATRIX.md](FUNCTION_MATRIX.md#boundary-decisions) for why that edge exists and
why it was not wrapped in a `trait` port instead.

`ArtifactSetOps` is the narrow filesystem port used by the recoverable publisher and its
failure-injection tests. Production uses `StdArtifactSetOps`. `src/io/artifact_set.rs` is compiled
once in the library and is the single implementation used by both generated workflows and exact
vector-diagnostics publication; the CLI no longer includes a binary-local copy. Other I/O boundaries
use small concrete adapters (`ImageCodec`, `AtomicFileWriter`, `AtomicDirectoryWriter`,
`AtomicArtifactSetWriter`) because there is only one production mechanism and no domain policy
depends on substituting them.

## Authoritative state

| State | Authority | Scope |
| --- | --- | --- |
| Raster or SVG computation | Immutable in-memory value owned by the current command | One command invocation |
| Generated file ownership | `.perfectpixel-generation.json` | One output root; workflows `normalize`, `bundle`, `motion-scaffold`, `motion-build` |
| Publication transaction | Durable sibling transaction directory and markers | One attempted artifact-set replacement |
| Publisher concurrency | Sibling OS-locked file | All cooperating publishers for one output root |
| Vector route/quality policy | Embedded content-addressed vector authority | Process-wide immutable product policy |

No README, filename convention, directory scan, or stale output may override these
authorities. Legacy generated files are adopted only when the authority file is absent
and a recognized control artifact proves the complete workflow generation.

The `edit` and `chroma-plan` commands are intentionally one-invocation pure raster boundaries:
`chroma-plan` only returns a fixed-palette OKLab decision, while `edit` stages one PNG through
`AtomicFileWriter` after its bounded transforms succeed. `remove_background_auto` computes a
complete edge histogram and an integer coverage proof before reusing the existing four-connected
keyed flood; it is controlled chroma/checker/flat-edge cleanup, not semantic segmentation or
matting, and heterogeneous photographic edges fail closed when the requested coverage is unmet.

## Generated workflow

The four generated-output commands share one pipeline:

```text
request/source path
  -> capture exact bytes + canonical path + file revision + SHA-256
  -> parse/decode those captured bytes
  -> pure domain decision/transformation
  -> construct GenerationAuthorityEvent::Replace
  -> acquire output-root OS lock
  -> recover an interrupted prior transaction
  -> read current authority from one stable file snapshot
  -> pure authority reduction and stale-set calculation
  -> verify retained/superseded/unowned-output preconditions
  -> stage all next bytes
  -> snapshot all preexisting touched files
  -> record journal /2 backup evidence (path + byteCount + SHA-256)
  -> durable BackupsRecorded event
  -> verify the exact backup tree, inputs, targets, and authority again (BeforeMutation)
  -> install next files and remove verified stale files
  -> synchronize affected directories from deepest parent through output-root parent
  -> verify complete next generation and inputs (BeforeCommit)
  -> durable InstallRecorded event
  -> remove transaction journal
```

The pure generation reducer returns:

- the complete next authority;
- retained artifacts that must remain exact;
- superseded artifacts that prove prior ownership;
- stale artifacts that may be removed only while their recorded size and digest match.

`motion-scaffold` explicitly invalidates `motion-build`, because the previous build is
bound to the old normalized scene and request. A failed normalize gate is a valid
report-only generation, so previous success frames and `sprite-request.json` become
stale instead of remaining deceptively current.

## Publication state machine

The generic artifact-set writer owns this state model:

```text
Prepared --BackupsRecorded--> BackupsReady
Prepared/BackupsReady --PublicationAborted--> Aborted
BackupsReady --InstallRecorded--> Installed
BackupsReady --RollbackRestored--> Restored
any state --RecoveryRequired--> RecoveryRequired
```

The writer stages every new byte before mutation. Journal schema
`perfectpixel.artifact-set-transaction/2` records every preexisting touched file as a
strictly ordered `(path, byteCount, sha256)` backup record; touched paths without a record
were absent at snapshot time. Marker reads bind the opened handle to the path revision, and
transaction tree traversal is capped by artifact-count and normalized-path limits. Before
ordinary installation it verifies that the backup tree exactly matches those records.

Crash recovery does not mutate an output immediately. It first validates the complete
journal and immutable backup tree, copies every verified backup into a separate durable
`restore/` tree, verifies the complete restore tree, then verifies the original backup
tree again. Only after that barrier does it process each sorted touched path once:
restore a recorded prior file, remove a newly created write, or leave a previously absent
removal untouched. The original backup tree is never consumed, so an interrupted restore
can retry from the same evidence.

A failed application condition before mutation writes an `aborted.json` marker and cleans
the untouched transaction. A failure after mutation but before the terminal marker restores the
previous managed set. `installed.json` is published only after installed bytes and managed
directories have been verified and synchronized. If the marker rename becomes visible but syncing
the transaction directory fails, rollback is forbidden: the adapter returns an explicit
commit-durability uncertainty and retains the terminal journal for the next locked recovery. Once the
marker is durable, cleanup failure is still reported but cannot undo the committed generation.

If restoration cannot complete, the journal remains and the next cooperating publication performs
recovery before planning new work. A terminal journal that reappears after an unconfirmed cleanup is
cleaned idempotently. Schema `/1` is rejected fail-closed: because it recorded only paths, hashing its
current backup bytes would not prove those bytes still equal the historical pre-transaction state.
Numeric transaction names are recognized by grammar rather than native integer range, so stale
transaction evidence is not ignored merely because a PID or attempt component overflows the current
process's integer type. Transaction directory creation/removal and output-root
creation/replacement are synchronized in their parent; an unsupported directory-sync operation is an
explicit failure rather than a silent durability fallback.

This is set-level durability and recovery, not generation-atomic visibility for
uncoordinated readers. Such readers can observe individual file replacements while a
set is installing.

## Input revision binding

`normalize`, `bundle`, `motion-scaffold`, and `motion-build` compute from immutable byte
snapshots. The same snapshot evidence is checked immediately before output mutation and
again before the durable commit marker. Publication aborts or rolls back when an input
path retargets, an opened file revision changes, the byte count changes, or SHA-256 no
longer matches.

On Unix, revision evidence includes device, inode, size, modification time, and change
time. Other platforms use the strongest portable `std` metadata plus canonical path,
size, and full digest verification.

## Capability flows

### Raster inspect / convert / upscale

```text
CLI arguments -> bounded decode -> pure inspection/resize -> format adapter -> atomic file write
```

These commands have no multi-file state. A direct `Result` contract and one atomic file writer are
sufficient. The writer stages and synchronizes a same-directory temporary file, then renames it and
synchronizes the parent. Rename is the visible commit point. Failures before it preserve the previous
destination; a parent-sync failure is reported explicitly after the replacement may already be
visible. A reducer or actor would add no authority here because there is no recoverable multi-step
state machine.

### Normalize

```text
strict request + raster snapshots
  -> deterministic normalization pipeline
  -> quality report and gate decision
  -> success generation (report + request + frames)
     or failure generation (report only)
  -> shared generated-output publisher
```

### Bundle

```text
strict request + frame snapshots
  -> pure atlas/manifest plan
  -> encoded atlas, Aseprite JSON, frame copies, manifest
  -> shared generated-output publisher
```

### Vector

```text
bounded raster decode
  -> immutable route/policy resolution
  -> raster-free candidate generation
  -> SVG validation and render-back evaluation
  -> VectorOutcome::{Approved, Rejected}
  -> explicit report / diagnostics / final-SVG transaction order
```

Only an approved outcome can publish the final SVG. Analysis never generates a candidate or writes
diagnostics. Report, diagnostics, and final SVG are intentionally separate commits because they have
different truth and retention contracts. Diagnostics are nevertheless internally consistent as one
exact managed file set: `AtomicDirectoryWriter` discovers the current regular-file set under the
output-root lock, derives stale removals, and delegates stage/journal `/2`/rollback/recovery to the
shared artifact-set writer. A zero-file exact set still runs both publication guards around root
creation under the same lock; a newly created empty root is removed when the final guard fails.
Standalone empty directories and file↔directory path-shape transitions fail closed because this
protocol owns files, not unrecorded directory-only state.

### Motion

```text
SVG snapshot -> scaffolded scene/layers/request -> scaffold generation
strict motion request + scene snapshot -> validated compiler -> build generation
```

Motion supports transform/opacity animation over raster-free path geometry. Semantic
segmentation, path morphing, bones/IK, and packed `.lottie` archives remain outside the
product contract.

## Bounded work and contamination control

The implementation rejects unsafe relative paths, traversal, NUL bytes, backslash
separators, symlinked managed files/parents, input-output aliases, duplicate ownership,
unknown request fields, non-finite policy values, and configured work beyond explicit
limits. Relevant caps include generated artifact count/bytes, input snapshot count/bytes,
decoded raster bytes, authority bytes, transaction journal bytes, image dimensions and
pixels, vector diagnostics entries, normalization palette size, and chroma unmix reach.

Failure is explicit through `Result` and typed product errors. There is no silent
fallback, automatic authority reconstruction after corruption, or best-effort deletion
of modified user files.

## Residual boundaries

- The publisher coordinates cooperating writers only; an unrelated process can still
  race ordinary path-based filesystem operations.
- Crash and power-loss behavior depends on filesystem `fsync` and rename semantics and requires
  runtime fault testing on the deployment filesystem. The implementation does not convert an
  unsupported directory durability operation, visible installed-marker sync ambiguity, or committed
  transaction cleanup failure into success.
- The direct output-root parent and transaction parent are synchronized. When a previously absent
  multi-level parent path is created with `create_dir_all`, every newly created ancestor entry is not
  individually journaled and synchronized; power-loss durability of that ancestor chain therefore
  requires deployment-filesystem evidence.
- Single-file rename is a visible commit point; a subsequent parent-sync error can be returned after
  the new file has become visible. Callers must inspect the destination before retrying that error.
- Exact directory publication does not represent standalone empty subdirectories or file↔directory
  path-shape transitions.
- Windows compilation, replacement semantics, and non-Unix file-revision behavior require
  target-specific verification.
- The source package records these as verification risks rather than hiding them with a
  fallback path.
