use std::io::Write;
use crate::png::{CompressionLevel, DecodedPng};
use std::time::Instant;
use anyhow::bail;
use pbkdf2::pbkdf2_hmac;
use rand::RngCore;
use sha2::Sha256;
use clap::Parser;
mod png;

#[derive(Parser, Debug)]
struct Args {
    #[arg(short = 'i', long = "input")]
    input_file: Option<String>,
    
    #[arg(short = 'k', long = "key")]
    key_path: Option<String>,
    
    #[arg(short = 'g', long = "generate")]
    password: Option<String>,
    
    #[arg(short = 'e', long = "encrypt")]
    encrypt: bool,
    
    #[arg(short = 'd', long = "decrypt")]
    decrypt: bool,

    #[arg(short = 'm', long = "level", required = false, default_value = "lossless")]
    compression_level: CompressionLevel,

    #[arg(short = 'o', required = false)]
    outfile: Option<String>,
}

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

fn load_key(key_path: &str) -> anyhow::Result<[u8; 32]> {
    let data = std::fs::read(key_path)?;
    if data.len() != 32 {
        anyhow::bail!("Key file has invalid length (expected 32 bytes, got {})", data.len());
    }
    let mut key_array = [0u8; 32];
    key_array.copy_from_slice(&data);
    Ok(key_array)
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    if let Some(password) = args.password {
        let key_path = args.key_path.ok_or_else(|| anyhow::anyhow!("Key path (-k) required when generating key"))?;
        let (new_key, salt) = derive_key_from_password(&password, None);
        let mut file = std::fs::File::create(&key_path)?;
        file.write_all(&new_key)?;
        println!("Key generated and saved to: {}", key_path);
        println!("Salt: {:?}", salt);
        return Ok(());
    }

    if args.encrypt {
        let input_file = args.input_file.ok_or_else(|| anyhow::anyhow!("Input file required when encrypting"))?;
        let key = if let Some(key_path) = args.key_path {
            load_key(&key_path)?
        }else{
            bail!("Input file required when encrypting");
        };

        let start_time = Instant::now();
        let image = DecodedPng::read_from_file(&input_file, None)?;
        let elapsed = start_time.elapsed();
        println!("Reading PNG took: {:?}, using compression mode: {:?}", elapsed, args.compression_level);

        let output_file = if args.outfile.is_some() {
            args.outfile.unwrap()
        } else {
            format!("{}_encrypted.png", input_file.trim_end_matches(".png"))
        };

        image.save_optimized(&output_file, args.compression_level, Some(&key))?;
        return Ok(());
    }

    if args.decrypt {
        let input_file = args.input_file.ok_or_else(|| anyhow::anyhow!("Input file required when decrypting"))?;
        let key = if let Some(key_path) = args.key_path {
            load_key(&key_path)?
        }else{
            bail!("Input file required when decrypting");
        };

        let start_time = Instant::now();
        let image = DecodedPng::read_from_file(&input_file, Some(&key))?;
        let elapsed = start_time.elapsed();
        println!("Reading PNG took: {:#?}", elapsed);

        let output_file = if args.outfile.is_some() {
            args.outfile.unwrap()
        } else {
            format!("{}_decrypted.png", input_file.trim_end_matches(".png"))
        };

        image.save_optimized(&output_file, CompressionLevel::Lossless, None)?;
        return Ok(());
    }

    anyhow::bail!("Please specify one of: -g (generate key), -e (encrypt), or -d (decrypt)");
}