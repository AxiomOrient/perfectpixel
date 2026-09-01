# PerfectPixel Agent Protocol v2 — M7/M8 execution contract

## Status

The local protocol is complete through the M8 canonical boundary. PerfectPixel
keeps one compact RGBA8/sRGB/straight working raster for agent composition.
UpAgent owns semantic adapters and task qualification; PerfectPixel never
fabricates semantic masks or generative pixels.

## Identity

PerfectPixel is the deterministic/non-generative image execution plane used by UpAgent.
It accepts content-addressed local artifact snapshots and explicit stateless requests.
It never owns hidden canvas/selection state and never reconstructs missing pixels with a generative model.

`agent-schema` returns the strict capability manifest with protocol schema
`perfectpixel.agent-image/2`, protocol version `2.0.0`, behavior version
`2.0.0`, implementation version `0.3.1`, and the complete capability list. The
manifest identity used by UpAgent is the lowercase SHA-256 of:

```text
perfectpixel.agent-image/capability-manifest/2\0<canonical manifest JSON>
```

Canonical JSON keeps the manifest fields in schema order and sorts capabilities
by `(name, version, determinism)`. Capability names must be unique. UpAgent
stores this digest alongside the executable SHA-256 and rejects an operation
when any executable, protocol, behavior, implementation, or manifest identity
changes after composition. Additive capabilities remain subset-compatible for
operation admission, but a changed manifest digest requires a new explicit
composition identity.

Current agent commands:

```text
agent-schema
agent-inspect --request <request.json>
agent-extract --request <request.json> --out-dir <new-or-empty-dir>
agent-render --request <request.json> --out-dir <new-or-empty-dir>
agent-compare --request <request.json> --out-dir <new-or-empty-dir>
```

## M3 extract

Implemented deterministic selectors:

- `existing_alpha`
- `chroma_key` (edge-connected RGB-key behavior inherited from the deterministic raster core)
- `color_range`
- `alpha_component`
- `provided_mask`

`agent-extract` produces:

- cropped `Object` PNG whose alpha is the final matte;
- cropped `Mask` PNG;
- optional full-canvas remainder PNG;
- source canvas/bbox/pivot/completeness facts;
- content-addressed descriptors and operation receipt.

Matte refinement currently supports bounded separable feathering, thresholding and a `requireSoftAlpha` gate.
The matte is clamped to source alpha, so existing semi-transparency is never squared or amplified.
The object bbox includes every non-zero alpha pixel.

### Explicit M3 limits

PerfectPixel's deterministic plane does not implement natural-image segmentation
or high-fidelity hair/glass matting. UpAgent exposes an explicit
`extract.semantic_mask` adapter boundary for point/box/scribble requests; when
no native backend is configured it returns a structured unavailable failure or
cancellation, never a fabricated mask. Native semantic quality is [UNKNOWN]
until an external adapter is qualified.

## M4 render

`agent-render` executes a one-shot composition graph:

```text
external raster/object artifact leaves
        ↓
per-node z + 3x3 affine transform + opacity
        ↓
nearest sampling + source-over
        ↓
canvas root
        ↓
atomic render.png
```

The request contains the entire composition. There is no durable mutable scene graph.
Nodes are uniquely identified and deterministically ordered by `(z, id)`. Raster
and object nodes may carry an ordered `operations` list containing crop, rotate,
flip, nearest/Lanczos3 resize, explicit edge-key removal, or controlled automatic
edge-background removal. A text node instead carries shaped, font-byte-bound text
input and does not accept raster operations. These operations are the canonical
replacement for the removed model-facing compatibility editor and run before
composition.
Transforms must be finite 2D affine matrices with an invertible 2x2 linear component.
The renderer has explicit dimension/node/pixel-work bounds.

M4 exposes raster/object leaves, typed text leaves, ordered deterministic
preprocessing, and nearest affine sampling. Vector nodes, perspective/mesh warp
and additional blend modes are not part of this agent wire request.

## M5 compare

`agent-compare` evaluates bounded deterministic assertions over equal-sized before/after rasters and optional mask artifacts. Required assertion failures return a `rejected` operation receipt while the command itself succeeds, preserving diagnostics for the orchestration layer. Supported assertions cover exact equality, changed ratios, mask-locality, unchanged regions, alpha IoU, mask leakage, object geometry, maximum channel error and mean absolute error.

The canonical strict wire types are `agent::CompareRequest` and `agent::CompareResult`, mirrored by `docs/schemas/agent-compare-request-v2.schema.json` and `docs/schemas/agent-compare-result-v2.schema.json`. Unknown fields are rejected. Assertion IDs are unique and bounded, `changed_ratio` must define at least one bound, and every request must contain at least one `required` assertion. Optional bounds are omitted rather than encoded as `null`. The capability manifest advertises `compare.exact_spec`; UpAgent must require it before using a compare result for task qualification. The complete manifest digest is part of the UpAgent image composition identity.

Optional difference and mask-overlay previews are bounded and published atomically. Every input and preview descriptor is bound by content hash and the canonical dependency closure. Comparison is `bit_exact`.

PerfectPixel does not decide whether an aesthetically plausible image is acceptable; UpAgent's Image Director combines this deterministic evidence with the actual multimodal observation.

## M7 text contract

`PixelSpec` is explicit at the agent composition boundary and the working
contract is `rgba8/srgb/straight`. The agent image wire deliberately does not
pretend to preserve high-precision or alternate color-space samples; callers
must normalize such inputs before invoking agent operations.

`TextNode` is a separate library contract (`perfectpixel.text-node/1`; see
[`text-node-v1.schema.json`](schemas/text-node-v1.schema.json)). It binds
content, direction, language, box, alignment, line-break mode, font size and
colour to supplied TTF/OTF bytes plus SHA-256. `rustybuzz 0.20.1`, `resvg
0.47.0` (for its pinned tiny-skia path), and `ttf-parser 0.25.1` are pinned in
the lockfile. Missing glyphs, unsupported font data, overflowed layout, and
unsupported output specs fail explicitly; system-font lookup and fallback are
disabled. Rustybuzz's shaped positions drive the supplied glyph outlines
directly; no SVG text relayout occurs. A successful render returns glyph
ID/cluster/position snapshots, line spans/advances, and a SHA-256 over the
exact RGBA raster.

## Artifact and trust rules

- path is transport only; SHA-256 + media type + byte length are identity;
- all inputs are captured and hash-verified before decode;
- dependencies are sorted/deduplicated before descriptor/receipt publication;
- output directories must be absolute, new or empty, and must not contain the request or any input artifact;
- outputs are published atomically as a managed directory;
- UpAgent re-reads output bytes and validates descriptor hashes, dependency closure and receipt binding.

## Determinism

- deterministic extraction: `bit_exact`;
- integer source-over: `bit_exact`;
- general affine coordinate inversion uses finite `f64`, therefore the full render capability is conservatively advertised as `backend_deterministic`.

Preprocessing operation order is part of the request hash and the parity corpus;
each output receipt remains content-addressed and stateless.

## Non-goals

Current protocol does not:

- generate a new image except through UpAgent's separate generation provider;
- perform generative fill/inpainting;
- invent occluded background;
- claim semantic segmentation or production-grade natural-image matting without
  an externally qualified adapter;
- claim that the compact agent-composition wire preserves high-precision pixels;
  use the explicit `convert` PixelSpec boundary for RGBA16/Display-P3 assets;
- expose hidden mutable editor state.

The standalone human `perfectpixel edit` command remains a supported local JSON
contract. It is not a model-visible tool and shares the same `RasterEdit` core
as agent-render preprocessing. The parity corpus covers crop, rotate, flip,
nearest/Lanczos3 resize, keyed removal and automatic edge-background planning.
