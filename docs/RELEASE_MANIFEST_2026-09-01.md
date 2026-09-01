# PerfectPixel Clean-Break Release Manifest — 2026-09-01

Canonical release branch:

```text
release/perfectpixel-cleanbreak-final-2026-09-01
```

This branch is the final source-tree archive of the 0.3.1 clean-break implementation. It replaces the earlier `release/perfectpixel-cleanbreak-2026-09-01` archive. It is intentionally independent of the current `main` tree and must not receive a merge/cherry-pick from that old trunk before trunk replacement.

## Included completion

- direct CLI → `Operation` dispatch;
- direct MCP `*Params` → `Operation` dispatch;
- one `OperationRegistry` metadata/timeout/capability authority;
- conservative operation risk metadata: commands that can conditionally publish, including `vector-analyze --report`, are never advertised as read-only;
- bounded scalar newtypes without an internal `expect` during construction;
- explicit external Effect success/failure/cancellation and stale-result identity;
- `ArtifactRef` / `PixelSpec` / ICC provenance;
- exact/region/CIEDE2000 verification;
- alpha/geometry/composite core;
- layered `DocumentIR` PSD compiler;
- verified KTX2 texture compiler;
- Apple Vision foreground-instance candidate Effect;
- deterministic sprite/vector/motion flows and checked publication;
- provenance-aware `inspect` output;
- final architecture, contract, research, local-handoff, and release documents.

## Deliberately absent

- fake or placeholder ICC transform;
- unused `VerificationProfile` abstraction;
- `ApplicationRequest`/argv compatibility roundtrip;
- SSIMULACRA2 without a reachable product requirement;
- unselected external VTracer path;
- speculative SAM/ViTMatte/libvips/Vision/OpenRaster adapters.

## I/O threat model

Publication uses descriptor-relative no-follow capability I/O, durable single-file rename, and journaled artifact-set replacement. Managed-directory enumeration still assumes the documented trusted local root with cooperating local writers; PerfectPixel does not claim a hostile same-user filesystem sandbox. This is a deliberate boundary, not a second publication authority.

## Local verification ownership

Build/test/lint/runtime execution belongs to the target MacBook handoff. This manifest does not claim those commands have run in the packaging environment. See `FINAL_HANDOFF_2026-09-01.md` and `LOCAL_VERIFICATION_HANDOFF.md` when replacing `main`.
