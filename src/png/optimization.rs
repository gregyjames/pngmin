use crate::png::filter::apply_filter;

pub const BLACK_VEC: [u8; 4] = [0, 0, 0, 0];
pub const FILTERS: [u8; 4] = [1u8, 2u8, 3u8, 4u8];

pub fn optimize_alpha_channel(rgba: &[u8]) -> Vec<u8> {
    //todo!("Think oxipng brute forces by actually compressing the bytes after the filter is applied? Maybe implement that for max compression.");
    rgba.chunks_exact(4).map(|chunk| {
        let a = chunk[3];

        if a == 0{
            BLACK_VEC
        }else{
            [chunk[0], chunk[1], chunk[2], a]
        }
    }).flatten().collect()
}

// apparently, according to GPT small values near zero compress better? So this is a cheap scoring metric
fn score_filtered_row(filtered: &[u8]) -> u64 {
    filtered.iter()
        .map(|&b| (b as i8 as i32).abs() as u64)
        .sum()
}
pub fn choose_best_filter(row: &[u8], prev: Option<&[u8]>, bytes_per_pixel: usize) -> (u8, Vec<u8>) {
    let mut best_filter = 0u8;
    let mut best_bytes = apply_filter(0, bytes_per_pixel, row, prev);
    let mut best_score = score_filtered_row(&best_bytes);

    for f in FILTERS {
        let bytes = apply_filter(f, bytes_per_pixel, row, prev);
        let s = score_filtered_row(&bytes);
        if s < best_score {
            best_score = s;
            best_filter = f;
            best_bytes = bytes;
        }
    }

    (best_filter, best_bytes)
}

// https://en.wikipedia.org/wiki/Color_depth
// https://www.geeksforgeeks.org/electronics-engineering/difference-between-uniform-and-non-uniform-quantization/
pub fn quantize_colors(rgba: &[u8], bits: u8) -> Vec<u8> {
    if bits == 0 || bits >= 8 {
        return rgba.to_vec();
    }

    let shift = 8 - bits;
    let max_q = ((1u16 << bits) - 1) as u16;

    rgba.chunks_exact(4).flat_map(|pixel| {
        [
            quantize_channel(pixel[0], shift, max_q),
            quantize_channel(pixel[1], shift, max_q),
            quantize_channel(pixel[2], shift, max_q),
            pixel[3], // Don't quantize alpha
        ]
    }).collect()
}

#[inline]
fn quantize_channel(value: u8, shift: u8, max_q: u16) -> u8 {
    let q = (value >> shift) as u16;
    ((q * 255) / max_q) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quantize_colors_preserves_black_white_and_alpha() {
        let input = [
            0, 0, 0, 0,
            255, 255, 255, 128,
            128, 64, 192, 255,
        ];

        for bits in [4, 6] {
            let out = quantize_colors(&input, bits);
            // Black stays black
            assert_eq!(&out[0..4], &[0, 0, 0, 0]);
            // Pure white stays pure white (255), alpha preserved
            assert_eq!(&out[4..8], &[255, 255, 255, 128]);
            // Alpha preserved on midtones
            assert_eq!(out[11], 255);
        }
    }

    #[test]
    fn test_quantize_colors_eight_bits_is_lossless() {
        let input = [12, 34, 56, 78, 255, 0, 127, 200];
        assert_eq!(quantize_colors(&input, 8), input);
    }
}