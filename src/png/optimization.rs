use crate::png::filter::apply_filter_into;

pub const BLACK_VEC: [u8; 4] = [0, 0, 0, 0];
pub const FILTERS: [u8; 4] = [1u8, 2u8, 3u8, 4u8];

/// Zeroes RGB on fully transparent pixels and returns `(has_alpha, is_grayscale)`.
pub fn prepare_rgba_in_place(rgba: &mut [u8]) -> (bool, bool) {
    let mut has_alpha = false;
    let mut is_grayscale = true;
    for pixel in rgba.chunks_exact_mut(4) {
        let a = pixel[3];
        if a == 0 {
            pixel.copy_from_slice(&BLACK_VEC);
            has_alpha = true;
        } else {
            if a != 255 {
                has_alpha = true;
            }
            if pixel[0] != pixel[1] || pixel[1] != pixel[2] {
                is_grayscale = false;
            }
        }
    }
    (has_alpha, is_grayscale)
}

#[inline]
pub fn palette_bit_depth(num_colors: usize) -> u8 {
    if num_colors <= 2 {
        1
    } else if num_colors <= 4 {
        2
    } else if num_colors <= 16 {
        4
    } else {
        8
    }
}

pub fn pack_indices_by_row(indices: &[u8], width: usize, height: usize, bit_depth: u8) -> Vec<u8> {
    if bit_depth == 8 {
        return indices.to_vec();
    }
    let ppb = 8 / bit_depth as usize;
    let row_bytes = (width * bit_depth as usize).div_ceil(8);
    let mut packed = vec![0u8; height * row_bytes];
    for y in 0..height {
        let src_row = &indices[y * width..(y + 1) * width];
        let dst_row = &mut packed[y * row_bytes..(y + 1) * row_bytes];
        for (x, &idx) in src_row.iter().enumerate() {
            let byte_idx = x / ppb;
            let slot = x % ppb;
            let shift = 8 - (slot + 1) * (bit_depth as usize);
            dst_row[byte_idx] |= idx << shift;
        }
    }
    packed
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

#[derive(Clone, Copy)]
struct ColorCount {
    color: [u8; 4],
    count: u32,
}

struct ColorBox {
    start: usize,
    end: usize,
    min: [u8; 4],
    max: [u8; 4],
    population: u64,
}

impl ColorBox {
    fn from_slice(colors: &[ColorCount], start: usize, end: usize) -> Self {
        let mut min = [255u8; 4];
        let mut max = [0u8; 4];
        let mut population = 0u64;
        for entry in &colors[start..end] {
            for ch in 0..4 {
                min[ch] = min[ch].min(entry.color[ch]);
                max[ch] = max[ch].max(entry.color[ch]);
            }
            population += entry.count as u64;
        }
        Self {
            start,
            end,
            min,
            max,
            population,
        }
    }

    fn len(&self) -> usize {
        self.end - self.start
    }

    fn widest_channel(&self) -> (usize, u8) {
        let mut best_ch = 0;
        let mut best_range = 0u8;
        for ch in 0..4 {
            let range = self.max[ch].saturating_sub(self.min[ch]);
            if range >= best_range {
                best_range = range;
                best_ch = ch;
            }
        }
        (best_ch, best_range)
    }

    fn average_color(&self, colors: &[ColorCount]) -> [u8; 4] {
        let mut sums = [0u64; 4];
        let mut total = 0u64;
        for entry in &colors[self.start..self.end] {
            let w = entry.count as u64;
            for ch in 0..4 {
                sums[ch] += entry.color[ch] as u64 * w;
            }
            total += w;
        }
        if total == 0 {
            return [0, 0, 0, 0];
        }
        [
            ((sums[0] + total / 2) / total) as u8,
            ((sums[1] + total / 2) / total) as u8,
            ((sums[2] + total / 2) / total) as u8,
            ((sums[3] + total / 2) / total) as u8,
        ]
    }
}

#[inline]
fn nearest_palette_index(palette: &[[u8; 4]], pixel: [u8; 4]) -> u8 {
    let mut best_idx = 0u8;
    let mut best_dist = u32::MAX;
    for (idx, &cand) in palette.iter().enumerate() {
        let dr = pixel[0] as i32 - cand[0] as i32;
        let dg = pixel[1] as i32 - cand[1] as i32;
        let db = pixel[2] as i32 - cand[2] as i32;
        let da = pixel[3] as i32 - cand[3] as i32;
        let dist = (dr * dr + dg * dg + db * db + da * da) as u32;
        if dist < best_dist {
            best_dist = dist;
            best_idx = idx as u8;
            if dist == 0 {
                break;
            }
        }
    }
    best_idx
}

/// Quantizes an RGBA image to at most `max_colors` (clamped to `1..=256`) using
/// median-cut color quantization and optional Floyd-Steinberg dithering.
/// Returns `(plte_rgb_bytes, trns_alpha_bytes, pixel_indices)`.
pub fn quantize_to_palette(
    rgba: &[u8],
    width: usize,
    height: usize,
    max_colors: usize,
    dither: bool,
) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    use std::collections::HashMap;

    let max_colors = max_colors.clamp(1, 256);

    let mut counts: HashMap<[u8; 4], u32> = HashMap::new();
    let mut has_transparent = false;
    for chunk in rgba.chunks_exact(4) {
        if chunk[3] == 0 {
            has_transparent = true;
        } else {
            let px = [chunk[0], chunk[1], chunk[2], chunk[3]];
            *counts.entry(px).or_insert(0) += 1;
        }
    }

    let mut palette: Vec<[u8; 4]> = Vec::with_capacity(max_colors);
    let transparent_idx = if has_transparent {
        palette.push([0, 0, 0, 0]);
        Some(0u8)
    } else {
        None
    };

    let remaining_slots = max_colors.saturating_sub(palette.len());
    if remaining_slots > 0 && !counts.is_empty() {
        if counts.len() <= remaining_slots {
            let mut exact: Vec<[u8; 4]> = counts.into_keys().collect();
            exact.sort_unstable();
            palette.extend(exact);
        } else {
            let mut color_vec: Vec<ColorCount> = counts
                .into_iter()
                .map(|(color, count)| ColorCount { color, count })
                .collect();
            let total_len = color_vec.len();
            let mut boxes = Vec::with_capacity(remaining_slots);
            boxes.push(ColorBox::from_slice(&color_vec, 0, total_len));

            while boxes.len() < remaining_slots {
                let Some((box_idx, _)) = boxes
                    .iter()
                    .enumerate()
                    .filter(|(_, b)| b.len() > 1)
                    .max_by_key(|(_, b)| {
                        let (_, range) = b.widest_channel();
                        (range as u64) * b.population
                    })
                else {
                    break;
                };

                let chosen = boxes.swap_remove(box_idx);
                let (split_ch, _) = chosen.widest_channel();
                color_vec[chosen.start..chosen.end]
                    .sort_unstable_by_key(|entry| entry.color[split_ch]);

                let half_pop = chosen.population / 2;
                let mut acc = 0u64;
                let mut split_rel = 1usize;
                for (i, entry) in color_vec[chosen.start..chosen.end].iter().enumerate() {
                    acc += entry.count as u64;
                    if acc >= half_pop && i > 0 {
                        split_rel = i;
                        break;
                    }
                }
                split_rel = split_rel.clamp(1, chosen.len() - 1);
                let mid = chosen.start + split_rel;

                boxes.push(ColorBox::from_slice(&color_vec, chosen.start, mid));
                boxes.push(ColorBox::from_slice(&color_vec, mid, chosen.end));
            }

            for b in boxes {
                palette.push(b.average_color(&color_vec));
            }
        }
    }

    if palette.is_empty() {
        palette.push([0, 0, 0, 0]);
    }

    let pixel_count = width * height;
    let mut indices = Vec::with_capacity(pixel_count);

    if dither && width > 0 && height > 0 {
        let mut curr_err = vec![[0i16; 4]; width];
        let mut next_err = vec![[0i16; 4]; width];

        for y in 0..height {
            for x in 0..width {
                let base = (y * width + x) * 4;
                let orig = [rgba[base], rgba[base + 1], rgba[base + 2], rgba[base + 3]];

                if orig[3] == 0 {
                    if let Some(t_idx) = transparent_idx {
                        indices.push(t_idx);
                        continue;
                    }
                }

                let target = [
                    (orig[0] as i16 + curr_err[x][0]).clamp(0, 255) as u8,
                    (orig[1] as i16 + curr_err[x][1]).clamp(0, 255) as u8,
                    (orig[2] as i16 + curr_err[x][2]).clamp(0, 255) as u8,
                    (orig[3] as i16 + curr_err[x][3]).clamp(0, 255) as u8,
                ];

                let idx = nearest_palette_index(&palette, target);
                indices.push(idx);

                let chosen = palette[idx as usize];
                let err = [
                    target[0] as i16 - chosen[0] as i16,
                    target[1] as i16 - chosen[1] as i16,
                    target[2] as i16 - chosen[2] as i16,
                    target[3] as i16 - chosen[3] as i16,
                ];

                if x + 1 < width {
                    for ch in 0..4 {
                        curr_err[x + 1][ch] += (err[ch] * 7) / 16;
                    }
                }
                if y + 1 < height {
                    if x > 0 {
                        for ch in 0..4 {
                            next_err[x - 1][ch] += (err[ch] * 3) / 16;
                        }
                    }
                    for ch in 0..4 {
                        next_err[x][ch] += (err[ch] * 5) / 16;
                    }
                    if x + 1 < width {
                        for ch in 0..4 {
                            next_err[x + 1][ch] += err[ch] / 16;
                        }
                    }
                }
            }

            std::mem::swap(&mut curr_err, &mut next_err);
            next_err.fill([0i16; 4]);
        }
    } else {
        let mut cache: HashMap<[u8; 4], u8> = HashMap::new();
        for chunk in rgba.chunks_exact(4) {
            if chunk[3] == 0 {
                if let Some(t_idx) = transparent_idx {
                    indices.push(t_idx);
                    continue;
                }
            }
            let px = [chunk[0], chunk[1], chunk[2], chunk[3]];
            let idx = if let Some(&cached) = cache.get(&px) {
                cached
            } else {
                let nearest = nearest_palette_index(&palette, px);
                cache.insert(px, nearest);
                nearest
            };
            indices.push(idx);
        }
    }

    let mut plte = Vec::with_capacity(palette.len() * 3);
    let mut alphas = Vec::with_capacity(palette.len());
    for c in &palette {
        plte.extend_from_slice(&c[0..3]);
        alphas.push(c[3]);
    }
    while alphas.last() == Some(&255) {
        alphas.pop();
    }

    (plte, alphas, indices)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prepare_rgba_in_place_zeroes_transparent_rgb_and_tracks_alpha_and_gray() {
        let mut opaque_color = [12, 34, 56, 255, 200, 100, 50, 255];
        assert_eq!(prepare_rgba_in_place(&mut opaque_color), (false, false));
        assert_eq!(opaque_color, [12, 34, 56, 255, 200, 100, 50, 255]);

        let mut opaque_gray = [42, 42, 42, 255, 200, 200, 200, 255];
        assert_eq!(prepare_rgba_in_place(&mut opaque_gray), (false, true));

        let mut with_transparent_gray = [123, 45, 67, 0, 56, 56, 56, 1];
        assert_eq!(prepare_rgba_in_place(&mut with_transparent_gray), (true, true));
        assert_eq!(with_transparent_gray, [0, 0, 0, 0, 56, 56, 56, 1]);
    }

    #[test]
    fn test_palette_bit_depth_and_pack_indices_by_row() {
        assert_eq!(palette_bit_depth(1), 1);
        assert_eq!(palette_bit_depth(2), 1);
        assert_eq!(palette_bit_depth(3), 2);
        assert_eq!(palette_bit_depth(4), 2);
        assert_eq!(palette_bit_depth(5), 4);
        assert_eq!(palette_bit_depth(16), 4);
        assert_eq!(palette_bit_depth(17), 8);
        assert_eq!(palette_bit_depth(256), 8);

        // 3x2 image at 4-bit (2 bytes per row, 0-padded trailing nibble)
        let packed = pack_indices_by_row(&[1, 2, 3, 4, 5, 6], 3, 2, 4);
        assert_eq!(packed, vec![0x12, 0x30, 0x45, 0x60]);
    }

    #[test]
    fn test_quantize_to_palette_reduces_gradient_and_preserves_transparency() {
        let width = 32usize;
        let height = 32usize;
        let mut rgba = Vec::with_capacity(width * height * 4);
        for y in 0..height {
            for x in 0..width {
                if x == 0 && y == 0 {
                    rgba.extend_from_slice(&[99, 88, 77, 0]);
                } else {
                    rgba.extend_from_slice(&[(x * 8) as u8, (y * 8) as u8, ((x + y) * 4) as u8, 255]);
                }
            }
        }

        for dither in [false, true] {
            let (plte, trns, indices) = quantize_to_palette(&rgba, width, height, 64, dither);
            let palette_len = plte.len() / 3;
            assert!(palette_len > 1 && palette_len <= 64);
            assert_eq!(indices.len(), width * height);
            assert!(indices.iter().all(|&i| (i as usize) < palette_len));

            // Transparent pixel at (0, 0) maps to dedicated transparent entry (alpha == 0)
            let t_idx = indices[0] as usize;
            assert_eq!(trns.get(t_idx).copied().unwrap_or(255), 0);
        }
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