use std::io::Cursor;
use std::path::Path;

use image::{ImageReader, Limits as ImageLimits};

use crate::core::{PpError, PpResult, Raster};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecodeLimits {
    pub max_dimension: u32,
    pub max_pixels: u64,
}

impl DecodeLimits {
    pub const PRODUCTION: Self = Self {
        max_dimension: 8192,
        max_pixels: 8192 * 8192,
    };

    pub fn validate(&self, width: u32, height: u32) -> PpResult<()> {
        self.validate_path(Path::new("<unknown>"), width, height)
    }

    fn validate_path(&self, path: &Path, width: u32, height: u32) -> PpResult<()> {
        let pixels = u64::from(width) * u64::from(height);
        if width > self.max_dimension || height > self.max_dimension || pixels > self.max_pixels {
            return Err(PpError::ImageTooLarge {
                path: path.to_path_buf(),
                width,
                height,
                max_dimension: self.max_dimension,
                max_pixels: self.max_pixels,
            });
        }
        Ok(())
    }
}

impl Default for DecodeLimits {
    fn default() -> Self {
        Self::PRODUCTION
    }
}

pub struct ImageCodec;

impl ImageCodec {
    /// Decodes a path through one open reader. The previous implementation opened
    /// the path once for dimensions and again for pixels, allowing two different
    /// file revisions to participate in one decode.
    pub fn decode_rgba(path: impl AsRef<Path>, limits: DecodeLimits) -> PpResult<Raster> {
        let path = path.as_ref();
        let reader = ImageReader::open(path).map_err(|source| PpError::FileIo {
            path: path.to_path_buf(),
            message: source.to_string(),
        })?;
        decode_reader(path, reader, limits)
    }

    /// Decodes the exact immutable byte snapshot supplied by an I/O adapter.
    /// This is used by multi-file generation workflows so computation and the
    /// publication precondition refer to the same source revision.
    pub fn decode_rgba_bytes(
        path: impl AsRef<Path>,
        bytes: &[u8],
        limits: DecodeLimits,
    ) -> PpResult<Raster> {
        let path = path.as_ref();
        decode_reader(path, ImageReader::new(Cursor::new(bytes)), limits)
    }
}

fn decode_reader<R: std::io::BufRead + std::io::Seek>(
    path: &Path,
    reader: ImageReader<R>,
    limits: DecodeLimits,
) -> PpResult<Raster> {
    let mut reader = reader
        .with_guessed_format()
        .map_err(|source| PpError::ImageDecode {
            path: path.to_path_buf(),
            message: source.to_string(),
        })?;
    reader.limits(to_image_limits(limits));
    let image = reader
        .decode()
        .map_err(|source| PpError::ImageDecode {
            path: path.to_path_buf(),
            message: source.to_string(),
        })?
        .into_rgba8();
    limits.validate_path(path, image.width(), image.height())?;
    Raster::new(image.width(), image.height(), image.into_raw())
}

fn to_image_limits(limits: DecodeLimits) -> ImageLimits {
    let mut image_limits = ImageLimits::default();
    image_limits.max_image_width = Some(limits.max_dimension);
    image_limits.max_image_height = Some(limits.max_dimension);
    image_limits.max_alloc = Some(limits.max_pixels.saturating_mul(4));
    image_limits
}
