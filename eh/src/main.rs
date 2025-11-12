use eh::{Cli, Command, CommandFactory, Parser};
use error::Result;
use std::env;
use std::path::Path;

mod build;
mod command;
mod error;
mod run;
mod shell;
mod util;

fn main() {
    let format = tracing_subscriber::fmt::format()
        .with_level(true) // don't include levels in formatted output
        .with_target(true) // don't include targets
        .with_thread_ids(false) // include the thread ID of the current thread
        .with_thread_names(false) // include the name of the current thread
        .compact(); // use the `Compact` formatting style.
    tracing_subscriber::fmt().event_format(format).init();

    let result = run_app();

    match result {
        Ok(code) => std::process::exit(code),
        Err(e) => {
            eprintln!("Error: {e}");
            std::process::exit(e.exit_code());
        }
    }
}

// Design partially taken from Stash
fn dispatch_multicall(app_name: &str, args: std::env::Args) -> Option<Result<i32>> {
    let rest: Vec<String> = args.collect();
    let hash_extractor = util::RegexHashExtractor;
    let fixer = util::DefaultNixFileFixer;
    let classifier = util::DefaultNixErrorClassifier;

    match app_name {
        "nr" => Some(run::handle_nix_run(
            &rest,
            &hash_extractor,
            &fixer,
            &classifier,
        )),
        "ns" => Some(shell::handle_nix_shell(
            &rest,
            &hash_extractor,
            &fixer,
            &classifier,
        )),
        "nb" => Some(build::handle_nix_build(
            &rest,
            &hash_extractor,
            &fixer,
            &classifier,
        )),
        _ => None,
    }
}

fn run_app() -> Result<i32> {
    let mut args = env::args();
    let bin = args.next().unwrap_or_else(|| "eh".to_string());
    let app_name = Path::new(&bin)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("eh");

    // If invoked as nr/ns/nb, dispatch directly and exit
    if let Some(result) = dispatch_multicall(app_name, args) {
        return result;
    }

    let cli = Cli::parse();

    let hash_extractor = util::RegexHashExtractor;
    let fixer = util::DefaultNixFileFixer;
    let classifier = util::DefaultNixErrorClassifier;

    match cli.command {
        Some(Command::Run { args }) => {
            run::handle_nix_run(&args, &hash_extractor, &fixer, &classifier)
        }

        Some(Command::Shell { args }) => {
            shell::handle_nix_shell(&args, &hash_extractor, &fixer, &classifier)
        }

        Some(Command::Build { args }) => {
            build::handle_nix_build(&args, &hash_extractor, &fixer, &classifier)
        }

        _ => {
            Cli::command().print_help()?;
            println!();
            Ok(0)
        }
    }
}
