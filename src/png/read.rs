use std::io::{Read, Cursor};
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use aes_gcm::aead::Aead;
use anyhow::{bail, Context, Result};
use byteorder::{BigEndian, ReadBytesExt};
use flate2::read::ZlibDecoder;
use indicatif::ProgressBar;
use crate::png::types::*;
use crate::png::constants::*;
use crate::png::filter::unfilter_row;
use crate::png::parse_image_type;

impl DecodedPng {
    pub fn get(&self, x: u32, y: u32) -> Pixel {
        let w = self.info.width as usize;
        let x = x as usize;
        let y = y as usize;

        let i = y * w + x;
        let base = i * 4;

        Pixel {
            red: self.rgba[base],
            green: self.rgba[base + 1],
            blue: self.rgba[base + 2],
            alpha: self.rgba[base + 3],
        }
    }
    pub fn read_from_file(path: &str, decryption_key: Option<&[u8; 32]>, pb: &ProgressBar) -> Result<DecodedPng> {
        pb.set_message(format!("Reading image {}", path));
        let mut file = std::fs::File::open(path).with_context(|| format!("Could not open file {}", path))?;

        let mut bytes: Vec<u8> = Vec::new();
        file.read_to_end(&mut bytes).with_context(|| format!("Could not read file {}", path))?;

        let mut cursor = Cursor::new(bytes);
        let mut signature: [u8; 8] = [0u8; 8];
        cursor.read_exact(&mut signature).map_err(|e| e.to_string()).unwrap();

        if signature != PNG_SIG {
            bail!("Signature doesn't match PNG signature");
        }

        let mut info: Option<PngInfo> = None;
        let mut idat_data: Vec<u8> = Vec::new();

        loop {
            let length = match cursor.read_u32::<BigEndian>() {
                Ok(length) => length as usize,
                Err(_) => break,
            };

            let mut chunk_type: Vec<u8> = vec![0u8; 4];
            cursor.read_exact(&mut chunk_type).with_context(|| "Could not read chunk type")?;

            let mut data = vec![0u8; length];
            cursor.read_exact(&mut data).with_context(|| "Could not read data")?;

            let _crc = cursor.read_u32::<BigEndian>().with_context(|| "Could not read CRC")?;

            if chunk_type == IHDR{
                if length != 13{
                    bail!("Length doesn't match 13 chunk length");
                }
                let mut data_cursor = Cursor::new(data);
                let width = data_cursor.read_u32::<BigEndian>().with_context(|| "Could not read width")?;
                let height = data_cursor.read_u32::<BigEndian>().with_context(|| "Could not read height")?;
                let bit_depth = data_cursor.read_u8().with_context(|| "Could not read bit_depth")?;
                let color_type = data_cursor.read_u8().with_context(|| "Could not read color type")?;
                let compression = data_cursor.read_u8().with_context(|| "Could not read compression")?;
                let filter = data_cursor.read_u8().with_context(|| "Could not read filter type")?;
                let interlace = data_cursor.read_u8().with_context(|| "Could not read interlace")?;

                if compression != 0 && filter != 0 {
                    bail!("Unsupported compression format for image data.");
                }
                if interlace != 0 {
                    bail!("Interlaced PNG not supported in this minimal decoder");
                }
                if bit_depth != 8 {
                    bail!("Only 8-bit PNG supported in this minimal decoder");
                }
                if color_type != 2 && color_type != 6 {
                    bail!("Only color types 2 (RGB) and 6 (RGBA) supported");
                }
                info = Some(PngInfo{
                    width,
                    height,
                    bit_depth,
                    color_type,
                    interlace,
                    image_type: parse_image_type(color_type, bit_depth)
                })
            }
            else if chunk_type == IDAT{
                let decrypted_data = if decryption_key.is_some() && data.len() > 12 {
                    let key = decryption_key.unwrap();
                    let cipher = Aes256Gcm::new_from_slice(key).map_err(|e| anyhow::anyhow!(e))?;

                    // Extract nonce (first 12 bytes) and ciphertext (rest)
                    let nonce = Nonce::from_slice(&data[..12]);
                    let ciphertext = &data[12..];

                    // Decrypt
                    cipher.decrypt(nonce, ciphertext).map_err(|e| anyhow::anyhow!(e))?
                } else {
                    data
                };
                idat_data.extend(&decrypted_data[..]);
            }
            else if chunk_type == IEND{
                break;
            }
            else {
                // ignore
            }
        }

        let info = info.ok_or("Missing IHDR image info.").unwrap();

        let mut decoder = ZlibDecoder::new(&idat_data[..]);
        let mut raw: Vec<u8> = Vec::new();
        decoder.read_to_end(&mut raw).with_context(|| "Could not read file")?;

        // Truecolor with alpha: red, green, blue, alpha.
        // Truecolor: red, green, blue

        let bytes_per_pixel = match info.image_type {
            ImageType::TruecolorAlpha => 4, // RGBA
            ImageType::Truecolor => 3,      // RGB
            _ => panic!("Unsupported color type")
        };

        let width = info.width as usize;
        let height = info.height as usize;
        let row_bytes = width * bytes_per_pixel;
        let expected = height * (1 +row_bytes); // 7.3 there is one filter byte per row

        if raw.len() != expected {
            bail!("Decompressed image data doesn't match expected image data.");
        }

        pb.inc(1);

        let mut unfiltered = vec![0u8; height * row_bytes];

        pb.set_message("Unfilting rows in image...");
        for row in 0..height {
            let start = row * (1 +row_bytes);
            let filter_type = raw[start];
            let source = &raw[start + 1 .. start + 1 + row_bytes];

            let dest_row_start = row * row_bytes;

            let prev = if row == 0 {
                None
            } else {
                let prev_start = (row - 1) * row_bytes;
                let prev_data = unfiltered[prev_start..prev_start + row_bytes].to_vec();
                Some(prev_data)
            };

            let dest = &mut unfiltered[dest_row_start..dest_row_start + row_bytes];

            unfilter_row(filter_type, bytes_per_pixel, source, prev, dest);
        }
        pb.inc(1);

        //let mut pixels: Vec<Pixel> = Vec::with_capacity(width * height);

        pb.set_message("Decompressed image data...");
        let mut rgba = Vec::with_capacity(width * height * 4);
        match info.image_type {
            ImageType::Truecolor => {
                for i in 0..(width * height) {
                    let r = unfiltered[i * 3];
                    let g = unfiltered[i * 3 + 1];
                    let b = unfiltered[i * 3 + 2];
                    rgba.extend_from_slice(&[r, g, b, 255]);
                }
            },
            ImageType::TruecolorAlpha => {
                rgba = unfiltered;
            },
            _ => unreachable!()
        }
        pb.inc(1);

        let image = DecodedPng{
            info,
            rgba,
        };

        Ok(image)
    }
}