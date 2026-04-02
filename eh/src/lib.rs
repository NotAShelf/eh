pub mod commands;
pub mod config;
pub mod error;
pub mod util;

pub use clap::{CommandFactory, Parser, Subcommand};
pub use error::{EhError, Result};

#[derive(Parser)]
#[command(name = "eh")]
#[command(about = "Ergonomic Nix helper", long_about = None)]
#[command(version)]
pub struct Cli {
  #[command(subcommand)]
  pub command: Option<Command>,
}

#[derive(Subcommand)]
pub enum Command {
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
  /// Enter a Nix development shell
  Develop {
    #[arg(trailing_var_arg = true)]
    args: Vec<String>,
  },
  /// Show package information
  Info {
    #[arg(trailing_var_arg = true)]
    args: Vec<String>,
  },
  /// Update flake inputs interactively
  Update {
    #[arg(trailing_var_arg = true)]
    args: Vec<String>,
  },
}
