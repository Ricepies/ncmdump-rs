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
    pub music_id: Option<u64>,
    pub album_id: Option<u64>,
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

    let music_id = v.get("musicId").and_then(|x| {
        x.as_u64().or_else(|| x.as_str().and_then(|s| s.parse().ok()))
    });
    let album_id = v.get("albumId").and_then(|x| {
        x.as_u64().or_else(|| x.as_str().and_then(|s| s.parse().ok()))
    });

    Some(NeteaseMusicMetadata {
        name,
        album,
        artist,
        format,
        duration,
        bitrate,
        music_id,
        album_id,
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
    lyrics: Option<String>,
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
            lyrics: None,
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

    /// Attempt to load a cover image from a specific directory (e.g. ~/meta or output/meta).
    /// Looks for track-{musicId}.jpg/png, {musicId}.jpg/png, album-{albumId}.jpg/png.
    pub fn load_cover_from_meta_dir(&mut self, meta_dir: &Path) -> Option<PathBuf> {
        let stem = self.filepath.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        let (mid, aid) = self.metadata.as_ref().map(|m| (m.music_id, m.album_id)).unwrap_or((None, None));
        if let Some((path, data)) = resolve_cover_image(&[meta_dir.to_path_buf()], stem, mid, aid) {
            self.image_data = data;
            Some(path)
        } else {
            None
        }
    }

    /// Searches candidate directories for a matching cover image when the .ncm file
    /// does not contain an embedded album image (or when force is true).
    /// Checks both `<dir>/meta` and `<dir>` for each candidate search directory.
    pub fn try_resolve_cover(
        &mut self,
        search_dirs: &[PathBuf],
        force: bool,
    ) -> Option<PathBuf> {
        if !self.image_data.is_empty() && !force {
            return None;
        }
        let stem = self.filepath.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        let (mid, aid) = self.metadata.as_ref().map(|m| (m.music_id, m.album_id)).unwrap_or((None, None));
        if let Some((path, data)) = resolve_cover_image(search_dirs, stem, mid, aid) {
            self.image_data = data;
            Some(path)
        } else {
            None
        }
    }

    /// Returns a reference to the loaded/fused lyrics, if any.
    pub fn lyrics(&self) -> Option<&str> {
        self.lyrics.as_deref()
    }

    /// Sets the lyrics explicitly.
    pub fn set_lyrics(&mut self, lrc: String) {
        self.lyrics = Some(lrc);
    }

    /// Searches candidate directories for a matching .lrc file.
    /// Looks for `<stem>.lrc`, `track-{musicId}.lrc`, `{musicId}.lrc` in `<dir>` and `<dir>/meta`.
    /// When found, parses and fuses any JSON-formatted metadata/lines into standard LRC timestamps.
    pub fn try_resolve_lyrics(&mut self, search_dirs: &[PathBuf]) -> Option<PathBuf> {
        let stem = self.filepath.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        let mid = self.metadata.as_ref().and_then(|m| m.music_id);
        if let Some((path, fused)) = resolve_lyrics_file(search_dirs, stem, mid) {
            self.lyrics = Some(fused);
            Some(path)
        } else {
            None
        }
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
                if let Some(ref lrc) = self.lyrics {
                    tag.add_frame(id3::frame::Lyrics {
                        lang: "XXX".to_string(),
                        description: String::new(),
                        text: lrc.clone(),
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
                if let Some(ref lrc) = self.lyrics {
                    let comments = tag.vorbis_comments_mut();
                    comments.set("LYRICS", vec![lrc.clone()]);
                    comments.set("UNSYNCEDLYRICS", vec![lrc.clone()]);
                    comments.set("SYNCEDLYRICS", vec![lrc.clone()]);
                }
                tag.save()?;
            }
        }
        Ok(())
    }
}

/// Fuses NetEase hybrid JSON lines (`{"t": 0, "c": [{"tx": "..."}]}`) and standard LRC lines
/// into unified standard LRC format `[mm:ss.xxx] text`.
pub fn parse_and_fuse_lrc(content: &str) -> String {
    let content_trimmed = content.trim().trim_start_matches('\u{feff}');
    // Check if the entire file is a NetEase JSON response (e.g. {"lrc": {"lyric": "..."}})
    if content_trimmed.starts_with('{') && content_trimmed.ends_with('}') {
        if let Ok(root) = serde_json::from_str::<serde_json::Value>(content_trimmed) {
            if let Some(lyric_str) = root.get("lrc").and_then(|l| l.get("lyric")).and_then(|s| s.as_str()) {
                return parse_and_fuse_lrc(lyric_str);
            }
            if let Some(lyric_str) = root.get("lyric").and_then(|s| s.as_str()) {
                return parse_and_fuse_lrc(lyric_str);
            }
        }
    }

    let mut out_lines = Vec::new();
    for raw_line in content.lines() {
        let trimmed = raw_line.trim().trim_start_matches('\u{feff}');
        if trimmed.starts_with('{') && trimmed.ends_with('}') {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(trimmed) {
                if let Some(t) = v.get("t").and_then(|t| t.as_i64()) {
                    let mins = t / 60000;
                    let secs = (t % 60000) / 1000;
                    let ms = t % 1000;
                    let mut text = String::new();
                    if let Some(c) = v.get("c").and_then(|c| c.as_array()) {
                        for item in c {
                            if let Some(tx) = item.get("tx").or_else(|| item.get("text")).and_then(|x| x.as_str()) {
                                text.push_str(tx);
                            } else if let Some(s) = item.as_str() {
                                text.push_str(s);
                            }
                        }
                    } else if let Some(s) = v.get("c").or_else(|| v.get("text")).or_else(|| v.get("tx")).or_else(|| v.get("lyric")).and_then(|x| x.as_str()) {
                        text.push_str(s);
                    }
                    if text.is_empty() {
                        out_lines.push(format!("[{mins:02}:{secs:02}.{ms:03}]"));
                    } else {
                        out_lines.push(format!("[{mins:02}:{secs:02}.{ms:03}] {text}"));
                    }
                    continue;
                }
            }
        }
        out_lines.push(raw_line.to_string());
    }
    out_lines.join("\n")
}

/// Attempts to parse 163 key NetEase metadata embedded in comment or description fields.
pub fn parse_163_key(raw: &str) -> Option<NeteaseMusicMetadata> {
    let b64_candidate = if let Some(pos) = raw.find("163 key(Don't modify):") {
        &raw[pos + "163 key(Don't modify):".len()..]
    } else {
        raw
    };
    let b64_candidate = b64_candidate.trim();
    if let Ok(b64_decoded) = BASE64_STANDARD.decode(b64_candidate.as_bytes()) {
        let decrypted_meta = aes_128_ecb_decrypt(&MODIFY_KEY, &b64_decoded);
        if decrypted_meta.len() >= 6 {
            if let Ok(json_str) = std::str::from_utf8(&decrypted_meta[6..]) {
                if let Some(m) = parse_metadata(json_str) {
                    return Some(m);
                }
            }
        }
    }
    parse_metadata(raw)
}

/// Resolves an album cover image from candidate search directories.
/// Checks candidate names for stem, track/music ID, album ID, and generic cover names
/// under each directory's `meta/` subfolder and the directory itself.
pub fn resolve_cover_image(
    search_dirs: &[PathBuf],
    stem: &str,
    music_id: Option<u64>,
    album_id: Option<u64>,
) -> Option<(PathBuf, Vec<u8>)> {
    let mut candidate_filenames = Vec::new();

    if !stem.is_empty() {
        candidate_filenames.push(format!("{stem}.jpg"));
        candidate_filenames.push(format!("{stem}.jpeg"));
        candidate_filenames.push(format!("{stem}.png"));
    }

    if let Some(mid) = music_id {
        candidate_filenames.push(format!("track-{mid}.jpg"));
        candidate_filenames.push(format!("track-{mid}.jpeg"));
        candidate_filenames.push(format!("track-{mid}.png"));
        candidate_filenames.push(format!("track-{mid}"));
        candidate_filenames.push(format!("{mid}.jpg"));
        candidate_filenames.push(format!("{mid}.jpeg"));
        candidate_filenames.push(format!("{mid}.png"));
    }

    if let Some(aid) = album_id {
        candidate_filenames.push(format!("album-{aid}.jpg"));
        candidate_filenames.push(format!("album-{aid}.jpeg"));
        candidate_filenames.push(format!("album-{aid}.png"));
        candidate_filenames.push(format!("{aid}.jpg"));
        candidate_filenames.push(format!("{aid}.jpeg"));
        candidate_filenames.push(format!("{aid}.png"));
    }

    candidate_filenames.push("cover.jpg".to_string());
    candidate_filenames.push("cover.jpeg".to_string());
    candidate_filenames.push("cover.png".to_string());
    candidate_filenames.push("folder.jpg".to_string());
    candidate_filenames.push("folder.jpeg".to_string());
    candidate_filenames.push("folder.png".to_string());
    candidate_filenames.push("front.jpg".to_string());
    candidate_filenames.push("front.jpeg".to_string());
    candidate_filenames.push("front.png".to_string());

    for dir in search_dirs {
        for name in &candidate_filenames {
            let p1 = dir.join("meta").join(name);
            if p1.is_file() {
                if let Ok(bytes) = std::fs::read(&p1) {
                    if !bytes.is_empty() {
                        return Some((p1, bytes));
                    }
                }
            }
            let p2 = dir.join(name);
            if p2.is_file() {
                if let Ok(bytes) = std::fs::read(&p2) {
                    if !bytes.is_empty() {
                        return Some((p2, bytes));
                    }
                }
            }
        }
    }
    None
}

/// Resolves a lyrics file from candidate search directories.
/// Looks for `<stem>.lrc`, `track-{musicId}.lrc`, `{musicId}.lrc`.
/// When found, parses and fuses hybrid JSON/LRC into standard timestamps.
pub fn resolve_lyrics_file(
    search_dirs: &[PathBuf],
    stem: &str,
    music_id: Option<u64>,
) -> Option<(PathBuf, String)> {
    let mut candidate_names = Vec::new();
    if !stem.is_empty() {
        candidate_names.push(format!("{stem}.lrc"));
    }
    if let Some(mid) = music_id {
        candidate_names.push(format!("track-{mid}.lrc"));
        candidate_names.push(format!("{mid}.lrc"));
    }

    for dir in search_dirs {
        for name in &candidate_names {
            let p1 = dir.join(name);
            if p1.is_file() {
                if let Ok(content) = std::fs::read_to_string(&p1) {
                    let fused = parse_and_fuse_lrc(&content);
                    return Some((p1, fused));
                }
            }
            let p2 = dir.join("meta").join(name);
            if p2.is_file() {
                if let Ok(content) = std::fs::read_to_string(&p2) {
                    let fused = parse_and_fuse_lrc(&content);
                    return Some((p2, fused));
                }
            }
        }
    }
    None
}

#[derive(Debug, Clone, Default)]
pub struct AudioMetadata {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub format: String,
    pub duration: i64,
    pub bitrate: i64,
    pub music_id: Option<u64>,
    pub album_id: Option<u64>,
    pub has_cover: bool,
    pub has_lyrics: bool,
    pub netease_meta: Option<NeteaseMusicMetadata>,
}

pub fn inspect_flac(path: &Path) -> Result<AudioMetadata, Box<dyn std::error::Error>> {
    let tag = metaflac::Tag::read_from_path(path)?;
    let mut meta = AudioMetadata {
        format: "FLAC (lossless)".to_string(),
        ..Default::default()
    };

    if let Some(info) = tag.get_streaminfo() {
        if info.sample_rate > 0 {
            meta.duration = (info.total_samples as f64 / info.sample_rate as f64 * 1000.0) as i64;
        }
    }

    if let Some(comments) = tag.vorbis_comments() {
        meta.title = comments.title().and_then(|v| v.first()).cloned();
        meta.artist = comments.artist().and_then(|v| v.first()).cloned();
        meta.album = comments.album().and_then(|v| v.first()).cloned();
        meta.has_lyrics = comments.get("LYRICS").is_some() || comments.get("UNSYNCEDLYRICS").is_some();

        for values in comments.comments.values() {
            for val in values {
                if let Some(parsed) = parse_163_key(val) {
                    meta.netease_meta = Some(parsed);
                    break;
                }
            }
            if meta.netease_meta.is_some() {
                break;
            }
        }
    }

    if let Some(ref m) = meta.netease_meta {
        if meta.title.is_none() && !m.name.is_empty() {
            meta.title = Some(m.name.clone());
        }
        if meta.artist.is_none() && !m.artist.is_empty() {
            meta.artist = Some(m.artist.clone());
        }
        if meta.album.is_none() && !m.album.is_empty() {
            meta.album = Some(m.album.clone());
        }
        if meta.duration == 0 && m.duration > 0 {
            meta.duration = m.duration;
        }
        if m.bitrate > 0 {
            meta.bitrate = m.bitrate;
        }
        meta.music_id = m.music_id;
        meta.album_id = m.album_id;
    }

    if meta.music_id.is_none() {
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        let candidate = if let Some(stripped) = stem.strip_prefix("track-") {
            stripped
        } else {
            stem
        };
        if let Ok(id) = candidate.parse::<u64>() {
            meta.music_id = Some(id);
        }
    }

    meta.has_cover = tag
        .pictures()
        .any(|p| p.picture_type == metaflac::block::PictureType::CoverFront);

    Ok(meta)
}

pub fn inspect_mp3(path: &Path) -> Result<AudioMetadata, Box<dyn std::error::Error>> {
    let tag = id3::Tag::read_from_path(path).unwrap_or_else(|_| id3::Tag::new());
    let mut meta = AudioMetadata {
        title: tag.title().map(|s| s.to_string()),
        artist: tag.artist().map(|s| s.to_string()),
        album: tag.album().map(|s| s.to_string()),
        format: "MP3".to_string(),
        has_lyrics: tag.lyrics().next().is_some(),
        has_cover: tag
            .pictures()
            .any(|p| p.picture_type == id3::frame::PictureType::CoverFront),
        ..Default::default()
    };

    for comment in tag.comments() {
        if let Some(parsed) = parse_163_key(&comment.text) {
            meta.netease_meta = Some(parsed);
            break;
        }
    }
    if meta.netease_meta.is_none() {
        for frame in tag.extended_texts() {
            if let Some(parsed) = parse_163_key(&frame.value) {
                meta.netease_meta = Some(parsed);
                break;
            }
        }
    }

    if let Some(ref m) = meta.netease_meta {
        if meta.title.is_none() && !m.name.is_empty() {
            meta.title = Some(m.name.clone());
        }
        if meta.artist.is_none() && !m.artist.is_empty() {
            meta.artist = Some(m.artist.clone());
        }
        if meta.album.is_none() && !m.album.is_empty() {
            meta.album = Some(m.album.clone());
        }
        if m.duration > 0 {
            meta.duration = m.duration;
        }
        if m.bitrate > 0 {
            meta.bitrate = m.bitrate;
        }
        meta.music_id = m.music_id;
        meta.album_id = m.album_id;
    }

    if meta.music_id.is_none() {
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        let candidate = if let Some(stripped) = stem.strip_prefix("track-") {
            stripped
        } else {
            stem
        };
        if let Ok(id) = candidate.parse::<u64>() {
            meta.music_id = Some(id);
        }
    }

    Ok(meta)
}

pub fn update_flac_tags(
    path: &Path,
    netease_meta: Option<&NeteaseMusicMetadata>,
    cover: Option<(&[u8], &'static str)>,
    lyrics: Option<&str>,
    prefer_meta_cover: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut tag = metaflac::Tag::read_from_path(path)?;
    let mut changed = false;

    if let Some(meta) = netease_meta {
        let comments = tag.vorbis_comments_mut();
        if comments.title().is_none() && !meta.name.is_empty() {
            comments.set_title(vec![meta.name.clone()]);
            changed = true;
        }
        if comments.artist().is_none() && !meta.artist.is_empty() {
            comments.set_artist(vec![meta.artist.clone()]);
            changed = true;
        }
        if comments.album().is_none() && !meta.album.is_empty() {
            comments.set_album(vec![meta.album.clone()]);
            changed = true;
        }
    }

    if let Some((data, mime)) = cover {
        let has_cover = tag
            .pictures()
            .any(|p| p.picture_type == metaflac::block::PictureType::CoverFront);
        if !has_cover || prefer_meta_cover {
            if prefer_meta_cover {
                tag.remove_picture_type(metaflac::block::PictureType::CoverFront);
            }
            tag.add_picture(
                mime,
                metaflac::block::PictureType::CoverFront,
                data.to_vec(),
            );
            changed = true;
        }
    }

    if let Some(lrc) = lyrics {
        let comments = tag.vorbis_comments_mut();
        comments.set("LYRICS", vec![lrc.to_string()]);
        comments.set("UNSYNCEDLYRICS", vec![lrc.to_string()]);
        comments.set("SYNCEDLYRICS", vec![lrc.to_string()]);
        changed = true;
    }

    if changed {
        tag.save()?;
    }
    Ok(())
}

pub fn update_mp3_tags(
    path: &Path,
    netease_meta: Option<&NeteaseMusicMetadata>,
    cover: Option<(&[u8], &'static str)>,
    lyrics: Option<&str>,
    prefer_meta_cover: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut tag = id3::Tag::read_from_path(path).unwrap_or_else(|_| id3::Tag::new());
    let mut changed = false;

    if let Some(meta) = netease_meta {
        if tag.title().is_none() && !meta.name.is_empty() {
            tag.set_title(&meta.name);
            changed = true;
        }
        if tag.artist().is_none() && !meta.artist.is_empty() {
            tag.set_artist(&meta.artist);
            changed = true;
        }
        if tag.album().is_none() && !meta.album.is_empty() {
            tag.set_album(&meta.album);
            changed = true;
        }
    }

    if let Some((data, mime)) = cover {
        let has_cover = tag
            .pictures()
            .any(|p| p.picture_type == id3::frame::PictureType::CoverFront);
        if !has_cover || prefer_meta_cover {
            if prefer_meta_cover {
                tag.remove_all_pictures();
            }
            tag.add_frame(id3::frame::Picture {
                mime_type: mime.to_string(),
                picture_type: id3::frame::PictureType::CoverFront,
                description: String::new(),
                data: data.to_vec(),
            });
            changed = true;
        }
    }

    if let Some(lrc) = lyrics {
        tag.remove_all_lyrics();
        tag.add_frame(id3::frame::Lyrics {
            lang: "XXX".to_string(),
            description: String::new(),
            text: lrc.to_string(),
        });
        changed = true;
    }

    if changed {
        tag.write_to_path(path, id3::Version::Id3v24)?;
    }
    Ok(())
}

