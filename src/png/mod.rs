pub mod types;
pub mod constants;
pub mod read;
pub mod write;
pub mod filter;
pub mod optimization;

pub use types::*;

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

pub fn derive_key_from_password(
    password: &str,
    salt: Option<&[u8; 16]>,
) -> anyhow::Result<EncryptionKey> {
    use anyhow::Context;
    use argon2::{Algorithm, Argon2, ParamsBuilder, Version};
    use rand::TryRng;

    let salt = match salt {
        Some(s) => *s,
        None => {
            let mut s = [0u8; 16];
            rand::rng()
                .try_fill_bytes(&mut s)
                .context("failed to generate salt")?;
            s
        }
    };

    let params = ParamsBuilder::new()
        .m_cost(65536)
        .t_cost(100)
        .p_cost(4)
        .output_len(32)
        .build()
        .map_err(|e| anyhow::anyhow!("Invalid Argon2 parameters: {}", e))?;

    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);

    let mut key = [0u8; 32];
    argon2
        .hash_password_into(password.as_bytes(), &salt, &mut key)
        .map_err(|e| anyhow::anyhow!("Argon2 hashing failed: {}", e))?;

    Ok(EncryptionKey { key, salt })
}