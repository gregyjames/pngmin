[![Rust](https://github.com/gregyjames/pngmin/actions/workflows/rust.yml/badge.svg)](https://github.com/gregyjames/pngmin/actions/workflows/rust.yml)

# pngmin
PNG Parser

# Rambling
I don't know why I am making this. I got super ADHD today and decided to understand how PNG works so I read through the spec and put this together. Eventually, I want to use this to compress PNGs so I can stop using TinyPNG but that is a bit away, going to look at what magic OxiPNG is doing. Currently just support parsing (reading) and writing PNG files. Overall, this has been pretty fun actually learning more about one of the super common file formats we interact with on the web daily.

# Current Limiations
- Only RGB/RGBA Support
- No iterlaced image Support
- 8bit support only

# License
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
