use std::io::Write;
use anyhow::{Context, Result};
use byteorder::{BigEndian, WriteBytesExt};
use crc32fast::Hasher;
use flate2::write::ZlibEncoder;
use flate2::Compression;

use crate::png::types::*;
use crate::png::constants::*;
use crate::png::filter::apply_filter;
use crate::png::optimization::optimize_alpha_channel;

impl DecodedPng {
    pub fn save(&self, path: &str) -> Result<()> {
        self.save_optimized(path, CompressionLevel::Balanced)
    }

    pub fn save_optimized(&self, path: &str, compression_level: CompressionLevel) -> Result<()> {
        let width = self.info.width as usize;
        let height = self.info.height as usize;

        let optimized_rgba = optimize_alpha_channel(&self.rgba);

        let has_alpha = optimized_rgba.chunks_exact(4).any(|pixel| pixel[3] != 255);

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

            let filter_type = 0u8; // None filter
            let filtered_row = apply_filter(filter_type, bytes_per_pixel, row_data, prev_row);

            filtered.push(filter_type);
            filtered.extend_from_slice(&filtered_row);

            //todo!("Try and select the best filter here.")
        }

        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&filtered)?;
        let compressed = encoder.finish()?;

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
        write_chunk(&mut file, &IHDR, &ihdr_data)?;

        // Write IDAT chunk
        write_chunk(&mut file, &IDAT, &compressed)?;

        // Write IEND chunk
        write_chunk(&mut file, &IEND, &[])?;

        Ok(())
    }
}

pub fn write_chunk(writer: &mut impl Write, chunk_type: &[u8; 4], data: &[u8]) -> Result<()> {
    let mut hasher = Hasher::new();
    hasher.update(chunk_type);
    hasher.update(data);
    let crc = hasher.finalize();

    writer.write_u32::<BigEndian>(data.len() as u32)?;
    writer.write_all(chunk_type)?;
    writer.write_all(data)?;
    writer.write_u32::<BigEndian>(crc)?;

    Ok(())
}