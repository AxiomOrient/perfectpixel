# PerfectPixel Clean-Break Release Manifest — 2026-09-01

Canonical release branch:

```text
release/perfectpixel-cleanbreak-2026-09-01
```

This branch is a source-tree archive of the completed 0.3.1 clean-break implementation. It is intentionally independent of the current `main` tree and must not receive a merge/cherry-pick from that old trunk before replacement.

## Included completion

- direct CLI → `Operation` dispatch;
- direct MCP `*Params` → `Operation` dispatch;
- one `OperationRegistry` metadata/timeout authority;
- explicit external Effect success/failure/cancellation and stale-result identity;
- `ArtifactRef` / `PixelSpec` / ICC provenance;
- exact/region/CIEDE2000 verification;
- alpha/geometry/composite core;
- layered `DocumentIR` PSD compiler;
- verified KTX2 texture compiler;
- Apple Vision foreground-instance candidate Effect;
- deterministic sprite/vector/motion flows and checked publication;
- provenance-aware `inspect` output;
- final architecture, contract, research, local-verification, and handoff documents.

## Deliberately absent

- fake or placeholder ICC transform;
- unused `VerificationProfile` abstraction;
- `ApplicationRequest`/argv compatibility roundtrip;
- SSIMULACRA2 without a reachable product requirement;
- unselected external VTracer path;
- speculative SAM/ViTMatte/libvips/Vision/OpenRaster adapters.

## Verification state

Target-host build/test/lint/runtime verification is not represented as PASS in this manifest. Run the commands in `FINAL_HANDOFF_2026-09-01.md` and `LOCAL_VERIFICATION_HANDOFF.md` on the target MacBook.
