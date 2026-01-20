use std::io::Write;
use std::num::NonZeroU64;
use aes_gcm::{AeadCore, Aes256Gcm, KeyInit};
use aes_gcm::aead::{Aead, OsRng};
use anyhow::{Context, Result};
use byteorder::{BigEndian, WriteBytesExt};
use crc32fast::Hasher;
use flate2::Compression;
use flate2::write::ZlibEncoder;
use indicatif::ProgressBar;
use zopfli::{compress, Format, Options};

use crate::png::types::*;
use crate::png::constants::*;
use crate::png::optimization::{choose_best_filter, optimize_alpha_channel, quantize_colors};

impl DecodedPng {
    pub fn save_optimized(&self, path: &str, compression_level: CompressionLevel, encryption_key: Option<&[u8; 32]>, pb: &ProgressBar) -> Result<()> {
        let width = self.info.width as usize;
        let height = self.info.height as usize;

        pb.set_message("Optimizing image...");
        let quantized_rgba = match compression_level {
            CompressionLevel::Lossless => {
                &self.rgba[..]
            },
            CompressionLevel::Balanced => {
                &quantize_colors(&self.rgba, 6)
            },
            CompressionLevel::Maximum => {
                &quantize_colors(&self.rgba, 4)
            }
        };

        let optimized_rgba = optimize_alpha_channel(quantized_rgba);

        let has_alpha = optimized_rgba.chunks_exact(4).any(|pixel| pixel[3] != 255);

        pb.inc(1);

        let (color_type, bytes_per_pixel) = if has_alpha {
            (6u8, 4usize) // RGBA
        } else {
            (2u8, 3usize) // RGB
        };

        let row_bytes = width * bytes_per_pixel;
        let mut image_data = Vec::with_capacity(height * row_bytes);

        for y in 0..height {
            for x in 0..width {
                let idx = (y * width + x) * 4;
                image_data.push(optimized_rgba[idx]);     // R
                image_data.push(optimized_rgba[idx + 1]); // G
                image_data.push(optimized_rgba[idx + 2]); // B
                if bytes_per_pixel == 4 {
                    image_data.push(optimized_rgba[idx + 3]); // A
                }
            }
        }

        // Apply filters and build filtered scanlines
        pb.set_message("Applying optimal filters...");
        let mut filtered = Vec::with_capacity(height * (1 + row_bytes));
        for row in 0..height {
            let row_start = row * row_bytes;
            let row_data = &image_data[row_start..row_start + row_bytes];

            let prev_row = if row == 0 {
                None
            } else {
                let prev_start = (row - 1) * row_bytes;
                Some(&image_data[prev_start..prev_start + row_bytes])
            };

            //let filter_type = 0u8; // None filter
            //let filtered_row = apply_filter(filter_type, bytes_per_pixel, row_data, prev_row);

            let (filter_type, filtered_row) = choose_best_filter(row_data, prev_row, bytes_per_pixel);
            filtered.push(filter_type);
            filtered.extend_from_slice(&filtered_row);
        }
        pb.inc(1);

        pb.set_message("Compressing image...");
        let mut compressed = Vec::new();

        match compression_level {
            CompressionLevel::Lossless => {
                let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
                encoder.write_all(&filtered)?;
                compressed = encoder.finish()?;
            },
            CompressionLevel::Balanced => {
                let mut encoder = ZlibEncoder::new(Vec::new(), Compression::best());
                encoder.write_all(&filtered)?;
                compressed = encoder.finish()?;
            },
            CompressionLevel::Maximum => {
                let options = Options{
                    iteration_count: NonZeroU64::new(100).unwrap(),
                    iterations_without_improvement: NonZeroU64::new(u64::MAX).unwrap(),
                    maximum_block_splits: 0
                };
                compress(options, Format::Zlib, &filtered[..], &mut compressed)?;
            }
        }

        pb.inc(1);

        pb.set_message("Writing image...");
        let mut file = std::fs::File::create(path).with_context(|| format!("Could not create file {}", path))?;

        // Write PNG signature
        file.write_all(&PNG_SIG)?;

        // Write IHDR chunk
        let mut ihdr_data = Vec::new();
        ihdr_data.write_u32::<BigEndian>(self.info.width)?;
        ihdr_data.write_u32::<BigEndian>(self.info.height)?;
        ihdr_data.write_u8(8)?; // bit_depth
        ihdr_data.write_u8(color_type)?;
        ihdr_data.write_u8(0)?; // compression
        ihdr_data.write_u8(0)?; // filter
        ihdr_data.write_u8(0)?; // interlace
        write_chunk(&mut file, &IHDR, &ihdr_data, None)?;

        // Write IDAT chunk
        write_chunk(&mut file, &IDAT, &compressed, encryption_key)?;

        // Write IEND chunk
        write_chunk(&mut file, &IEND, &[], None)?;
        pb.inc(1);

        Ok(())
    }
}

pub fn write_chunk(writer: &mut impl Write, chunk_type: &[u8; 4], data: &[u8], encryption_key: Option<&[u8; 32]>) -> Result<()> {
    let data_to_write = if let Some(encryption_key) = encryption_key {
        let cipher = Aes256Gcm::new_from_slice(encryption_key).map_err(|e| anyhow::anyhow!(e))?;
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
        let ciphertext = cipher.encrypt(&nonce, data).map_err(|e| anyhow::anyhow!(e))?;

        let mut encrypted_data = Vec::with_capacity(12 + ciphertext.len());
        encrypted_data.extend_from_slice(nonce.as_slice());
        encrypted_data.extend_from_slice(&ciphertext);

        encrypted_data
    } else {
        data.to_vec()
    };

    let mut hasher = Hasher::new();
    hasher.update(chunk_type);
    hasher.update(&data_to_write);
    let crc = hasher.finalize();

    writer.write_u32::<BigEndian>(data_to_write.len() as u32)?;
    writer.write_all(chunk_type)?;
    writer.write_all(&data_to_write)?;
    writer.write_u32::<BigEndian>(crc)?;

    Ok(())
}