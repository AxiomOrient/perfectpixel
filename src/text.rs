//! Deterministic, font-byte-bound text rasterization.
//!
//! Text is deliberately a separate node contract from the RGBA8 composition
//! engine. The caller supplies the font bytes and their digest, so no
//! installed-font or locale lookup can change the result. Rustybuzz owns
//! shaping and the resulting glyph positions are used directly to rasterize
//! the supplied glyph outlines; no SVG text engine or installed-font fallback
//! participates in the result.

use resvg::tiny_skia::{self, FillRule, Paint, PathBuilder};
use rustybuzz::{Direction, Face as ShapingFace, Language, UnicodeBuffer};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use ttf_parser::{Face, GlyphId, OutlineBuilder};
use unicode_script::UnicodeScript;

use crate::agent::{AlphaMode, ColorSpace, PixelFormat, PixelSpec};
use crate::{PpError, PpResult, Raster};

pub const TEXT_NODE_SCHEMA: &str = "perfectpixel.text-node/1";
pub const MAX_TEXT_BYTES: usize = 64 * 1024;
pub const MAX_FONT_BYTES: usize = 32 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextDirection {
    Ltr,
    Rtl,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextAlignment {
    Start,
    Center,
    End,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LineBreakMode {
    NoWrap,
    WordWrap,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TextNode {
    pub content: String,
    pub direction: TextDirection,
    pub language: String,
    pub box_width: u32,
    pub box_height: u32,
    pub alignment: TextAlignment,
    pub line_break: LineBreakMode,
    pub font_size: f32,
    pub color: [u8; 4],
    pub pixel_spec: PixelSpec,
    pub font_bytes: Vec<u8>,
    pub font_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TextGlyphSnapshot {
    pub glyph_id: u32,
    pub cluster: u32,
    pub x_advance: i32,
    pub y_advance: i32,
    pub x_offset: i32,
    pub y_offset: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TextLineSnapshot {
    pub text: String,
    pub glyph_start: u32,
    pub glyph_count: u32,
    pub advance_units: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TextLayoutSnapshot {
    pub schema: String,
    pub font_sha256: String,
    pub lines: Vec<String>,
    pub line_snapshots: Vec<TextLineSnapshot>,
    pub glyphs: Vec<TextGlyphSnapshot>,
    pub glyph_count: u32,
    pub width: u32,
    pub height: u32,
    pub raster_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextRenderOutput {
    pub raster: Raster,
    pub layout: TextLayoutSnapshot,
}

impl TextNode {
    pub fn validate(&self) -> PpResult<()> {
        if self.content.is_empty() || self.content.len() > MAX_TEXT_BYTES {
            return Err(PpError::InvalidRequest(
                "text content must contain 1..=65536 UTF-8 bytes".to_owned(),
            ));
        }
        if self.box_width == 0
            || self.box_height == 0
            || self.box_width > 8192
            || self.box_height > 8192
        {
            return Err(PpError::InvalidRequest(
                "text box dimensions must be within 1..=8192".to_owned(),
            ));
        }
        if !self.font_size.is_finite() || !(0.1..=1024.0).contains(&self.font_size) {
            return Err(PpError::InvalidRequest(
                "text fontSize must be finite and within 0.1..=1024".to_owned(),
            ));
        }
        if self.language.is_empty()
            || self.language.len() > 35
            || !self
                .language
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-'))
        {
            return Err(PpError::InvalidRequest(
                "text language must be a bounded BCP-47 ASCII tag".to_owned(),
            ));
        }
        if self.pixel_spec
            != (PixelSpec {
                format: PixelFormat::Rgba8,
                color_space: ColorSpace::Srgb,
                alpha_mode: AlphaMode::Straight,
            })
        {
            return Err(PpError::InvalidRequest(
                "text rasterization currently produces rgba8/srgb/straight".to_owned(),
            ));
        }
        if self.font_bytes.is_empty() || self.font_bytes.len() > MAX_FONT_BYTES {
            return Err(PpError::InvalidRequest(
                "font bytes must contain 1..=33554432 bytes".to_owned(),
            ));
        }
        if self.font_sha256.len() != 64
            || !self
                .font_sha256
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
            || !self
                .font_sha256
                .eq_ignore_ascii_case(&format!("{:x}", Sha256::digest(&self.font_bytes)))
        {
            return Err(PpError::InvalidRequest(
                "fontSha256 does not match the supplied font bytes".to_owned(),
            ));
        }
        let face = Face::parse(&self.font_bytes, 0).map_err(|error| {
            PpError::InvalidRequest(format!(
                "font bytes are not a readable TTF/OTF face: {error:?}"
            ))
        })?;
        for character in self
            .content
            .chars()
            .filter(|character| !character.is_control())
        {
            if character.script() == unicode_script::Script::Unknown {
                return Err(PpError::InvalidRequest(format!(
                    "U+{:04X} belongs to an unsupported Unicode script",
                    u32::from(character)
                )));
            }
            if face.glyph_index(character).is_none() {
                return Err(PpError::InvalidRequest(format!(
                    "font has no glyph for U+{:04X}; fallback is disabled",
                    u32::from(character)
                )));
            }
        }
        Ok(())
    }

    pub fn render(&self) -> PpResult<TextRenderOutput> {
        self.validate()?;
        let lines = layout_lines(self)?;
        let shaped_lines = lines
            .iter()
            .map(|line| shape_line(self, line))
            .collect::<PpResult<Vec<_>>>()?;
        let line_height = self.font_size * 1.2;
        let mut pixmap =
            tiny_skia::Pixmap::new(self.box_width, self.box_height).ok_or_else(|| {
                PpError::InvalidRequest("text raster target could not be allocated".to_owned())
            })?;
        let face = Face::parse(&self.font_bytes, 0).map_err(|error| {
            PpError::InvalidRequest(format!(
                "font bytes are not a readable TTF/OTF face: {error:?}"
            ))
        })?;
        let scale = self.font_size / f32::from(face.units_per_em());
        let mut paint = Paint::default();
        paint.set_color_rgba8(self.color[0], self.color[1], self.color[2], self.color[3]);
        // The high-quality pipeline is explicit so the same tiny-skia raster
        // path is used by every process and every supported platform.
        paint.force_hq_pipeline = true;
        for (index, shaped) in shaped_lines.iter().enumerate() {
            rasterize_line(
                &face,
                shaped,
                self.alignment,
                self.box_width,
                scale,
                self.font_size * 0.9 + index as f32 * line_height,
                &paint,
                &mut pixmap,
            )?;
        }
        let raster = Raster::new(self.box_width, self.box_height, pixmap.take_demultiplied())?;
        let mut glyphs = Vec::new();
        let mut line_snapshots = Vec::with_capacity(lines.len());
        for (line, shaped) in lines.iter().zip(shaped_lines.iter()) {
            let glyph_start = u32::try_from(glyphs.len())
                .map_err(|_| PpError::InvalidRequest("text glyph count overflowed".to_owned()))?;
            let glyph_count = u32::try_from(shaped.glyphs.len())
                .map_err(|_| PpError::InvalidRequest("text glyph count overflowed".to_owned()))?;
            glyphs.extend_from_slice(&shaped.glyphs);
            line_snapshots.push(TextLineSnapshot {
                text: line.clone(),
                glyph_start,
                glyph_count,
                advance_units: shaped.advance_units,
            });
        }
        let glyph_count = u32::try_from(glyphs.len())
            .map_err(|_| PpError::InvalidRequest("text glyph count overflowed".to_owned()))?;
        let raster_sha256 = format!("{:x}", Sha256::digest(raster.pixels()));
        Ok(TextRenderOutput {
            raster,
            layout: TextLayoutSnapshot {
                schema: TEXT_NODE_SCHEMA.to_owned(),
                font_sha256: self.font_sha256.to_ascii_lowercase(),
                lines,
                line_snapshots,
                glyphs,
                glyph_count,
                width: self.box_width,
                height: self.box_height,
                raster_sha256,
            },
        })
    }
}

#[allow(clippy::too_many_arguments)]
fn rasterize_line(
    face: &Face<'_>,
    line: &ShapedLine,
    alignment: TextAlignment,
    box_width: u32,
    scale: f32,
    baseline: f32,
    paint: &Paint<'_>,
    pixmap: &mut tiny_skia::Pixmap,
) -> PpResult<()> {
    if !scale.is_finite() || scale <= 0.0 {
        return Err(PpError::InvalidRequest(
            "font scale is not finite and positive".to_owned(),
        ));
    }
    let width_units = line.advance_units.unsigned_abs();
    let available_units = f64::from(box_width) / f64::from(scale);
    let alignment_units = match alignment {
        TextAlignment::Start => 0.0,
        TextAlignment::Center => (available_units - width_units as f64) / 2.0,
        TextAlignment::End => available_units - width_units as f64,
    }
    .max(0.0);
    let direction_shift = if line.advance_units < 0 {
        width_units as f64
    } else {
        0.0
    };
    let mut x_cursor = 0_i64;
    let mut y_cursor = 0_i64;
    for glyph in &line.glyphs {
        let glyph_id = u16::try_from(glyph.glyph_id).map_err(|_| {
            PpError::InvalidRequest("shaped glyph identifier exceeds the font range".to_owned())
        })?;
        let x_units = x_cursor
            .checked_add(i64::from(glyph.x_offset))
            .ok_or_else(|| {
                PpError::InvalidRequest("shaped glyph x position overflowed".to_owned())
            })?;
        let y_units = y_cursor
            .checked_add(i64::from(glyph.y_offset))
            .ok_or_else(|| {
                PpError::InvalidRequest("shaped glyph y position overflowed".to_owned())
            })?;
        let origin_x =
            ((direction_shift + alignment_units + x_units as f64) * f64::from(scale)) as f32;
        let origin_y = baseline - y_units as f32 * scale;
        let id = GlyphId(glyph_id);
        if face.glyph_bounding_box(id).is_some() {
            let mut builder = GlyphPathBuilder {
                path: PathBuilder::new(),
                origin_x,
                origin_y,
                scale,
            };
            if face.outline_glyph(id, &mut builder).is_none() {
                return Err(PpError::InvalidRequest(
                    "font glyph outline could not be read".to_owned(),
                ));
            }
            let path = builder.path.finish().ok_or_else(|| {
                PpError::InvalidRequest("font glyph outline produced an empty path".to_owned())
            })?;
            pixmap.fill_path(
                &path,
                paint,
                FillRule::Winding,
                tiny_skia::Transform::identity(),
                None,
            );
        }
        x_cursor = x_cursor
            .checked_add(i64::from(glyph.x_advance))
            .ok_or_else(|| {
                PpError::InvalidRequest("shaped glyph x advance overflowed".to_owned())
            })?;
        y_cursor = y_cursor
            .checked_add(i64::from(glyph.y_advance))
            .ok_or_else(|| {
                PpError::InvalidRequest("shaped glyph y advance overflowed".to_owned())
            })?;
    }
    Ok(())
}

struct GlyphPathBuilder {
    path: PathBuilder,
    origin_x: f32,
    origin_y: f32,
    scale: f32,
}

impl GlyphPathBuilder {
    fn point(&self, x: f32, y: f32) -> (f32, f32) {
        (
            self.origin_x + x * self.scale,
            self.origin_y - y * self.scale,
        )
    }
}

impl OutlineBuilder for GlyphPathBuilder {
    fn move_to(&mut self, x: f32, y: f32) {
        let (x, y) = self.point(x, y);
        self.path.move_to(x, y);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        let (x, y) = self.point(x, y);
        self.path.line_to(x, y);
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let (x1, y1) = self.point(x1, y1);
        let (x, y) = self.point(x, y);
        self.path.quad_to(x1, y1, x, y);
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let (x1, y1) = self.point(x1, y1);
        let (x2, y2) = self.point(x2, y2);
        let (x, y) = self.point(x, y);
        self.path.cubic_to(x1, y1, x2, y2, x, y);
    }

    fn close(&mut self) {
        self.path.close();
    }
}

fn layout_lines(node: &TextNode) -> PpResult<Vec<String>> {
    let mut lines = Vec::new();
    for source_line in node.content.split('\n') {
        if node.line_break == LineBreakMode::NoWrap {
            if measured_line_width(node, source_line)? > node.box_width as f32 {
                return Err(PpError::InvalidRequest(
                    "text line exceeds the requested box width".to_owned(),
                ));
            }
            lines.push(source_line.to_owned());
            continue;
        }
        let mut current = String::new();
        for word in source_line.split_inclusive(char::is_whitespace) {
            let candidate = format!("{current}{word}");
            if !current.is_empty() && measured_line_width(node, &candidate)? > node.box_width as f32
            {
                lines.push(current.trim_end().to_owned());
                current.clear();
            }
            if measured_line_width(node, word)? > node.box_width as f32 {
                for character in word.chars() {
                    let candidate = format!("{current}{character}");
                    if !current.is_empty()
                        && measured_line_width(node, &candidate)? > node.box_width as f32
                    {
                        lines.push(std::mem::take(&mut current));
                    }
                    current.push(character);
                }
            } else {
                current.push_str(word);
            }
        }
        lines.push(current.trim_end().to_owned());
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    let max_lines = (node.box_height as f32 / (node.font_size * 1.2)).floor() as usize;
    if lines.len() > max_lines.max(1) {
        return Err(PpError::InvalidRequest(
            "text content exceeds the requested box height".to_owned(),
        ));
    }
    Ok(lines)
}

fn measured_line_width(node: &TextNode, line: &str) -> PpResult<f32> {
    shaped_line_metrics(node, line).map(|(width, _)| width)
}

fn shaped_line_metrics(node: &TextNode, line: &str) -> PpResult<(f32, usize)> {
    let shaped = shape_line(node, line)?;
    Ok((shaped.width, shaped.glyphs.len()))
}

struct ShapedLine {
    width: f32,
    advance_units: i64,
    glyphs: Vec<TextGlyphSnapshot>,
}

fn shape_line(node: &TextNode, line: &str) -> PpResult<ShapedLine> {
    let face = ShapingFace::from_slice(&node.font_bytes, 0).ok_or_else(|| {
        PpError::InvalidRequest("font bytes are not a readable shaping face".to_owned())
    })?;
    let mut buffer = UnicodeBuffer::new();
    buffer.push_str(line);
    buffer.set_direction(match node.direction {
        TextDirection::Ltr => Direction::LeftToRight,
        TextDirection::Rtl => Direction::RightToLeft,
    });
    let language = node
        .language
        .parse::<Language>()
        .map_err(|error| PpError::InvalidRequest(format!("text language is invalid: {error}")))?;
    buffer.set_language(language);
    let shaped = rustybuzz::shape(&face, &[], buffer);
    let units = face.units_per_em() as f32;
    if units <= 0.0 {
        return Err(PpError::InvalidRequest(
            "font units-per-em must be positive".to_owned(),
        ));
    }
    let advance_units = shaped
        .glyph_positions()
        .iter()
        .try_fold(0_i64, |total, position| {
            total.checked_add(i64::from(position.x_advance))
        })
        .ok_or_else(|| PpError::InvalidRequest("text advance overflowed".to_owned()))?;
    let width =
        (advance_units as f64).abs() / f64::from(face.units_per_em()) * f64::from(node.font_size);
    if !width.is_finite() || width > f64::from(f32::MAX) {
        return Err(PpError::InvalidRequest(
            "text line width is not representable".to_owned(),
        ));
    }
    let glyphs = shaped
        .glyph_infos()
        .iter()
        .zip(shaped.glyph_positions().iter())
        .map(|(info, position)| TextGlyphSnapshot {
            glyph_id: info.glyph_id,
            cluster: info.cluster,
            x_advance: position.x_advance,
            y_advance: position.y_advance,
            x_offset: position.x_offset,
            y_offset: position.y_offset,
        })
        .collect();
    Ok(ShapedLine {
        width: width as f32,
        advance_units,
        glyphs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_node(content: &str, box_width: u32, box_height: u32) -> TextNode {
        let font_bytes = include_bytes!("../tests/fixtures/Tuffy.ttf").to_vec();
        TextNode {
            content: content.to_owned(),
            direction: TextDirection::Ltr,
            language: "en".to_owned(),
            box_width,
            box_height,
            alignment: TextAlignment::Start,
            line_break: LineBreakMode::NoWrap,
            font_size: 18.0,
            color: [12, 34, 56, 255],
            pixel_spec: PixelSpec::rgba8_srgb_straight(),
            font_sha256: format!("{:x}", Sha256::digest(&font_bytes)),
            font_bytes,
        }
    }

    #[test]
    fn invalid_font_digest_and_missing_glyph_fail_before_render() {
        let node = TextNode {
            content: "A".to_owned(),
            direction: TextDirection::Ltr,
            language: "en".to_owned(),
            box_width: 32,
            box_height: 32,
            alignment: TextAlignment::Start,
            line_break: LineBreakMode::NoWrap,
            font_size: 12.0,
            color: [0, 0, 0, 255],
            pixel_spec: PixelSpec::rgba8_srgb_straight(),
            font_bytes: vec![1, 2, 3],
            font_sha256: "0".repeat(64),
        };
        assert!(node.render().is_err());
    }

    #[test]
    fn fixture_font_raster_uses_shaped_glyphs_and_is_non_empty() {
        let output = fixture_node("Office", 160, 40)
            .render()
            .expect("fixture text");
        assert!(output.layout.glyph_count > 0);
        assert!(output
            .raster
            .pixels()
            .chunks_exact(4)
            .any(|pixel| pixel[3] > 0));
        assert_eq!(
            output.layout.raster_sha256,
            format!("{:x}", Sha256::digest(output.raster.pixels()))
        );
    }

    #[test]
    fn missing_glyph_unsupported_script_and_overflow_are_explicit_failures() {
        assert!(fixture_node("界", 160, 40).render().is_err());
        assert!(fixture_node("\u{0378}", 160, 40).render().is_err());
        assert!(fixture_node("A", 1, 40).render().is_err());
        let mut multiline = fixture_node("A\nA", 160, 1);
        multiline.line_break = LineBreakMode::WordWrap;
        assert!(multiline.render().is_err());
    }
}
