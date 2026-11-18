use crate::png::filter::apply_filter_into;

pub const BLACK_VEC: [u8; 4] = [0, 0, 0, 0];
pub const FILTERS: [u8; 4] = [1u8, 2u8, 3u8, 4u8];

/// Quantizes RGB channels in-place (if `bits` is `Some(1..=7)`), zeroes RGB on
/// fully transparent pixels, and returns whether any pixel has non-opaque alpha (`a != 255`).
pub fn prepare_rgba_in_place(rgba: &mut [u8], bits: Option<u8>) -> bool {
    let quant_params = bits
        .filter(|&b| b > 0 && b < 8)
        .map(|b| (8 - b, (1u16 << b) - 1));

    let mut has_alpha = false;
    for pixel in rgba.chunks_exact_mut(4) {
        let a = pixel[3];
        if a == 0 {
            pixel.copy_from_slice(&BLACK_VEC);
            has_alpha = true;
        } else {
            if let Some((shift, max_q)) = quant_params {
                pixel[0] = quantize_channel(pixel[0], shift, max_q);
                pixel[1] = quantize_channel(pixel[1], shift, max_q);
                pixel[2] = quantize_channel(pixel[2], shift, max_q);
            }
            if a != 255 {
                has_alpha = true;
            }
        }
    }
    has_alpha
}

// apparently, according to GPT small values near zero compress better? So this is a cheap scoring metric
fn score_filtered_row(filtered: &[u8]) -> u64 {
    filtered.iter()
        .map(|&b| (b as i8 as i32).abs() as u64)
        .sum()
}
pub fn choose_best_filter_into(
    row: &[u8],
    prev: Option<&[u8]>,
    bytes_per_pixel: usize,
    best_buf: &mut Vec<u8>,
    candidate_buf: &mut Vec<u8>,
) -> u8 {
    let mut best_filter = 0u8;
    apply_filter_into(0, bytes_per_pixel, row, prev, best_buf);
    let mut best_score = score_filtered_row(best_buf);

    for f in FILTERS {
        apply_filter_into(f, bytes_per_pixel, row, prev, candidate_buf);
        let s = score_filtered_row(candidate_buf);
        if s < best_score {
            best_score = s;
            best_filter = f;
            std::mem::swap(best_buf, candidate_buf);
        }
    }

    best_filter
}

#[inline]
fn quantize_channel(value: u8, shift: u8, max_q: u16) -> u8 {
    let q = (value >> shift) as u16;
    ((q * 255) / max_q) as u8
}

/// Attempts to build an indexed palette (color type 3) if the image has at most 256 unique RGBA colors.
/// Returns `(plte_rgb_bytes, trns_alpha_bytes, pixel_indices)` or `None` if > 256 unique colors.
pub fn try_build_palette(rgba: &[u8]) -> Option<(Vec<u8>, Vec<u8>, Vec<u8>)> {
    use std::collections::HashMap;

    let mut color_to_idx: HashMap<[u8; 4], u8> = HashMap::new();
    let mut plte = Vec::new();
    let mut alphas = Vec::new();
    let mut indices = Vec::with_capacity(rgba.len() / 4);

    for chunk in rgba.chunks_exact(4) {
        let pixel = [chunk[0], chunk[1], chunk[2], chunk[3]];
        let idx = if let Some(&existing) = color_to_idx.get(&pixel) {
            existing
        } else {
            if alphas.len() >= 256 {
                return None;
            }
            let new_idx = alphas.len() as u8;
            color_to_idx.insert(pixel, new_idx);
            plte.extend_from_slice(&pixel[0..3]);
            alphas.push(pixel[3]);
            new_idx
        };
        indices.push(idx);
    }

    if alphas.is_empty() {
        return None;
    }

    while alphas.last() == Some(&255) {
        alphas.pop();
    }

    Some((plte, alphas, indices))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prepare_rgba_in_place_preserves_black_white_and_alpha() {
        let input = [
            0, 0, 0, 0,
            255, 255, 255, 128,
            128, 64, 192, 255,
        ];

        for bits in [4, 6] {
            let mut out = input;
            assert!(prepare_rgba_in_place(&mut out, Some(bits)));
            // Black stays black
            assert_eq!(&out[0..4], &[0, 0, 0, 0]);
            // Pure white stays pure white (255), alpha preserved
            assert_eq!(&out[4..8], &[255, 255, 255, 128]);
            // Alpha preserved on midtones
            assert_eq!(out[11], 255);
        }
    }

    #[test]
    fn test_prepare_rgba_in_place_lossless_and_eight_bits() {
        let input = [12, 34, 56, 78, 255, 0, 127, 200];
        let mut out_none = input;
        assert!(prepare_rgba_in_place(&mut out_none, None));
        assert_eq!(out_none, input);

        let mut out_eight = input;
        assert!(prepare_rgba_in_place(&mut out_eight, Some(8)));
        assert_eq!(out_eight, input);
    }

    #[test]
    fn test_prepare_rgba_in_place_zeroes_transparent_rgb_and_tracks_alpha() {
        let mut opaque = [12, 34, 56, 255, 200, 100, 50, 255];
        assert!(!prepare_rgba_in_place(&mut opaque, None));
        assert_eq!(opaque, [12, 34, 56, 255, 200, 100, 50, 255]);

        let mut with_transparent = [123, 45, 67, 0, 12, 34, 56, 1];
        assert!(prepare_rgba_in_place(&mut with_transparent, None));
        assert_eq!(with_transparent, [0, 0, 0, 0, 12, 34, 56, 1]);
    }

    #[test]
    fn test_choose_best_filter_into_reuses_buffers() {
        let prev = [10u8, 20, 30, 40, 50, 60, 70, 80];
        let row = [11u8, 21, 31, 41, 51, 61, 71, 81];

        let mut best_buf = Vec::with_capacity(row.len());
        let mut candidate_buf = Vec::with_capacity(row.len());
        let cap_best = best_buf.capacity();
        let cap_cand = candidate_buf.capacity();

        let filter = choose_best_filter_into(&row, Some(&prev), 4, &mut best_buf, &mut candidate_buf);

        // Up filter (2): each byte is prev[i] + 1, so filtered row is all 1s (score = 8).
        assert_eq!(filter, 2);
        assert_eq!(best_buf, [1u8; 8]);
        assert_eq!(best_buf.capacity(), cap_best);
        assert_eq!(candidate_buf.capacity(), cap_cand);
    }
}