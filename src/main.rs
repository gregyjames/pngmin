use std::io::Read;
use anyhow::{bail, Context};
use std::io::Cursor;
use byteorder::{BigEndian, ReadBytesExt};
use flate2::read::ZlibDecoder;

#[derive(Debug)]
pub enum ImageType{
    Grayscale,
    Truecolor,
    IndexedColor,
    GrayscaleAlpha,
    TruecolorAlpha,
    Unknown
}

#[derive(Debug)]
pub struct PngInfo {
    pub width: u32,
    pub height: u32,
    pub bit_depth: u8,
    pub color_type: u8,
    pub interlace: u8,
    pub image_type: ImageType
}

#[derive(Debug, Clone, Copy)]
pub struct Pixel{
    red: u8,
    green: u8,
    blue: u8,
    alpha: u8,
}

#[derive(Debug)]
pub struct DecodedPng {
    pub info: PngInfo,
    pub rgba: Vec<u8>,
    //pixels: Vec<Pixel>,
}

impl DecodedPng {
    pub fn read_from_file(path: &str) -> anyhow::Result<DecodedPng> {
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

            //println!("length: {}", length);

            let mut chunk_type: Vec<u8> = vec![0u8; 4];
            cursor.read_exact(&mut chunk_type).with_context(|| "Could not read chunk type")?;
            //let chunk_type_str = std::str::from_utf8(&chunk_type).map_err(|e| "Bad chunk type").unwrap().to_string();

            //println!("Chunk type: {}", chunk_type_str);

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
                idat_data.extend(&data[..]);
            }
            else if chunk_type == IEND{
                break;
            }
            else {
                // ignore
            }
        }

        let info = info.ok_or("Missing IHDR image info.").unwrap();
        //println!("{:?}", info);

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

        let mut unfiltered = vec![0u8; height * row_bytes];

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
            //println!("{:?}", filter_type);

            unfilter_row(filter_type, bytes_per_pixel, source, prev, dest);
        }

        //let mut pixels: Vec<Pixel> = Vec::with_capacity(width * height);

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

        let image = DecodedPng{
            info,
            rgba,
        };

        Ok(image)
    }
    pub fn get(&self, x: u32, y: u32) -> Pixel {
        let w = self.info.width as usize;
        let x = x as usize;
        let y = y as usize;

        let i = y * w + x;
        let base = i + 4;

        Pixel {
            red: self.rgba[base],
            green: self.rgba[base + 1],
            blue: self.rgba[base + 2],
            alpha: self.rgba[base + 3],
        }
    }
}

// https://www.w3.org/TR/png-3/#4Concepts.Encoding
const PNG_SIG: [u8; 8] = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];
const IHDR: [u8; 4] = [0x49, 0x48, 0x44, 0x52];
const IDAT: [u8; 4] = [0x49, 0x44, 0x41, 0x54];
const IEND: [u8; 4] = [0x49, 0x45, 0x4e, 0x44];

fn parse_image_type(color_type: u8, bit_depth: u8) -> ImageType {
    match (color_type, bit_depth) {
        (0, 1 | 2 | 4 | 8 | 16) => ImageType::Grayscale,
        (2, 8 | 16) => ImageType::Truecolor,
        (3, 1 | 2 | 4 | 8 ) => ImageType::IndexedColor,
        (4, 8 | 16) => ImageType::GrayscaleAlpha,
        (6, 8 | 16) => ImageType::TruecolorAlpha,
        _ => ImageType::Unknown
    }
}
fn main() -> anyhow::Result<()> {
    let image = DecodedPng::read_from_file("SailFlow.png");
    println!("image: {:?}", image);

    Ok(())
}

fn paeth_predictor(a: u8, b: u8, c: u8) -> u8{
    //convert to i32 since we may need negatives here for abs
    let a = a as i32;
    let b = b as i32;
    let c = c as i32;

    let p = a + b - c;
    let pa = (p - a).abs();
    let pb = (p - b).abs();
    let pc = (p - c).abs();

    if pa <= pb && pa <= pc {
        a as u8
    } else if pb <= pc {
        b as u8
    }
    else {
        c as u8
    }
}
fn unfilter_row(filter_type: u8, bytes_per_pixel: usize, src: &[u8], prev: Option<Vec<u8>>, dst: &mut [u8]){
    let prev = prev.unwrap_or(vec![0u8; src.len()]);

    match filter_type {
        0 => { // None
            dst.copy_from_slice(src);
        },
        1 => { // Sub
            for i in 0..src.len() {
                let left = if i >= bytes_per_pixel { dst[i - bytes_per_pixel] } else { 0 };
                dst[i] = src[i].wrapping_add(left);
            }
        },
        2 => { // Up
            // Recon(x) = Filt(x) + Recon(b)
            for i in 0..src.len() {
                dst[i] = src[i].wrapping_add(prev[i]);
            }
        },
        3 => { // Avg
            // Recon(x) = Filt(x) + floor((Recon(a) + Recon(b)) / 2)
            for i in 0..src.len() {
                let left = if i >= bytes_per_pixel { dst[i - bytes_per_pixel] } else { 0 };
                let up = prev[i];
                let avg = ((left as u16 + up as u16) / 2) as u8;
                dst[i] = src[i].wrapping_add(avg);
            }
        },
        4 => { // Paeth
            for i in 0..src.len() {
                let top = prev[i];
                let left = if i >= bytes_per_pixel { dst[i - bytes_per_pixel] } else { 0 };
                let top_left = if i >= bytes_per_pixel { prev[i - bytes_per_pixel] } else { 0 };
                let p = paeth_predictor(left, top, top_left);
                dst[i] = src[i].wrapping_add(p);
            }
        }
        _ => {
            panic!("Unsupported filter type: {}", filter_type);
        }
    }
}
