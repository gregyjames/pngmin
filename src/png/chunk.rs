use std::io::Write;
use byteorder::{BigEndian, WriteBytesExt};
use crc32fast::Hasher;
use anyhow::Result;

use crate::png::constants::*;
use crate::png::ImageType;

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

pub fn parse_image_type(color_type: u8, bit_depth: u8) -> ImageType {
    match (color_type, bit_depth) {
        (0, 1 | 2 | 4 | 8 | 16) => ImageType::Grayscale,
        (2, 8 | 16) => ImageType::Truecolor,
        (3, 1 | 2 | 4 | 8 ) => ImageType::IndexedColor,
        (4, 8 | 16) => ImageType::GrayscaleAlpha,
        (6, 8 | 16) => ImageType::TruecolorAlpha,
        _ => ImageType::Unknown
    }
}