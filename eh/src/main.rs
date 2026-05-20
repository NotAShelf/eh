use std::{env, path::Path};

use clap_complete::{Shell, generate};
use eh::{Cli, Command, CommandFactory, Parser, Shell as EhShell};
use yansi::Paint;

mod commands;
mod error;
mod eval;
mod hash;
mod nix_config;
mod retry;
mod suggestions;

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

fn handle_command(
  command: &str,
  args: Vec<String>,
  nix_args: Vec<String>,
  ask: bool,
) -> error::Result<i32> {
  let mut all_args = args;
  all_args.extend(nix_args);
  let cfg = eh_config::load();
  let cmd_cfg = cfg.for_command(command);

  match command {
    "info" => commands::info::handle_info(&all_args, &cmd_cfg),

    "update" => commands::update::handle_update(&all_args, &cmd_cfg),
    "run" | "shell" | "build" | "develop" => {
      commands::handle_default_nix_command(command, &all_args, &cmd_cfg, ask)
    },
    "comma" => commands::comma::handle_comma(&all_args, &cmd_cfg, ask),
    _ => unreachable!(),
  }
}

fn dispatch_multicall(
  app_name: &str,
  args: impl IntoIterator<Item = String>,
) -> Option<error::Result<i32>> {
  let mut verbosity = 0i8;
  let mut rest = Vec::new();
  for arg in args {
    match arg.as_str() {
      "-v" | "--verbose" => verbosity += 1,
      "-vv" => verbosity += 2,
      "-vvv" => verbosity += 3,
      "-q" | "--quiet" => verbosity -= 1,
      "-qq" => verbosity -= 2,
      _ => rest.push(arg),
    }
  }
  eh_log::set_verbosity(verbosity);

  let subcommand = match app_name {
    "nr" => "run",
    "ns" => "shell",
    "nb" => "build",
    "nd" | "dev" => "develop",
    "ni" => "info",
    "nu" => "update",
    "," => "comma",
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

  Some(handle_command(subcommand, rest, vec![], false))
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
  eh_log::set_verbosity(cli.verbose as i8 - cli.quiet as i8);

  match cli.command {
    Some(Command::Comma {
      ask,
      args,
      nix_args,
    }) => handle_command("comma", args, nix_args, ask),

    Some(Command::Run {
      ask,
      args,
      nix_args,
    }) => handle_command("run", args, nix_args, ask),

    Some(Command::Shell {
      ask,
      args,
      nix_args,
    }) => handle_command("shell", args, nix_args, ask),

    Some(Command::Build {
      ask,
      args,
      nix_args,
    }) => handle_command("build", args, nix_args, ask),

    Some(Command::Develop {
      ask,
      args,
      nix_args,
    }) => handle_command("develop", args, nix_args, ask),

    Some(Command::Info { args, nix_args }) => {
      handle_command("info", args, nix_args, false)
    },

    Some(Command::Update { args, nix_args }) => {
      handle_command("update", args, nix_args, false)
    },

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
