use crate::png::{CompressionLevel, DecodedPng};
use anyhow::bail;
use argon2::{Algorithm, Argon2, ParamsBuilder, Version};
use clap::Parser;
use futures_lite::stream::StreamExt;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use rand::RngCore;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
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

#[derive(Clone)]
struct KeyObject {
    key: [u8; 32],
    salt: [u8; 16],
}

impl KeyObject {
    pub fn derive_key_from_password(password: &str, salt: Option<&[u8; 16]>) -> KeyObject {
        let salt = match salt {
            Some(s) => *s,
            None => {
                let mut s = [0u8; 16];
                rand::rng().fill_bytes(&mut s);
                s
            }
        };

        let params = ParamsBuilder::new()
            .m_cost(65536)
            .t_cost(100)
            .p_cost(4)
            .output_len(32)
            .build()
            .expect("Invalid parameters for Argon2");

        let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);

        let mut key = [0u8; 32];
        argon2
            .hash_password_into(password.as_bytes(), &salt, &mut key)
            .expect("Argon2 hashing failed.");

        KeyObject { key, salt }
    }

    pub async fn derive_key_from_password_async(
        password: String,
        salt: Option<[u8; 16]>,
    ) -> KeyObject {
        smol::unblock(move || Self::derive_key_from_password(&password, salt.as_ref())).await
    }

    #[allow(dead_code)]
    pub fn save_key_to(&self, key_path: &str) -> anyhow::Result<()> {
        let mut file = std::fs::File::create(key_path)?;
        file.write_all(&self.salt)?;
        file.write_all(&self.key)?;
        Ok(())
    }

    pub async fn save_key_to_async(&self, key_path: &str) -> anyhow::Result<()> {
        let mut data = Vec::with_capacity(48);
        data.extend_from_slice(&self.salt);
        data.extend_from_slice(&self.key);
        smol::fs::write(key_path, &data).await?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn load_key(key_path: &str) -> anyhow::Result<KeyObject> {
        let data = std::fs::read(key_path)?;
        if data.len() != 48 {
            bail!(
                "Key file has invalid length (expected 48 bytes, got {})",
                data.len()
            );
        }

        let mut salt = [0u8; 16];
        let mut key = [0u8; 32];
        salt.copy_from_slice(&data[0..16]);
        key.copy_from_slice(&data[16..48]);

        Ok(KeyObject { key, salt })
    }

    pub async fn load_key_async(key_path: &str) -> anyhow::Result<KeyObject> {
        let data = smol::fs::read(key_path).await?;
        if data.len() != 48 {
            bail!(
                "Key file has invalid length (expected 48 bytes, got {})",
                data.len()
            );
        }

        let mut salt = [0u8; 16];
        let mut key = [0u8; 32];
        salt.copy_from_slice(&data[0..16]);
        key.copy_from_slice(&data[16..48]);

        Ok(KeyObject { key, salt })
    }
}

async fn get_png_files_async(dir: &str) -> anyhow::Result<Vec<PathBuf>> {
    let dir_path = Path::new(dir);
    let metadata = smol::fs::metadata(dir_path).await?;
    if !metadata.is_dir() {
        bail!("Path is not a directory: {}", dir);
    }

    let mut entries = smol::fs::read_dir(dir_path).await?;
    let mut png_files = Vec::new();

    while let Some(entry) = entries.next().await {
        let entry = entry?;
        let path = entry.path();
        if let Ok(meta) = entry.metadata().await {
            if meta.is_file() {
                if let Some(ext) = path.extension() {
                    if ext.to_string_lossy().to_lowercase() == "png" {
                        png_files.push(path);
                    }
                }
            }
        }
    }
    png_files.sort();
    Ok(png_files)
}

fn get_output_path(input_path: &str, out_dir: Option<&str>, suffix: &str) -> String {
    let input_path = Path::new(input_path);
    let file_name = input_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("output.png");

    let base_name = file_name.trim_end_matches(".png");
    let new_name = format!("{}{}.png", base_name, suffix);

    if let Some(out_dir) = out_dir {
        Path::new(out_dir)
            .join(&new_name)
            .to_string_lossy()
            .to_string()
    } else {
        input_path
            .parent()
            .map(|p| p.join(&new_name))
            .unwrap_or_else(|| PathBuf::from(&new_name))
            .to_string_lossy()
            .to_string()
    }
}

async fn process_file_encrypt_async(
    input_file: &str,
    output_file: Option<String>,
    out_dir: Option<&str>,
    key: [u8; 32],
    compression_level: CompressionLevel,
    pb: &ProgressBar,
) -> anyhow::Result<()> {
    let image = DecodedPng::read_from_file_async(input_file, None, pb).await?;

    let output = output_file.unwrap_or_else(|| get_output_path(input_file, out_dir, "_encrypted"));

    if let Some(out_dir) = out_dir {
        smol::fs::create_dir_all(out_dir).await?;
    }

    image
        .save_optimized_async(&output, compression_level, Some(key), pb)
        .await?;
    Ok(())
}

async fn process_file_decrypt_async(
    input_file: &str,
    output_file: Option<String>,
    out_dir: Option<&str>,
    key: [u8; 32],
    pb: &ProgressBar,
) -> anyhow::Result<()> {
    let image = DecodedPng::read_from_file_async(input_file, Some(key), pb).await?;

    let output = output_file.unwrap_or_else(|| get_output_path(input_file, out_dir, "_decrypted"));

    if let Some(out_dir) = out_dir {
        smol::fs::create_dir_all(out_dir).await?;
    }

    image
        .save_optimized_async(&output, CompressionLevel::Lossless, None, pb)
        .await?;
    Ok(())
}

const PROGRESS_TEMPLATE: &str = "{spinner:.green} [{elapsed_precise}] {bar:40.cyan/blue} {msg}";

async fn async_main() -> anyhow::Result<()> {
    let args = Args::parse();

    if let Some(password) = args.password {
        let key_path = args
            .key_path
            .ok_or_else(|| anyhow::anyhow!("Key path (-k) required when generating key"))?;
        let key_obj = KeyObject::derive_key_from_password_async(password, None).await;
        key_obj.save_key_to_async(&key_path).await?;
        println!("Key generated and saved to: {}", key_path);
        return Ok(());
    }

    if let Some(dir) = args.directory {
        let key_obj = if let Some(key_path) = args.key_path {
            KeyObject::load_key_async(&key_path).await?
        } else {
            bail!("Key path (-k) required when processing directory");
        };

        let png_files = get_png_files_async(&dir).await?;
        let m = MultiProgress::new();

        if png_files.is_empty() {
            println!("No PNG files found in directory: {}", dir);
            return Ok(());
        }

        println!(
            "Found {} PNG file(s) in directory: {}",
            png_files.len(),
            dir
        );

        // Create output directory if specified
        if let Some(ref out_dir) = args.out_dir {
            smol::fs::create_dir_all(out_dir).await?;
            println!("Output directory: {}", out_dir);
        }

        if !args.encrypt && !args.decrypt {
            bail!("Please specify -e (encrypt) or -d (decrypt) when using --dir");
        }

        let num_cpus = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4);
        let sem = Arc::new(smol::lock::Semaphore::new(num_cpus));
        let is_encrypt = args.encrypt;
        let mut tasks = Vec::with_capacity(png_files.len());

        for file_path in png_files {
            let pb = m.add(ProgressBar::new(7));
            pb.set_style(
                ProgressStyle::with_template(PROGRESS_TEMPLATE)?
                    .tick_chars("⠁⠂⠄⡀⢀⠠⠐⠈ "),
            );
            pb.enable_steady_tick(std::time::Duration::from_millis(100));

            let sem_clone = sem.clone();
            let input_file = file_path.to_string_lossy().to_string();
            let out_dir_clone = args.out_dir.clone();
            let key = key_obj.key;
            let level = args.compression_level.clone();

            tasks.push(smol::spawn(async move {
                let _permit = sem_clone.acquire().await;
                let res = if is_encrypt {
                    process_file_encrypt_async(
                        &input_file,
                        None,
                        out_dir_clone.as_deref(),
                        key,
                        level,
                        &pb,
                    )
                    .await
                } else {
                    process_file_decrypt_async(
                        &input_file,
                        None,
                        out_dir_clone.as_deref(),
                        key,
                        &pb,
                    )
                    .await
                };

                match &res {
                    Ok(_) => {
                        let action = if is_encrypt {
                            "encrypted"
                        } else {
                            "decrypted"
                        };
                        pb.finish_with_message(format!("{} {}.", input_file, action));
                    }
                    Err(_) => {
                        pb.finish_with_message(format!("{} failed!", input_file));
                    }
                }
                (input_file, res)
            }));
        }

        let mut errors = Vec::new();
        for task in tasks {
            let (file, res) = task.await;
            if let Err(e) = res {
                errors.push((file, e));
            }
        }

        if !errors.is_empty() {
            eprintln!("\nFailed to process {} file(s):", errors.len());
            for (file, error) in &errors {
                eprintln!("{}: {}", file, error);
            }
        }
        return Ok(());
    }

    if args.encrypt {
        let input_file = args
            .input_file
            .ok_or_else(|| anyhow::anyhow!("Input file required when encrypting"))?;
        let key_obj = if let Some(key_path) = args.key_path {
            KeyObject::load_key_async(&key_path).await?
        } else {
            bail!("Key file required when encrypting");
        };

        let pb = ProgressBar::new(7);
        pb.set_style(
            ProgressStyle::with_template(PROGRESS_TEMPLATE)?
                .tick_chars("⠁⠂⠄⡀⢀⠠⠐⠈ "),
        );
        pb.enable_steady_tick(std::time::Duration::from_millis(100));

        let output_file = if args.outfile.is_some() {
            args.outfile.unwrap()
        } else {
            format!("{}_encrypted.png", input_file.trim_end_matches(".png"))
        };

        process_file_encrypt_async(
            &input_file,
            Some(output_file),
            None,
            key_obj.key,
            args.compression_level,
            &pb,
        )
        .await?;

        pb.finish();
        return Ok(());
    }

    if args.decrypt {
        let input_file = args
            .input_file
            .ok_or_else(|| anyhow::anyhow!("Input file required when decrypting"))?;
        let key_obj = if let Some(key_path) = args.key_path {
            KeyObject::load_key_async(&key_path).await?
        } else {
            bail!("Key file required when decrypting");
        };

        let pb = ProgressBar::new(7);
        pb.set_style(
            ProgressStyle::with_template(PROGRESS_TEMPLATE)?
                .tick_chars("⠁⠂⠄⡀⢀⠠⠐⠈ "),
        );
        pb.enable_steady_tick(std::time::Duration::from_millis(100));

        let output_file = if args.outfile.is_some() {
            args.outfile.unwrap()
        } else {
            format!("{}_decrypted.png", input_file.trim_end_matches(".png"))
        };

        process_file_decrypt_async(
            &input_file,
            Some(output_file),
            None,
            key_obj.key,
            &pb,
        )
        .await?;

        pb.finish();
        return Ok(());
    }

    bail!("Please specify one of: -g (generate key), -e (encrypt), or -d (decrypt)");
}

fn main() -> anyhow::Result<()> {
    smol::block_on(async_main())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::png::{ImageType, PngInfo};

    #[test]
    fn test_async_key_derivation_and_io() {
        smol::block_on(async {
            let password = "test-password-123".to_string();
            let key_obj = KeyObject::derive_key_from_password_async(password.clone(), None).await;
            assert_eq!(key_obj.key.len(), 32);
            assert_eq!(key_obj.salt.len(), 16);

            let temp_key_path = "target/test_key.bin";
            key_obj.save_key_to_async(temp_key_path).await.unwrap();

            let loaded_key = KeyObject::load_key_async(temp_key_path).await.unwrap();
            assert_eq!(key_obj.key, loaded_key.key);
            assert_eq!(key_obj.salt, loaded_key.salt);

            let _ = smol::fs::remove_file(temp_key_path).await;
        });
    }

    #[test]
    fn test_async_image_roundtrip() {
        smol::block_on(async {
            if !Path::new("d_file.png").exists() {
                return;
            }
            let pb = ProgressBar::hidden();
            let key_obj = KeyObject::derive_key_from_password_async("png-secret".to_string(), None).await;

            let enc_path = "target/test_enc.png";
            let dec_path = "target/test_dec.png";

            process_file_encrypt_async("d_file.png", Some(enc_path.to_string()), None, key_obj.key, CompressionLevel::Lossless, &pb)
                .await
                .unwrap();

            process_file_decrypt_async(enc_path, Some(dec_path.to_string()), None, key_obj.key, &pb)
                .await
                .unwrap();

            let orig = DecodedPng::read_from_file_async("d_file.png", None, &pb).await.unwrap();
            let dec = DecodedPng::read_from_file_async(dec_path, None, &pb).await.unwrap();

            assert_eq!(orig.info.width, dec.info.width);
            assert_eq!(orig.info.height, dec.info.height);
            assert_eq!(orig.rgba, dec.rgba);

            let _ = smol::fs::remove_file(enc_path).await;
            let _ = smol::fs::remove_file(dec_path).await;
        });
    }

    #[test]
    fn test_async_compression_levels() {
        smol::block_on(async {
            let width = 16u32;
            let height = 16u32;
            let mut rgba = Vec::with_capacity((width * height * 4) as usize);
            for y in 0..height {
                for x in 0..width {
                    rgba.extend_from_slice(&[x as u8 * 16, y as u8 * 16, 128, 255]);
                }
            }

            let test_image = DecodedPng {
                info: PngInfo {
                    width,
                    height,
                    bit_depth: 8,
                    color_type: 6,
                    interlace: 0,
                    image_type: ImageType::TruecolorAlpha,
                },
                rgba,
            };

            let pb = ProgressBar::hidden();
            let key = [42u8; 32];

            for level in [CompressionLevel::Lossless, CompressionLevel::Balanced, CompressionLevel::Maximum] {
                let enc_path = format!("target/test_level_{:?}_enc.png", level);
                let dec_path = format!("target/test_level_{:?}_dec.png", level);

                test_image
                    .save_optimized_async(&enc_path, level.clone(), Some(key), &pb)
                    .await
                    .unwrap();

                process_file_decrypt_async(&enc_path, Some(dec_path.clone()), None, key, &pb)
                    .await
                    .unwrap();

                let dec = DecodedPng::read_from_file_async(&dec_path, None, &pb).await.unwrap();
                assert_eq!(dec.info.width, width);
                assert_eq!(dec.info.height, height);

                let _ = smol::fs::remove_file(&enc_path).await;
                let _ = smol::fs::remove_file(&dec_path).await;
            }
        });
    }

    #[test]
    fn test_async_batch_directory_and_scan() {
        smol::block_on(async {
            let test_dir = "target/test_batch_dir";
            let out_dir = "target/test_batch_out";
            let _ = smol::fs::remove_dir_all(test_dir).await;
            let _ = smol::fs::remove_dir_all(out_dir).await;
            smol::fs::create_dir_all(test_dir).await.unwrap();

            if Path::new("d_file.png").exists() {
                smol::fs::copy("d_file.png", format!("{}/img1.png", test_dir)).await.unwrap();
                smol::fs::copy("d_file.png", format!("{}/img2.png", test_dir)).await.unwrap();
            }

            let discovered = get_png_files_async(test_dir).await.unwrap();
            assert_eq!(discovered.len(), 2);

            let key = [7u8; 32];
            let pb = ProgressBar::hidden();

            for file in &discovered {
                process_file_encrypt_async(&file.to_string_lossy(), None, Some(out_dir), key, CompressionLevel::Lossless, &pb)
                    .await
                    .unwrap();
            }

            let encrypted_files = get_png_files_async(out_dir).await.unwrap();
            assert_eq!(encrypted_files.len(), 2);

            let _ = smol::fs::remove_dir_all(test_dir).await;
            let _ = smol::fs::remove_dir_all(out_dir).await;
        });
    }

    #[test]
    fn test_async_invalid_key_error() {
        smol::block_on(async {
            if !Path::new("d_file.png").exists() {
                return;
            }
            let pb = ProgressBar::hidden();
            let key1 = [1u8; 32];
            let key2 = [2u8; 32];

            let enc_path = "target/test_bad_key_enc.png";
            process_file_encrypt_async("d_file.png", Some(enc_path.to_string()), None, key1, CompressionLevel::Lossless, &pb)
                .await
                .unwrap();

            // Attempt decrypting with wrong key
            let res = DecodedPng::read_from_file_async(enc_path, Some(key2), &pb).await;
            assert!(res.is_err());

            let _ = smol::fs::remove_file(enc_path).await;
        });
    }
}