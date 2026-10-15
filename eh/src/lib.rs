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
  /// Run a binary from nixpkgs using the SPAM package index
  Exec(ExecArgs),

  /// Find package files in the SPAM database
  Locate(LocateArgs),

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

#[derive(Debug, clap::Args)]
pub struct ExecArgs {
  /// Ask before retrying or fixing a Nix error
  #[arg(short, long)]
  pub ask: bool,

  /// Binary followed by arguments passed through without interpretation
  #[arg(required = true, num_args = 1.., trailing_var_arg = true, value_name = "BINARY", value_hint = clap::ValueHint::CommandWithArguments)]
  pub args: Vec<String>,
}

#[derive(Debug, clap::Args)]
pub struct LocateArgs {
  /// Literal path fragment to find (or a regular expression with --regex)
  pub pattern: String,

  /// Treat the path and package patterns as regular expressions
  #[arg(short, long)]
  pub regex: bool,

  /// Restrict matches to package names containing NAME
  #[arg(short, long, value_name = "NAME")]
  pub package: Option<String>,

  /// Match any of these types: regular, executable, directory, symlink
  #[arg(short = 't', long = "type", value_name = "TYPE")]
  pub file_types: Vec<eh_locate::FileType>,

  /// Anchor the pattern at the start of the output-relative path
  #[arg(long)]
  pub at_root: bool,

  /// Anchor the pattern at the end of the path
  #[arg(short = 'w', long)]
  pub whole_name: bool,

  /// Print each matching package once, without file metadata
  #[arg(long)]
  pub minimal: bool,

  /// Include descendants of matching directories
  #[arg(long)]
  pub no_group: bool,

  /// SPAM database file (overrides SPAM_DATABASE)
  #[arg(long, value_name = "FILE")]
  pub db: Option<std::path::PathBuf>,
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn exec_preserves_child_flags_and_separator() {
    let cli = Cli::try_parse_from([
      "eh",
      "-v",
      "exec",
      "--ask",
      "hello",
      "--help",
      "--version",
      "-v",
      "-q",
      "--",
      "--ask",
    ])
    .unwrap();
    assert_eq!(cli.verbose, 1);
    assert_eq!(cli.quiet, 0);
    let Some(Command::Exec(exec)) = cli.command else {
      panic!("expected exec")
    };
    assert!(exec.ask);
    assert_eq!(exec.args, [
      "hello",
      "--help",
      "--version",
      "-v",
      "-q",
      "--",
      "--ask"
    ]);
  }

  #[test]
  fn exec_help_is_only_interpreted_before_binary() {
    let error = Cli::try_parse_from(["eh", "exec", "--help"]).err().unwrap();
    assert_eq!(error.kind(), clap::error::ErrorKind::DisplayHelp);
    assert!(Cli::try_parse_from(["eh", "exec"]).is_err());
    assert!(Cli::try_parse_from(["eh", "comma", "hello"]).is_err());
  }

  #[test]
  fn locate_rejects_unknown_file_types() {
    assert!(Cli::try_parse_from(["eh", "locate", "bin", "-t", "z"]).is_err());
  }
}
