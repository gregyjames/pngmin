use crate::png::{derive_key_from_password, CompressionLevel, DecodedPng, EncryptionKey};
use anyhow::bail;
use clap::Parser;
use futures_lite::stream::StreamExt;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use zeroize::Zeroizing;
mod png;

#[derive(Parser, Debug)]
struct Args {
    #[arg(short = 'i', long = "input")]
    input_file: Option<String>,

    #[arg(long = "dir")]
    directory: Option<String>,

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

async fn derive_key_from_password_async(
    password: Zeroizing<String>,
    salt: Option<[u8; 16]>,
) -> anyhow::Result<EncryptionKey> {
    smol::unblock(move || derive_key_from_password(&password, salt.as_ref())).await
}

fn prompt_password(confirm: bool) -> anyhow::Result<Zeroizing<String>> {
    let password = Zeroizing::new(rpassword::prompt_password("Enter password: ")?);
    if password.is_empty() {
        bail!("Password cannot be empty");
    }
    if confirm {
        let confirm_pw = Zeroizing::new(rpassword::prompt_password("Confirm password: ")?);
        if password.as_str() != confirm_pw.as_str() {
            bail!("Passwords do not match");
        }
    }
    Ok(password)
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

fn strip_png_extension(name: &str) -> &str {
    name.get(name.len().saturating_sub(4)..)
        .filter(|ext| ext.eq_ignore_ascii_case(".png"))
        .map(|_| &name[..name.len() - 4])
        .unwrap_or(name)
}

fn get_output_path(input_path: &str, out_dir: Option<&str>, suffix: &str) -> String {
    let input_path = Path::new(input_path);
    let file_name = input_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("output.png");

    let base_name = strip_png_extension(file_name);
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
    encryption: EncryptionKey,
    compression_level: CompressionLevel,
    pb: &ProgressBar,
) -> anyhow::Result<()> {
    let image = DecodedPng::read_from_file_async(input_file, None, pb).await?;

    let output = output_file.unwrap_or_else(|| get_output_path(input_file, out_dir, "_encrypted"));

    if let Some(out_dir) = out_dir {
        smol::fs::create_dir_all(out_dir).await?;
    }

    image
        .save_optimized_async(&output, compression_level, Some(encryption), pb)
        .await?;
    Ok(())
}

async fn process_file_decrypt_async(
    input_file: &str,
    output_file: Option<String>,
    out_dir: Option<&str>,
    password: Zeroizing<String>,
    pb: &ProgressBar,
) -> anyhow::Result<()> {
    let image = DecodedPng::read_from_file_async(input_file, Some(password), pb).await?;

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

    if let Some(dir) = args.directory {
        if !args.encrypt && !args.decrypt {
            bail!("Please specify -e (encrypt) or -d (decrypt) when using --dir");
        }

        let png_files = get_png_files_async(&dir).await?;
        let m = MultiProgress::new();

        if png_files.is_empty() {
            println!("No PNG files found in directory: {}", dir);
            return Ok(());
        }

        let password = prompt_password(args.encrypt)?;
        let batch_enc = if args.encrypt {
            Some(derive_key_from_password_async(password.clone(), None).await?)
        } else {
            None
        };

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
            let enc_clone = batch_enc.clone();
            let pw_clone = password.clone();
            let level = args.compression_level.clone();

            tasks.push(smol::spawn(async move {
                let _permit = sem_clone.acquire().await;
                let res = if is_encrypt {
                    process_file_encrypt_async(
                        &input_file,
                        None,
                        out_dir_clone.as_deref(),
                        enc_clone.expect("encryption key derived"),
                        level,
                        &pb,
                    )
                    .await
                } else {
                    process_file_decrypt_async(
                        &input_file,
                        None,
                        out_dir_clone.as_deref(),
                        pw_clone,
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
        let password = prompt_password(true)?;
        let encryption = derive_key_from_password_async(password, None).await?;

        let pb = ProgressBar::new(7);
        pb.set_style(
            ProgressStyle::with_template(PROGRESS_TEMPLATE)?
                .tick_chars("⠁⠂⠄⡀⢀⠠⠐⠈ "),
        );
        pb.enable_steady_tick(std::time::Duration::from_millis(100));

        let output_file = if args.outfile.is_some() {
            args.outfile.unwrap()
        } else {
            format!("{}_encrypted.png", strip_png_extension(&input_file))
        };

        process_file_encrypt_async(
            &input_file,
            Some(output_file),
            None,
            encryption,
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
        let password = prompt_password(false)?;

        let pb = ProgressBar::new(7);
        pb.set_style(
            ProgressStyle::with_template(PROGRESS_TEMPLATE)?
                .tick_chars("⠁⠂⠄⡀⢀⠠⠐⠈ "),
        );
        pb.enable_steady_tick(std::time::Duration::from_millis(100));

        let output_file = if args.outfile.is_some() {
            args.outfile.unwrap()
        } else {
            format!("{}_decrypted.png", strip_png_extension(&input_file))
        };

        process_file_decrypt_async(
            &input_file,
            Some(output_file),
            None,
            password,
            &pb,
        )
        .await?;

        pb.finish();
        return Ok(());
    }

    bail!("Please specify -e (encrypt) or -d (decrypt)");
}

fn main() -> anyhow::Result<()> {
    smol::block_on(async_main())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::png::{ImageType, PngInfo};

    #[test]
    fn test_async_key_derivation_determinism() {
        smol::block_on(async {
            let password = Zeroizing::new("test-password-123".to_string());
            let key1 = derive_key_from_password_async(password.clone(), None)
                .await
                .unwrap();
            assert_eq!(key1.key.len(), 32);
            assert_eq!(key1.salt.len(), 16);

            let key2 = derive_key_from_password_async(password, Some(key1.salt))
                .await
                .unwrap();
            assert_eq!(key1.key, key2.key);
            assert_eq!(key1.salt, key2.salt);
        });
    }

    fn make_test_image() -> DecodedPng {
        let width = 16u32;
        let height = 16u32;
        let mut rgba = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height {
            for x in 0..width {
                rgba.extend_from_slice(&[x as u8 * 16, y as u8 * 16, 128, 255]);
            }
        }
        DecodedPng {
            info: PngInfo {
                width,
                height,
                bit_depth: 8,
                color_type: 6,
                interlace: 0,
                image_type: ImageType::TruecolorAlpha,
            },
            rgba,
        }
    }

    #[test]
    fn test_async_compression_levels_with_embedded_salt() {
        smol::block_on(async {
            let test_image = make_test_image();
            let pb = ProgressBar::hidden();
            let password = Zeroizing::new("png-secret".to_string());
            let enc = derive_key_from_password_async(password.clone(), None)
                .await
                .unwrap();

            for level in [CompressionLevel::Lossless, CompressionLevel::Balanced, CompressionLevel::Maximum] {
                let enc_path = format!("target/test_level_{:?}_enc.png", level);
                let dec_path = format!("target/test_level_{:?}_dec.png", level);

                test_image
                    .clone()
                    .save_optimized_async(&enc_path, level.clone(), Some(enc.clone()), &pb)
                    .await
                    .unwrap();

                process_file_decrypt_async(&enc_path, Some(dec_path.clone()), None, password.clone(), &pb)
                    .await
                    .unwrap();

                let dec = DecodedPng::read_from_file_async(&dec_path, None, &pb).await.unwrap();
                assert_eq!(dec.info.width, test_image.info.width);
                assert_eq!(dec.info.height, test_image.info.height);

                let _ = smol::fs::remove_file(&enc_path).await;
                let _ = smol::fs::remove_file(&dec_path).await;
            }
        });
    }

    #[test]
    fn test_async_invalid_password_error() {
        smol::block_on(async {
            let test_image = make_test_image();
            let pb = ProgressBar::hidden();
            let enc = derive_key_from_password_async(Zeroizing::new("correct-pw".to_string()), None)
                .await
                .unwrap();

            let enc_path = "target/test_bad_pw_enc.png";
            test_image
                .save_optimized_async(enc_path, CompressionLevel::Lossless, Some(enc), &pb)
                .await
                .unwrap();

            let res = DecodedPng::read_from_file_async(
                enc_path,
                Some(Zeroizing::new("wrong-pw".to_string())),
                &pb,
            )
            .await;
            assert!(res.is_err());

            let _ = smol::fs::remove_file(enc_path).await;
        });
    }

    #[test]
    fn test_decrypt_unencrypted_png_fails_without_salt_chunk() {
        smol::block_on(async {
            let test_image = make_test_image();
            let pb = ProgressBar::hidden();
            let plain_bytes = test_image
                .into_encoded_optimized(CompressionLevel::Lossless, None, &pb)
                .unwrap();

            let res = DecodedPng::from_bytes(&plain_bytes, Some("any-pw"), &pb);
            assert!(res.is_err());
        });
    }

    #[test]
    fn test_strip_png_extension_case_insensitive() {
        assert_eq!(strip_png_extension("image.png"), "image");
        assert_eq!(strip_png_extension("image.PNG"), "image");
        assert_eq!(strip_png_extension("image.Png"), "image");
        assert_eq!(strip_png_extension("image.png.png"), "image.png");
        assert_eq!(strip_png_extension("no_ext"), "no_ext");
        assert_eq!(strip_png_extension("café.PNG"), "café");
        assert_eq!(strip_png_extension("😀"), "😀");
    }

    #[test]
    fn test_get_output_path_handles_uppercase_png() {
        assert_eq!(
            get_output_path("/tmp/photo.PNG", None, "_encrypted"),
            "/tmp/photo_encrypted.png"
        );
        assert_eq!(
            get_output_path("/tmp/photo.Png", Some("/out"), "_decrypted"),
            "/out/photo_decrypted.png"
        );
    }
}