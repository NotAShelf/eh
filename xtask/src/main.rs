use std::{
    error, fs,
    path::{Path, PathBuf},
    process,
};

use clap::Parser;

#[derive(clap::Parser)]
struct Cli {
    #[clap(subcommand)]
    command: Command,
}

#[derive(clap::Subcommand)]
enum Command {
    /// Create multicall binaries (hardlinks or copies).
    Multicall {
        /// Directory to install multicall binaries.
        #[arg(long, default_value = "bin")]
        bin_dir: PathBuf,

        /// Path to the main binary.
        #[arg(long, default_value = "target/release/eh")]
        main_binary: PathBuf,
    },
}

#[derive(Debug, Clone, Copy)]
enum Binary {
    Nr,
    Ns,
    Nb,
}

impl Binary {
    fn name(self) -> &'static str {
        match self {
            Self::Nr => "nr",
            Self::Ns => "ns",
            Self::Nb => "nb",
        }
    }
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Command::Multicall {
            bin_dir,
            main_binary,
        } => {
            if let Err(error) = create_multicall_binaries(&bin_dir, &main_binary) {
                eprintln!("error creating multicall binaries: {error}");
                process::exit(1);
            }
        }
    }
}

fn create_multicall_binaries(
    bin_dir: &Path,
    main_binary: &Path,
) -> Result<(), Box<dyn error::Error>> {
    println!("creating multicall binaries...");

    fs::create_dir_all(bin_dir)?;

    if !main_binary.exists() {
        return Err(format!("main binary not found at: {}", main_binary.display()).into());
    }

    let multicall_binaries = [Binary::Nr, Binary::Ns, Binary::Nb];
    let bin_path = Path::new(bin_dir);

    for binary in multicall_binaries {
        let target_path = bin_path.join(binary.name());

        if target_path.exists() {
            fs::remove_file(&target_path)?;
        }

        match fs::hard_link(main_binary, &target_path) {
            Ok(()) => {
                println!(
                    "  created hardlink: {} points to {}",
                    target_path.display(),
                    main_binary.display(),
                );
            }
            Err(e) => {
                eprintln!(
                    "  warning: could not create hardlink for {}: {e}",
                    binary.name(),
                );
                eprintln!("  warning: falling back to copying binary...");

                fs::copy(main_binary, &target_path)?;

                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let mut perms = fs::metadata(&target_path)?.permissions();
                    perms.set_mode(perms.mode() | 0o755);
                    fs::set_permissions(&target_path, perms)?;
                }

                println!("  created copy: {}", target_path.display());
            }
        }
    }

    println!("multicall binaries created successfully!");
    println!("multicall binaries are in: {}", bin_dir.display());
    println!();

    Ok(())
}
