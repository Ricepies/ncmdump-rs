# ncmdump-rs

A fast, memory-safe Rust implementation of `ncmdump`, converting NetEase Cloud Music encrypted dump files (`.ncm`) to `.mp3` or `.flac` with full ID3v2 / FLAC Vorbis comments metadata and album cover artwork.

## Features

- **Pure Rust Standalone Binary**: Statically bundles all cipher, parsing, and tagging logic into a single executable without C shared libraries or external C/C++ dependencies.
- **Cross-platform**: Native builds for Linux, macOS (`aarch64-apple-darwin`), and Windows.
- **CLI Options**:
  - `-h, --help`: Print usage
  - `-v, --version`: Print version
  - `-d, --directory <dir>`: Process files in directory
  - `-r, --recursive`: Process recursively (requires `-d`)
  - `-o, --output <dir>`: Output folder
  - `-m, --remove`: Remove source files after conversion
  - `<files>`: Input `.ncm` files

## Usage

```bash
# Print help
ncmdump -h

# Convert files
ncmdump 1.ncm 2.ncm -o /path/to/output

# Process folder recursively
ncmdump -d /path/to/music -r -o /path/to/output
```

## Building

### Standalone macOS Apple Silicon (`aarch64-apple-darwin`)
```bash
cargo build --target aarch64-apple-darwin --release
```
Binary output:
`/home/ricepies/fun/ncmdump-rs/target/aarch64-apple-darwin/release/ncmdump`
