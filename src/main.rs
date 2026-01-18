use crate::png::{CompressionLevel, DecodedPng};
use std::time::Instant;
mod png;

fn main() -> anyhow::Result<()> {
    let start_time = Instant::now();
    let image = DecodedPng::read_from_file("SailFlow.png")?;
    let elapsed = start_time.elapsed();
    println!("{:#?}", image.info);
    println!("Reading PNG took: {:#?}", elapsed);

    let start_time = Instant::now();
    image.save_optimized("file.png", CompressionLevel::Maximum)?;
    let elapsed = start_time.elapsed();
    println!("Saving PNG Took: {:?}", elapsed);

    Ok(())
}