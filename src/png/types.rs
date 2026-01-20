use clap::ValueEnum;

#[derive(Debug)]
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

#[derive(Debug)]
pub struct PngInfo {
    pub width: u32,
    pub height: u32,
    pub bit_depth: u8,
    pub color_type: u8,
    pub interlace: u8,
    pub image_type: ImageType,
}

#[derive(Debug, Clone, Copy)]
pub struct Pixel {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
    pub alpha: u8,
}

#[derive(Debug)]
pub struct DecodedPng {
    pub info: PngInfo,
    pub rgba: Vec<u8>,
}