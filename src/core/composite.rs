use super::{PpError, PpResult, Raster};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlendMode {
    Normal,
    Multiply,
    Screen,
    Overlay,
    Darken,
    Lighten,
}

/// Composites two straight-alpha RGBA8 rasters in linear sRGB using fixed-point arithmetic.
/// Dimensions must match. The returned raster is straight-alpha RGBA8.
pub fn composite_source_over_linear_srgb(
    backdrop: &Raster,
    source: &Raster,
    blend: BlendMode,
) -> PpResult<Raster> {
    if backdrop.width() != source.width() || backdrop.height() != source.height() {
        return Err(PpError::InvalidRequest(
            "composite source and backdrop dimensions must match".to_string(),
        ));
    }
    let mut output = vec![0u8; backdrop.pixels().len()];
    for ((backdrop_pixel, source_pixel), output_pixel) in backdrop
        .pixels()
        .chunks_exact(4)
        .zip(source.pixels().chunks_exact(4))
        .zip(output.chunks_exact_mut(4))
    {
        composite_pixel(backdrop_pixel, source_pixel, output_pixel, blend);
    }
    Raster::new(backdrop.width(), backdrop.height(), output)
}

fn composite_pixel(backdrop: &[u8], source: &[u8], output: &mut [u8], blend: BlendMode) {
    let alpha_backdrop = u32::from(backdrop[3]) * 257;
    let alpha_source = u32::from(source[3]) * 257;
    let one_minus_source = 65_535 - alpha_source;
    let alpha_output = alpha_source + mul_u16(alpha_backdrop, one_minus_source);
    output[3] = u16_to_u8(alpha_output);

    if alpha_output == 0 {
        output[..3].fill(0);
        return;
    }

    for channel in 0..3 {
        let cb = u32::from(SRGB8_TO_LINEAR16[backdrop[channel] as usize]);
        let cs = u32::from(SRGB8_TO_LINEAR16[source[channel] as usize]);
        let blended = blend_channel(cb, cs, blend);

        // W3C blending followed by Porter-Duff source-over:
        // co = as*(1-ab)*Cs + as*ab*B(Cb,Cs) + (1-as)*ab*Cb
        let source_uncovered = mul_u16(alpha_source, 65_535 - alpha_backdrop);
        let source_overlap = mul_u16(alpha_source, alpha_backdrop);
        let backdrop_remaining = mul_u16(one_minus_source, alpha_backdrop);
        let premultiplied = mul_u16(source_uncovered, cs)
            .saturating_add(mul_u16(source_overlap, blended))
            .saturating_add(mul_u16(backdrop_remaining, cb))
            .min(65_535);
        let straight = div_u16(premultiplied, alpha_output);
        output[channel] = linear16_to_srgb8(straight as u16);
    }
}

fn blend_channel(backdrop: u32, source: u32, mode: BlendMode) -> u32 {
    match mode {
        BlendMode::Normal => source,
        BlendMode::Multiply => mul_u16(backdrop, source),
        BlendMode::Screen => 65_535 - mul_u16(65_535 - backdrop, 65_535 - source),
        BlendMode::Overlay => {
            if backdrop <= 32_767 {
                mul_u16(backdrop.saturating_mul(2).min(65_535), source)
            } else {
                65_535 - mul_u16(
                    (65_535 - backdrop).saturating_mul(2).min(65_535),
                    65_535 - source,
                )
            }
        }
        BlendMode::Darken => backdrop.min(source),
        BlendMode::Lighten => backdrop.max(source),
    }
}

fn mul_u16(left: u32, right: u32) -> u32 {
    ((u64::from(left) * u64::from(right) + 32_767) / 65_535) as u32
}

fn div_u16(premultiplied: u32, alpha: u32) -> u32 {
    if alpha == 0 {
        0
    } else {
        ((u64::from(premultiplied) * 65_535 + u64::from(alpha) / 2) / u64::from(alpha))
            .min(65_535) as u32
    }
}

fn u16_to_u8(value: u32) -> u8 {
    ((value.min(65_535) + 128) / 257) as u8
}

fn linear16_to_srgb8(value: u16) -> u8 {
    match SRGB8_TO_LINEAR16.binary_search(&value) {
        Ok(index) => index as u8,
        Err(0) => 0,
        Err(256) => 255,
        Err(index) => {
            let lower = SRGB8_TO_LINEAR16[index - 1];
            let upper = SRGB8_TO_LINEAR16[index];
            if value - lower <= upper - value {
                (index - 1) as u8
            } else {
                index as u8
            }
        }
    }
}

// IEC 61966-2-1 sRGB transfer function sampled at all RGBA8 code values and rounded to u16.
// Keeping this table in source avoids platform/libm-dependent pow() differences in publication
// bytes while preserving explicit linear-light compositing semantics.
const SRGB8_TO_LINEAR16: [u16; 256] = [
    0, 20, 40, 60, 80, 99, 119, 139, 159, 179, 199, 219, 241, 264, 288, 313,
    340, 367, 396, 427, 458, 491, 526, 562, 599, 637, 677, 718, 761, 805, 851, 898,
    947, 997, 1048, 1101, 1156, 1212, 1270, 1330, 1391, 1453, 1517, 1583, 1651, 1720,
    1790, 1863, 1937, 2013, 2090, 2170, 2250, 2333, 2418, 2504, 2592, 2681, 2773, 2866,
    2961, 3058, 3157, 3258, 3360, 3464, 3570, 3678, 3788, 3900, 4014, 4129, 4247, 4366,
    4488, 4611, 4736, 4864, 4993, 5124, 5257, 5392, 5530, 5669, 5810, 5953, 6099, 6246,
    6395, 6547, 6700, 6856, 7014, 7174, 7335, 7500, 7666, 7834, 8004, 8177, 8352, 8528,
    8708, 8889, 9072, 9258, 9445, 9635, 9828, 10022, 10219, 10417, 10619, 10822, 11028,
    11235, 11446, 11658, 11873, 12090, 12309, 12530, 12754, 12980, 13209, 13440, 13673,
    13909, 14146, 14387, 14629, 14874, 15122, 15371, 15623, 15878, 16135, 16394, 16656,
    16920, 17187, 17456, 17727, 18001, 18277, 18556, 18837, 19121, 19407, 19696, 19987,
    20281, 20577, 20876, 21177, 21481, 21787, 22096, 22407, 22721, 23038, 23357, 23678,
    24002, 24329, 24658, 24990, 25325, 25662, 26001, 26344, 26688, 27036, 27386, 27739,
    28094, 28452, 28813, 29176, 29542, 29911, 30282, 30656, 31033, 31412, 31794, 32179,
    32567, 32957, 33350, 33745, 34143, 34544, 34948, 35355, 35764, 36176, 36591, 37008,
    37429, 37852, 38278, 38706, 39138, 39572, 40009, 40449, 40891, 41337, 41785, 42236,
    42690, 43147, 43606, 44069, 44534, 45002, 45473, 45947, 46423, 46903, 47385, 47871,
    48359, 48850, 49344, 49841, 50341, 50844, 51349, 51858, 52369, 52884, 53401, 53921,
    54445, 54971, 55500, 56032, 56567, 57105, 57646, 58190, 58737, 59287, 59840, 60396,
    60955, 61517, 62082, 62650, 63221, 63795, 64372, 64952, 65535,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transparent_source_is_identity() -> PpResult<()> {
        let backdrop = Raster::new(1, 1, vec![12, 34, 56, 200])?;
        let source = Raster::new(1, 1, vec![255, 0, 0, 0])?;
        let output = composite_source_over_linear_srgb(&backdrop, &source, BlendMode::Normal)?;
        assert_eq!(output, backdrop);
        Ok(())
    }

    #[test]
    fn opaque_source_replaces_backdrop_in_normal_mode() -> PpResult<()> {
        let backdrop = Raster::new(1, 1, vec![12, 34, 56, 200])?;
        let source = Raster::new(1, 1, vec![240, 100, 20, 255])?;
        let output = composite_source_over_linear_srgb(&backdrop, &source, BlendMode::Normal)?;
        assert_eq!(output, source);
        Ok(())
    }

    #[test]
    fn transfer_lut_is_monotonic_and_round_trips_codes() {
        for window in SRGB8_TO_LINEAR16.windows(2) {
            assert!(window[0] < window[1]);
        }
        for code in 0u8..=255 {
            assert_eq!(linear16_to_srgb8(SRGB8_TO_LINEAR16[code as usize]), code);
        }
    }
}
