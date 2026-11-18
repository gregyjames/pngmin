use std::io::Write;
use std::num::NonZeroU64;
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use aes_gcm::aead::{Aead, Generate};
use anyhow::{Context, Result};
use byteorder::{BigEndian, WriteBytesExt};
use crc32fast::Hasher;
use flate2::Compression;
use flate2::write::ZlibEncoder;
use indicatif::ProgressBar;
use zopfli::{compress, Format, Options};

use crate::png::types::*;
use crate::png::constants::*;
use crate::png::optimization::{choose_best_filter_into, prepare_rgba_in_place, try_build_palette};

impl DecodedPng {
    pub fn encode_optimized(&self, compression_level: CompressionLevel, encryption: Option<&EncryptionKey>, pb: &ProgressBar) -> Result<Vec<u8>> {
        self.clone()
            .into_encoded_optimized(compression_level, encryption, pb)
    }

    pub fn into_encoded_optimized(
        mut self,
        compression_level: CompressionLevel,
        encryption: Option<&EncryptionKey>,
        pb: &ProgressBar,
    ) -> Result<Vec<u8>> {
        let width = self.info.width as usize;
        let height = self.info.height as usize;

        pb.set_message("Optimizing image...");
        let quant_bits = match compression_level {
            CompressionLevel::Lossless => None,
            CompressionLevel::Balanced => Some(6),
            CompressionLevel::Maximum => Some(4),
        };
        let has_alpha = prepare_rgba_in_place(&mut self.rgba, quant_bits);
        let palette_data = try_build_palette(&self.rgba);

        pb.inc(1);

        let (color_type, bytes_per_pixel, image_data, plte_trns) =
            if let Some((plte, trns, indices)) = palette_data {
                drop(self.rgba);
                (3u8, 1usize, indices, Some((plte, trns)))
            } else if has_alpha {
                (6u8, 4usize, self.rgba, None)
            } else {
                let pixel_count = width * height;
                for i in 0..pixel_count {
                    let src = i * 4;
                    let dst = i * 3;
                    self.rgba[dst] = self.rgba[src];
                    self.rgba[dst + 1] = self.rgba[src + 1];
                    self.rgba[dst + 2] = self.rgba[src + 2];
                }
                self.rgba.truncate(pixel_count * 3);
                (2u8, 3usize, self.rgba, None)
            };

        let row_bytes = width * bytes_per_pixel;

        // Apply filters and build filtered scanlines
        pb.set_message("Applying optimal filters...");
        let mut filtered = Vec::with_capacity(height * (1 + row_bytes));
        let mut best_row = Vec::with_capacity(row_bytes);
        let mut candidate_row = Vec::with_capacity(row_bytes);
        for row in 0..height {
            let row_start = row * row_bytes;
            let row_data = &image_data[row_start..row_start + row_bytes];

            let prev_row = if row == 0 {
                None
            } else {
                let prev_start = (row - 1) * row_bytes;
                Some(&image_data[prev_start..prev_start + row_bytes])
            };

            let filter_type = choose_best_filter_into(
                row_data,
                prev_row,
                bytes_per_pixel,
                &mut best_row,
                &mut candidate_row,
            );
            filtered.push(filter_type);
            filtered.extend_from_slice(&best_row);
        }
        pb.inc(1);

        pb.set_message("Compressing image...");
        let mut compressed = Vec::new();

        match compression_level {
            CompressionLevel::Lossless | CompressionLevel::Balanced => {
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
        let mut output_bytes = Vec::new();

        // Write PNG signature
        output_bytes.write_all(&PNG_SIG)?;

        // Write IHDR chunk
        let mut ihdr_data = Vec::new();
        ihdr_data.write_u32::<BigEndian>(self.info.width)?;
        ihdr_data.write_u32::<BigEndian>(self.info.height)?;
        ihdr_data.write_u8(8)?; // bit_depth
        ihdr_data.write_u8(color_type)?;
        ihdr_data.write_u8(0)?; // compression
        ihdr_data.write_u8(0)?; // filter
        ihdr_data.write_u8(0)?; // interlace
        write_chunk(&mut output_bytes, &IHDR, &ihdr_data, None)?;

        // Write PLTE (and optional tRNS) for indexed-color images
        if let Some((plte, trns)) = plte_trns {
            write_chunk(&mut output_bytes, &PLTE, &plte, None)?;
            if !trns.is_empty() {
                write_chunk(&mut output_bytes, &TRNS, &trns, None)?;
            }
        }

        // Write pnGm salt chunk if encrypting
        if let Some(enc) = encryption {
            write_chunk(&mut output_bytes, &PNGM, &enc.salt, None)?;
        }

        // Write IDAT chunk
        write_chunk(
            &mut output_bytes,
            &IDAT,
            &compressed,
            encryption.map(|enc| &enc.key),
        )?;

        // Write IEND chunk
        write_chunk(&mut output_bytes, &IEND, &[], None)?;
        pb.inc(1);

        Ok(output_bytes)
    }

    pub async fn save_optimized_async(self, path: &str, compression_level: CompressionLevel, encryption: Option<EncryptionKey>, pb: &ProgressBar) -> Result<()> {
        let pb_clone = pb.clone();
        let encoded_bytes = smol::unblock(move || {
            self.into_encoded_optimized(compression_level, encryption.as_ref(), &pb_clone)
        }).await?;

        smol::fs::write(path, &encoded_bytes)
            .await
            .with_context(|| format!("Could not write file {}", path))?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn save_optimized(&self, path: &str, compression_level: CompressionLevel, encryption: Option<&EncryptionKey>, pb: &ProgressBar) -> Result<()> {
        let encoded_bytes = self.encode_optimized(compression_level, encryption, pb)?;
        let mut file = std::fs::File::create(path).with_context(|| format!("Could not create file {}", path))?;
        file.write_all(&encoded_bytes)?;
        Ok(())
    }
}

pub fn write_chunk(writer: &mut impl Write, chunk_type: &[u8; 4], data: &[u8], encryption_key: Option<&[u8; 32]>) -> Result<()> {
    let data_to_write = if let Some(encryption_key) = encryption_key {
        let cipher = Aes256Gcm::new_from_slice(encryption_key).map_err(|e| anyhow::anyhow!(e))?;
        let nonce = Nonce::generate();
        let cipher_text = cipher
            .encrypt(&nonce, data)
            .map_err(|e| anyhow::anyhow!(e))?;

        let mut encrypted_data = Vec::with_capacity(12 + cipher_text.len());
        encrypted_data.extend_from_slice(nonce.as_slice());
        encrypted_data.extend_from_slice(&cipher_text);

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lossless_optimizes_transparent_pixels_and_preserves_visible() {
        let img = DecodedPng {
            info: PngInfo {
                width: 2,
                height: 1,
                bit_depth: 8,
                color_type: 6,
                interlace: 0,
                image_type: ImageType::TruecolorAlpha,
            },
            rgba: vec![
                123, 45, 67, 0,   // fully transparent: RGB should be zeroed
                10, 20, 30, 128,  // semi-transparent: preserved exactly
            ],
        };

        let pb = ProgressBar::hidden();
        let encoded = img
            .encode_optimized(CompressionLevel::Lossless, None, &pb)
            .unwrap();
        let decoded = DecodedPng::from_bytes(&encoded, None, &pb).unwrap();

        assert_eq!(decoded.info.color_type, 3);
        assert_eq!(decoded.rgba, vec![0, 0, 0, 0, 10, 20, 30, 128]);
    }

    #[test]
    fn test_auto_plte_opaque_and_fallback_over_256_colors() {
        let pb = ProgressBar::hidden();

        // Exactly 256 unique opaque colors -> indexed (color_type = 3)
        let mut rgba_256 = Vec::with_capacity(256 * 4);
        for i in 0..=255u8 {
            rgba_256.extend_from_slice(&[i, i.wrapping_mul(3), 42, 255]);
        }
        let img_256 = DecodedPng {
            info: PngInfo {
                width: 16,
                height: 16,
                bit_depth: 8,
                color_type: 6,
                interlace: 0,
                image_type: ImageType::TruecolorAlpha,
            },
            rgba: rgba_256.clone(),
        };
        let enc_256 = img_256
            .encode_optimized(CompressionLevel::Lossless, None, &pb)
            .unwrap();
        let dec_256 = DecodedPng::from_bytes(&enc_256, None, &pb).unwrap();
        assert_eq!(dec_256.info.color_type, 3);
        assert_eq!(dec_256.rgba, rgba_256);

        // 257 unique opaque colors -> falls back to RGB (color_type = 2)
        let mut rgba_257 = rgba_256;
        rgba_257.extend_from_slice(&[1, 2, 3, 255]);
        let img_257 = DecodedPng {
            info: PngInfo {
                width: 257,
                height: 1,
                bit_depth: 8,
                color_type: 6,
                interlace: 0,
                image_type: ImageType::TruecolorAlpha,
            },
            rgba: rgba_257.clone(),
        };
        let enc_257 = img_257
            .encode_optimized(CompressionLevel::Lossless, None, &pb)
            .unwrap();
        let dec_257 = DecodedPng::from_bytes(&enc_257, None, &pb).unwrap();
        assert_eq!(dec_257.info.color_type, 2);
        assert_eq!(dec_257.rgba, rgba_257);
    }
}