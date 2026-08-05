use serde::{Deserialize, Serialize};

use super::{content_bbox, FrameRect, Point, Raster, ALPHA_THRESHOLD};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RasterInspection {
    pub width: u32,
    pub height: u32,
    pub foreground_pixels: usize,
    pub alpha_ratio: f64,
    pub content_box: FrameRect,
    pub center: Point,
    pub ground_y: u32,
    pub touches_edge: bool,
}

pub fn inspect_raster(image: &Raster) -> RasterInspection {
    let foreground_pixels = image
        .pixels()
        .chunks_exact(4)
        .filter(|rgba| rgba[3] > ALPHA_THRESHOLD)
        .count();
    let content_box = content_bbox(image);
    let pixel_count = u64::from(image.width()) * u64::from(image.height());

    RasterInspection {
        width: image.width(),
        height: image.height(),
        foreground_pixels,
        alpha_ratio: if pixel_count == 0 {
            0.0
        } else {
            foreground_pixels as f64 / pixel_count as f64
        },
        center: content_center(content_box),
        ground_y: content_box.y + content_box.h,
        touches_edge: touches_edge(content_box, image.width(), image.height()),
        content_box,
    }
}

fn content_center(rect: FrameRect) -> Point {
    if rect.w == 0 || rect.h == 0 {
        return Point::default();
    }
    Point {
        x: rect.x + rect.w / 2,
        y: rect.y + rect.h / 2,
    }
}

fn touches_edge(rect: FrameRect, width: u32, height: u32) -> bool {
    if rect.w == 0 || rect.h == 0 {
        return false;
    }
    rect.x == 0 || rect.y == 0 || rect.x + rect.w >= width || rect.y + rect.h >= height
}
