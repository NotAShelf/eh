use thiserror::Error;

#[derive(Error, Debug)]
pub enum EhError {
  #[error("no binary name provided")]
  MissingBinary,

  #[error("invalid binary name '{binary}'")]
  InvalidBinaryName { binary: String },

  #[error("spam-db database error: {0}")]
  SpamIndex(#[from] spam_db::Error),

  #[error("no package found providing binary '{binary}'")]
  BinaryNotFound { binary: String },

  #[error("multiple packages provide binary '{binary}': {candidates:?}")]
  AmbiguousBinary {
    binary:     String,
    candidates: Vec<String>,
  },

  #[error("nix {command} failed")]
  NixCommandFailed { command: String },

  #[error("io: {0}")]
  Io(#[from] std::io::Error),

  #[error(transparent)]
  NixCommand(#[from] nix_command::Error),

  #[error("regex: {0}")]
  Regex(#[from] regex::Error),

  #[error("utf-8 conversion: {0}")]
  Utf8(#[from] std::string::FromUtf8Error),

  #[error("could not extract hash from nix output")]
  HashExtractionFailed { stderr: String },

  #[error("no .nix files found in the current directory")]
  NoNixFilesFound,

  #[error("could not update hash in {path}")]
  HashFixFailed { path: String },

  #[error("process exited with code {code}")]
  ProcessExit { code: i32 },

  #[error("'{expression}' failed to evaluate: {stderr}")]
  PreEvalFailed {
    expression: String,
    stderr:     String,
  },

  #[error("failed to parse JSON from nix output: {detail}")]
  JsonParse { detail: String },

  #[error("no flake inputs found in lock file")]
  NoFlakeInputs,

  #[error("no inputs selected")]
  UpdateCancelled,

  #[error("empty nix expression")]
  InvalidEvalInput,

  #[error(
    "package {reason} but `--impure` is disabled for `{command}` in config"
  )]
  ImpureRequired { command: String, reason: String },
}

pub type Result<T> = std::result::Result<T, EhError>;

impl EhError {
  #[must_use]
  pub const fn exit_code(&self) -> i32 {
    match self {
      Self::ProcessExit { code } => *code,
      Self::NixCommandFailed { .. } => 2,
      Self::HashExtractionFailed { .. } => 4,
      Self::NoNixFilesFound => 5,
      Self::HashFixFailed { .. } => 6,
      Self::Io(_) | Self::NixCommand(_) => 8,
      Self::Regex(_) => 9,
      Self::Utf8(_) => 10,
      Self::PreEvalFailed { .. } => 12,
      Self::JsonParse { .. } => 13,
      Self::NoFlakeInputs => 14,
      Self::UpdateCancelled => 0,
      Self::ImpureRequired { .. } => 15,
      Self::InvalidEvalInput => 16,
      Self::MissingBinary => 17,
      Self::InvalidBinaryName { .. } => 18,
      Self::SpamIndex(_) => 19,
      Self::BinaryNotFound { .. } => 20,
      Self::AmbiguousBinary { .. } => 21,
    }
  }

  #[must_use]
  pub const fn hint(&self) -> Option<&str> {
    match self {
      Self::MissingBinary => {
        Some("pass a binary name, for example `eh comma hello`")
      },
      Self::InvalidBinaryName { .. } => {
        Some("pass the executable name without a path")
      },
      Self::SpamIndex(_) => {
        Some("run `spam index` to generate or refresh the local database")
      },
      Self::BinaryNotFound { .. } => {
        Some(
          "run `spam index` to refresh the local database or check the binary \
           name",
        )
      },
      Self::AmbiguousBinary { .. } => {
        Some(
          "run one of the listed installables directly with `eh shell \
           nixpkgs#<package-name> -c <binary>`",
        )
      },
      Self::NixCommandFailed { .. } => {
        Some("run with --show-trace for more details")
      },
      Self::PreEvalFailed { .. } => {
        Some("check that the expression exists and is spelled correctly")
      },
      Self::HashExtractionFailed { .. } => {
        Some("nix reported a hash mismatch but the hash could not be parsed")
      },
      Self::NoNixFilesFound => {
        Some("run this command from a directory containing .nix files")
      },
      Self::JsonParse { .. } => {
        Some("ensure 'nix flake metadata --json' produces valid output")
      },
      Self::NoFlakeInputs => {
        Some("run this from a directory with a flake.lock that has inputs")
      },
      Self::ImpureRequired { .. } => {
        Some(
          "set `impure = true` for this command (or globally) in .eh.toml or \
           ~/.config/eh/config.toml, or pass `--impure` manually",
        )
      },
      Self::InvalidEvalInput => {
        Some("pass a package name, flake reference, or path")
      },
      Self::Io(_)
      | Self::NixCommand(_)
      | Self::Regex(_)
      | Self::Utf8(_)
      | Self::HashFixFailed { .. }
      | Self::ProcessExit { .. }
      | Self::UpdateCancelled => None,
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_exit_codes() {
    assert_eq!(
      EhError::NixCommandFailed {
        command: "build".into(),
      }
      .exit_code(),
      2
    );
    assert_eq!(
      EhError::HashExtractionFailed {
        stderr: String::new(),
      }
      .exit_code(),
      4
    );
    assert_eq!(EhError::NoNixFilesFound.exit_code(), 5);
    assert_eq!(EhError::HashFixFailed { path: "x".into() }.exit_code(), 6);
    assert_eq!(
      EhError::PreEvalFailed {
        expression: "x".into(),
        stderr:     "y".into(),
      }
      .exit_code(),
      12
    );
    assert_eq!(EhError::ProcessExit { code: 42 }.exit_code(), 42);
    assert_eq!(EhError::JsonParse { detail: "x".into() }.exit_code(), 13);
    assert_eq!(EhError::NoFlakeInputs.exit_code(), 14);
    assert_eq!(EhError::UpdateCancelled.exit_code(), 0);
  }

  #[test]
  fn test_display_messages() {
    let err = EhError::PreEvalFailed {
      expression: "nixpkgs#hello".into(),
      stderr:     "attribute not found".into(),
    };
    assert!(err.to_string().contains("nixpkgs#hello"));
    assert!(err.to_string().contains("attribute not found"));

    let err = EhError::HashExtractionFailed {
      stderr: "some output".into(),
    };
    assert!(err.to_string().contains("could not extract hash"));
  }

  #[test]
  fn test_hints() {
    assert!(
      EhError::PreEvalFailed {
        expression: "x".into(),
        stderr:     "y".into(),
      }
      .hint()
      .is_some()
    );
    assert!(
      EhError::HashExtractionFailed {
        stderr: String::new(),
      }
      .hint()
      .is_some()
    );
    assert!(EhError::NoNixFilesFound.hint().is_some());
    // Variants with hints
    assert!(
      EhError::NixCommandFailed {
        command: "build".into(),
      }
      .hint()
      .is_some()
    );
    assert!(EhError::JsonParse { detail: "x".into() }.hint().is_some());
    assert!(EhError::NoFlakeInputs.hint().is_some());
    // Variants without hints
    assert!(EhError::ProcessExit { code: 1 }.hint().is_none());
    assert!(EhError::UpdateCancelled.hint().is_none());
  }
}
