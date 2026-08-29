# Third-party notices

This source publication uses the direct Rust dependencies declared in
[`Cargo.toml`](Cargo.toml). The versions below are the versions selected by
[`Cargo.lock`](Cargo.lock); the SPDX expressions are the `license` values
reported by `cargo metadata --locked` for those lock-selected package
manifests. This is factual attribution, not a legal conclusion.

Only direct entries under `[dependencies]` are listed. Cargo.lock also records
transitive and build-time packages needed to compile the source; they are not
direct dependencies and are not repeated here. Consult the lock file and each
package's upstream metadata for that complete resolved graph.

## Direct Rust dependencies

| Component | Cargo.toml requirement | Cargo.lock version | SPDX license expression |
| --- | --- | --- | --- |
| `binpack2d` | `=1.0.1` | `1.0.1` | `MIT` |
| `fast_image_resize` | `=6.1.0` | `6.1.0` | `MIT OR Apache-2.0` |
| `image` | `0.25` | `0.25.10` | `MIT OR Apache-2.0` |
| `serde` | `1` | `1.0.228` | `MIT OR Apache-2.0` |
| `serde_json` | `1` | `1.0.150` | `MIT OR Apache-2.0` |
| `schemars` | `1` | `1.2.2` | `MIT OR Apache-2.0` |
| `svgtypes` | `=0.16.1` | `0.16.1` | `Apache-2.0 OR MIT` |
| `thiserror` | `1` | `1.0.69` | `MIT OR Apache-2.0` |
| `resvg` | `0.47` | `0.47.0` | `Apache-2.0 OR MIT` |
| `roxmltree` | `=0.21.1` | `0.21.1` | `MIT OR Apache-2.0` |
| `rmcp` | `=3.1.4` | `3.1.4` | `Apache-2.0` |
| `tokio` | `1` | `1.53.1` | `MIT OR Apache-2.0` |

The feature selections and `default-features` settings remain exactly those in
`Cargo.toml`; this table does not imply any additional features. A bare version
in the requirement column is Cargo's implicit caret requirement.

## Source-only publication limits

This repository publishes the Rust source, manifests, lockfile, tests, and
documentation. It does not publish `target/` output or generated asset files.
Build and runtime evidence therefore depends on the operator's local Rust
toolchain; this notice does not assert that environment-specific evidence is
included here.
