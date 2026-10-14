use ncmdump::{NcmFormat, NeteaseCrypt};
use std::path::{Path, PathBuf};
use std::process;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Instant;

const VERSION: &str = "1.5.1";

const HELP_MESSAGE: &str = "\x1b[1;36mncmdump-rs\x1b[0m \x1b[2mv1.5.1\x1b[0m
A fast, memory-safe standalone tool to convert NetEase Cloud Music (.ncm) files.

\x1b[1mUSAGE:\x1b[0m
  ncmdump [OPTIONS] [FILES/DIRS...]

\x1b[1mOPTIONS:\x1b[0m
  -h, --help               Print this help message
  -v, --version            Print version information
  -d, --directory <DIR>    Process files in a directory
  -r, --recursive          Process directory recursively (requires -d or dir argument)
  -o, --output <DIR>       Output directory (default: same as source file)
  -m, --remove             Remove source .ncm (and .lrc) files after successful conversion
  -i, --info               Inspect metadata without decrypting audio
  -j, --threads <NUM>      Number of parallel conversion threads (default: CPU cores)
      --meta <DIR>         Custom meta folder path containing track-<id>.jpg artwork
      --prefer-meta-cover  Prefer external meta/ cover over embedded .ncm cover
      --no-lrc             Do not search for or embed .lrc lyrics
      --no-overwrite       Skip conversion if output file already exists

\x1b[1mEXAMPLES:\x1b[0m
  ncmdump song.ncm
  ncmdump song1.ncm song2.ncm -o ./Music
  ncmdump -d ./Music -r -o ./Decrypted
  ncmdump -i song.ncm
";

#[derive(Default)]
struct Stats {
    succeeded: AtomicUsize,
    skipped: AtomicUsize,
    failed: AtomicUsize,
}

fn format_duration(ms: i64) -> String {
    let total_secs = ms / 1000;
    let mins = total_secs / 60;
    let secs = total_secs % 60;
    format!("{mins}:{secs:02}")
}

fn get_search_dirs(
    file_path: &Path,
    output_folder: Option<&Path>,
    custom_meta_dir: Option<&Path>,
) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(meta) = custom_meta_dir {
        dirs.push(meta.to_path_buf());
    }
    if let Some(out) = output_folder {
        dirs.push(out.to_path_buf());
    }
    if let Some(parent) = file_path.parent() {
        if !parent.as_os_str().is_empty() {
            dirs.push(parent.to_path_buf());
        }
    }
    dirs
}

fn print_info(path: &Path, custom_meta: Option<&Path>) {
    match NeteaseCrypt::open(path) {
        Ok(mut crypt) => {
            let search_dirs = get_search_dirs(path, None, custom_meta);
            let resolved_cover = crypt.try_resolve_cover(&search_dirs, false);
            let resolved_lrc = crypt.try_resolve_lyrics(&search_dirs);

            let meta = crypt.metadata();
            println!("\x1b[1;36m┌─────────────────────────────────────────────────────────────┐\x1b[0m");
            println!(
                "\x1b[1;36m│\x1b[0m \x1b[1mFile:\x1b[0m     {:<49} \x1b[1;36m│\x1b[0m",
                path.file_name().and_then(|n| n.to_str()).unwrap_or("")
            );
            if let Some(m) = meta {
                if let Some(mid) = m.music_id {
                    println!(
                        "\x1b[1;36m│\x1b[0m \x1b[1mTrack ID:\x1b[0m {:<49} \x1b[1;36m│\x1b[0m",
                        mid
                    );
                }
                println!(
                    "\x1b[1;36m│\x1b[0m \x1b[1mTitle:\x1b[0m    {:<49} \x1b[1;36m│\x1b[0m",
                    m.name
                );
                println!(
                    "\x1b[1;36m│\x1b[0m \x1b[1mArtist:\x1b[0m   {:<49} \x1b[1;36m│\x1b[0m",
                    m.artist
                );
                println!(
                    "\x1b[1;36m│\x1b[0m \x1b[1mAlbum:\x1b[0m    {:<49} \x1b[1;36m│\x1b[0m",
                    m.album
                );
                let format_str = if m.format.is_empty() {
                    match crypt.format() {
                        NcmFormat::Mp3 => "MP3",
                        NcmFormat::Flac => "FLAC (lossless)",
                    }
                } else {
                    &m.format
                };
                println!(
                    "\x1b[1;36m│\x1b[0m \x1b[1mFormat:\x1b[0m   {:<49} \x1b[1;36m│\x1b[0m",
                    format_str
                );
                if m.duration > 0 {
                    println!(
                        "\x1b[1;36m│\x1b[0m \x1b[1mDuration:\x1b[0m {:<49} \x1b[1;36m│\x1b[0m",
                        format_duration(m.duration)
                    );
                }
                if m.bitrate > 0 {
                    println!(
                        "\x1b[1;36m│\x1b[0m \x1b[1mBitrate:\x1b[0m  {:<49} \x1b[1;36m│\x1b[0m",
                        format!("{} kbps", m.bitrate / 1000)
                    );
                }
            }
            let cover_info = if let Some(ref p) = resolved_cover {
                let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
                format!("From meta/{} ({}, {:.1} KB)", name, crypt.image_mime(), crypt.image_data().len() as f64 / 1024.0)
            } else if !crypt.image_data().is_empty() {
                format!("Embedded ({}, {:.1} KB)", crypt.image_mime(), crypt.image_data().len() as f64 / 1024.0)
            } else {
                "None (missing in .ncm and meta/)".to_string()
            };
            println!(
                "\x1b[1;36m│\x1b[0m \x1b[1mCover:\x1b[0m    {:<49} \x1b[1;36m│\x1b[0m",
                cover_info
            );
            let lrc_info = if let Some(ref p) = resolved_lrc {
                let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
                format!("Found & fused ({name})")
            } else {
                "None".to_string()
            };
            println!(
                "\x1b[1;36m│\x1b[0m \x1b[1mLyrics:\x1b[0m   {:<49} \x1b[1;36m│\x1b[0m",
                lrc_info
            );
            println!("\x1b[1;36m└─────────────────────────────────────────────────────────────┘\x1b[0m");
        }
        Err(e) => {
            eprintln!(
                "\x1b[1;31m✖ FAIL\x1b[0m  '{}': {e}",
                path.display()
            );
        }
    }
}

fn process_file(
    file_path: &Path,
    output_folder: Option<&Path>,
    custom_meta_dir: Option<&Path>,
    remove: bool,
    no_overwrite: bool,
    prefer_meta_cover: bool,
    no_lrc: bool,
    stats: &Stats,
    output_lock: &Mutex<()>,
) {
    if !file_path.exists() {
        let _g = output_lock.lock().unwrap();
        eprintln!(
            "\x1b[1;31m✖ FAIL\x1b[0m  File '{}' does not exist.",
            file_path.display()
        );
        stats.failed.fetch_add(1, Ordering::Relaxed);
        return;
    }

    if file_path.extension().and_then(|s| s.to_str()) != Some("ncm") {
        return;
    }

    // Check potential output path to skip if no-overwrite is requested
    if no_overwrite {
        let file_name = match file_path.file_name() {
            Some(n) => n,
            None => return,
        };
        let target_dir = output_folder.unwrap_or_else(|| file_path.parent().unwrap_or(Path::new("")));
        let potential_mp3 = target_dir.join(file_name).with_extension("mp3");
        let potential_flac = target_dir.join(file_name).with_extension("flac");
        if potential_mp3.exists() || potential_flac.exists() {
            let _g = output_lock.lock().unwrap();
            println!(
                "\x1b[1;33m⚡ SKIP\x1b[0m  '{}' (already exists)",
                file_path.display()
            );
            stats.skipped.fetch_add(1, Ordering::Relaxed);
            return;
        }
    }

    match NeteaseCrypt::open(file_path) {
        Ok(mut crypt) => {
            let search_dirs = get_search_dirs(file_path, output_folder, custom_meta_dir);
            let resolved_cover = crypt.try_resolve_cover(&search_dirs, prefer_meta_cover);
            let resolved_lrc = if !no_lrc {
                crypt.try_resolve_lyrics(&search_dirs)
            } else {
                None
            };

            let meta_summary = crypt
                .metadata()
                .map(|m| format!(" ({} - {})", m.name, m.artist))
                .unwrap_or_default();

            let cover_note = if let Some(ref p) = resolved_cover {
                let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("meta");
                format!(" \x1b[2;36m[cover: {name}]\x1b[0m")
            } else {
                String::new()
            };

            let lrc_note = if let Some(ref p) = resolved_lrc {
                let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("lrc");
                format!(" \x1b[2;35m[lyrics: {name}]\x1b[0m")
            } else {
                String::new()
            };

            match crypt.dump(output_folder) {
                Ok(dump_path) => {
                    if let Err(e) = crypt.fix_metadata() {
                        let _g = output_lock.lock().unwrap();
                        eprintln!(
                            "\x1b[1;33m⚠ WARN\x1b[0m  Failed to fix metadata for '{}': {e}",
                            file_path.display()
                        );
                    }

                    // Also export the clean, fused standard .lrc file alongside the dumped audio
                    if let Some(lrc_text) = crypt.lyrics() {
                        let out_lrc_path = dump_path.with_extension("lrc");
                        let _ = std::fs::write(&out_lrc_path, lrc_text);
                    }

                    let mut removed_note = String::new();
                    if remove {
                        match std::fs::remove_file(file_path) {
                            Ok(_) => removed_note.push_str(" \x1b[2m[source removed]\x1b[0m"),
                            Err(e) => removed_note.push_str(&format!(" \x1b[1;31m[failed to remove ncm: {e}]\x1b[0m")),
                        }
                        if let Some(ref src_lrc) = resolved_lrc {
                            let out_lrc_path = dump_path.with_extension("lrc");
                            if src_lrc != &out_lrc_path && src_lrc.exists() {
                                let _ = std::fs::remove_file(src_lrc);
                            }
                        }
                    }

                    let _g = output_lock.lock().unwrap();
                    println!(
                        "\x1b[1;32m✔ DONE\x1b[0m  '{}' \x1b[2m->\x1b[0m '\x1b[36m{}\x1b[0m'{}{}{}{}",
                        file_path.display(),
                        dump_path.display(),
                        meta_summary,
                        cover_note,
                        lrc_note,
                        removed_note
                    );
                    stats.succeeded.fetch_add(1, Ordering::Relaxed);
                }
                Err(e) => {
                    let _g = output_lock.lock().unwrap();
                    eprintln!(
                        "\x1b[1;31m✖ FAIL\x1b[0m  '{}': {e}",
                        file_path.display()
                    );
                    stats.failed.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
        Err(e) => {
            let _g = output_lock.lock().unwrap();
            eprintln!(
                "\x1b[1;31m✖ FAIL\x1b[0m  '{}': {e}",
                file_path.display()
            );
            stats.failed.fetch_add(1, Ordering::Relaxed);
        }
    }
}

fn walk_dir_recursive(dir: &Path, files: &mut Vec<PathBuf>) -> std::io::Result<()> {
    if dir.is_dir() {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                walk_dir_recursive(&path, files)?;
            } else if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("ncm") {
                files.push(path);
            }
        }
    }
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    let mut help = false;
    let mut version = false;
    let mut recursive = false;
    let mut remove = false;
    let mut info_mode = false;
    let mut no_overwrite = false;
    let mut prefer_meta_cover = false;
    let mut no_lrc = false;
    let mut custom_meta: Option<String> = None;
    let mut threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);
    let mut directory: Option<String> = None;
    let mut output: Option<String> = None;
    let mut filenames: Vec<String> = Vec::new();

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if arg == "-h" || arg == "--help" {
            help = true;
        } else if arg == "-v" || arg == "--version" {
            version = true;
        } else if arg == "-r" || arg == "--recursive" {
            recursive = true;
        } else if arg == "-m" || arg == "--remove" {
            remove = true;
        } else if arg == "-i" || arg == "--info" {
            info_mode = true;
        } else if arg == "--no-overwrite" {
            no_overwrite = true;
        } else if arg == "--prefer-meta-cover" {
            prefer_meta_cover = true;
        } else if arg == "--no-lrc" {
            no_lrc = true;
        } else if arg == "--meta" {
            i += 1;
            if i < args.len() {
                custom_meta = Some(args[i].clone());
            }
        } else if arg.starts_with("--meta=") {
            custom_meta = Some(arg["--meta=".len()..].to_string());
        } else if arg == "-j" || arg == "--threads" {
            i += 1;
            if i < args.len() {
                if let Ok(t) = args[i].parse::<usize>() {
                    threads = t.max(1);
                }
            }
        } else if arg == "-d" || arg == "--directory" {
            i += 1;
            if i >= args.len() {
                println!("{}", HELP_MESSAGE);
                process::exit(1);
            }
            directory = Some(args[i].clone());
        } else if arg.starts_with("--directory=") {
            directory = Some(arg["--directory=".len()..].to_string());
        } else if arg == "-o" || arg == "--output" {
            i += 1;
            if i >= args.len() {
                println!("{}", HELP_MESSAGE);
                process::exit(1);
            }
            output = Some(args[i].clone());
        } else if arg.starts_with("--output=") {
            output = Some(arg["--output=".len()..].to_string());
        } else if arg.starts_with('-') {
            println!("{}", HELP_MESSAGE);
            process::exit(1);
        } else {
            filenames.push(arg.clone());
        }
        i += 1;
    }

    if help {
        println!("{}", HELP_MESSAGE);
        process::exit(0);
    }

    if version {
        println!("ncmdump version {VERSION}");
        process::exit(0);
    }

    let meta_path = custom_meta.as_deref().map(Path::new);

    // Info mode
    if info_mode {
        if filenames.is_empty() && directory.is_none() {
            println!("{}", HELP_MESSAGE);
            process::exit(1);
        }
        for f in &filenames {
            print_info(Path::new(f), meta_path);
        }
        return;
    }

    // Auto-detect if positional arguments contain directories
    if directory.is_none() && filenames.len() == 1 {
        let p = Path::new(&filenames[0]);
        if p.is_dir() {
            directory = Some(filenames[0].clone());
            filenames.clear();
        }
    }

    if recursive && directory.is_none() {
        eprintln!("\x1b[1;31m✖ FAIL\x1b[0m  -r option requires a directory (-d option or directory path).");
        process::exit(1);
    }

    if directory.is_none() && filenames.is_empty() {
        println!("{}", HELP_MESSAGE);
        process::exit(1);
    }

    let output_dir_specified = output.is_some();
    let mut output_dir = PathBuf::new();
    if let Some(ref out) = output {
        output_dir = PathBuf::from(out);
        if output_dir.exists() && !output_dir.is_dir() {
            eprintln!(
                "\x1b[1;31m✖ FAIL\x1b[0m  '{}' is not a valid directory.",
                output_dir.display()
            );
            process::exit(1);
        }
        if let Err(e) = std::fs::create_dir_all(&output_dir) {
            eprintln!("\x1b[1;31m✖ FAIL\x1b[0m  Failed to create output directory: {e}");
            process::exit(1);
        }
    }

    let start_time = Instant::now();
    let stats = Stats::default();
    let output_lock = Mutex::new(());

    // Collect list of files to process: (source_path, target_folder)
    let mut task_list: Vec<(PathBuf, Option<PathBuf>)> = Vec::new();

    if let Some(ref dir) = directory {
        let source_dir = PathBuf::from(dir);
        if !source_dir.is_dir() {
            eprintln!(
                "\x1b[1;31m✖ FAIL\x1b[0m  '{}' is not a valid directory.",
                source_dir.display()
            );
            process::exit(1);
        }

        if recursive {
            let mut all_files = Vec::new();
            if let Err(e) = walk_dir_recursive(&source_dir, &mut all_files) {
                eprintln!("\x1b[1;31m✖ FAIL\x1b[0m  Failed to read directory: {e}");
                process::exit(1);
            }

            let target_base = if output_dir_specified {
                output_dir.clone()
            } else {
                source_dir.clone()
            };

            for path in all_files {
                if let Ok(rel) = path.strip_prefix(&source_dir) {
                    let dest_path = target_base.join(rel);
                    if let Some(parent) = dest_path.parent() {
                        task_list.push((path, Some(parent.to_path_buf())));
                    }
                }
            }
        } else {
            match std::fs::read_dir(&source_dir) {
                Ok(entries) => {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("ncm") {
                            let out_folder = if output_dir_specified {
                                Some(output_dir.clone())
                            } else {
                                None
                            };
                            task_list.push((path, out_folder));
                        }
                    }
                }
                Err(e) => {
                    eprintln!("\x1b[1;31m✖ FAIL\x1b[0m  Failed to read directory: {e}");
                    process::exit(1);
                }
            }
        }
    } else {
        for file_path_str in &filenames {
            let path = PathBuf::from(file_path_str);
            if !path.is_file() {
                eprintln!(
                    "\x1b[1;31m✖ FAIL\x1b[0m  '{}' is not a valid file.",
                    path.display()
                );
                stats.failed.fetch_add(1, Ordering::Relaxed);
                continue;
            }
            let out_folder = if output_dir_specified {
                Some(output_dir.clone())
            } else {
                None
            };
            task_list.push((path, out_folder));
        }
    }

    if task_list.is_empty() {
        if stats.failed.load(Ordering::Relaxed) > 0 {
            process::exit(1);
        }
        return;
    }

    // Execute tasks in parallel or sequential based on task count
    if task_list.len() <= 1 || threads <= 1 {
        for (file_path, out_folder) in &task_list {
            process_file(
                file_path,
                out_folder.as_deref(),
                meta_path,
                remove,
                no_overwrite,
                prefer_meta_cover,
                no_lrc,
                &stats,
                &output_lock,
            );
        }
    } else {
        use std::sync::atomic::AtomicUsize;
        let index = AtomicUsize::new(0);
        let task_ref = &task_list;
        let stats_ref = &stats;
        let lock_ref = &output_lock;

        std::thread::scope(|s| {
            for _ in 0..threads.min(task_list.len()) {
                s.spawn(|| loop {
                    let i = index.fetch_add(1, Ordering::Relaxed);
                    if i >= task_ref.len() {
                        break;
                    }
                    let (file_path, out_folder) = &task_ref[i];
                    process_file(
                        file_path,
                        out_folder.as_deref(),
                        meta_path,
                        remove,
                        no_overwrite,
                        prefer_meta_cover,
                        no_lrc,
                        stats_ref,
                        lock_ref,
                    );
                });
            }
        });
    }

    // Print summary if more than 1 file was processed
    if task_list.len() > 1 {
        let elapsed = start_time.elapsed();
        let succ = stats.succeeded.load(Ordering::Relaxed);
        let skip = stats.skipped.load(Ordering::Relaxed);
        let fail = stats.failed.load(Ordering::Relaxed);
        println!("\x1b[2m─────────────────────────────────────────────────────────────\x1b[0m");
        println!(
            "\x1b[1;32m✨ Complete:\x1b[0m {} converted, {} skipped, {} failed in {:.2?}",
            succ, skip, fail, elapsed
        );
    }
}
