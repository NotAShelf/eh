pub mod error;

pub use clap::{CommandFactory, Parser, Subcommand};
pub use error::{EhError, Result};

/// Supported shells for completion generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Shell {
  /// Bash shell
  Bash,
  /// Zsh shell
  Zsh,
  /// Fish shell
  Fish,
}

#[derive(Parser)]
#[command(name = "eh")]
#[command(about = "Ergonomic Nix helper", long_about = None)]
#[command(version)]
pub struct Cli {
  /// Increase logging verbosity (-v, -vv, -vvv)
  #[arg(short, long, action = clap::ArgAction::Count, global = true)]
  pub verbose: u8,

  /// Decrease logging verbosity (-q, -qq)
  #[arg(short, long, action = clap::ArgAction::Count, global = true)]
  pub quiet: u8,

  #[command(subcommand)]
  pub command: Option<Command>,
}

#[derive(Subcommand)]
pub enum Command {
  /// Run a Nix derivation
  Run {
    #[arg(short, long, default_value = "false")]
    ask:  bool,
    #[arg(trailing_var_arg = true)]
    args: Vec<String>,
  },
  /// Enter a Nix shell
  Shell {
    #[arg(short, long, default_value = "false")]
    ask:  bool,
    #[arg(trailing_var_arg = true)]
    args: Vec<String>,
  },
  /// Build a Nix derivation
  Build {
    #[arg(short, long, default_value = "false")]
    ask:  bool,
    #[arg(trailing_var_arg = true)]
    args: Vec<String>,
  },
  /// Enter a Nix development shell
  Develop {
    #[arg(short, long, default_value = "false")]
    ask:  bool,
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
  /// Generate shell completions
  Completion {
    /// Shell to generate completions for
    #[arg(value_enum)]
    shell: Shell,
  },
}
