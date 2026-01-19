use std::io::Write;
use crate::png::{CompressionLevel, DecodedPng};
use std::time::Instant;
use pbkdf2::pbkdf2_hmac;
use rand::RngCore;
use sha2::Sha256;
mod png;

pub fn derive_key_from_password(password: &str, salt: Option<&[u8; 16]>) -> ([u8; 32], [u8; 16]) {
    let salt = match salt {
        Some(s) => *s,
        None => {
            let mut s = [0u8; 16];
            rand::rng().fill_bytes(&mut s);
            s
        }
    };

    let mut key = [0u8; 32];
    pbkdf2_hmac::<Sha256>(password.as_bytes(), &salt, 100_000, &mut key);

    (key, salt)
}
fn main() -> anyhow::Result<()> {
    let mut key_outer = [0u8; 32];

    if !std::fs::exists("./key.txt")? {
        let (new_key, salt) = derive_key_from_password("helios", None);
        let mut file = std::fs::File::create("./key.txt")?;
        file.write_all(&new_key)?;
        println!("Key: {:?} Salt: {:?}", new_key, salt);
        key_outer = new_key;
    }else{
        let data = std::fs::read("./key.txt")?;
        if data.len() != 32 {
            anyhow::bail!("Key file has invalid length (expected 32 bytes, got {})", data.len());
        }
        let mut key_array = [0u8; 32];
        key_array.copy_from_slice(&data);
        key_outer = key_array;
    }

    let start_time = Instant::now();
    let image = DecodedPng::read_from_file("e_file.png", Some(&key_outer))?;
    let elapsed = start_time.elapsed();
    println!("{:#?}", image.info);
    println!("Reading PNG took: {:#?}", elapsed);

    let start_time = Instant::now();
    image.save_optimized("d_file.png", CompressionLevel::Lossless, None)?;
    let elapsed = start_time.elapsed();
    println!("Saving PNG Took: {:?}", elapsed);

    Ok(())
}