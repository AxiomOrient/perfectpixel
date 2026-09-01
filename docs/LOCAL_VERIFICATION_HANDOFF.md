# PerfectPixel Local Verification Handoff

This file is the target-Mac verification handoff for the 2026-09-01 roadmap implementation. It records commands to run; it is not evidence that they already passed.

## 1. Locked Rust gate

From the repository root:

```bash
rustc --version
cargo --version
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo test --locked --workspace --all-targets --all-features
```

Do not regenerate `Cargo.lock` merely to make a failure disappear. If `--locked` reports a manifest/lock mismatch, treat that as a defect and inspect the dependency edge first.

## 2. Public-surface gate

```bash
cargo run --locked --bin perfectpixel -- schema
cargo run --locked --bin perfectpixel -- --help
```

Confirm that the schema and CLI expose these additional compiler commands:

```text
document-psd
texture-compile
vision-foreground-instances
```

`tests/docs_contract.rs` also checks that schema command inventory matches the actual CLI dispatcher and that every public command is represented in `docs/FUNCTION_MATRIX.md`.

## 3. Operation-authority gate

The implementation must retain this flow:

```text
CLI syntax -> typed Operation -> application dispatch
MCP DTO    -> typed Operation -> application dispatch
```

There must be no `ApplicationRequest -> argv -> CLI parser` roundtrip. The CLI tests include a direct mapping check for the three new request-driven compiler commands.

## 4. Layered PSD gate

Prepare a strict `document-psd` request with:

- `schemaVersion: 2`;
- `operation: "document.compile_psd"`;
- a valid `DocumentIR`;
- exact `ArtifactRef`/`PixelSpec` bindings;
- one `.psd` output.

Run:

```bash
cargo run --locked --bin perfectpixel -- document-psd --request <request.json>
```

Verify:

- declared artifact SHA-256 values match source bytes;
- request/input changes during execution fail with a precondition error;
- the response contains structural readback and merged RGBA digest evidence;
- an existing output is not replaced on pre-publication failure;
- independent/native PSD opening is recorded separately from Rust structural verification.

## 5. KTX2 gate

Use an installed compatible Khronos `ktx` executable. Record its exact SHA-256 in the texture request.

Run:

```bash
cargo run --locked --bin perfectpixel -- texture-compile --request <texture-request.json>
```

Exercise at least:

- `color_srgb` with explicit CIEDE2000 thresholds;
- `linear` with `maximumAbsoluteChannelError`;
- `normal_map` + UASTC;
- one invalid/stale input `ArtifactRef`;
- one invalid executable digest.

Confirm the result performs encode -> KTX2 inspection -> extract -> decode roundtrip -> verification before final publication. Confirm failed/cancelled/timed-out Effects do not publish the final KTX2 and temporary carrier/candidate files are removed.

## 6. Apple Vision gate

Build `tools/apple-vision-helper/main.swift` with the local macOS SDK, then hash the executable and place that digest in the request.

Run:

```bash
cargo run --locked --bin perfectpixel -- vision-foreground-instances --request <vision-request.json>
```

Confirm:

- request revision 1 is accepted and unsupported revisions fail explicitly;
- the helper receipt records provider, adapter version, OS version, request type/revision, and instances;
- returned L8 mask pixels become PerfectPixel `Mask` coverage, not synthetic RGBA alpha;
- deterministic threshold/close/fill-holes/feather cleanup occurs after the Effect;
- coverage/component/bounds verification occurs before publication;
- helper staging state is removed before final artifact-set publication;
- request/input changes, stale result identity, failure, cancellation, and timeout cannot publish.

Vision output itself is not expected to be bit-exact across OS/model revisions; verify the deterministic postconditions PerfectPixel owns.

## 7. Existing regression surface

Do not limit verification to the new commands. The locked test suite must preserve existing raster edit, flattened PSD, sprite normalize/bundle, vector analyze/compile, motion scaffold/build, publication recovery, and MCP contracts.

The MCP tool inventory remains its existing fixed compatibility surface. Do not interpret CLI/MCP parity as identical inventory; parity means shared semantic implementation for operations exposed by both.

## 8. Failure reporting

Record any failing command with:

```text
operation
cause
context
retryable / non-retryable
failure vs cancellation vs timeout
whether destination bytes changed
```

Do not weaken a gate, add a fallback, or update a golden merely to obtain PASS.

## Current evidence state

At package creation, the implementation and static repository contracts were reviewed in the remote source. The target Rust toolchain, KTX executable, Apple Vision runtime, and native PSD interoperability were not executed in this environment. Those items remain UNKNOWN until this handoff is run locally.
