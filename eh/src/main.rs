use std::{env, path::Path};

use eh::{Cli, Command, CommandFactory, Parser};
use error::Result;

mod build;
mod command;
mod error;
mod run;
mod shell;
mod util;

fn main() {
  let result = run_app();

  match result {
    Ok(code) => std::process::exit(code),
    Err(e) => {
      eprintln!("Error: {e}");
      if let Some(hint) = e.hint() {
        eprintln!("Hint: {hint}");
      }
      std::process::exit(e.exit_code());
    },
  }
}

// Design partially taken from Stash
fn dispatch_multicall(
  app_name: &str,
  args: std::env::Args,
) -> Option<Result<i32>> {
  let rest: Vec<String> = args.collect();

  let subcommand = match app_name {
    "nr" => "run",
    "ns" => "shell",
    "nb" => "build",
    _ => return None,
  };

  // Handle --help/-h/--version before forwarding to nix
  if rest.iter().any(|a| a == "--help" || a == "-h") {
    eprintln!("{app_name}: shorthand for 'eh {subcommand}'");
    eprintln!("Usage: {app_name} [args...]");
    eprintln!("All arguments are forwarded to 'nix {subcommand}'.");
    return Some(Ok(0));
  }

  if rest.iter().any(|a| a == "--version") {
    eprintln!("{app_name} (eh {})", env!("CARGO_PKG_VERSION"));
    return Some(Ok(0));
  }

  let hash_extractor = util::RegexHashExtractor;
  let fixer = util::DefaultNixFileFixer;
  let classifier = util::DefaultNixErrorClassifier;

  Some(match subcommand {
    "run" => run::handle_nix_run(&rest, &hash_extractor, &fixer, &classifier),
    "shell" => {
      shell::handle_nix_shell(&rest, &hash_extractor, &fixer, &classifier)
    },
    "build" => {
      build::handle_nix_build(&rest, &hash_extractor, &fixer, &classifier)
    },
    // subcommand is assigned from the match on app_name above;
    // only "run"/"shell"/"build" are possible values.
    _ => unreachable!(),
  })
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
    },

    Some(Command::Shell { args }) => {
      shell::handle_nix_shell(&args, &hash_extractor, &fixer, &classifier)
    },

    Some(Command::Build { args }) => {
      build::handle_nix_build(&args, &hash_extractor, &fixer, &classifier)
    },

    None => {
      Cli::command().print_help()?;
      println!();
      Ok(0)
    },
  }
}
