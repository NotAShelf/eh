use std::{
    error, fs,
    path::{Path, PathBuf},
    process,
};

use clap::{CommandFactory, Parser};
use clap_complete::{Shell, generate};

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
    /// Generate shell completion scripts
    Completions {
        /// Shell to generate completions for
        #[arg(value_enum)]
        shell: Shell,
        /// Directory to output completion files
        #[arg(long, default_value = "completions")]
        output_dir: PathBuf,
    },
}

#[derive(Debug, Clone, Copy)]
enum Binary {
    Nr,
    Ns,
    Nb,
}

impl Binary {
    const fn name(self) -> &'static str {
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
        Command::Completions { shell, output_dir } => {
            if let Err(error) = generate_completions(shell, &output_dir) {
                eprintln!("error generating completions: {error}");
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

fn generate_completions(shell: Shell, output_dir: &Path) -> Result<(), Box<dyn error::Error>> {
    println!("generating {shell} completions...");

    fs::create_dir_all(output_dir)?;

    let mut cmd = eh::Cli::command();
    let bin_name = "eh";

    let completion_file = output_dir.join(format!("{bin_name}.{shell}"));
    let mut file = fs::File::create(&completion_file)?;

    generate(shell, &mut cmd, bin_name, &mut file);

    println!("completion file generated: {}", completion_file.display());

    // Create symlinks for multicall binaries
    let multicall_names = ["nb", "nr", "ns"];
    for name in &multicall_names {
        let symlink_path = output_dir.join(format!("{name}.{shell}"));
        if symlink_path.exists() {
            fs::remove_file(&symlink_path)?;
        }

        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&completion_file, &symlink_path)?;
            println!("completion symlink created: {}", symlink_path.display());
        }

        #[cfg(not(unix))]
        {
            fs::copy(&completion_file, &symlink_path)?;
            println!("completion copy created: {}", symlink_path.display());
        }
    }

    println!("completions generated successfully!");
    Ok(())
}
