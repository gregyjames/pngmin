// Basically the opposite of the method below lol https://www.w3.org/TR/png-3/#9Filters
pub fn apply_filter(filter_type: u8, bytes_per_pixel: usize, row: &[u8], prev_row: Option<&[u8]>) -> Vec<u8> {
    match filter_type {
        0 => {
            // None: Filt(x) = Orig(x)
            row.to_vec()
        },
        1 => {
            // Sub: Filt(x) = Orig(x) - Orig(a)
            let mut filtered = Vec::with_capacity(row.len());
            for i in 0..row.len() {
                let left = if i >= bytes_per_pixel { row[i - bytes_per_pixel] } else { 0 };
                filtered.push(row[i].wrapping_sub(left));
            }
            filtered
        },
        2 => {
            // Up: Filt(x) = Orig(x) - Orig(b)
            let prev = prev_row.unwrap_or(&[]);
            let mut filtered = Vec::with_capacity(row.len());
            for i in 0..row.len() {
                let up = if i < prev.len() { prev[i] } else { 0 };
                filtered.push(row[i].wrapping_sub(up));
            }
            filtered
        },
        3 => {
            // Average: Filt(x) = Orig(x) - floor((Orig(a) + Orig(b)) / 2)
            let prev = prev_row.unwrap_or(&[]);
            let mut filtered = Vec::with_capacity(row.len());
            for i in 0..row.len() {
                let left = if i >= bytes_per_pixel { row[i - bytes_per_pixel] } else { 0 };
                let up = if i < prev.len() { prev[i] } else { 0 };
                let avg = ((left as u16 + up as u16) / 2) as u8;
                filtered.push(row[i].wrapping_sub(avg));
            }
            filtered
        },
        4 => {
            // Paeth: Filt(x) = Orig(x) - PaethPredictor(Orig(a), Orig(b), Orig(c))
            let prev = prev_row.unwrap_or(&[]);
            let mut filtered = Vec::with_capacity(row.len());
            for i in 0..row.len() {
                let left = if i >= bytes_per_pixel { row[i - bytes_per_pixel] } else { 0 };
                let up = if i < prev.len() { prev[i] } else { 0 };
                let top_left = if i >= bytes_per_pixel && i < prev.len() { prev[i - bytes_per_pixel] } else { 0 };
                let p = paeth_predictor(left, up, top_left);
                filtered.push(row[i].wrapping_sub(p));
            }
            filtered
        },
        _ => {
            panic!("Unsupported filter type: {}", filter_type);
        }
    }
}

pub fn unfilter_row(filter_type: u8, bytes_per_pixel: usize, src: &[u8], prev: Option<Vec<u8>>, dst: &mut [u8]){
    let prev = prev.unwrap_or(vec![0u8; src.len()]);

    match filter_type {
        0 => { // None
            dst.copy_from_slice(src);
        },
        1 => { // Sub
            for i in 0..src.len() {
                let left = if i >= bytes_per_pixel { dst[i - bytes_per_pixel] } else { 0 };
                dst[i] = src[i].wrapping_add(left);
            }
        },
        2 => { // Up
            // Recon(x) = Filt(x) + Recon(b)
            for i in 0..src.len() {
                dst[i] = src[i].wrapping_add(prev[i]);
            }
        },
        3 => { // Avg
            // Recon(x) = Filt(x) + floor((Recon(a) + Recon(b)) / 2)
            for i in 0..src.len() {
                let left = if i >= bytes_per_pixel { dst[i - bytes_per_pixel] } else { 0 };
                let up = prev[i];
                let avg = ((left as u16 + up as u16) / 2) as u8;
                dst[i] = src[i].wrapping_add(avg);
            }
        },
        4 => { // Paeth
            for i in 0..src.len() {
                let top = prev[i];
                let left = if i >= bytes_per_pixel { dst[i - bytes_per_pixel] } else { 0 };
                let top_left = if i >= bytes_per_pixel { prev[i - bytes_per_pixel] } else { 0 };
                let p = paeth_predictor(left, top, top_left);
                dst[i] = src[i].wrapping_add(p);
            }
        }
        _ => {
            panic!("Unsupported filter type: {}", filter_type);
        }
    }
}

fn paeth_predictor(a: u8, b: u8, c: u8) -> u8{
    //convert to i32 since we may need negatives here for abs
    let a = a as i32;
    let b = b as i32;
    let c = c as i32;

    let p = a + b - c;
    let pa = (p - a).abs();
    let pb = (p - b).abs();
    let pc = (p - c).abs();

    if pa <= pb && pa <= pc {
        a as u8
    } else if pb <= pc {
        b as u8
    }
    else {
        c as u8
    }
}