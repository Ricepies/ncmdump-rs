use std::path::{Path, PathBuf};
use std::process;

const VERSION: &str = "1.5.1";

const HELP_MESSAGE: &str = "ncmdump
Usage:
  ncmdump [OPTION...] <files>

  -h, --help               Print usage
  -d, --directory arg      Process files in a folder (requires folder path)
  -r, --recursive          Process files recursively (requires -d option)
  -o, --output arg         Output folder (default: original file folder)
  -v, --version            Print version information
  -m, --remove             Remove original file if done
";

fn print_help() {
    println!("{}", HELP_MESSAGE.trim_end());
}

fn print_version() {
    println!("ncmdump version {VERSION}");
}

fn process_file(file_path: &Path, output_folder: Option<&Path>, remove: bool) {
    if !file_path.exists() {
        eprintln!(
            "\x1b[1m\x1b[31m[Error] \x1b[0mfile '{}' does not exist.",
            file_path.display()
        );
        return;
    }

    if file_path.extension().and_then(|s| s.to_str()) != Some("ncm") {
        return;
    }

    match ncmdump::NeteaseCrypt::open(file_path) {
        Ok(mut crypt) => match crypt.dump(output_folder) {
            Ok(dump_path) => {
                if let Err(e) = crypt.fix_metadata() {
                    eprintln!(
                        "\x1b[1m\x1b[33m[Warn] \x1b[0mfailed to fix metadata for '{}': {e}",
                        file_path.display()
                    );
                }
                print!(
                    "\x1b[1m\x1b[32m[Done] \x1b[0m'{}' -> '{}'",
                    file_path.display(),
                    dump_path.display()
                );
                if remove {
                    if let Err(e) = std::fs::remove_file(file_path) {
                        eprintln!(" failed to remove: {e}");
                        return;
                    }
                    print!(" with removed as required.");
                }
                println!();
            }
            Err(e) => {
                eprintln!(
                    "\x1b[1m\x1b[31m[Exception] \x1b[0m\x1b[31m{}\x1b[0m '{}'",
                    e,
                    file_path.display()
                );
            }
        },
        Err(e) => {
            eprintln!(
                "\x1b[1m\x1b[31m[Exception] \x1b[0m\x1b[31m{}\x1b[0m '{}'",
                e,
                file_path.display()
            );
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
            } else if path.is_file() {
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
        } else if arg == "-d" || arg == "--directory" {
            i += 1;
            if i >= args.len() {
                print_help();
                process::exit(1);
            }
            directory = Some(args[i].clone());
        } else if arg.starts_with("--directory=") {
            directory = Some(arg["--directory=".len()..].to_string());
        } else if arg == "-o" || arg == "--output" {
            i += 1;
            if i >= args.len() {
                print_help();
                process::exit(1);
            }
            output = Some(args[i].clone());
        } else if arg.starts_with("--output=") {
            output = Some(arg["--output=".len()..].to_string());
        } else if arg.starts_with('-') {
            print_help();
            process::exit(1);
        } else {
            filenames.push(arg.clone());
        }
        i += 1;
    }

    if help {
        print_help();
        process::exit(0);
    }

    if version {
        print_version();
        process::exit(0);
    }

    if recursive && directory.is_none() {
        eprintln!("\x1b[1m\x1b[31m[Error] \x1b[0m-r option requires -d option.");
        process::exit(1);
    }

    if directory.is_none() && filenames.is_empty() {
        print_help();
        process::exit(1);
    }

    let output_dir_specified = output.is_some();
    let mut output_dir = PathBuf::new();
    if let Some(ref out) = output {
        output_dir = PathBuf::from(out);
        if output_dir.exists() && !output_dir.is_dir() {
            eprintln!(
                "\x1b[1m\x1b[31m[Error] \x1b[0m'{}' is not a valid directory.",
                output_dir.display()
            );
            process::exit(1);
        }
        if let Err(e) = std::fs::create_dir_all(&output_dir) {
            eprintln!("\x1b[1m\x1b[31m[Error] \x1b[0mfailed to create output directory: {e}");
            process::exit(1);
        }
    }

    // Process directory
    if let Some(ref dir) = directory {
        let source_dir = PathBuf::from(dir);
        if !source_dir.is_dir() {
            eprintln!(
                "\x1b[1m\x1b[31m[Error] \x1b[0m'{}' is not a valid directory.",
                source_dir.display()
            );
            process::exit(1);
        }

        if recursive {
            let mut all_files = Vec::new();
            if let Err(e) = walk_dir_recursive(&source_dir, &mut all_files) {
                eprintln!("\x1b[1m\x1b[31m[Error] \x1b[0mfailed to read directory: {e}");
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
                        let _ = std::fs::create_dir_all(parent);
                        process_file(&path, Some(parent), remove);
                    }
                }
            }
        } else {
            match std::fs::read_dir(&source_dir) {
                Ok(entries) => {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if path.is_file() {
                            let out_folder = if output_dir_specified {
                                Some(output_dir.as_path())
                            } else {
                                None
                            };
                            process_file(&path, out_folder, remove);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("\x1b[1m\x1b[31m[Error] \x1b[0mfailed to read directory: {e}");
                    process::exit(1);
                }
            }
        }
        return;
    }

    // Process individual files
    for file_path_str in &filenames {
        let path = PathBuf::from(file_path_str);
        if !path.is_file() {
            eprintln!(
                "\x1b[1m\x1b[31m[Error] \x1b[0m'{}' is not a valid file.",
                path.display()
            );
            continue;
        }

        let out_folder = if output_dir_specified {
            Some(output_dir.as_path())
        } else {
            None
        };
        process_file(&path, out_folder, remove);
    }
}
