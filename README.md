# ncmdump-rs

[![License: AGPL-3.0-or-later](https://img.shields.io/badge/License-AGPL_3.0--or--later-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-2021%20edition-orange.svg)](https://www.rust-lang.org)

A fast, memory-safe, standalone Rust implementation of `ncmdump` for decrypting NetEase Cloud Music encrypted files (`.ncm`) into clean `.flac` or `.mp3` audio files, with full metadata tagging, automated cover art resolution, and direct in-tag time-synced lyrics fusing.

---

## Attribution & Origins

This project is derived from and inspired by the original C++ work of **[taurusxin/ncmdump](https://github.com/taurusxin/ncmdump)** by [taurusxin](https://github.com/taurusxin).

`ncmdump-rs` reimagines the utility in modern, memory-safe pure Rust, adding high-performance multi-threaded batch processing, intelligent cover art lookup, NetEase hybrid JSON lyrics normalization, direct in-tag synced lyrics embedding, and audio library post-processing capabilities.

---

## Key Features

- **🚀 High Performance & Multi-threaded**: Uses Rust's thread-pool parallelism (`-j / --threads`) and optimized lookup-table byte substitutions for maximum conversion throughput.
- **🛡️ Memory Safe & Zero External C Dependencies**: Pure Rust codebase statically compiling to a self-contained binary without requiring OpenSSL, TagLib, or external C/C++ shared libraries.
- **🎵 In-Tag Synced Lyrics Fusing**:
  - Automatically resolves lyrics files (`.lrc`) matching the track stem or music ID.
  - Seamlessly parses and converts NetEase hybrid JSON lyrics (`{"t": ..., "c": [{"tx": "..."}]}`) and full API JSON into standard centisecond LRC timestamps (`[mm:ss.xx]`).
  - Fuses time-synced lyrics **directly** into FLAC Vorbis comments (`LYRICS` tag) and MP3 ID3v2 (`USLT` frame). **No standalone `.lrc` sidecar files required or left behind!**
- **🖼️ Smart Album Artwork Resolution**:
  - Extracts and embeds native artwork from `.ncm` container headers.
  - Automatically resolves external high-resolution artwork from local `meta/` directories (`meta/track-<id>.jpg`, `meta/<id>.jpg`, `meta/<stem>.jpg`, etc.) located next to the source files or in the output folder.
  - Gracefully handles duplicate filename suffixes (such as macOS `Song (1).ncm`) to find the parent cover artwork and lyrics.
  - Option `--prefer-meta-cover` to prioritize external high-res artwork over embedded thumbnails.
- **✨ Full Audio Processing (`-a, --all`)**:
  - Can scan and process existing non-encrypted `.flac` and `.mp3` files to embed missing cover art and fuse synced lyrics directly.
- **🔍 Metadata Inspector (`-i, --info`)**:
  - Quickly inspects embedded track metadata (title, artist, album, bitrate, duration, format) without decrypting audio streams.
- **🧹 Clean Source Removal (`-m, --remove`)**:
  - Automatically deletes source `.ncm` and `.lrc` files after a successful conversion.

---

## Installation & Building

### Prerequisites

Ensure you have the Rust toolchain installed:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

### Build from Source

```bash
git clone https://github.com/Ricepies/ncmdump-rs.git
cd ncmdump-rs
cargo build --release
```

The optimized binary will be located at `target/release/ncmdump`.

### Cross-Compiling for macOS Apple Silicon (`aarch64-apple-darwin`)

To build a standalone Mach-O 64-bit binary for Apple Silicon (M1/M2/M3/M4):

```bash
rustup target add aarch64-apple-darwin
cargo build --target aarch64-apple-darwin --release
```

The binary will be generated at `target/aarch64-apple-darwin/release/ncmdump`.

---

## CLI Usage

```text
USAGE:
  ncmdump [OPTIONS] [FILES/DIRS...]

OPTIONS:
  -h, --help               Print this help message
  -v, --version            Print version information
  -d, --directory <DIR>    Process files in a directory
  -r, --recursive          Process directory recursively (requires -d or dir argument)
  -a, --all                Also process existing .mp3 and .flac files (embed cover & fuse lyrics)
  -o, --output <DIR>       Output directory (default: same as source file)
  -m, --remove             Remove source .ncm (and .lrc) files after successful conversion
  -i, --info               Inspect metadata without decrypting audio
  -j, --threads <NUM>      Number of parallel conversion threads (default: CPU cores)
      --meta <DIR>         Custom meta folder path containing track-<id>.jpg artwork
      --prefer-meta-cover  Prefer external meta/ cover over embedded .ncm cover
      --no-lrc             Do not search for, embed, or save .lrc lyrics
      --no-overwrite       Skip conversion if output file already exists
```

### Examples

**Convert specific `.ncm` files:**
```bash
ncmdump song1.ncm song2.ncm -o /path/to/music
```

**Recursively convert an entire directory using all CPU cores:**
```bash
ncmdump -d /path/to/downloaded/ncm -r -o /path/to/music
```

**Convert `.ncm` files and also process existing `.flac` / `.mp3` files to embed covers and fuse lyrics:**
```bash
ncmdump -d /path/to/music -r -a -o /path/to/music
```

**Convert and clean up source `.ncm` and `.lrc` files:**
```bash
ncmdump -d /path/to/music -r -m -o /path/to/music
```

**Inspect metadata without decrypting:**
```bash
ncmdump -i song.ncm
```

---

## License

This program is free software: you can redistribute it and/or modify it under the terms of the **GNU Affero General Public License** as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.

See the [LICENSE](LICENSE) file for the full text of the license.
