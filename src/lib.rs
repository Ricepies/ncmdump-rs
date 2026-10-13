use aes::Aes128;
use base64::prelude::*;
use cipher::{generic_array::GenericArray, BlockDecrypt, KeyInit};
use id3::TagLike;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

const CORE_KEY: [u8; 16] = [
    0x68, 0x7A, 0x48, 0x52, 0x41, 0x6D, 0x73, 0x6F, 0x35, 0x6B, 0x49, 0x6E, 0x62, 0x61, 0x78, 0x57,
];

const MODIFY_KEY: [u8; 16] = [
    0x23, 0x31, 0x34, 0x6C, 0x6A, 0x6B, 0x5F, 0x21, 0x5C, 0x5D, 0x26, 0x30, 0x55, 0x3C, 0x27, 0x28,
];

const PNG_HEADER: [u8; 8] = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];

pub fn aes_128_ecb_decrypt(key: &[u8; 16], data: &[u8]) -> Vec<u8> {
    if data.len() < 16 {
        return Vec::new();
    }
    let cipher = Aes128::new(GenericArray::from_slice(key));
    let n = data.len() / 16;
    let mut decrypted = Vec::with_capacity(n * 16);
    let mut block = GenericArray::clone_from_slice(&[0u8; 16]);

    for i in 0..n {
        let chunk = &data[i * 16..(i + 1) * 16];
        block.copy_from_slice(chunk);
        cipher.decrypt_block(&mut block);
        if i == n - 1 {
            let pad = block[15] as usize;
            let pad_len = if pad > 0 && pad <= 16 { pad } else { 0 };
            decrypted.extend_from_slice(&block[..16 - pad_len]);
        } else {
            decrypted.extend_from_slice(&block);
        }
    }
    decrypted
}

pub fn build_key_box(key: &[u8]) -> [u8; 256] {
    let mut key_box = [0u8; 256];
    for i in 0..256 {
        key_box[i] = i as u8;
    }
    if key.is_empty() {
        return key_box;
    }
    let mut last_byte: u8 = 0;
    let mut key_offset = 0;
    for i in 0..256 {
        let swap = key_box[i];
        let c = ((swap as u32 + last_byte as u32 + key[key_offset] as u32) & 0xff) as u8;
        key_offset += 1;
        if key_offset >= key.len() {
            key_offset = 0;
        }
        key_box[i] = key_box[c as usize];
        key_box[c as usize] = swap;
        last_byte = c;
    }
    key_box
}

pub fn mime_type(data: &[u8]) -> &'static str {
    if data.len() >= 8 && &data[0..8] == PNG_HEADER {
        "image/png"
    } else {
        "image/jpeg"
    }
}

#[derive(Debug, Clone, Default)]
pub struct NeteaseMusicMetadata {
    pub name: String,
    pub album: String,
    pub artist: String,
    pub format: String,
    pub duration: i64,
    pub bitrate: i64,
}

fn parse_metadata(raw_json: &str) -> Option<NeteaseMusicMetadata> {
    let v: serde_json::Value = serde_json::from_str(raw_json).ok()?;
    let name = v
        .get("musicName")
        .and_then(|x| x.as_str())
        .unwrap_or_default()
        .to_string();
    let album = v
        .get("album")
        .and_then(|x| x.as_str())
        .unwrap_or_default()
        .to_string();

    let mut artist = String::new();
    if let Some(artists) = v.get("artist").and_then(|x| x.as_array()) {
        for a in artists {
            let item_str = if let Some(arr) = a.as_array() {
                arr.first()
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string())
            } else if let Some(obj) = a.as_object() {
                obj.get("name")
                    .or_else(|| obj.get("userName"))
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string())
            } else if let Some(s) = a.as_str() {
                Some(s.to_string())
            } else {
                None
            };
            if let Some(s) = item_str {
                if !artist.is_empty() {
                    artist.push('/');
                }
                artist.push_str(&s);
            }
        }
    }

    let bitrate = v.get("bitrate").and_then(|x| x.as_i64()).unwrap_or_default();
    let duration = v
        .get("duration")
        .and_then(|x| x.as_i64())
        .unwrap_or_default();
    let format = v
        .get("format")
        .and_then(|x| x.as_str())
        .unwrap_or_default()
        .to_string();

    Some(NeteaseMusicMetadata {
        name,
        album,
        artist,
        format,
        duration,
        bitrate,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NcmFormat {
    Mp3,
    Flac,
}

pub struct NeteaseCrypt {
    filepath: PathBuf,
    dump_filepath: Option<PathBuf>,
    format: NcmFormat,
    image_data: Vec<u8>,
    mask_table: [u8; 256],
    metadata: Option<NeteaseMusicMetadata>,
    audio_offset: u64,
}

pub fn compute_mask_table(key_box: &[u8; 256]) -> [u8; 256] {
    let mut mask = [0u8; 256];
    for m in 0..256 {
        let j = ((m + 1) & 0xff) as usize;
        let k = (key_box[j] as usize + key_box[(key_box[j] as usize + j) & 0xff] as usize) & 0xff;
        mask[m] = key_box[k];
    }
    mask
}

impl NeteaseCrypt {
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, Box<dyn std::error::Error>> {
        let path = path.as_ref().to_path_buf();
        let mut file = File::open(&path)?;

        let mut magic = [0u8; 8];
        file.read_exact(&mut magic)?;
        if &magic[0..4] != b"CTEN" || &magic[4..8] != b"FDAM" {
            return Err("Not netease protected file".into());
        }

        file.seek(SeekFrom::Current(2))?;

        let mut buf4 = [0u8; 4];
        file.read_exact(&mut buf4)?;
        let key_len = u32::from_le_bytes(buf4) as usize;
        if key_len == 0 {
            return Err("Broken NCM file".into());
        }

        let mut key_data = vec![0u8; key_len];
        file.read_exact(&mut key_data)?;
        for b in &mut key_data {
            *b ^= 0x64;
        }

        let decrypted_key = aes_128_ecb_decrypt(&CORE_KEY, &key_data);
        if decrypted_key.len() <= 17 {
            return Err("Broken NCM file (invalid key data)".into());
        }
        let key_box = build_key_box(&decrypted_key[17..]);
        let mask_table = compute_mask_table(&key_box);

        file.read_exact(&mut buf4)?;
        let meta_len = u32::from_le_bytes(buf4) as usize;
        let metadata = if meta_len == 0 {
            eprintln!(
                "\x1b[1m\x1b[33m[Warn] \x1b[0m'{}' missing metadata infomation can't fix some infomation!",
                path.display()
            );
            None
        } else {
            if meta_len < 22 {
                return Err("Broken NCM file (invalid metadata length)".into());
            }
            let mut modify_data = vec![0u8; meta_len];
            file.read_exact(&mut modify_data)?;
            for b in &mut modify_data {
                *b ^= 0x63;
            }

            let b64_decoded = BASE64_STANDARD
                .decode(&modify_data[22..])
                .map_err(|e| format!("Base64 decode error: {e}"))?;
            let decrypted_meta = aes_128_ecb_decrypt(&MODIFY_KEY, &b64_decoded);
            if decrypted_meta.len() < 6 {
                return Err("Broken NCM file (invalid decrypted metadata)".into());
            }
            let meta_json_bytes = &decrypted_meta[6..];
            let meta_json_str = std::str::from_utf8(meta_json_bytes)
                .map_err(|e| format!("Invalid UTF-8 in metadata: {e}"))?;
            parse_metadata(meta_json_str)
        };

        file.seek(SeekFrom::Current(5))?;

        file.read_exact(&mut buf4)?;
        let cover_frame_len = u32::from_le_bytes(buf4) as usize;
        file.read_exact(&mut buf4)?;
        let image_len = u32::from_le_bytes(buf4) as usize;

        let image_data = if image_len > 0 {
            let mut img = vec![0u8; image_len];
            file.read_exact(&mut img)?;
            img
        } else {
            eprintln!(
                "\x1b[1m\x1b[33m[Warn] \x1b[0m'{}' missing album can't fix album image!",
                path.display()
            );
            Vec::new()
        };

        if cover_frame_len > image_len {
            file.seek(SeekFrom::Current((cover_frame_len - image_len) as i64))?;
        }

        let audio_offset = file.stream_position()?;

        Ok(Self {
            filepath: path,
            dump_filepath: None,
            format: NcmFormat::Mp3,
            image_data,
            mask_table,
            metadata,
            audio_offset,
        })
    }

    pub fn filepath(&self) -> &Path {
        &self.filepath
    }

    pub fn dump_filepath(&self) -> Option<&Path> {
        self.dump_filepath.as_deref()
    }

    pub fn metadata(&self) -> Option<&NeteaseMusicMetadata> {
        self.metadata.as_ref()
    }

    pub fn image_data(&self) -> &[u8] {
        &self.image_data
    }

    pub fn format(&self) -> NcmFormat {
        self.format
    }

    pub fn image_mime(&self) -> &'static str {
        mime_type(&self.image_data)
    }

    pub fn dump<P: AsRef<Path>>(
        &mut self,
        output_dir: Option<P>,
    ) -> Result<PathBuf, Box<dyn std::error::Error>> {
        let file_name = self.filepath.file_name().ok_or("Invalid filename")?;
        let mut out_path = match output_dir {
            Some(dir) => {
                let dir_ref = dir.as_ref();
                if dir_ref.as_os_str().is_empty() {
                    self.filepath.clone()
                } else {
                    dir_ref.join(file_name)
                }
            }
            None => self.filepath.clone(),
        };

        let mut file = std::io::BufReader::with_capacity(128 * 1024, File::open(&self.filepath)?);
        file.seek(SeekFrom::Start(self.audio_offset))?;

        let mut buffer = [0u8; 0x10000]; // 64 KB buffer (multiple of 256)
        let mut out_file: Option<std::io::BufWriter<File>> = None;

        loop {
            let n = file.read(&mut buffer)?;
            if n == 0 {
                break;
            }

            // High-speed vectorized XOR using the precomputed 256-byte mask table
            for chunk in buffer[..n].chunks_mut(256) {
                for (b, &m) in chunk.iter_mut().zip(&self.mask_table) {
                    *b ^= m;
                }
            }

            if out_file.is_none() {
                if n >= 3 && buffer[0] == 0x49 && buffer[1] == 0x44 && buffer[2] == 0x33 {
                    out_path.set_extension("mp3");
                    self.format = NcmFormat::Mp3;
                } else {
                    out_path.set_extension("flac");
                    self.format = NcmFormat::Flac;
                }
                if let Some(parent) = out_path.parent() {
                    if !parent.as_os_str().is_empty() {
                        std::fs::create_dir_all(parent)?;
                    }
                }
                out_file = Some(std::io::BufWriter::with_capacity(128 * 1024, File::create(&out_path)?));
            }

            if let Some(ref mut out) = out_file {
                out.write_all(&buffer[..n])?;
            }
        }

        if let Some(mut out) = out_file {
            out.flush()?;
        }

        self.dump_filepath = Some(out_path.clone());
        Ok(out_path)
    }

    pub fn fix_metadata(&self) -> Result<(), Box<dyn std::error::Error>> {
        let dump_path = self.dump_filepath.as_ref().ok_or("No dump filepath set")?;
        let mime = mime_type(&self.image_data);

        match self.format {
            NcmFormat::Mp3 => {
                let mut tag = id3::Tag::read_from_path(dump_path).unwrap_or_else(|_| id3::Tag::new());
                if let Some(ref meta) = self.metadata {
                    tag.set_title(&meta.name);
                    tag.set_artist(&meta.artist);
                    tag.set_album(&meta.album);
                }
                if !self.image_data.is_empty() {
                    tag.add_frame(id3::frame::Picture {
                        mime_type: mime.to_string(),
                        picture_type: id3::frame::PictureType::CoverFront,
                        description: String::new(),
                        data: self.image_data.clone(),
                    });
                }
                tag.write_to_path(dump_path, id3::Version::Id3v24)?;
            }
            NcmFormat::Flac => {
                let mut tag = metaflac::Tag::read_from_path(dump_path)?;
                if let Some(ref meta) = self.metadata {
                    let comments = tag.vorbis_comments_mut();
                    comments.set_title(vec![meta.name.clone()]);
                    comments.set_artist(vec![meta.artist.clone()]);
                    comments.set_album(vec![meta.album.clone()]);
                }
                if !self.image_data.is_empty() {
                    tag.add_picture(
                        mime,
                        metaflac::block::PictureType::CoverFront,
                        self.image_data.clone(),
                    );
                }
                tag.save()?;
            }
        }
        Ok(())
    }
}
