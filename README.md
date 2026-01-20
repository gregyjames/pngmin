[![Rust](https://github.com/gregyjames/pngmin/actions/workflows/rust.yml/badge.svg)](https://github.com/gregyjames/pngmin/actions/workflows/rust.yml)
![GitHub repo size](https://img.shields.io/github/repo-size/gregyjames/pngmin)

# pngmin
PNG Parser and Compressor

## Rambling
I don't know why I am making this. I got super ADHD today and decided to understand how PNG works so I read through the spec and put this together. Eventually, I want to use this to compress PNGs so I can stop using TinyPNG but that is a bit away, going to look at what magic OxiPNG is doing. Currently just support parsing (reading) and writing PNG files. Overall, this has been pretty fun actually learning more about one of the super common file formats we interact with on the web daily.

### Rambling pt. 2
Okay, finally thought of a cool use case for this: encryption! Since a PNG is basically just a compressed data stream between a header and an end block, the compressed data can be encrypted. Why would you want to do this? Well what if you want to put your pictures on Google Drive/Photos but don't want them to train Gemini 7.5 Ultra Pro Max on your vacation photos without having to update an obscure new security setting every month (totally not mad about it)? If you want to do this, I assume that you need to update the storage mode to lossless, since compressing the image would probably destroy the encrypted data. Anyways, I converted this to a CLI app that allows you to read/encrypt files or directories with Aes256Gcm using a key generated from a password using Argon2ID.

## Usage
### Generate a Key

#### Generate a new encryption key from a password:
The key file contains both the salt and derived key (48 bytes total). Keep this file secure!
```
pngmin -g "your-secure-password" -k master-key.bin
```

#### Encrypt a single PNG file
```
# Creates image_encrypted.png in the same directory
pngmin -e -i image.png -k master-key.bin

# Encrypt with custom output filename
pngmin -e -i image.png -k master-key.bin -o encrypted-image.png
```

#### Decrypt a single PNG file
```
# Decrypt a PNG file (creates image_decrypted.png in the same directory)
pngmin -d -i image_encrypted.png -k master-key.bin

# Decrypt with custom output filename
pngmin -d -i image_encrypted.png -k master-key.bin -o decrypted-image.png
```

#### Encrypt/Decrypt Multiple PNG Files 
```
# Encrypt all PNG files and save to a different directory
pngmin -e --dir ./images -k master-key.bin --out-dir ./encrypted


# Decrypt all PNG files and save to a different directory
pngmin -d --dir ./encrypted -k master-key.bin --out-dir ./decrypted
```

## Command reference
| Flag      | Long Form  | Description                         |
|-----------|------------|-------------------------------------|
| -g        | --generate | Generate a key file from a password |
| -k        | --key      | Path to key file                    |
| -d        | --decrypt  | Encrypt mode                        |
| -e        | --encrypt  | Decrypt mode                        |
| --dir     |            | Input directory                     |
| -o        |            | Output filename                     |
| --out-dir |            | Output directory                    |
| -m        | --level    | Compression Level                   |
| -i        | --input    | Input PNG file                      |

Compression Levels:
- lossless (default) - Compress without quality loss (i.e only optimising alpha channel, using better, slower Zopfli compression)
- balanced - Good balance between quality and file size
- maximum - Maximum compression, may reduce quality



## Current Limiations
- Only RGB/RGBA Support
- No iterlaced image Support
- 8bit support only

## License
MIT License

Copyright (c) 2026 Greg James

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
