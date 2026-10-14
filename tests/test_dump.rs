use id3::TagLike;
use ncmdump::NeteaseCrypt;
use std::path::Path;

#[test]
fn test_dump_and_flac_metadata() {
    let test_ncm = Path::new("/home/ricepies/fun/ncmdump/test/test.ncm");
    let mut crypt = NeteaseCrypt::open(test_ncm).expect("Failed to open test.ncm");

    let meta = crypt.metadata().expect("Metadata should exist").clone();
    println!("Metadata: {:?}", meta);
    assert_eq!(meta.name, "贝贝");
    assert_eq!(meta.album, "耳朵");
    assert_eq!(meta.artist, "李荣浩");
    assert_eq!(meta.format, "flac");

    let temp_dir = std::env::temp_dir().join("ncmdump_rust_test_flac");
    let _ = std::fs::create_dir_all(&temp_dir);

    let out_file = crypt.dump(Some(&temp_dir)).expect("Dump should succeed");
    assert!(out_file.exists());
    crypt.fix_metadata().expect("Fix metadata should succeed");

    // Verify FLAC tags using metaflac
    let flac_tag = metaflac::Tag::read_from_path(&out_file).expect("Read flac tag");
    let vorbis = flac_tag.vorbis_comments().expect("Vorbis comments");
    assert_eq!(
        vorbis.title().map(|v| v.as_slice()),
        Some(&[meta.name.clone()][..])
    );
    assert_eq!(
        vorbis.artist().map(|v| v.as_slice()),
        Some(&[meta.artist.clone()][..])
    );
    assert_eq!(
        vorbis.album().map(|v| v.as_slice()),
        Some(&[meta.album.clone()][..])
    );
    assert_eq!(flac_tag.pictures().count(), 1);

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_mp3_metadata_tagging() {
    let temp_mp3 = std::env::temp_dir().join("test_dummy.mp3");
    std::fs::write(&temp_mp3, b"ID3\x04\x00\x00\x00\x00\x00\x00dummy audio content").unwrap();

    let mut tag = id3::Tag::new();
    tag.set_title("Test Song");
    tag.set_artist("Test Artist");
    tag.set_album("Test Album");
    tag.add_frame(id3::frame::Picture {
        mime_type: "image/png".to_string(),
        picture_type: id3::frame::PictureType::CoverFront,
        description: String::new(),
        data: vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A],
    });
    tag.add_frame(id3::frame::Lyrics {
        lang: "eng".to_string(),
        description: String::new(),
        text: "[00:01.00] Test Lyric".to_string(),
    });
    tag.write_to_path(&temp_mp3, id3::Version::Id3v24).unwrap();

    let read_tag = id3::Tag::read_from_path(&temp_mp3).unwrap();
    assert_eq!(read_tag.title(), Some("Test Song"));
    assert_eq!(read_tag.artist(), Some("Test Artist"));
    assert_eq!(read_tag.album(), Some("Test Album"));
    assert_eq!(read_tag.pictures().count(), 1);
    assert_eq!(read_tag.lyrics().next().map(|l| l.text.as_str()), Some("[00:01.00] Test Lyric"));

    let _ = std::fs::remove_file(temp_mp3);
}

#[test]
fn test_meta_folder_cover_resolution() {
    let test_ncm = Path::new("/home/ricepies/fun/ncmdump/test/test.ncm");
    let mut crypt = NeteaseCrypt::open(test_ncm).expect("Failed to open test.ncm");

    let meta = crypt.metadata().expect("Metadata should exist");
    assert_eq!(meta.music_id, Some(1318234987));
    assert_eq!(meta.album_id, Some(73914415));

    // Create a temporary meta folder with track-1318234987.jpg
    let temp_root = std::env::temp_dir().join("ncmdump_meta_test");
    let meta_dir = temp_root.join("meta");
    let _ = std::fs::create_dir_all(&meta_dir);

    let test_jpg = meta_dir.join("track-1318234987.jpg");
    let fake_jpg_bytes = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x4A, 0x46]; // JPEG header
    std::fs::write(&test_jpg, &fake_jpg_bytes).expect("Write test jpg");

    // Clear existing image to simulate a file with no embedded cover
    let resolved = crypt.try_resolve_cover(&[temp_root.clone()], true);
    assert!(resolved.is_some());
    assert_eq!(resolved.unwrap(), test_jpg);
    assert_eq!(crypt.image_data(), fake_jpg_bytes.as_slice());

    let _ = std::fs::remove_dir_all(&temp_root);
}

#[test]
fn test_lyrics_parsing_and_fusing() {
    let raw_lrc = r#"{"t":0,"c":[{"tx":"编曲: "},{"tx":"1AN孙毅然","li":"http://p1.music.126.net/ThT0LdYvONB9QFU_edbQuw==/109951163268870626.jpg","or":"orpheus://nm/artist/home?id=12622933&type=artist"}]}
{"t":1000,"c":[{"tx":"作词: "},{"tx":"锦衣小盆友"}]}
{"t":2000,"c":[{"tx":"作曲: "},{"tx":"银临","li":"http://p1.music.126.net/SC4xC0MMlPC4nViAO0768g==/109951163676566884.jpg","or":"orpheus://nm/artist/home?id=188565&type=artist"}]}
[00:06.395] 编曲Arranger：1AN孙毅然
[00:08.488]
[00:18.260] 你名字是揉碎、漂浮的，二三云朵，
"#;

    let fused = ncmdump::parse_and_fuse_lrc(raw_lrc);
    println!("Fused LRC:\n{fused}");

    assert!(fused.contains("[00:00.000] 编曲: 1AN孙毅然"));
    assert!(fused.contains("[00:01.000] 作词: 锦衣小盆友"));
    assert!(fused.contains("[00:02.000] 作曲: 银临"));
    assert!(fused.contains("[00:06.395] 编曲Arranger：1AN孙毅然"));
    assert!(fused.contains("[00:18.260] 你名字是揉碎、漂浮的，二三云朵，"));

    // Test resolving alongside dummy ncm file
    let temp_dir = std::env::temp_dir().join("ncmdump_lrc_test");
    let _ = std::fs::create_dir_all(&temp_dir);

    let lrc_file = temp_dir.join("test.lrc");
    std::fs::write(&lrc_file, raw_lrc).expect("Write lrc file");

    let test_ncm = Path::new("/home/ricepies/fun/ncmdump/test/test.ncm");
    let mut crypt = NeteaseCrypt::open(test_ncm).expect("Failed to open test.ncm");

    let resolved = crypt.try_resolve_lyrics(&[temp_dir.clone()]);
    assert!(resolved.is_some());
    assert_eq!(resolved.unwrap(), lrc_file);
    assert_eq!(crypt.lyrics(), Some(fused.as_str()));

    let out_file = crypt.dump(Some(&temp_dir)).expect("Dump audio");
    crypt.fix_metadata().expect("Fix metadata");

    // Verify FLAC vorbis comments contain lyrics
    let flac_tag = metaflac::Tag::read_from_path(&out_file).expect("Read flac tag");
    let vorbis = flac_tag.vorbis_comments().expect("Vorbis comments");
    assert_eq!(vorbis.get("LYRICS").map(|v| v.as_slice()), Some(&[fused.clone()][..]));

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_audio_metadata_inspection_and_tag_updating() {
    let temp_dir = std::env::temp_dir().join("ncmdump_audio_tag_test");
    let meta_dir = temp_dir.join("meta");
    let _ = std::fs::create_dir_all(&meta_dir);

    // Create a dummy MP3 file named track-778899.mp3
    let mp3_path = temp_dir.join("track-778899.mp3");
    std::fs::write(&mp3_path, b"ID3\x04\x00\x00\x00\x00\x00\x00dummy audio content").unwrap();

    let mut tag = id3::Tag::new();
    tag.set_title("Original Title");
    tag.set_artist("Original Artist");
    tag.write_to_path(&mp3_path, id3::Version::Id3v24).unwrap();

    // Create artwork in meta/track-778899.jpg
    let jpg_path = meta_dir.join("track-778899.jpg");
    let fake_jpg = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x4A, 0x46];
    std::fs::write(&jpg_path, &fake_jpg).unwrap();

    // Create hybrid JSON lyrics in track-778899.lrc
    let lrc_path = temp_dir.join("track-778899.lrc");
    let raw_lrc = r#"{"t":0,"c":[{"tx":"测试歌词"}]}"#;
    std::fs::write(&lrc_path, raw_lrc).unwrap();

    // Inspect
    let meta = ncmdump::inspect_mp3(&mp3_path).expect("Inspect mp3");
    assert_eq!(meta.music_id, Some(778899));
    assert_eq!(meta.title.as_deref(), Some("Original Title"));
    assert!(!meta.has_cover);
    assert!(!meta.has_lyrics);

    // Resolve cover & lyrics
    let resolved_cover = ncmdump::resolve_cover_image(&[temp_dir.clone()], "track-778899", meta.music_id, None);
    assert!(resolved_cover.is_some());
    let (c_path, c_data) = resolved_cover.unwrap();
    assert_eq!(c_path, jpg_path);
    assert_eq!(c_data, fake_jpg);

    let resolved_lrc = ncmdump::resolve_lyrics_file(&[temp_dir.clone()], "track-778899", meta.music_id);
    assert!(resolved_lrc.is_some());
    let (l_path, l_text) = resolved_lrc.unwrap();
    assert_eq!(l_path, lrc_path);
    assert!(l_text.contains("[00:00.000] 测试歌词"));

    // Update tags
    ncmdump::update_mp3_tags(
        &mp3_path,
        None,
        Some((&c_data, "image/jpeg")),
        Some(&l_text),
        false,
    ).expect("Update mp3 tags");

    // Re-inspect
    let updated_meta = ncmdump::inspect_mp3(&mp3_path).expect("Re-inspect mp3");
    assert!(updated_meta.has_cover);
    assert!(updated_meta.has_lyrics);

    let read_tag = id3::Tag::read_from_path(&mp3_path).unwrap();
    assert_eq!(read_tag.pictures().count(), 1);
    assert_eq!(read_tag.lyrics().next().unwrap().text, "[00:00.000] 测试歌词");

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_cli_all_audio_directory_flag() {
    let temp_root = std::env::temp_dir().join("ncmdump_cli_all_test");
    let src_dir = temp_root.join("src");
    let meta_dir = src_dir.join("meta");
    let out_dir = temp_root.join("out");
    let _ = std::fs::create_dir_all(&meta_dir);
    let _ = std::fs::create_dir_all(&out_dir);

    // Create a dummy mp3 with track-999.mp3
    let mp3_path = src_dir.join("track-999.mp3");
    std::fs::write(&mp3_path, b"ID3\x04\x00\x00\x00\x00\x00\x00dummy audio content").unwrap();

    let mut tag = id3::Tag::new();
    tag.set_title("Test Song 999");
    tag.set_artist("Test Singer");
    tag.write_to_path(&mp3_path, id3::Version::Id3v24).unwrap();

    let jpg_path = meta_dir.join("track-999.jpg");
    let fake_jpg = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x4A, 0x46];
    std::fs::write(&jpg_path, &fake_jpg).unwrap();

    let lrc_path = src_dir.join("track-999.lrc");
    let raw_lrc = r#"{"t":1500,"c":[{"tx":"你好世界"}]}"#;
    std::fs::write(&lrc_path, raw_lrc).unwrap();

    // 1. Without -a flag, directory should NOT process mp3
    let bin_path = env!("CARGO_BIN_EXE_ncmdump");
    let status_no_a = std::process::Command::new(bin_path)
        .arg("-d")
        .arg(&src_dir)
        .arg("-o")
        .arg(&out_dir)
        .status()
        .expect("Run binary without -a");
    assert!(status_no_a.success());
    assert!(!out_dir.join("track-999.mp3").exists());

    // 2. With -a flag, directory SHOULD process mp3, embed cover, and fuse lyrics
    let status_a = std::process::Command::new(bin_path)
        .arg("-a")
        .arg("-d")
        .arg(&src_dir)
        .arg("-o")
        .arg(&out_dir)
        .status()
        .expect("Run binary with -a");
    assert!(status_a.success());

    let dest_mp3 = out_dir.join("track-999.mp3");
    assert!(dest_mp3.exists());

    let dest_tag = id3::Tag::read_from_path(&dest_mp3).expect("Read dest tag");
    assert_eq!(dest_tag.title(), Some("Test Song 999"));
    assert_eq!(dest_tag.artist(), Some("Test Singer"));
    assert_eq!(dest_tag.pictures().count(), 1);
    let lyrics = dest_tag.lyrics().next().expect("Lyrics present");
    assert_eq!(lyrics.text, "[00:01.500] 你好世界");

    let _ = std::fs::remove_dir_all(&temp_root);
}

#[test]
fn test_cli_all_audio_flac_recursive() {
    let temp_root = std::env::temp_dir().join("ncmdump_cli_flac_test");
    let src_dir = temp_root.join("src");
    let sub_dir = src_dir.join("subfolder");
    let meta_dir = sub_dir.join("meta");
    let out_dir = temp_root.join("out");
    let _ = std::fs::create_dir_all(&meta_dir);
    let _ = std::fs::create_dir_all(&out_dir);

    // Dump real flac from test.ncm to get a valid flac
    let test_ncm = Path::new("/home/ricepies/fun/ncmdump/test/test.ncm");
    let mut crypt = NeteaseCrypt::open(test_ncm).expect("Open test.ncm");
    let dumped_flac = crypt.dump(Some(&temp_root)).expect("Dump flac");

    // Copy to subfolder
    let flac_path = sub_dir.join("my_song.flac");
    std::fs::copy(&dumped_flac, &flac_path).unwrap();
    let _ = std::fs::remove_file(&dumped_flac);

    // Add cover and lrc in subfolder
    let jpg_path = meta_dir.join("cover.jpg");
    let fake_jpg = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x4A, 0x46];
    std::fs::write(&jpg_path, &fake_jpg).unwrap();

    let lrc_path = sub_dir.join("my_song.lrc");
    let raw_lrc = "[00:03.141] Pi Song";
    std::fs::write(&lrc_path, raw_lrc).unwrap();

    let bin_path = env!("CARGO_BIN_EXE_ncmdump");
    let status = std::process::Command::new(bin_path)
        .arg("-a")
        .arg("-r")
        .arg("-d")
        .arg(&src_dir)
        .arg("-o")
        .arg(&out_dir)
        .status()
        .expect("Run binary with -a -r");
    assert!(status.success());

    let dest_flac = out_dir.join("subfolder").join("my_song.flac");
    assert!(dest_flac.exists());

    let flac_tag = metaflac::Tag::read_from_path(&dest_flac).expect("Read flac tag");
    let vorbis = flac_tag.vorbis_comments().expect("Vorbis comments");
    assert_eq!(vorbis.get("LYRICS").map(|v| v.as_slice()), Some(&["[00:03.141] Pi Song".to_string()][..]));
    assert_eq!(vorbis.get("UNSYNCEDLYRICS").map(|v| v.as_slice()), Some(&["[00:03.141] Pi Song".to_string()][..]));

    let _ = std::fs::remove_dir_all(&temp_root);
}




