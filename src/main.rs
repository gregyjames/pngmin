use std::io::Read;
use anyhow::bail;
use std::io::Cursor;
use byteorder::{BigEndian, ReadBytesExt};
use flate2::read::ZlibDecoder;

#[derive(Debug)]
enum ImageType{
    Grayscale,
    Truecolor,
    IndexedColor,
    GrayscaleAlpha,
    TruecolorAlpha,
    Unknown
}

enum CompressionLevel {
    Low,
    Medium,
    High,
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
    let mut file = match std::fs::File::open("SailFlow.png") {
        Ok(file) => file,
        Err(e) => panic!("Failed to open file: {}", e),
    };

    let mut bytes: Vec<u8> = Vec::new();
    file.read_to_end(&mut bytes).unwrap();

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
        cursor.read_exact(&mut chunk_type).map_err(|e| e.to_string()).unwrap();
        //let chunk_type_str = std::str::from_utf8(&chunk_type).map_err(|e| "Bad chunk type").unwrap().to_string();

        //println!("Chunk type: {}", chunk_type_str);

        let mut data = vec![0u8; length];
        cursor.read_exact(&mut data).map_err(|e| e.to_string()).unwrap();

        let _crc = cursor.read_u32::<BigEndian>().map_err(|e| e.to_string()).unwrap();

        if chunk_type == IHDR{
            if length != 13{
                bail!("Length doesn't match 13 chunk length");
            }
            let mut data_cursor = Cursor::new(data);
            let width = data_cursor.read_u32::<BigEndian>().map_err(|e| e.to_string()).unwrap();
            let height = data_cursor.read_u32::<BigEndian>().map_err(|e| e.to_string()).unwrap();
            let bit_depth = data_cursor.read_u8().map_err(|e| e.to_string()).unwrap();
            let color_type = data_cursor.read_u8().map_err(|e| e.to_string()).unwrap();
            let compression = data_cursor.read_u8().map_err(|e| e.to_string()).unwrap();
            let filter = data_cursor.read_u8().map_err(|e| e.to_string()).unwrap();
            let interlace = data_cursor.read_u8().map_err(|e| e.to_string()).unwrap();

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
    println!("{:?}", info);

    let mut decoder = ZlibDecoder::new(&idat_data[..]);
    let mut raw: Vec<u8> = Vec::new();
    decoder.read_to_end(&mut raw).map_err(|e| e.to_string()).unwrap();

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
        let dest = &mut unfiltered[dest_row_start..dest_row_start + row_bytes];
        //println!("{:?}", filter_type);

        let prev = if row == 0 {
            None
        } else {
            Some(&unfiltered[(row - 1) * row_bytes .. row * row_bytes])
        };

        unfilter_row(filter_type, bytes_per_pixel, source, prev, dest);
    }

    fn unfilter_row(filter_type: u8, bytes_per_pixel: usize, src: &[u8], prev: Option<&[u8]>, dst: &mut [u8]){
        let prev = prev.unwrap_or(&vec![0u8; src.len()]);

        match filter_type {
            0 => { // None
                dst.copy_from_slice(src);
            },
            _ => {
                panic!("Unsupported filter type: {}", filter_type);
            }
        }
    }
    Ok(())
}
