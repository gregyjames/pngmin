use clap::ValueEnum;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageType {
    Grayscale,
    Truecolor,
    IndexedColor,
    GrayscaleAlpha,
    TruecolorAlpha,
    Unknown,
}

#[derive(ValueEnum, Clone, Debug)]
pub enum CompressionLevel{
    Lossless,
    Balanced,
    Maximum
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct PngInfo {
    pub width: u32,
    pub height: u32,
    pub bit_depth: u8,
    pub color_type: u8,
    pub interlace: u8,
    pub image_type: ImageType,
}

#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
pub struct Pixel {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
    pub alpha: u8,
}

#[derive(Debug, Clone)]
pub struct DecodedPng {
    pub info: PngInfo,
    pub rgba: Vec<u8>,
}