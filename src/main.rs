use std::io::Read;
use anyhow::bail;
use std::io::Cursor;
use byteorder::{BigEndian, ReadBytesExt};

enum CompressionLevel {
    Low,
    Medium,
    High,
}
// https://www.w3.org/TR/png-3/#4Concepts.Encoding
const PNG_SIG: [u8; 8] = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];
const IHDR: [u8; 4] = [0x49, 0x48, 0x44, 0x52];
const IDAT: [u8; 4] = [0x49, 0x44, 0x41, 0x54];
const IEND: [u8; 4] = [0x49, 0x45, 0x4e, 0x44];

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
            println!("{}x{} {} bit {}, compression: {}, filter: {}, interlact: {}", width, height, bit_depth, color_type, compression, filter, interlace);
        }
        else if chunk_type == IDAT{

        }
        else if chunk_type == IEND{
            break;
        }
        else {
            // ignore
        }
    }

    Ok(())
}
