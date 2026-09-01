# Research Decisions — 2026-09-01

This document records external research used to close the current clean-break implementation. It is not runtime evidence.

## MCP 2026-07-28

Source: Model Context Protocol 2026-07-28 specification announcement.

Decision:

- keep the MCP adapter stateless at the protocol layer;
- do not reintroduce `initialize`/session compatibility state;
- do not infer that protocol statelessness requires application workers to be cancellable mid-publication;
- preserve explicit PerfectPixel application state only where the product owns real state.

Reference:
- https://blog.modelcontextprotocol.io/posts/2026-07-28/

## Apple Vision foreground instance masks

Apple documents `GenerateForegroundInstanceMaskRequest`, explicit request revisions, and `supportedRevisions`. Revision 1 is currently documented.

Decision:

- keep `vision.apple.foreground_instances` as a pinned external Effect;
- keep `requestRevision` explicit in request/receipt evidence;
- keep deterministic PerfectPixel mask cleanup and verification after Vision returns candidates;
- do not treat opaque Vision output as deterministic product success;
- keep target-host Vision behavior UNKNOWN until tested on the target macOS SDK/runtime.

References:
- https://developer.apple.com/documentation/vision/generateforegroundinstancemaskrequest
- https://developer.apple.com/documentation/vision/generateforegroundinstancemaskrequest/revision-swift.enum

## ICC provenance and color conversion

Rust `image` 0.25 exposes `ImageDecoder::icc_profile`, which is sufficient to preserve exact embedded profile provenance. The `lcms2` crate exposes real ICC `Profile` and `Transform` implementations backed by LittleCMS.

Decision:

- preserve ICC bytes and digest at decode;
- never infer sRGB when the source has no profile;
- remove the previous fake/stub transform API;
- fail explicitly when an operation requires ICC conversion;
- add LittleCMS only if a concrete product requirement needs ICC conversion and a verification corpus can be defined.

References:
- https://docs.rs/image/latest/image/trait.ImageDecoder.html
- https://docs.rs/lcms2/latest/lcms2/
- https://docs.rs/lcms2/latest/lcms2/struct.Transform.html

## KTX2 tooling

Khronos documents the unified `ktx` CLI and its `create`, `extract`, `transcode`, `validate`, and related commands. `ktx create` produces KTX2 from image inputs and `ktx extract` can emit PNG for supported formats.

Decision:

- keep the KTX executable as a SHA-256-pinned Effect dependency;
- external exit code 0 remains only a candidate event;
- retain structural KTX2 validation and decode/extract roundtrip before publication;
- keep timeout owned by `OperationRegistry` rather than handler constants.

References:
- https://github.khronos.org/KTX-Software/ktxtools/ktx.html
- https://github.khronos.org/KTX-Software/ktxtools/ktx_create.html
- https://github.khronos.org/KTX-Software/ktxtools/ktx_extract.html

## SSIMULACRA2

`fast-ssim2` 0.8.2 is a practical Rust SSIMULACRA2 implementation with runtime SIMD, bounded-memory strip comparison, precomputed references, and cooperative cancellation.

Decision:

- do not add it to the current release because no reachable product operation requires the extra metric;
- retain CIEDE2000 plus exact/alpha/geometry/region gates;
- if future image-optimization workflows need perceptual structural quality, `fast-ssim2` is the preferred candidate for evaluation rather than a custom metric implementation.

References:
- https://docs.rs/fast-ssim2/latest/fast_ssim2/
- https://github.com/imazen/fast-ssim2

## Final research conclusion

No researched dependency justifies adding another semantic owner to the current release.

The retained invariant is:

```text
external framework/library/tool
-> candidate bytes/data
-> PerfectPixel deterministic normalization
-> PerfectPixel verification
-> checked publication
```

Optional dependencies should be introduced only when a reachable product requirement needs them and the existing authority boundary can consume them without creating a second success policy.
