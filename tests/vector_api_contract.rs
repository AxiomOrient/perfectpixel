use perfectpixel::{
    DiagnosticsIntent, PpError, Raster, RequestValidationError, SvgProfile, UnitScore,
    VectorAnalysisRequest, VectorDetail, VectorPolicy, VectorPresetSelection, VectorRequest,
    Vectorizer,
};
use std::num::NonZeroUsize;

#[test]
fn request_values_are_validated_and_serde_is_closed() {
    assert_eq!(VectorDetail::new(1).unwrap().get(), 1);
    assert_eq!(VectorDetail::new(5).unwrap().get(), 5);
    assert_eq!(
        VectorDetail::new(0),
        Err(RequestValidationError::DetailOutOfRange(0))
    );
    assert_eq!(
        VectorDetail::new(6),
        Err(RequestValidationError::DetailOutOfRange(6))
    );
    assert_eq!(UnitScore::new(0.0).unwrap().get(), 0.0);
    assert_eq!(UnitScore::new(1.0).unwrap().get(), 1.0);
    assert!(matches!(
        UnitScore::new(f64::NAN),
        Err(RequestValidationError::ScoreNotFinite)
    ));
    assert!(matches!(
        UnitScore::new(-0.01),
        Err(RequestValidationError::ScoreOutOfRange(_))
    ));
    assert!(matches!(
        UnitScore::new(1.01),
        Err(RequestValidationError::ScoreOutOfRange(_))
    ));

    let policy: VectorPolicy = serde_json::from_str(r##"{"schema":"perfectpixel.vector-policy/1","version":"consumer/1","allowedPalette":["#112233"],"requiredPalette":["#112233"],"rejectUnmapped":true,"allowDropNoise":false}"##).unwrap();
    let request = VectorRequest::new(
        VectorPresetSelection::LegacyLossless,
        SvgProfile::Compact,
        Some(VectorDetail::new(3).unwrap()),
        Some(UnitScore::new(0.99).unwrap()),
        Some(UnitScore::new(0.01).unwrap()),
        NonZeroUsize::new(32),
        policy.clone(),
        DiagnosticsIntent::none(),
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(&request).unwrap()["preset"],
        "legacy-lossless"
    );
    assert_eq!(
        serde_json::to_value(&request).unwrap()["profile"],
        "compact"
    );
    assert_eq!(request.preset, VectorPresetSelection::LegacyLossless);
    assert_eq!(request.profile, SvgProfile::Compact);
    assert_eq!(request.detail.unwrap().get(), 3);
    assert_eq!(request.minimum_quality.unwrap().get(), 0.99);
    assert_eq!(request.maximum_quality_loss.unwrap().get(), 0.01);
    assert_eq!(request.maximum_paths.unwrap().get(), 32);
    assert_eq!(request.policy, policy);
    assert_eq!(request.diagnostics, DiagnosticsIntent::none());
    assert!(serde_json::from_str::<VectorRequest>(r#"{"preset":"legacy-lossless","profile":"compact","detail":3,"minimumQuality":0.9,"maximumQualityLoss":0.1,"maximumPaths":32,"policy":{"schema":"perfectpixel.vector-policy/1","version":"v","allowedPalette":[],"requiredPalette":[],"rejectUnmapped":true,"allowDropNoise":false},"diagnostics":{"requested":false,"artifactKinds":[]},"authority":"caller-controlled"}"#).is_err());
    assert!(serde_json::from_str::<VectorPolicy>(r##"{"schema":"wrong","version":"v","allowedPalette":[],"requiredPalette":[],"rejectUnmapped":false,"allowDropNoise":false}"##).is_err());
    assert!(serde_json::from_str::<VectorPolicy>(r##"{"schema":"perfectpixel.vector-policy/1","version":"v","allowedPalette":["#fff"],"requiredPalette":[],"rejectUnmapped":false,"allowDropNoise":false}"##).is_err());
    assert!(matches!(
        VectorPolicy::new("", vec![], vec![], true, false),
        Err(RequestValidationError::EmptyPolicyVersion)
    ));
    assert!(matches!(
        VectorPolicy::new(
            "v",
            vec!["#FFFFFF".into(), "#ffffff".into()],
            vec![],
            true,
            false
        ),
        Err(RequestValidationError::DuplicatePaletteColor(_))
    ));
    assert!(matches!(
        VectorPolicy::new(
            "v",
            vec!["#FFFFFF".into()],
            vec!["#000000".into()],
            true,
            false
        ),
        Err(RequestValidationError::RequiredPaletteNotAllowed(_))
    ));

    let analysis = VectorAnalysisRequest::new(
        VectorPresetSelection::Auto,
        SvgProfile::MotionStructureReady,
        policy,
    )
    .unwrap();
    assert_eq!(serde_json::to_value(&analysis).unwrap()["preset"], "auto");
    for preset in [
        VectorPresetSelection::Auto,
        VectorPresetSelection::PixelArt,
        VectorPresetSelection::LegacyLossless,
        VectorPresetSelection::FlatIcon,
        VectorPresetSelection::LineArt,
        VectorPresetSelection::BoundedIllustration,
    ] {
        assert_eq!(
            serde_json::from_value::<VectorPresetSelection>(serde_json::to_value(preset).unwrap())
                .unwrap(),
            preset
        );
    }
    for profile in [SvgProfile::Compact, SvgProfile::MotionStructureReady] {
        assert_eq!(
            serde_json::from_value::<SvgProfile>(serde_json::to_value(profile).unwrap()).unwrap(),
            profile
        );
    }
}

#[test]
fn only_validated_requests_reach_the_public_vectorizer_boundary() {
    let invalid_raster = Raster::new(2, 2, vec![0; 3]);
    assert!(
        matches!(invalid_raster, Err(PpError::InvalidRequest(_))),
        "infrastructure/input failures remain PpError"
    );
    let raster = Raster::new(1, 1, vec![0, 0, 0, 255]).unwrap();
    let request = VectorAnalysisRequest::new(
        VectorPresetSelection::Auto,
        SvgProfile::Compact,
        VectorPolicy::default(),
    )
    .unwrap();
    let analysis = Vectorizer::new()
        .unwrap()
        .analyze(&raster, &request)
        .unwrap();
    assert_eq!(analysis.schema(), "perfectpixel.vector-analysis/1");
}

#[test]
fn documented_json_examples_match_the_public_contract() {
    let policy: VectorPolicy =
        serde_json::from_str(include_str!("../docs/vectorize/examples/policy.json")).unwrap();
    assert_eq!(policy.schema(), VectorPolicy::SCHEMA);

    let request: VectorRequest = serde_json::from_str(include_str!(
        "../docs/vectorize/examples/generation-request.json"
    ))
    .unwrap();
    assert_eq!(request.preset(), VectorPresetSelection::PixelArt);
    assert_eq!(request.profile(), SvgProfile::Compact);

    let analysis_request: VectorAnalysisRequest = serde_json::from_str(include_str!(
        "../docs/vectorize/examples/analysis-request.json"
    ))
    .unwrap();
    assert_eq!(analysis_request.preset(), VectorPresetSelection::Auto);
    assert_eq!(analysis_request.profile(), SvgProfile::MotionStructureReady);

    let analysis: serde_json::Value =
        serde_json::from_str(include_str!("../docs/vectorize/examples/analysis.json")).unwrap();
    assert_eq!(analysis["schema"], "perfectpixel.vector-analysis/1");
    assert_eq!(
        analysis["digests"]["candidateBytes"],
        serde_json::Value::Null
    );
    for value in analysis["environmentMeasurements"]
        .as_object()
        .unwrap()
        .values()
    {
        assert!(value.is_null());
    }

    let evaluation: serde_json::Value = serde_json::from_str(include_str!(
        "../docs/vectorize/examples/evaluation-approved.json"
    ))
    .unwrap();
    assert_eq!(evaluation["schema"], perfectpixel::VECTOR_EVALUATION_SCHEMA);
    assert_eq!(evaluation["actualDecision"], "approved");
    assert_eq!(
        evaluation["candidateFacts"]["candidateSha256"],
        evaluation["digests"]["candidateBytes"]
    );
    assert_eq!(
        evaluation["profilePredicates"]["motionStructure"],
        "notEvaluated"
    );

    let rejection: serde_json::Value = serde_json::from_str(include_str!(
        "../docs/vectorize/examples/evaluation-preset-required.json"
    ))
    .unwrap();
    assert_eq!(rejection["actualDecision"], "notApplicable");
    assert_eq!(rejection["rejectionCodes"][0], "PRESET_REQUIRED");
    assert!(rejection["candidateFacts"].is_null());
    assert!(rejection["digests"]["candidateBytes"].is_null());
}
#[test]
fn versioned_documentation_matrix_covers_required_vector_behaviors() {
    let readme = include_str!("../docs/vectorize/README.md");
    let matrix_start = readme
        .find("## Versioned behavior matrix")
        .expect("versioned behavior matrix must exist");
    let matrix = &readme[matrix_start..];
    let matrix = &matrix[..matrix.find("\n## ").unwrap_or(matrix.len())];

    assert!(matrix.contains("Matrix version: `perfectpixel.vector-surface/1`"));

    for (case, evidence) in [
        (
            "Approved generation",
            "[Approved evaluation evidence](examples/evaluation-approved.json)",
        ),
        (
            "Rejected generation",
            "[Preset-required rejection evidence](examples/evaluation-preset-required.json)",
        ),
        (
            "Preset-required analysis",
            "Non-JSON behavior: [PROFILE_AND_SCOPE.md]",
        ),
        (
            "Unsupported route",
            "Non-JSON behavior: [PROFILE_AND_SCOPE.md]",
        ),
        (
            "Policy tightening/non-weakening",
            "[Policy authority boundary](POLICY_CONTRACT.md)",
        ),
        (
            "Outcome ownership/non-publication",
            "Non-JSON behavior: [Report ownership]",
        ),
        (
            "Artifact intent/diagnostics",
            "[Evaluation report contract](REPORT_SCHEMA.md",
        ),
        (
            "Motion-structure limitations",
            "Non-JSON behavior: [Output profiles]",
        ),
    ] {
        let row = matrix
            .lines()
            .find(|row| row.starts_with(&format!("| {case} |")))
            .unwrap_or_else(|| panic!("required matrix row is missing: {case}"));

        assert!(
            row.contains(evidence),
            "matrix row must retain its authoritative reference: {case}"
        );
        assert!(
            row.contains("](") || row.contains("Non-JSON behavior:"),
            "matrix row must link authority or explicitly document non-JSON behavior: {case}"
        );
    }

    let profile_scope = include_str!("../docs/vectorize/PROFILE_AND_SCOPE.md");
    assert!(profile_scope.contains("## Matrix boundary cases"));
    assert!(profile_scope.contains("Analysis reports abstention and route reasons"));
    assert!(profile_scope.contains("profile remains non-publishing for every family"));
}
