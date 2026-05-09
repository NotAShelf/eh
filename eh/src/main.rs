use std::{env, path::Path};

use clap_complete::{generate, Shell};
use eh::{Cli, Command, CommandFactory, Parser, Shell as EhShell};
use yansi::Paint;

mod commands;
mod config;
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

fn handle_command(command: &str, args: &[String], ask: bool) -> error::Result<i32> {
  let hash_extractor = util::RegexHashExtractor;
  let fixer = util::DefaultNixFileFixer;
  let classifier = util::DefaultNixErrorClassifier;
  let cfg = config::load();
  let cmd_cfg = cfg.for_command(command);

  match command {
    "info" => commands::info::handle_info(args, &cmd_cfg),

    "update" => commands::update::handle_update(args, &cmd_cfg),
    "run" | "shell" | "build" | "develop" => {
      commands::handle_nix_command(
        command,
        args,
        &hash_extractor,
        &fixer,
        &classifier,
        &cmd_cfg,
        ask,
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
    "nd" | "dev" => "develop",
    "ni" => "info",
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

  Some(handle_command(subcommand, &rest, false))
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
    Some(Command::Run { ask, args }) => handle_command("run", &args, ask),

    Some(Command::Shell { ask, args }) => handle_command("shell", &args, ask),

    Some(Command::Build { ask, args }) => handle_command("build", &args, ask),

    Some(Command::Develop { ask, args }) => handle_command("develop", &args, ask),

    Some(Command::Info { args }) => handle_command("info", &args, false),

    Some(Command::Update { args }) => handle_command("update", &args, false),

    Some(Command::Completion { shell }) => {
      let mut cmd = Cli::command();
      let shell: Shell = match shell {
        EhShell::Bash => Shell::Bash,
        EhShell::Zsh => Shell::Zsh,
        EhShell::Fish => Shell::Fish,
      };
      generate(shell, &mut cmd, "eh", &mut std::io::stdout());
      Ok(0)
    },

    None => {
      Cli::command().print_help()?;
      println!();
      Ok(0)
    },
  }
}
