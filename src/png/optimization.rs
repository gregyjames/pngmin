pub const BLACK_VEC: [u8; 4] = [0, 0, 0, 0];

pub fn optimize_alpha_channel(rgba: &[u8]) -> Vec<u8> {
    rgba.chunks_exact(4).map(|chunk| {
        let a = chunk[3];

        if a == 0{
            BLACK_VEC
        }else{
            [chunk[0], chunk[1], chunk[2], a]
        }
    }).flatten().collect()
}