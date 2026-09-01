# PerfectPixel Clean-Break Final Handoff — 2026-09-01

## Canonical source

The completed clean-break source is the release tree published from:

```text
release/perfectpixel-cleanbreak-2026-09-01
```

The working branch used to assemble it is:

```text
feat/perfectpixel-roadmap-cleanbreak-final
```

Do not merge or cherry-pick the previous `main` implementation back into this tree. The release tree is intended to replace the trunk as a complete canonical source after local verification.

## Product identity

PerfectPixel is a deterministic local image-asset compiler and verification engine.

External frameworks and executables may produce candidate bytes/data. They never own final success. The invariant is:

```text
Input / transport
-> typed Operation
-> deterministic domain transition
-> optional external Effect
-> explicit Succeeded | Failed | Cancelled result
-> current EffectIdentity check
-> deterministic verification
-> checked publication
```

## Final authority map

| Responsibility | Authority |
| --- | --- |
| operation identity / metadata / timeout | `Operation` + `OperationRegistry` |
| CLI syntax | CLI adapter |
| MCP protocol/root admission | MCP adapter |
| artifact identity | `ArtifactRef` SHA-256 |
| encoded pixel/color provenance | codec → `PixelSpec` |
| raster/alpha semantics | core |
| geometry/compositing | core |
| verification | `VerificationSpec` / core verify |
| document semantics | `DocumentIR` |
| PSD serialization | PSD adapter |
| external process execution | Effect/runtime boundary |
| stale-result rejection | `EffectIdentity` |
| visible mutation | checked atomic publisher |

## Deliberate exclusions

The release does not contain partial or placeholder implementations for:

- an ICC conversion engine;
- SSIMULACRA2;
- an external VTracer route;
- SAM / ViTMatte / libvips adapters;
- extra Apple Vision OCR/flow/feature operations;
- OpenRaster.

ICC profile bytes and digest are preserved as provenance. A workflow requiring ICC conversion fails explicitly with `Unsupported` until a real transform engine and verification corpus are introduced.

## Inspection contract

`perfectpixel inspect` reads one immutable encoded-byte snapshot and reports:

- source SHA-256 and byte count;
- content-addressed `ArtifactRef`;
- codec-observed `PixelSpec`;
- embedded ICC profile byte count;
- width/height, alpha/content geometry, edge palette, and related deterministic raster facts.

A bare core `Raster` does not claim sRGB or any other color provenance.

## Local verification

Run on the target MacBook from a fresh checkout of the release branch:

```bash
git fetch origin
git switch --detach origin/release/perfectpixel-cleanbreak-2026-09-01

cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo test --locked --workspace --all-targets --all-features
```

Then exercise environment-specific paths separately:

1. Apple Vision foreground-instance helper on the intended macOS SDK/runtime.
2. SHA-pinned KTX encode/extract roundtrip with the intended `ktx` executable.
3. Layered PSD open/readback with the intended independent/native application when native interoperability is required.

A failed local gate is evidence. Do not weaken the test or silently bypass the failing operation.

## Trunk replacement after local verification

The user owns this step. Preserve the current trunk before replacement, then make the release tree the trunk. Do not merge the old trunk into the release tree.

One safe local sequence is:

```bash
git fetch origin

git branch archive/main-pre-perfectpixel-cleanbreak-2026-09-01 origin/main
git push origin archive/main-pre-perfectpixel-cleanbreak-2026-09-01

git switch main
git reset --hard origin/release/perfectpixel-cleanbreak-2026-09-01
git push --force-with-lease origin main
```

If `main` has intentionally changed after the archive branch was created, stop and review before replacing it. Do not convert those changes into an implicit merge.

## Runtime status

Source-level completion does not prove target-host runtime behavior. Until the local gates above are executed, build/test/lint and the environment-specific integrations remain `UNKNOWN`.

See also:

- [Architecture](ARCHITECTURE.md)
- [Product contract](CONTRACT.md)
- [Function matrix](FUNCTION_MATRIX.md)
- [MCP contract](MCP_CONTRACT.md)
- [Roadmap completion](ROADMAP_COMPLETION_2026-09-01.md)
- [Research decisions](RESEARCH_DECISIONS_2026-09-01.md)
