use std::{env, path::Path};

use eh::{Cli, Command, CommandFactory, Parser};
use error::Result;
use yansi::Paint;

mod commands;
mod error;
mod util;

fn main() {
  let result = run_app();

  match result {
    Ok(code) => std::process::exit(code),
    Err(e) => {
      let code = e.exit_code();
      if code != 0 {
        eh_log::log_error!("{e}");
        if let Some(hint) = e.hint() {
          eh_log::log_hint!("{hint}");
        }
      }
      std::process::exit(code);
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
    "nu" => "update",
    _ => return None,
  };

  // Handle --help/-h/--version before forwarding to nix
  if rest.iter().any(|a| a == "--help" || a == "-h") {
    eprintln!(
      "{}: shorthand for '{}'",
      app_name.bold(),
      format!("eh {subcommand}").bold()
    );
    eprintln!("  {} {app_name} [args...]", "usage:".green().bold());
    eprintln!(
      "  All arguments are forwarded to '{}'.",
      format!("nix {subcommand}").dim()
    );
    return Some(Ok(0));
  }

  if rest.iter().any(|a| a == "--version") {
    eprintln!("{} (eh {})", app_name.bold(), env!("CARGO_PKG_VERSION"));
    return Some(Ok(0));
  }

  if subcommand == "update" {
    return Some(commands::update::handle_update(&rest));
  }

  let hash_extractor = util::RegexHashExtractor;
  let fixer = util::DefaultNixFileFixer;
  let classifier = util::DefaultNixErrorClassifier;

  Some(match subcommand {
    "run" | "shell" | "build" => {
      commands::handle_nix_command(
        subcommand,
        &rest,
        &hash_extractor,
        &fixer,
        &classifier,
      )
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
      commands::handle_nix_command(
        "run",
        &args,
        &hash_extractor,
        &fixer,
        &classifier,
      )
    },

    Some(Command::Shell { args }) => {
      commands::handle_nix_command(
        "shell",
        &args,
        &hash_extractor,
        &fixer,
        &classifier,
      )
    },

    Some(Command::Build { args }) => {
      commands::handle_nix_command(
        "build",
        &args,
        &hash_extractor,
        &fixer,
        &classifier,
      )
    },

    Some(Command::Update { args }) => commands::update::handle_update(&args),

    None => {
      Cli::command().print_help()?;
      println!();
      Ok(0)
    },
  }
}
