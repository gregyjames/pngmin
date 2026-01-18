use anyhow::bail;

enum CompressionLevel {
    Low,
    Medium,
    High,
}
// https://www.w3.org/TR/png-3/#4Concepts.Encoding
const PNG_SIG: [u8; 8] = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];

fn main() {
    let file = match std::fs::File::open("SailFlow.png") {
        Ok(file) => file,
        Err(e) => panic!("Failed to open file: {}", e),
    };

}
