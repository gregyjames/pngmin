use std::io::Write;
use std::path::{Path, PathBuf};
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

    #[arg(long = "dir")]
    directory: Option<String>,

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

    #[arg(long = "out-dir")]
    out_dir: Option<String>,
}

struct KeyObject{
    key: [u8; 32],
    salt: [u8; 16]
}

impl KeyObject{
    pub fn derive_key_from_password(password: &str, salt: Option<&[u8; 16]>) -> KeyObject {
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

        KeyObject{
            key,
            salt
        }
    }

    pub fn save_key_to(&self, key_path: &str) -> anyhow::Result<()>{
        let mut file = std::fs::File::create(key_path)?;
        file.write_all(&self.salt)?;
        file.write_all(&self.key)?;
        Ok(())
    }

    pub fn load_key(key_path: &str) -> anyhow::Result<KeyObject> {
        let data = std::fs::read(key_path)?;
        if data.len() != 48 {
            bail!("Key file has invalid length (expected 32 bytes, got {})", data.len());
        }

        let mut salt = [0u8; 16];
        let mut key = [0u8; 32];
        salt.copy_from_slice(&data[0..16]);
        key.copy_from_slice(&data[16..48]);

        Ok(KeyObject{
            key,
            salt
        })
    }
}
fn get_png_files(dir: &str) -> anyhow::Result<Vec<PathBuf>> {
    let dir_path = Path::new(dir);
    if !dir_path.is_dir() {
        bail!("Path is not a directory: {}", dir);
    }

    let mut png_files = Vec::new();
    for entry in std::fs::read_dir(dir_path)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() {
            if let Some(ext) = path.extension() {
                if ext.to_string_lossy().to_lowercase() == "png" {
                    png_files.push(path);
                }
            }
        }
    }
    Ok(png_files)
}

fn get_output_path(input_path: &str, out_dir: Option<&str>, suffix: &str) -> String {
    let input_path = Path::new(input_path);
    let file_name = input_path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("output.png");

    let base_name = file_name.trim_end_matches(".png");
    let new_name = format!("{}{}.png", base_name, suffix);

    if let Some(out_dir) = out_dir {
        Path::new(out_dir).join(&new_name).to_string_lossy().to_string()
    } else {
        input_path.parent()
            .map(|p| p.join(&new_name))
            .unwrap_or_else(|| PathBuf::from(&new_name))
            .to_string_lossy()
            .to_string()
    }
}


fn process_file_encrypt(input_file: &str, output_file: Option<String>, out_dir: Option<&str>, key: &[u8; 32], compression_level: CompressionLevel) -> anyhow::Result<()> {
    let start_time = Instant::now();
    let image = DecodedPng::read_from_file(input_file, None)?;
    let elapsed = start_time.elapsed();
    println!("Reading {} took: {:?}", input_file, elapsed);

    let output = output_file.unwrap_or_else(|| {
        get_output_path(input_file, out_dir, "_encrypted")
    });

    if let Some(out_dir) = out_dir {
        std::fs::create_dir_all(out_dir)?;
    }

    let start_time = Instant::now();
    image.save_optimized(&output, compression_level, Some(key))?;
    let elapsed = start_time.elapsed();
    println!("Encrypted {} -> {} (took: {:?})", input_file, output, elapsed);
    Ok(())
}

fn process_file_decrypt(input_file: &str, output_file: Option<String>, out_dir: Option<&str>, key: &[u8; 32]) -> anyhow::Result<()> {
    let start_time = Instant::now();
    let image = DecodedPng::read_from_file(input_file, Some(key))?;
    let elapsed = start_time.elapsed();
    println!("Reading {} took: {:?}", input_file, elapsed);

    let output = output_file.unwrap_or_else(|| {
        get_output_path(input_file, out_dir, "_decrypted")
    });

    if let Some(out_dir) = out_dir {
        std::fs::create_dir_all(out_dir)?;
    }

    let start_time = Instant::now();
    image.save_optimized(&output, CompressionLevel::Lossless, None)?;
    let elapsed = start_time.elapsed();
    println!("Decrypted {} -> {} (took: {:?})", input_file, output, elapsed);
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    if let Some(password) = args.password {
        let key_path = args.key_path.ok_or_else(|| anyhow::anyhow!("Key path (-k) required when generating key"))?;
        let key_obj = KeyObject::derive_key_from_password(&password, None);
        key_obj.save_key_to(&key_path)?;
        println!("Key generated and saved to: {}", key_path);
        return Ok(());
    }

    if let Some(dir) = args.directory {
        let key_obj = if let Some(key_path) = args.key_path {
            KeyObject::load_key(&key_path)?
        } else {
            bail!("Key path (-k) required when processing directory");
        };

        let png_files = get_png_files(&dir)?;
        if png_files.is_empty() {
            println!("No PNG files found in directory: {}", dir);
            return Ok(());
        }

        println!("Found {} PNG file(s) in directory: {}", png_files.len(), dir);

        // Create output directory if specified
        if let Some(ref out_dir) = args.out_dir {
            std::fs::create_dir_all(out_dir)?;
            println!("Output directory: {}", out_dir);
        }

        if args.encrypt {
            for file_path in &png_files {
                let input_file = file_path.to_string_lossy();
                if let Err(e) = process_file_encrypt(&input_file, None, args.out_dir.as_deref(), &key_obj.key, args.compression_level.clone()) {
                    eprintln!("Error encrypting {}: {}", input_file, e);
                }
            }
            return Ok(());
        }

        if args.decrypt {
            for file_path in &png_files {
                let input_file = file_path.to_string_lossy();
                if let Err(e) = process_file_decrypt(&input_file, None, args.out_dir.as_deref(), &key_obj.key) {
                    eprintln!("Error decrypting {}: {}", input_file, e);
                }
            }
            return Ok(());
        }

        bail!("Please specify -e (encrypt) or -d (decrypt) when using --dir");
    }

    if args.encrypt {
        let input_file = args.input_file.ok_or_else(|| anyhow::anyhow!("Input file required when encrypting"))?;
        let key_obj = if let Some(key_path) = args.key_path {
            KeyObject::load_key(&key_path)?
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

        image.save_optimized(&output_file, args.compression_level, Some(&key_obj.key))?;
        return Ok(());
    }

    if args.decrypt {
        let input_file = args.input_file.ok_or_else(|| anyhow::anyhow!("Input file required when decrypting"))?;
        let key_object = if let Some(key_path) = args.key_path {
            KeyObject::load_key(&key_path)?
        }else{
            bail!("Input file required when decrypting");
        };

        let start_time = Instant::now();
        let image = DecodedPng::read_from_file(&input_file, Some(&key_object.key))?;
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

    bail!("Please specify one of: -g (generate key), -e (encrypt), or -d (decrypt)");
}