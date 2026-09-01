# Third-party notices

PerfectPixel keeps the direct Rust dependency surface intentionally bounded. The canonical dependency declaration is [`Cargo.toml`](Cargo.toml); [`Cargo.lock`](Cargo.lock) is the resolved graph used by `--locked` local verification.

## Direct Rust dependencies

| Component | Requirement | Purpose |
| --- | --- | --- |
| `base64` | `0.22` | bounded binary metadata encoding |
| `binpack2d` | `=1.0.1` | deterministic sprite atlas packing |
| `fast_image_resize` | `=6.1.0` | bounded raster resampling |
| `image` | `0.25` | PNG/JPEG/WebP decode and encode boundary |
| `libc` | `0.2` | Unix process/runtime primitives |
| `resvg` | `0.47` | SVG parse/render verification |
| `rmcp` | `3.1.4` | typed local stdio MCP transport |
| `roxmltree` | `=0.21.1` | bounded SVG/XML parsing support |
| `schemars` | `1.0` | MCP/request schema generation |
| `serde` | `1` | typed serialization |
| `serde_json` | `1` | strict JSON contracts and receipts |
| `svgtypes` | `=0.16.1` | SVG path parsing |
| `thiserror` | `1` | typed error definitions |
| `tokio` | `1` | MCP/runtime async boundary |

`moxcms` and `fast-ssim2` are not PerfectPixel direct dependencies. Embedded ICC provenance is retained and fails closed at the transform boundary in this minimal build. Perceptual verification uses the in-repository CIEDE2000 implementation together with exact, alpha, geometry, and region gates.

Cargo.lock may contain transitive packages required by the crates above. A transitive package does not own PerfectPixel semantics or success policy.

## Source-only publication limits

This repository publishes source, manifests, lockfile, tests, documentation, and the Apple Vision helper source. It does not claim build/runtime evidence unless that evidence was produced by the verifying host.
