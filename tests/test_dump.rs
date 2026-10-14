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
    tag.write_to_path(&temp_mp3, id3::Version::Id3v24).unwrap();

    let read_tag = id3::Tag::read_from_path(&temp_mp3).unwrap();
    assert_eq!(read_tag.title(), Some("Test Song"));
    assert_eq!(read_tag.artist(), Some("Test Artist"));
    assert_eq!(read_tag.album(), Some("Test Album"));
    assert_eq!(read_tag.pictures().count(), 1);

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

