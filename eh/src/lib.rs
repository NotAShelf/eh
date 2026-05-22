pub use clap::{CommandFactory, Parser, Subcommand};
pub use eh_error::{EhError, Result};

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
  /// Runa binary from nixpkgs via nix-index
  Comma {
    #[arg(short, long, default_value = "false")]
    ask:      bool,
    /// Installable(s) to run
    #[arg(num_args(0..))]
    args:     Vec<String>,
    /// Extra flags forwarded verbatim to `the binary` (after `--`)
    #[arg(last = true)]
    nix_args: Vec<String>,
  },

  /// Run a Nix derivation
  Run {
    #[arg(short, long, default_value = "false")]
    ask:      bool,
    /// Installable(s) to run
    #[arg(num_args(0..))]
    args:     Vec<String>,
    /// Extra flags forwarded verbatim to `nix run` (after `--`)
    #[arg(last = true)]
    nix_args: Vec<String>,
  },
  /// Enter a Nix shell
  Shell {
    #[arg(short, long, default_value = "false")]
    ask:      bool,
    /// Installable(s) for the shell environment
    #[arg(num_args(0..))]
    args:     Vec<String>,
    /// Extra flags forwarded verbatim to `nix shell` (after `--`)
    #[arg(last = true)]
    nix_args: Vec<String>,
  },
  /// Build a Nix derivation
  Build {
    #[arg(short, long, default_value = "false")]
    ask:      bool,
    /// Installable(s) to build
    #[arg(num_args(0..))]
    args:     Vec<String>,
    /// Extra flags forwarded verbatim to `nix build` (after `--`)
    #[arg(last = true)]
    nix_args: Vec<String>,
  },
  /// Enter a Nix development shell
  Develop {
    #[arg(short, long, default_value = "false")]
    ask:      bool,
    /// Installable for the development shell
    #[arg(num_args(0..))]
    args:     Vec<String>,
    /// Extra flags forwarded verbatim to `nix develop` (after `--`)
    #[arg(last = true)]
    nix_args: Vec<String>,
  },
  /// Show package information
  Info {
    /// Installable(s) to query
    #[arg(num_args(0..))]
    args:     Vec<String>,
    /// Extra flags forwarded verbatim to `nix eval` (after `--`)
    #[arg(last = true)]
    nix_args: Vec<String>,
  },
  /// Update flake inputs interactively
  Update {
    /// Flake input(s) to update
    #[arg(num_args(0..))]
    args:     Vec<String>,
    /// Extra flags forwarded verbatim to `nix flake update` (after `--`)
    #[arg(last = true)]
    nix_args: Vec<String>,
  },
  /// Generate shell completions
  Completion {
    /// Shell to generate completions for
    #[arg(value_enum)]
    shell: Shell,
  },
}
