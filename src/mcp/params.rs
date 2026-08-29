use rmcp::schemars::JsonSchema;
use serde::Deserialize;

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum AssetFilter {
    Nearest,
    Lanczos3,
}

impl AssetFilter {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Nearest => "nearest",
            Self::Lanczos3 => "lanczos3",
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct SchemaParams {}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct InspectParams {
    /// Root-relative UTF-8 slash path to an existing regular PNG, JPEG, or WebP file.
    pub(crate) input_path: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ConvertParams {
    /// Root-relative UTF-8 slash path to an existing regular PNG, JPEG, or WebP file.
    pub(crate) input_path: String,
    /// Root-relative UTF-8 slash path to the output image file.
    pub(crate) output_path: String,
    /// Optional positive output width. If absent, the source width is retained.
    #[schemars(range(min = 1))]
    pub(crate) width: Option<u32>,
    /// Optional positive output height. If absent, the source height is retained.
    #[schemars(range(min = 1))]
    pub(crate) height: Option<u32>,
    /// Resampling filter: nearest or lanczos3.
    pub(crate) filter: Option<AssetFilter>,
    /// JPEG quality in the inclusive range 1..=100.
    #[schemars(range(min = 1, max = 100))]
    pub(crate) jpeg_quality: Option<u8>,
    /// JPEG matte as #RRGGBB; only valid for JPEG output.
    pub(crate) background: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct UpscaleParams {
    /// Root-relative UTF-8 slash path to an existing regular PNG, JPEG, or WebP file.
    pub(crate) input_path: String,
    /// Root-relative UTF-8 slash path to the output image file.
    pub(crate) output_path: String,
    /// Integer scale in the inclusive range 2..=u32::MAX.
    #[schemars(range(min = 2))]
    pub(crate) scale: u32,
    /// Resampling filter: nearest or lanczos3.
    pub(crate) filter: Option<AssetFilter>,
    /// JPEG quality in the inclusive range 1..=100.
    #[schemars(range(min = 1, max = 100))]
    pub(crate) jpeg_quality: Option<u8>,
    /// JPEG matte as #RRGGBB; only valid for JPEG output.
    pub(crate) background: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RequestDirectoryParams {
    /// Root-relative UTF-8 slash path to an existing JSON request file.
    pub(crate) request_path: String,
    /// Root-relative UTF-8 slash path to a strict child output directory.
    pub(crate) output_dir: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum VectorPreset {
    Auto,
    PixelArt,
    LegacyLossless,
    FlatIcon,
    LineArt,
    BoundedIllustration,
}

impl VectorPreset {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::PixelArt => "pixel-art",
            Self::LegacyLossless => "legacy-lossless",
            Self::FlatIcon => "flat-icon",
            Self::LineArt => "line-art",
            Self::BoundedIllustration => "bounded-illustration",
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum VectorProfile {
    Compact,
    MotionStructureReady,
}

impl VectorProfile {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::Compact => "compact",
            Self::MotionStructureReady => "motion-structure-ready",
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) enum VectorDetail {
    #[serde(rename = "auto")]
    #[schemars(rename = "auto")]
    Auto,
    #[serde(rename = "1")]
    #[schemars(rename = "1")]
    One,
    #[serde(rename = "2")]
    #[schemars(rename = "2")]
    Two,
    #[serde(rename = "3")]
    #[schemars(rename = "3")]
    Three,
    #[serde(rename = "4")]
    #[schemars(rename = "4")]
    Four,
    #[serde(rename = "5")]
    #[schemars(rename = "5")]
    Five,
}

impl VectorDetail {
    pub(crate) fn as_cli_value(&self) -> Option<String> {
        match self {
            Self::Auto => None,
            Self::One => Some("1".to_string()),
            Self::Two => Some("2".to_string()),
            Self::Three => Some("3".to_string()),
            Self::Four => Some("4".to_string()),
            Self::Five => Some("5".to_string()),
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct VectorParams {
    /// Root-relative UTF-8 slash path to an existing regular PNG, JPEG, or WebP file.
    pub(crate) input_path: String,
    /// Root-relative UTF-8 slash path to the final SVG file.
    pub(crate) output_path: String,
    /// Vector preset; absent means auto.
    pub(crate) preset: Option<VectorPreset>,
    /// SVG output profile; absent means compact.
    pub(crate) profile: Option<VectorProfile>,
    /// Candidate detail auto or an integer in 1..=5.
    pub(crate) detail: Option<VectorDetail>,
    /// Minimum quality in the inclusive range 0..=1.
    #[schemars(range(min = 0, max = 1))]
    pub(crate) min_quality: Option<f64>,
    /// Maximum quality loss in the inclusive range 0..=1.
    #[schemars(range(min = 0, max = 1))]
    pub(crate) max_quality_loss: Option<f64>,
    /// Positive maximum path count.
    #[schemars(range(min = 1))]
    pub(crate) max_paths: Option<usize>,
    /// Optional root-relative UTF-8 slash path to a policy JSON file.
    pub(crate) policy_path: Option<String>,
    /// Optional root-relative UTF-8 slash path to an evaluation JSON file.
    pub(crate) report_path: Option<String>,
    /// Optional root-relative UTF-8 slash path to a diagnostics directory.
    pub(crate) diagnostics_dir: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct VectorAnalyzeParams {
    /// Root-relative UTF-8 slash path to an existing regular PNG, JPEG, or WebP file.
    pub(crate) input_path: String,
    /// Vector preset; absent means auto.
    pub(crate) preset: Option<VectorPreset>,
    /// SVG analysis profile; absent means compact.
    pub(crate) profile: Option<VectorProfile>,
    /// Optional root-relative UTF-8 slash path to a policy JSON file.
    pub(crate) policy_path: Option<String>,
    /// Optional root-relative UTF-8 slash path to an analysis JSON file. Supplying it writes.
    pub(crate) report_path: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct MotionScaffoldParams {
    /// Root-relative UTF-8 slash path to an existing regular raster-free SVG file.
    pub(crate) input_path: String,
    /// Root-relative UTF-8 slash path to a strict child output directory.
    pub(crate) output_dir: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct MotionBuildParams {
    /// Root-relative UTF-8 slash path to an existing motion JSON request file.
    pub(crate) request_path: String,
    /// Root-relative UTF-8 slash path to a strict child output directory.
    pub(crate) output_dir: String,
}
