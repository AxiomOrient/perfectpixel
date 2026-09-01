use std::fs;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use perfectpixel::{
    agent_capability_manifest, apply_raster_edits, capability_manifest_sha256,
    dependency_closure_sha256, sha256_hex, AgentArtifactDependency, AgentArtifactPinSet,
    AgentCompareAssertionRequest, AgentCompareResult, ImageCodec, PngEncoder, Raster, RasterEdit,
    ResampleFilter, AGENT_BEHAVIOR_VERSION, AGENT_PIN_SET_SCHEMA, AGENT_PROTOCOL_SCHEMA,
    AGENT_PROTOCOL_VERSION,
};

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn temp_root(label: &str) -> std::path::PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time")
        .as_nanos();
    let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "perfectpixel-agent-v2-{label}-{}-{nanos}-{sequence}",
        std::process::id()
    ))
}

fn valid_png() -> Vec<u8> {
    vec![
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x04, 0x00, 0x00, 0x00, 0xb5,
        0x1c, 0x0c, 0x02, 0x00, 0x00, 0x00, 0x0b, 0x49, 0x44, 0x41, 0x54, 0x78, 0xda, 0x63, 0x64,
        0xf8, 0x0f, 0x00, 0x01, 0x05, 0x01, 0x01, 0x27, 0x18, 0xe3, 0x66, 0x00, 0x00, 0x00, 0x00,
        0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
    ]
}

#[test]
fn agent_manifest_exposes_v2_foundation_capabilities() {
    let manifest = agent_capability_manifest("0.3.0");
    assert_eq!(manifest.schema, AGENT_PROTOCOL_SCHEMA);
    assert_eq!(manifest.protocol_version, AGENT_PROTOCOL_VERSION);
    assert_eq!(manifest.behavior_version, AGENT_BEHAVIOR_VERSION);
    for required in [
        "inspect.basic",
        "artifact.content_addressed",
        "artifact.dependency_closure",
        "artifact.pin_set",
        "compare.basic",
        "compare.exact_spec",
        "compare.regions",
        "compare.masks",
        "compare.geometry",
        "compare.preview",
        "render.text_node",
    ] {
        assert!(manifest
            .capabilities
            .iter()
            .any(|capability| capability.name == required));
    }
}

#[test]
fn capability_manifest_digest_is_order_independent_and_rejects_duplicates() {
    let manifest = agent_capability_manifest("0.3.0");
    let digest = capability_manifest_sha256(&manifest).expect("manifest digest");
    assert_eq!(
        digest,
        "81f148286b5e3773a7682c3ee5f3c132fa38b85cea9014db0be26bdcb54494a7"
    );

    let mut reordered = manifest.clone();
    reordered.capabilities.reverse();
    assert_eq!(
        capability_manifest_sha256(&reordered).expect("reordered digest"),
        digest
    );

    let mut changed = manifest.clone();
    changed.capabilities[0].version = "2.0.1".to_owned();
    assert_ne!(
        capability_manifest_sha256(&changed).expect("changed digest"),
        digest
    );

    let mut duplicate = manifest;
    duplicate
        .capabilities
        .push(duplicate.capabilities[0].clone());
    assert!(capability_manifest_sha256(&duplicate).is_err());
}

#[test]
fn optional_compare_bounds_are_omitted_in_canonical_wire_json() {
    let changed: AgentCompareAssertionRequest = serde_json::from_value(serde_json::json!({
        "type":"changed_ratio",
        "id":"changed",
        "severity":"required",
        "minimum":0.1
    }))
    .expect("minimum-only changed ratio");
    let changed_json = serde_json::to_value(changed).expect("changed ratio JSON");
    assert_eq!(changed_json["minimum"], 0.1);
    assert!(changed_json.get("maximum").is_none());

    let inside: AgentCompareAssertionRequest = serde_json::from_value(serde_json::json!({
        "type":"inside_mask_changed_ratio",
        "id":"inside",
        "severity":"required",
        "minimum":0.5,
        "mask":{
            "path":"/tmp/mask.png",
            "expectedSha256":"0".repeat(64),
            "mediaType":"image/png",
            "byteLength":1,
            "kind":"mask",
            "pixelSpec":{"format":"rgba8","colorSpace":"srgb","alphaMode":"straight"}
        }
    }))
    .expect("inside ratio without maximum");
    let inside_json = serde_json::to_value(inside).expect("inside ratio JSON");
    assert_eq!(inside_json["minimum"], 0.5);
    assert!(inside_json.get("maximum").is_none());
}

#[test]
fn dependency_closure_is_order_independent_and_deduplicated() {
    let a = AgentArtifactDependency {
        sha256: "a".repeat(64),
        media_type: "image/png".to_owned(),
        byte_length: 10,
    };
    let b = AgentArtifactDependency {
        sha256: "b".repeat(64),
        media_type: "image/png".to_owned(),
        byte_length: 20,
    };
    let left = dependency_closure_sha256(&[a.clone(), b.clone(), a.clone()]).unwrap();
    let right = dependency_closure_sha256(&[b, a]).unwrap();
    assert_eq!(left, right);
}

#[test]
fn pin_set_rejects_duplicates() {
    let set = AgentArtifactPinSet {
        schema: AGENT_PIN_SET_SCHEMA.to_owned(),
        pins: vec!["a".repeat(64), "a".repeat(64)],
    };
    assert!(set.validate().is_err());
}

#[test]
fn agent_schema_cli_is_machine_readable() {
    let output = Command::new(env!("CARGO_BIN_EXE_perfectpixel"))
        .arg("agent-schema")
        .output()
        .expect("run agent-schema");
    assert!(output.status.success());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).expect("json");
    assert_eq!(value["schema"], AGENT_PROTOCOL_SCHEMA);
    assert_eq!(value["protocolVersion"], AGENT_PROTOCOL_VERSION);
}

#[test]
fn agent_inspect_binds_request_source_and_dependency_receipt() {
    let root = temp_root("success");
    fs::create_dir_all(&root).unwrap();
    let input = root.join("input.png");
    let png = valid_png();
    fs::write(&input, &png).unwrap();
    let request = root.join("request.json");
    let digest = sha256_hex(&png);
    let body = serde_json::json!({
        "schema": "perfectpixel.agent-image/inspect/2",
        "requestId": "req-success",
        "source": {
            "path": input,
            "expectedSha256": digest,
            "mediaType": "image/png",
            "byteLength": png.len(),
            "kind": "working_raster",
            "pixelSpec": {"format":"rgba8","colorSpace":"srgb","alphaMode":"straight"}
        }
    });
    let request_bytes = serde_json::to_vec(&body).unwrap();
    fs::write(&request, &request_bytes).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_perfectpixel"))
        .args(["agent-inspect", "--request"])
        .arg(&request)
        .output()
        .expect("run agent-inspect");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let dependency = AgentArtifactDependency {
        sha256: digest.clone(),
        media_type: "image/png".to_owned(),
        byte_length: png.len() as u64,
    };
    assert_eq!(value["schema"], "perfectpixel.agent-image/inspect-result/2");
    assert_eq!(value["source"]["sha256"], digest);
    assert_eq!(
        value["receipt"]["requestSha256"],
        sha256_hex(&request_bytes)
    );
    assert_eq!(
        value["receipt"]["dependencyClosureSha256"],
        dependency_closure_sha256(&[dependency]).unwrap()
    );
    assert_eq!(value["receipt"]["determinism"], "bit_exact");
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn agent_inspect_rejects_content_address_mismatch() {
    let root = temp_root("mismatch");
    fs::create_dir_all(&root).unwrap();
    let input = root.join("input.png");
    let png = valid_png();
    fs::write(&input, &png).unwrap();
    let request = root.join("request.json");
    let body = serde_json::json!({
        "schema": "perfectpixel.agent-image/inspect/2",
        "requestId": "req-mismatch",
        "source": {
            "path": input,
            "expectedSha256": "0".repeat(64),
            "mediaType": "image/png",
            "byteLength": png.len(),
            "kind": "working_raster",
            "pixelSpec": {"format":"rgba8","colorSpace":"srgb","alphaMode":"straight"}
        }
    });
    fs::write(&request, serde_json::to_vec(&body).unwrap()).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_perfectpixel"))
        .args(["agent-inspect", "--request"])
        .arg(&request)
        .output()
        .expect("run agent-inspect");
    assert!(!output.status.success());
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn agent_inspect_rejects_missing_required_pixel_spec() {
    let root = temp_root("missing-pixel-spec");
    fs::create_dir_all(&root).unwrap();
    let input = root.join("input.png");
    let png = valid_png();
    fs::write(&input, &png).unwrap();
    let request = root.join("request.json");
    let body = serde_json::json!({
        "schema": "perfectpixel.agent-image/inspect/2",
        "requestId": "req-missing-pixel-spec",
        "source": {
            "path": input,
            "expectedSha256": sha256_hex(&png),
            "mediaType": "image/png",
            "byteLength": png.len(),
            "kind": "working_raster"
        }
    });
    fs::write(&request, serde_json::to_vec(&body).unwrap()).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_perfectpixel"))
        .args(["agent-inspect", "--request"])
        .arg(&request)
        .output()
        .expect("run agent-inspect");
    assert!(!output.status.success());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(value["message"].as_str().is_some_and(|message| {
        message.to_ascii_lowercase().contains("pixel")
            && message.to_ascii_lowercase().contains("missing")
    }));
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn agent_render_applies_canonical_operations_in_order() {
    let root = temp_root("render-operations");
    fs::create_dir_all(&root).unwrap();
    let input = root.join("input.png");
    let output_dir = root.join("rendered");
    let image = Raster::new(
        2,
        3,
        vec![
            1, 0, 0, 255, 2, 0, 0, 255, 3, 0, 0, 255, 4, 0, 0, 255, 5, 0, 0, 255, 6, 0, 0, 255,
        ],
    )
    .unwrap();
    let encoded = PngEncoder::encode_rgba(&image).unwrap();
    fs::write(&input, &encoded).unwrap();
    let operations = vec![
        RasterEdit::Crop {
            x: 0,
            y: 1,
            width: 2,
            height: 2,
        },
        RasterEdit::RotateQuarterTurns { quarter_turns: 1 },
        RasterEdit::FlipHorizontal,
        RasterEdit::Resize {
            width: 2,
            height: 2,
            filter: ResampleFilter::Nearest,
        },
    ];
    let expected = apply_raster_edits(&image, &operations).unwrap();
    let request = root.join("request.json");
    let body = serde_json::json!({
        "schema": "perfectpixel.agent-image/render/2",
        "requestId": "req-render-operations",
        "canvas": {"width": 2, "height": 2, "background": [0, 0, 0, 0]},
        "nodes": [{
            "id": "source",
            "z": 0,
            "source": {
                "path": input,
                "expectedSha256": sha256_hex(&encoded),
                "mediaType": "image/png",
                "byteLength": encoded.len(),
                "kind": "working_raster",
                "pixelSpec": {"format":"rgba8","colorSpace":"srgb","alphaMode":"straight"}
            },
            "transform": {"matrix": [1.0,0.0,0.0,0.0,1.0,0.0,0.0,0.0,1.0]},
            "opacity": 255,
            "filter": "nearest",
            "operations": operations
        }]
    });
    fs::write(&request, serde_json::to_vec(&body).unwrap()).unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_perfectpixel"))
        .args(["agent-render", "--request"])
        .arg(&request)
        .args(["--out-dir"])
        .arg(&output_dir)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stdout)
    );
    let evidence: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(evidence["status"], "committed");
    let rendered =
        ImageCodec::decode_rgba(output_dir.join("render.png"), Default::default()).unwrap();
    assert_eq!(rendered, expected);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn agent_render_text_node_is_cross_process_bit_exact_and_reports_layout_evidence() {
    let root = temp_root("render-text");
    fs::create_dir_all(&root).unwrap();
    let font = include_bytes!("fixtures/Tuffy.ttf");
    let font_path = root.join("Tuffy.ttf");
    fs::write(&font_path, font).unwrap();
    let font_sha256 = sha256_hex(font);
    let font_ref = serde_json::json!({
        "id": format!("art-{font_sha256}"),
        "sha256": font_sha256,
        "mediaType": "font/ttf",
        "byteLength": font.len(),
        "relativePath": format!("{}/{}.ttf", &sha256_hex(font)[..2], sha256_hex(font))
    });
    let request = root.join("text-render.json");
    let body = serde_json::json!({
        "schema": "perfectpixel.agent-image/render/2",
        "requestId": "req-render-text",
        "canvas": {"width": 192, "height": 48, "background": [0, 0, 0, 0]},
        "nodes": [{
            "id": "label",
            "z": 0,
            "kind": "text",
            "text": {
                "content": "Office",
                "direction": "ltr",
                "language": "en",
                "boxWidth": 192,
                "boxHeight": 48,
                "alignment": "start",
                "lineBreak": "no_wrap",
                "fontSize": 18.0,
                "color": [12, 34, 56, 255],
                "pixelSpec": {"format": "rgba8", "colorSpace": "srgb", "alphaMode": "straight"},
                "font": {"artifact": font_ref, "path": font_path}
            },
            "transform": {"matrix": [1.0,0.0,0.0,0.0,1.0,0.0,0.0,0.0,1.0]},
            "opacity": 255,
            "filter": "nearest",
            "operations": []
        }]
    });
    let request_bytes = serde_json::to_vec(&body).unwrap();
    fs::write(&request, &request_bytes).unwrap();
    let run = |suffix: &str| {
        let output_dir = root.join(suffix);
        let output = Command::new(env!("CARGO_BIN_EXE_perfectpixel"))
            .args(["agent-render", "--request"])
            .arg(&request)
            .args(["--out-dir"])
            .arg(&output_dir)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap()
    };
    let first = run("text-render-1");
    let second = run("text-render-2");
    assert_eq!(first["status"], "committed");
    assert_eq!(first["textNodes"][0]["nodeId"], "label");
    assert_eq!(first["textNodes"][0]["layout"]["fontSha256"], font_sha256);
    assert_eq!(first["textNodes"][0]["layout"]["glyphCount"], 6);
    assert_eq!(first["textNodes"][0]["layout"]["width"], 192);
    assert_eq!(first["textNodes"][0]["layout"]["height"], 48);
    assert_eq!(first["receipt"]["dependencies"][0]["mediaType"], "font/ttf");
    assert_eq!(first["receipt"]["dependencies"][0]["sha256"], font_sha256);
    assert_eq!(
        first["output"]["descriptor"]["sha256"],
        second["output"]["descriptor"]["sha256"]
    );
    assert_eq!(first["textNodes"], second["textNodes"]);
    assert_eq!(
        first["output"]["descriptor"]["sha256"],
        "8b9168d069edfa28ab8d3caface10ced2dda048784a726a645425edbed43746a"
    );
    let mut mismatched = body.clone();
    mismatched["nodes"][0]["text"]["font"]["artifact"]["sha256"] =
        serde_json::json!("0".repeat(64));
    let mismatch_request = root.join("text-render-mismatch.json");
    fs::write(&mismatch_request, serde_json::to_vec(&mismatched).unwrap()).unwrap();
    let mismatch = Command::new(env!("CARGO_BIN_EXE_perfectpixel"))
        .args(["agent-render", "--request"])
        .arg(&mismatch_request)
        .args(["--out-dir"])
        .arg(root.join("text-render-mismatch-out"))
        .output()
        .unwrap();
    assert!(!mismatch.status.success());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn canonical_render_operations_cover_the_retained_edit_parity_corpus() {
    // The standalone human `edit` command and model-facing `agent-render` must
    // share the same RasterEdit semantics.  Keep one small, exact corpus for
    // every retained legacy behavior that is now represented by operations.
    let root = temp_root("render-parity-corpus");
    fs::create_dir_all(&root).unwrap();
    let gradient = Raster::new(
        2,
        2,
        vec![0, 0, 0, 255, 255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255],
    )
    .unwrap();
    let mut checker_pixels = Vec::new();
    for y in 0..5 {
        for x in 0..5 {
            let rgb = if (1..=3).contains(&x) && (1..=3).contains(&y) {
                [255, 255, 255]
            } else if (x + y) % 2 == 0 {
                [220, 10, 20]
            } else {
                [10, 30, 220]
            };
            checker_pixels.extend_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
        }
    }
    let checker = Raster::new(5, 5, checker_pixels).unwrap();
    let cases = vec![
        (
            "lanczos3",
            gradient.clone(),
            vec![RasterEdit::Resize {
                width: 4,
                height: 3,
                filter: ResampleFilter::Lanczos3,
            }],
        ),
        (
            "remove-background",
            checker.clone(),
            vec![RasterEdit::RemoveBackground {
                keys: vec![[220, 10, 20], [10, 30, 220]],
                tolerance: 0,
                feather: 0,
            }],
        ),
        (
            "remove-background-auto",
            checker,
            vec![RasterEdit::RemoveBackgroundAuto {
                max_keys: 2,
                min_edge_coverage_basis_points: 10_000,
                tolerance: 0,
                feather: 0,
            }],
        ),
    ];

    for (index, (label, image, operations)) in cases.into_iter().enumerate() {
        let mut expected = apply_raster_edits(&image, &operations).unwrap();
        // Composition uses canonical source-over semantics: RGB under a fully
        // transparent pixel is normalized to zero on a transparent canvas.
        for pixel in expected.pixels_mut().chunks_exact_mut(4) {
            if pixel[3] == 0 {
                pixel[..3].copy_from_slice(&[0, 0, 0]);
            }
        }
        let input = root.join(format!("{label}-input.png"));
        let encoded = PngEncoder::encode_rgba(&image).unwrap();
        fs::write(&input, &encoded).unwrap();
        let request = root.join(format!("{label}-request.json"));
        let output_dir = root.join(format!("{label}-output"));
        let body = serde_json::json!({
            "schema": "perfectpixel.agent-image/render/2",
            "requestId": format!("req-render-parity-{index}"),
            "canvas": {"width": expected.width(), "height": expected.height(), "background": [0, 0, 0, 0]},
            "nodes": [{
                "id": "source",
                "z": 0,
                "source": {
                    "path": input,
                    "expectedSha256": sha256_hex(&encoded),
                    "mediaType": "image/png",
                    "byteLength": encoded.len(),
                    "kind": "working_raster",
                    "pixelSpec": {"format":"rgba8","colorSpace":"srgb","alphaMode":"straight"}
                },
                "transform": {"matrix": [1.0,0.0,0.0,0.0,1.0,0.0,0.0,0.0,1.0]},
                "opacity": 255,
                "filter": "nearest",
                "operations": operations
            }]
        });
        fs::write(&request, serde_json::to_vec(&body).unwrap()).unwrap();
        let result = Command::new(env!("CARGO_BIN_EXE_perfectpixel"))
            .args(["agent-render", "--request"])
            .arg(&request)
            .args(["--out-dir"])
            .arg(&output_dir)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{label}: {}",
            String::from_utf8_lossy(&result.stdout)
        );
        let rendered =
            ImageCodec::decode_rgba(output_dir.join("render.png"), Default::default()).unwrap();
        assert_eq!(rendered, expected, "{label} parity");
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn agent_compare_rejects_assertion_overflow_before_reading_artifacts() {
    let root = temp_root("compare-preflight");
    fs::create_dir_all(&root).unwrap();
    let request = root.join("compare.json");
    let output_dir = root.join("compare-out");
    let missing = root.join("missing.png");
    let artifact = serde_json::json!({
        "path": missing,
        "expectedSha256": "0".repeat(64),
        "mediaType": "image/png",
        "byteLength": 1,
        "kind": "working_raster",
        "pixelSpec": {"format":"rgba8","colorSpace":"srgb","alphaMode":"straight"}
    });
    let assertions = (0..=256)
        .map(|index| {
            serde_json::json!({
                "type": "exact_equal",
                "id": format!("assertion-{index}"),
                "severity": "required"
            })
        })
        .collect::<Vec<_>>();
    let body = serde_json::json!({
        "schema": "perfectpixel.agent-image/compare/2",
        "requestId": "compare-preflight",
        "before": artifact,
        "after": artifact,
        "assertions": assertions,
        "preview": {"difference": false, "maskOverlay": false, "maximumEdge": 256}
    });
    fs::write(&request, serde_json::to_vec(&body).unwrap()).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_perfectpixel"))
        .args(["agent-compare", "--request"])
        .arg(&request)
        .args(["--out-dir"])
        .arg(&output_dir)
        .output()
        .expect("run agent-compare");
    assert!(!output.status.success());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(value["message"]
        .as_str()
        .is_some_and(|message| message.contains("1..=256")));
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn agent_compare_rejects_advisory_only_spec_before_reading_artifacts() {
    let root = temp_root("compare-advisory-only");
    fs::create_dir_all(&root).unwrap();
    let request = root.join("compare.json");
    let output_dir = root.join("compare-out");
    let missing = root.join("missing.png");
    let artifact = serde_json::json!({
        "path": missing,
        "expectedSha256": "0".repeat(64),
        "mediaType": "image/png",
        "byteLength": 1,
        "kind": "working_raster",
        "pixelSpec": {"format":"rgba8","colorSpace":"srgb","alphaMode":"straight"}
    });
    let body = serde_json::json!({
        "schema": "perfectpixel.agent-image/compare/2",
        "requestId": "compare-advisory-only",
        "before": artifact,
        "after": artifact,
        "assertions": [{
            "type": "exact_equal",
            "id": "diagnostic",
            "severity": "advisory"
        }],
        "preview": {"difference": false, "maskOverlay": false, "maximumEdge": 256}
    });
    fs::write(&request, serde_json::to_vec(&body).unwrap()).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_perfectpixel"))
        .args(["agent-compare", "--request"])
        .arg(&request)
        .args(["--out-dir"])
        .arg(&output_dir)
        .output()
        .expect("run agent-compare");
    assert!(!output.status.success());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(value["message"]
        .as_str()
        .is_some_and(|message| message.contains("required assertion")));
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn agent_compare_returns_rejected_diagnostic_result_without_process_failure() {
    let root = temp_root("compare-rejected");
    fs::create_dir_all(&root).unwrap();
    let before = root.join("before.png");
    let after = root.join("after.png");
    let png = valid_png();
    fs::write(&before, &png).unwrap();
    fs::write(&after, &png).unwrap();
    let digest = sha256_hex(&png);
    let request = root.join("compare.json");
    let output_dir = root.join("compare-out");
    let artifact = |path: &std::path::Path| {
        serde_json::json!({
            "path": path,
            "expectedSha256": digest,
            "mediaType": "image/png",
            "byteLength": png.len(),
            "kind": "working_raster",
            "pixelSpec": {"format":"rgba8","colorSpace":"srgb","alphaMode":"straight"}
        })
    };
    let body = serde_json::json!({
        "schema": "perfectpixel.agent-image/compare/2",
        "requestId": "compare-rejected",
        "before": artifact(&before),
        "after": artifact(&after),
        "assertions": [{
            "type": "changed_ratio",
            "id": "must-change",
            "severity": "required",
            "minimum": 1.0
        }],
        "preview": {"difference": false, "maskOverlay": false, "maximumEdge": 256}
    });
    fs::write(&request, serde_json::to_vec(&body).unwrap()).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_perfectpixel"))
        .args(["agent-compare", "--request"])
        .arg(&request)
        .args(["--out-dir"])
        .arg(&output_dir)
        .output()
        .expect("run agent-compare");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result: AgentCompareResult = serde_json::from_slice(&output.stdout).unwrap();
    let value = serde_json::to_value(result).unwrap();
    assert_eq!(value["schema"], "perfectpixel.agent-image/compare-result/2");
    assert_eq!(value["status"], "rejected");
    assert_eq!(value["allRequiredPassed"], false);
    assert_eq!(value["assertions"][0]["id"], "must-change");
    assert_eq!(value["assertions"][0]["passed"], false);
    assert_eq!(value["receipt"]["status"], "rejected");
    let _ = fs::remove_dir_all(&root);
}
