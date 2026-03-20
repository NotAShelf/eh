use std::{env, path::Path};

use eh::{Cli, Command, CommandFactory, Parser};
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

fn handle_command(command: &str, args: &[String]) -> error::Result<i32> {
  let hash_extractor = util::RegexHashExtractor;
  let fixer = util::DefaultNixFileFixer;
  let classifier = util::DefaultNixErrorClassifier;

  match command {
    "update" => commands::update::handle_update(args),
    "run" | "shell" | "build" | "develop" => {
      commands::handle_nix_command(
        command,
        args,
        &hash_extractor,
        &fixer,
        &classifier,
      )
    },
    _ => unreachable!(),
  }
}

fn dispatch_multicall(
  app_name: &str,
  args: std::env::Args,
) -> Option<error::Result<i32>> {
  let rest: Vec<String> = args.collect();

  let subcommand = match app_name {
    "nr" => "run",
    "ns" => "shell",
    "nb" => "build",
    "nd" => "develop",
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

  Some(handle_command(subcommand, &rest))
}

fn run_app() -> error::Result<i32> {
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

  match cli.command {
    Some(Command::Run { args }) => handle_command("run", &args),

    Some(Command::Shell { args }) => handle_command("shell", &args),

    Some(Command::Build { args }) => handle_command("build", &args),

    Some(Command::Develop { args }) => handle_command("develop", &args),

    Some(Command::Update { args }) => handle_command("update", &args),

    None => {
      Cli::command().print_help()?;
      println!();
      Ok(0)
    },
  }
}
