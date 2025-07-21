use clap::{CommandFactory, Parser, Subcommand};
use std::env;
use std::path::Path;

mod build;
mod command;
mod run;
mod shell;
mod util;

#[derive(Parser)]
#[command(name = "eh")]
#[command(about = "Ergonomic Nix helper", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Run a Nix derivation
    Run {
        #[arg(trailing_var_arg = true)]
        args: Vec<String>,
    },
    /// Enter a Nix shell
    Shell {
        #[arg(trailing_var_arg = true)]
        args: Vec<String>,
    },
    /// Build a Nix derivation
    Build {
        #[arg(trailing_var_arg = true)]
        args: Vec<String>,
    },
}

fn main() {
    let mut args = env::args();
    let bin = args.next().unwrap_or_else(|| "eh".to_string());
    let app_name = Path::new(&bin)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("eh");

    match app_name {
        "nr" => {
            let rest: Vec<String> = args.collect();
            let hash_extractor = util::RegexHashExtractor;
            let fixer = util::DefaultNixFileFixer;
            let classifier = util::DefaultNixErrorClassifier;
            run::handle_nix_run(&rest, &hash_extractor, &fixer, &classifier);
            return;
        }
        "ns" => {
            let rest: Vec<String> = args.collect();
            let hash_extractor = util::RegexHashExtractor;
            let fixer = util::DefaultNixFileFixer;
            let classifier = util::DefaultNixErrorClassifier;
            shell::handle_nix_shell(&rest, &hash_extractor, &fixer, &classifier);
            return;
        }
        "nb" => {
            let rest: Vec<String> = args.collect();
            let hash_extractor = util::RegexHashExtractor;
            let fixer = util::DefaultNixFileFixer;
            let classifier = util::DefaultNixErrorClassifier;
            build::handle_nix_build(&rest, &hash_extractor, &fixer, &classifier);
            return;
        }
        _ => {}
    }

    let cli = Cli::parse();

    let hash_extractor = util::RegexHashExtractor;
    let fixer = util::DefaultNixFileFixer;
    let classifier = util::DefaultNixErrorClassifier;

    match cli.command {
        Some(Command::Run { args }) => {
            run::handle_nix_run(&args, &hash_extractor, &fixer, &classifier);
        }
        Some(Command::Shell { args }) => {
            shell::handle_nix_shell(&args, &hash_extractor, &fixer, &classifier);
        }
        Some(Command::Build { args }) => {
            build::handle_nix_build(&args, &hash_extractor, &fixer, &classifier);
        }
        None => {
            Cli::command().print_help().unwrap();
            println!();
            std::process::exit(0);
        }
    }
}
