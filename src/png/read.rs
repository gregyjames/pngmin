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
use crate::png::{derive_key_from_password, parse_image_type};
use zeroize::Zeroizing;

impl DecodedPng {
    #[allow(dead_code)]
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
    pub async fn read_from_file_async(path: &str, password: Option<Zeroizing<String>>, pb: &ProgressBar) -> Result<DecodedPng> {
        pb.set_message(format!("Reading image {}", path));
        let bytes = smol::fs::read(path).await.with_context(|| format!("Could not read file {}", path))?;
        let pb_clone = pb.clone();
        smol::unblock(move || {
            Self::from_bytes(&bytes, password.as_deref().map(String::as_str), &pb_clone)
        }).await
    }

    #[allow(dead_code)]
    pub fn read_from_file(path: &str, password: Option<&str>, pb: &ProgressBar) -> Result<DecodedPng> {
        pb.set_message(format!("Reading image {}", path));
        let mut file = std::fs::File::open(path).with_context(|| format!("Could not open file {}", path))?;
        let mut bytes: Vec<u8> = Vec::new();
        file.read_to_end(&mut bytes).with_context(|| format!("Could not read file {}", path))?;
        Self::from_bytes(&bytes, password, pb)
    }

    pub fn from_bytes(bytes: &[u8], password: Option<&str>, pb: &ProgressBar) -> Result<DecodedPng> {
        let mut cursor = Cursor::new(bytes);
        let mut signature: [u8; 8] = [0u8; 8];
        cursor.read_exact(&mut signature).map_err(|e| anyhow::anyhow!(e.to_string()))?;

        if signature != PNG_SIG {
            bail!("Signature doesn't match PNG signature");
        }

        let mut info: Option<PngInfo> = None;
        let mut salt: Option<[u8; 16]> = None;
        let mut derived_key: Option<EncryptionKey> = None;
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

                if compression != 0 || filter != 0 {
                    bail!("Unsupported compression or filter method in IHDR.");
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
            else if chunk_type == PNGM {
                if data.len() != 16 {
                    bail!("Invalid pnGm salt chunk length: expected 16, got {}", data.len());
                }
                let mut s = [0u8; 16];
                s.copy_from_slice(&data);
                salt = Some(s);
            }
            else if chunk_type == IDAT{
                let decrypted_data = if let Some(pw) = password {
                    if data.len() <= 12 {
                        bail!("Encrypted IDAT chunk is too short");
                    }
                    if derived_key.is_none() {
                        let s = salt.ok_or_else(|| anyhow::anyhow!("Encrypted PNG is missing pnGm salt chunk"))?;
                        derived_key = Some(derive_key_from_password(pw, Some(&s))?);
                    }
                    let key = &derived_key.as_ref().unwrap().key;
                    let cipher = Aes256Gcm::new_from_slice(key).map_err(|e| anyhow::anyhow!(e))?;

                    // Extract nonce (first 12 bytes) and ciphertext (rest)
                    let nonce = Nonce::try_from(&data[..12]).map_err(|e| anyhow::anyhow!(e))?;
                    let ciphertext = &data[12..];

                    // Decrypt
                    cipher.decrypt(&nonce, ciphertext).map_err(|e| anyhow::anyhow!(e))?
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
            let (before, rest) = unfiltered.split_at_mut(dest_row_start);
            let prev = if row == 0 {
                None
            } else {
                Some(&before[dest_row_start - row_bytes..dest_row_start])
            };
            let dest = &mut rest[..row_bytes];

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

#[cfg(test)]
mod tests {
    use super::*;
    use byteorder::WriteBytesExt;

    fn build_minimal_png_with_ihdr(compression: u8, filter: u8) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&PNG_SIG);

        let mut ihdr = Vec::new();
        ihdr.write_u32::<BigEndian>(1).unwrap();
        ihdr.write_u32::<BigEndian>(1).unwrap();
        ihdr.push(8); // bit_depth
        ihdr.push(6); // color_type RGBA
        ihdr.push(compression);
        ihdr.push(filter);
        ihdr.push(0); // interlace

        out.write_u32::<BigEndian>(ihdr.len() as u32).unwrap();
        out.extend_from_slice(&IHDR);
        out.extend_from_slice(&ihdr);
        out.write_u32::<BigEndian>(0).unwrap(); // dummy CRC

        out.write_u32::<BigEndian>(0).unwrap();
        out.extend_from_slice(&IEND);
        out.write_u32::<BigEndian>(0).unwrap();
        out
    }

    #[test]
    fn test_ihdr_rejects_invalid_filter_when_compression_is_zero() {
        let png = build_minimal_png_with_ihdr(0, 1);
        let pb = ProgressBar::hidden();
        let err = DecodedPng::from_bytes(&png, None, &pb).unwrap_err();
        assert!(err.to_string().contains("compression or filter"));
    }

    #[test]
    fn test_ihdr_rejects_invalid_compression_when_filter_is_zero() {
        let png = build_minimal_png_with_ihdr(1, 0);
        let pb = ProgressBar::hidden();
        let err = DecodedPng::from_bytes(&png, None, &pb).unwrap_err();
        assert!(err.to_string().contains("compression or filter"));
    }
}