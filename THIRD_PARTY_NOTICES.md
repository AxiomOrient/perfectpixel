# Third-party notices

PerfectPixel keeps the direct Rust dependency surface intentionally small. The canonical dependency declaration is [`Cargo.toml`](Cargo.toml); [`Cargo.lock`](Cargo.lock) is the resolved graph used by `--locked` local verification.

## Direct Rust dependencies

| Component | Cargo.toml requirement | Purpose |
| --- | --- | --- |
| `base64` | `0.22` | bounded binary metadata encoding |
| `image` | `0.25` | PNG/JPEG/WebP decode and encode boundary |
| `libc` | `0.2` | Unix process/runtime primitives |
| `resvg` | `0.47` | SVG parse/render verification |
| `rmcp` | `3.1.4` | typed local stdio MCP transport |
| `schemars` | `1.0` | MCP/request schema generation |
| `serde` | `1` | typed serialization |
| `serde_json` | `1` | strict JSON contracts and receipts |
| `tokio` | `1` | MCP/runtime async boundary |

`moxcms` and `fast-ssim2` are not direct dependencies. Embedded ICC provenance is retained and fails closed at the transform boundary in this minimal build; perceptual verification uses the in-repository CIEDE2000 implementation plus exact/alpha/geometry/region gates.

Transitive packages remain recorded by Cargo.lock and may include implementation dependencies of the direct crates. A transitive package is not a PerfectPixel semantic authority.

## Source-only publication limits

This repository publishes source, manifests, lockfile, tests, documentation, and the Apple Vision helper source. It does not claim local build/runtime evidence unless that evidence was produced by the verifying host.
