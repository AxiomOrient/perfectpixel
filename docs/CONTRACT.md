# Product Contract
> **Status: current user-facing contract, 0.3.0.** This document records shipped public behavior; `perfectpixel schema`, the implementation, and tests are the executable source of truth if it ever drifts. Historical transition plans are non-normative and cannot authorize commands, APIs, routes, thresholds, reports, or publication.


This document defines the public behavior of the local Rust CLI. Where it and the code
disagree, the code and its tests win — see the [Verification Contract](#verification-contract)
for the gate that keeps them together.

## Product Boundary

`perfectpixel schema` identifies the CLI with `role: "deterministic-asset-compiler"`.
It finalizes AI-generated or authored local assets; image generation and model inference are
outside this product. The machine-readable boundary is `modelInference: false` and
`networkRequired: false`. Publication follows `publicationPolicy: "evaluate-before-publish"`:
quality and format checks complete before an atomic file replacement becomes visible.

The raster adapter advertises these stable evidence contracts under `assetAdapter`:

- `inspectionSchema: "perfectpixel.asset-inspection/1"`;
- `transformSchema: "perfectpixel.asset-transform/1"`; and
- `digestEncoding: "sha256-lowercase-hex"`.

## Public Commands

```txt
perfectpixel schema
perfectpixel inspect <input.png|jpg|jpeg|webp>
perfectpixel convert <input.png|jpg|jpeg|webp> --out <output.png|jpg|jpeg|webp> [--width <positive integer>] [--height <positive integer>] [--filter nearest|lanczos3] [--jpeg-quality <1..100>] [--background <#RRGGBB>]
perfectpixel upscale <input.png|jpg|jpeg|webp> --out <output.png|jpg|jpeg|webp> --scale <integer >=2> [--filter nearest|lanczos3] [--jpeg-quality <1..100>] [--background <#RRGGBB>]
perfectpixel normalize --request <normalize-request.json> --out-dir <dir>
perfectpixel bundle --request <sprite-request.json> --out-dir <dir>
perfectpixel vector <input.png|jpg|jpeg|webp> --out <output.svg> [--preset auto|pixel-art|legacy-lossless|flat-icon|line-art|bounded-illustration] [--profile compact|motion-structure-ready] [--detail auto|1|2|3|4|5] [--min-quality <0..1>] [--max-quality-loss <0..1>] [--max-paths <positive integer>] [--policy <vector-policy.json>] [--report <evaluation.json>] [--diagnostics <dir>]
perfectpixel vector-analyze <input.png|jpg|jpeg|webp> [--preset auto|pixel-art|legacy-lossless|flat-icon|line-art|bounded-illustration] [--profile compact|motion-structure-ready] [--policy <vector-policy.json>] [--report <analysis.json>]
perfectpixel motion-scaffold <input.svg> --out-dir <dir>
perfectpixel motion-build --request <motion-request.json> --out-dir <dir>
```

## Raster Asset Adapter Contract

`convert` and `upscale` are CLI adapters over the format-independent `Raster` transform core.
They accept PNG, JPG, JPEG, and WebP inputs and select PNG, JPEG, or WebP output from the
`--out` extension. They cannot produce SVG; `vector` is the sole raster-to-SVG quality-gated
publication command. `motion-build` is separate: it may publish derived `animated.svg` from an
accepted raster-free SVG source, but it does not convert raster input or grant vector publication
authority.

`inspect` reads at most the bounded raster input limit once, decodes that exact byte vector, and
prints `perfectpixel.asset-inspection/1`. Its success JSON adds `schema`, `ok`, `input`,
`inputSha256`, and `inputByteCount` to the existing raster facts. `inputSha256` is the lowercase
64-hex SHA-256 of the bytes that were decoded, and `inputByteCount` is their exact length.

`convert` changes only the file format when neither `--width` nor `--height` is supplied. With
one dimension it preserves the input aspect ratio using nearest-integer rounding; with both it
uses the requested exact rectangle. Its default resize filter is `lanczos3`. `upscale` requires
an integer `--scale` of at least two and defaults to `nearest`, which preserves pixel-art source
pixels exactly. Either command accepts `--filter nearest|lanczos3` when it changes dimensions.
The output dimensions remain subject to the normal 8192-pixel-per-side and 8192²-pixel limits.
Every `Raster` value has strictly positive width and height; zero-width or zero-height values are
invalid at construction rather than being deferred to a resize or encoder panic.

PNG preserves RGBA. WebP output is lossless RGBA only. JPEG has no alpha channel: if any input
pixel is transparent, the command fails until the caller supplies `--background #RRGGBB`. The
adapter composites that matte in linear light, then encodes RGB JPEG. `--jpeg-quality` accepts
1 through 100 and defaults to 85; it and `--background` are invalid for PNG or WebP output.
All successful transforms use the single-file atomic writer and print a
`perfectpixel.asset-transform/1` JSON summary. New bytes are written and synchronized in a
same-directory temporary file before rename. Rename is the visible commit point: any failure before
rename preserves the previous destination; if parent-directory synchronization fails after rename,
the command returns an explicit I/O error stating that the replacement may already be visible. A
failure to confirm directory durability is never converted to success. The asset adapter does not
silently select lossy WebP, AI upscaling, or a generic SVG conversion route.

The transform summary keeps the existing fields and adds `inputSha256`, `inputByteCount`,
`outputSha256`, and `outputByteCount`. The input fields describe the exact bounded bytes decoded
for the transform. The output fields describe the exact encoded bytes passed to the atomic writer.
No successful summary is emitted before that writer returns success; failed writes therefore cannot
be reported as successful evidence.


## Normalize Contract

`normalize` is a fully Rust pre-bundle stage. A request requires `character`, `cellWidth`,
`cellHeight`, and ordered `states`. `sheetImage` is optional and defaults to
`"sprite-sheet.png"`; `safeMarginX`, `safeMarginY`, `packing`, `chroma`, `fit`, and
`quality` are also optional.

`normalize-request.json` and every nested request object reject unknown fields. `fit.alignY`
currently accepts only `bottom`; other vertical modes are rejected instead of being accepted as
no-op configuration. All floating-point policy values must be finite. Non-negative quality and
chroma thresholds are required; `fit.paletteSize` is 1 through 256,
`chroma.unmixReach` is at most 32, and `chroma.spillMaxFraction` is from 0 through 1.

Each state uses exactly one source form:

- `frames`: ordered PNG/JPG/JPEG/WebP frame paths relative to the request file; or
- `strip` plus `frameCount`: one row/strip image that is split by alpha-connected components.

Absolute paths, `..` traversal, NUL bytes, and backslash separators are invalid for all source paths. State names must be path-safe because they become `frames/<state>/frame-NN.png`. Normalize-state `fps` defaults to `8`, `loop` defaults to `true`, and `fps` must be from 1 through 1000.

`normalize` writes only:

- `normalize-report.json`;
- `sprite-request.json` when all gates pass;
- `frames/<state>/frame-NN.png` normalized RGBA frames when all gates pass.

The generated `sprite-request.json` is bundle-compatible and is intended to be consumed by `perfectpixel bundle` without changing the bundle schema.

The implemented normalization stages are deterministic and fully Rust:

1. optional chroma hard-key erase;
2. soft-alpha key/subject unmix for keyed edge blends;
3. trapped-spill despill for small key-tinted interior clusters;
4. alpha-connected component extraction and satellite filtering for strip sources;
5. state-local scale conformance;
6. upper-body overlap registration;
7. baseline/ground placement;
8. optional pixel-perfect pitch/phase detection, grid snap, dominant-block downscale, shared palette, and deterministic outline;
9. quality report and gates.

Quality gates reject success output when any state violates configured or default thresholds for sparse frames, chroma-adjacent residue, content-height variance, ground-line variance, or registration drift. On gate failure, `normalize-report.json` is still written with `ok:false`, but bundle-ready `sprite-request.json` and normalized frame outputs are not written.

## Bundle Contract

`bundle` accepts a JSON request with required `character`, `cellWidth`, `cellHeight`, and ordered
`states`; `sheetImage` is optional and defaults to `"sprite-sheet.png"`, and `packing` is optional.
The request, packing object, and every state reject unknown fields.

Each state has a path-safe unique `name`, required `fps`, required `loop`, and ordered PNG/JPG/JPEG/WebP frame paths relative to the request file. Absolute paths, `..` traversal, NUL bytes, and backslash separators are invalid. `fps` must be from 1 through 1000.

`cellWidth` and `cellHeight` define the logical source frame size. Input frames larger than this logical size are invalid. The atlas may store only a trimmed rectangle, but the manifest keeps the logical source size and trim offset.

`packing` defaults to:

```json
{
  "maxWidth": 2048,
  "maxHeight": 2048,
  "padding": 2,
  "trim": true,
  "allowRotation": false,
  "multipack": true
}
```

`sheetImage` must be a plain `.png` file name. It must not be `.`, `..`, a path, `manifest.json`, `sprite-sheet.json`, or `frames`.

`bundle` writes only:

- `manifest.json`;
- one or more atlas PNG pages;
- one Aseprite JSON file per atlas page;
- `frames/<state>/frame-NN.png` copies of source frames.

When one page is enough, the atlas page uses `sheetImage`. When multiple pages are needed, pages are named by adding `-00`, `-01`, ... before `.png`.

Before writing, `bundle` rejects any generated output path that would overwrite the request file or a frame input file.

Before writing generated files, `bundle` also rejects output paths whose parent
components already exist as files or symlinks. This prevents partial bundles
from being written when `frames/` or another artifact parent is blocked or points
outside the intended output tree.

The generated-output commands capture every request and source as immutable bytes plus canonical
path, filesystem revision, byte count, and SHA-256. Parsing, decoding, and domain computation use
those exact bytes. Publication verifies the same evidence immediately before output mutation and
again before the durable commit marker. An input change aborts before mutation or rolls the whole
managed set back before commit.

After a successful write, each generated-output command records its owned files in
`.perfectpixel-generation.json`. A later run replaces only that workflow generation and removes stale
owned files that are no longer part of the current generation. Stale removal is allowed only after
the existing file still matches the recorded byte count and SHA-256. Unrelated files are not
cleanup targets. A corrupt authority, competing workflow authority, missing retained file,
unowned current output, symlink, directory, or modified managed file causes fail-closed rejection.
`motion-scaffold` invalidates any previous `motion-build` generation because the authored request
must be rebuilt from the new scene.

Publication acquires the output-root OS lock, recovers any interrupted transaction, resolves the
current authority, stages every next managed byte, and snapshots every preexisting touched file.
Journal schema `perfectpixel.artifact-set-transaction/2` records each such backup with a normalized
relative `path`, exact `byteCount`, and lowercase SHA-256. The backup directory must contain exactly
that file set. Marker reads bind the opened file handle to the observed path revision, and backup or
restore tree enumeration is bounded by the artifact-count and normalized-path limits. A touched path
omitted from `backups` is authoritative evidence that the path was absent at snapshot time.

The durable state machine records `backups-ready.json` before mutation, verifies product and target
preconditions, installs new files and verified stale removals, synchronizes every affected directory
including the output root's parent, verifies the complete next generation, and only then publishes
`installed.json`. Unsupported or failed directory durability confirmation is an error, not a silent
fallback. A pre-mutation condition failure records an abort without changing outputs. A post-mutation
failure before the installed marker restores the previous set or leaves the immutable journal for the
next recovery attempt.

The installed-marker rename is the terminal visible commit point. If that marker is visible but its
transaction-directory sync fails, the writer must not start rollback: it reports that commit-marker
durability is unconfirmed and retains the terminal transaction so the next locked publisher can
resolve it. Once the installed marker is durably recorded, a later transaction-directory cleanup or
parent-sync failure is also reported explicitly, but it cannot roll the committed generation back.

Before crash recovery changes any output, it must validate the complete journal and backup tree,
durably copy every backup into a separate restore tree, verify that entire restore tree, and verify
the immutable backup tree again. Recovery then processes each touched path once. Any validation,
restaging, restoration, or cleanup failure is surfaced; recovery evidence that remains visible is
kept for retry, and a terminal installed transaction is cleaned idempotently if it reappears after a
crash. Schema `/1` is not automatically upgraded or replayed because it contains no historical backup
integrity evidence; it fails closed for explicit operator resolution.

This transaction covers `normalize`, `bundle`, `motion-scaffold`, and `motion-build`. Legacy product
ownership is imported only when no generation authority exists and a recognized control artifact
proves the complete workflow generation.

The publisher serializes cooperating writers and recovers interrupted work. It does not promise
generation-atomic visibility to readers that do not participate in that lock: such readers may
observe individual files while an installed set is being replaced.

## Atlas Manifest Contract

`manifest.json` uses schema `perfectpixel.sprite/3`.

It records:

- packing settings;
- all atlas pages;
- animation entries;
- per-frame atlas rectangle;
- per-frame source size;
- per-frame trim rectangle;
- per-frame sheet index;
- per-frame rotation flag;
- original copied frame output path.

This metadata must be enough to reconstruct the original logical frame from the packed atlas rectangle.

## Vector Contract

`Vectorizer` is the public Rust capability API. `Vectorizer::new()` verifies the embedded,
content-addressed authority. `run(&Raster, &VectorRequest)` evaluates one generation request
and returns `VectorOutcome::{Approved, Rejected}`. `analyze(&Raster, &VectorAnalysisRequest)`
returns deterministic, candidate-free `VectorAnalysis`; it is exit-neutral and does not publish.

`VectorRequest` accepts a preset, SVG profile, optional detail, minimum quality, maximum quality
loss, maximum paths, `VectorPolicy`, and diagnostic intent. `VectorAnalysisRequest` accepts only
preset, SVG profile, and policy. `VectorPolicy` is `perfectpixel.vector-policy/1`; it may constrain
allowed/required paints and unmapped/noise handling, but contains no route, threshold, backend,
digest, or approval authority. Its palette fields validate exact candidate paints; they do not map or
merge similar source colors, and their length is not a requested output color count. Numeric
generation controls may tighten embedded limits only.

`perfectpixel vector` is the sole raster-to-SVG quality-gated publication command. It requires one
PNG/JPG/JPEG/WebP input and `--out <output.svg>`; optional reports are JSON. `motion-build` may
publish derived `animated.svg` from an accepted raster-free SVG source, but it is not a raster-to-SVG
generator and does not grant vector publication authority. `perfectpixel vector-analyze` accepts exactly one
raster input and only `--preset`, `--profile`, `--policy`, and `--report`. It cannot accept output,
detail, quality, path-limit, or diagnostics options, and cannot create SVG or diagnostics. No old
vector command or API aliases, wrappers, redirects, compatibility modules, or feature flags exist.

The profile keys are `compact` → `perfectpixel.svg-editable/1` and
`motion-structure-ready` → `perfectpixel.svg-motion-structure/1`. Generation validates extensions
and collisions, captures one bounded source-file snapshot, decodes only those snapshot bytes, profiles
content, normalizes request intent, resolves an embedded route, emits raster-free candidate bytes,
validates shared bounded SVG IR, renders back, then applies all foundation and route gates. SVG
containing image elements, image filters, data-image payloads, or base64 raster content is rejected.

The embedded authority is immutable at runtime and verifies the route registry, foundation,
threshold-bundle, profile, policy, and report identities. Foundation thresholds are universal and
cannot be weakened. Promoted family routes additionally bind their locked, digest-bound corpus,
bootstrap, family evidence, and family calibration bundle. `legacyActive` publishes under the
distinct `THRESH-BOOT-001` foundation/legacy-compatibility bundle defined by
[QUALITY_CONTRACT.md](vectorize/QUALITY_CONTRACT.md). Caller policies and reports cannot waive any
phase gate.

Generated authority binds the `perfectpixel.vector-evaluation/3` report schema identity. Promoted
compact routes and `legacyActive` publish only through the verified embedded closure;
motion-structure-ready routes remain `candidateShadow` and non-publishing.
Continuous-tone input—photos, textures, complex gradients, or interior translucency—is unsupported
and non-publishing. Ambiguous or unsupported input may require an explicit preset or be rejected;
an explicit preset never waives a gate.

A generation evaluation uses report schema `perfectpixel.vector-evaluation/3`; its `digests.report`
is the embedded report-schema digest
`sha256-962c423b7aa6e6de99010847b38943e3c0c9c9683e84d53ec264d9bb7152bcef`.
The report and diagnostic artifacts are immutable evidence, not publication authority. CLI transaction
truth is authoritative: only `VectorOutcome::Approved` atomically writes the requested SVG; a
rejection writes no final SVG and never exposes candidate bytes in the result or report. Explicitly
requested diagnostics may persist candidate evidence as non-publishing artifacts. Before each requested
report, diagnostics directory, or final SVG write, the CLI rechecks the captured source's canonical
path, filesystem revision, byte count, and SHA-256; a mismatch fails that transaction phase closed.
The report's semantic `inputDigest` remains the digest of the decoded RGBA raster, not the raw source
file bytes. Their directory is an exact managed **file set** published through the same OS lock, journal
`perfectpixel.artifact-set-transaction/2`, rollback, and crash-recovery implementation as generated
workflows. Files absent from the next diagnostic set and empty parents created solely by removed
snapshot-owned files are removed transactionally. A zero-file diagnostic set still executes
`BeforeMutation` and `BeforeCommit` guards around empty-root creation; if the final guard fails, a root
created by that attempt is removed and its parent is synchronized. Existing standalone empty
subdirectories, symlinks, special files, or a file↔directory path-shape conversion fail closed because
the file-set journal does not invent directory-only ownership. A requested report records the
evaluation outcome. Bounded
adaptive illustration evidence publishes only through its verified promoted compact route; broader
adaptive behavior remains unsupported and non-publishing.

## Motion Contract

`motion-scaffold` and `motion-build` follow
[the versioned agent-authored motion contract](MOTION_CONTRACT.md). The compiler supports
path-only SVG geometry, stable IDs, part groups, related-path suggestions, optional agent-authored
paths, transform/opacity keyframes, markers, animated SVG, classic Lottie JSON, and an exploded
dotLottie v2 layout. It does not claim semantic segmentation, path morphing, bones/IK, or a packed
`.lottie` archive.

## Failure Contract

All command failures print JSON with `ok:false` and `message`.

Exit codes:

- `2`: invalid options or invalid requests;
- `3`: file I/O, image decode, SVG render, oversized image, or image encode failure;
- `4`: vector quality below the required score, unsupported continuous-tone vector content, or a
  normalize quality gate failure;
- `1`: other failures.

## Verification Contract

```bash
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo test --locked --workspace --all-targets --all-features
```

These commands are the complete verification gate; there is no second toolchain and no
packaging step. `tests/docs_contract.rs` is part of that suite and fails the gate when a
documentation link breaks, when `perfectpixel schema` stops advertising exactly the commands
the binary dispatches, or when a public command is untracked in
[FUNCTION_MATRIX.md](FUNCTION_MATRIX.md) — so this document cannot silently drift from the
implementation it describes.
