use crate::png::DecodedPng;

mod png;

fn main() -> anyhow::Result<()> {
    let image = DecodedPng::read_from_file("SailFlow.png")?;
    println!("{:#?}", image.info);
    image.save("file.png")?;

    Ok(())
}