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