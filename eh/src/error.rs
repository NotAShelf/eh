use std::time::Duration;

use thiserror::Error;

#[derive(Error, Debug)]
pub enum EhError {
  #[error("Nix command 'nix {command}' failed")]
  NixCommandFailed { command: String },

  #[error("IO error: {0}")]
  Io(#[from] std::io::Error),

  #[error("Regex error: {0}")]
  Regex(#[from] regex::Error),

  #[error("UTF-8 conversion error: {0}")]
  Utf8(#[from] std::string::FromUtf8Error),

  #[error("Hash extraction failed: could not parse hash from nix output")]
  HashExtractionFailed { stderr: String },

  #[error("No Nix files found in the current directory")]
  NoNixFilesFound,

  #[error("Failed to fix hash in file: {path}")]
  HashFixFailed { path: String },

  #[error("Process exited with code: {code}")]
  ProcessExit { code: i32 },

  #[error("Command execution failed: {command}")]
  CommandFailed { command: String },

  #[error("Command '{command}' timed out after {} seconds", duration.as_secs())]
  Timeout {
    command:  String,
    duration: Duration,
  },

  #[error("Pre-evaluation of '{expression}' failed: {stderr}")]
  PreEvalFailed {
    expression: String,
    stderr:     String,
  },

  #[error("Invalid input: {input} - {reason}")]
  InvalidInput { input: String, reason: String },
}

pub type Result<T> = std::result::Result<T, EhError>;

impl EhError {
  #[must_use]
  pub const fn exit_code(&self) -> i32 {
    match self {
      Self::ProcessExit { code } => *code,
      Self::NixCommandFailed { .. } => 2,
      Self::CommandFailed { .. } => 3,
      Self::HashExtractionFailed { .. } => 4,
      Self::NoNixFilesFound => 5,
      Self::HashFixFailed { .. } => 6,
      Self::InvalidInput { .. } => 7,
      Self::Io(_) => 8,
      Self::Regex(_) => 9,
      Self::Utf8(_) => 10,
      Self::Timeout { .. } => 11,
      Self::PreEvalFailed { .. } => 12,
    }
  }

  #[must_use]
  pub fn hint(&self) -> Option<&str> {
    match self {
      Self::NixCommandFailed { .. } => {
        Some("Run with --show-trace for more details from nix")
      },
      Self::PreEvalFailed { .. } => {
        Some(
          "Check that the expression exists in the flake and is spelled \
           correctly",
        )
      },
      Self::HashExtractionFailed { .. } => {
        Some(
          "The nix output contained a hash mismatch but the hash could not be \
           parsed",
        )
      },
      Self::NoNixFilesFound => {
        Some("Run this command from a directory containing .nix files")
      },
      Self::Timeout { .. } => {
        Some(
          "The command took too long; try a faster network or a smaller \
           derivation",
        )
      },
      Self::InvalidInput { .. } => {
        Some("Avoid shell metacharacters in nix arguments")
      },
      Self::Io(_)
      | Self::Regex(_)
      | Self::Utf8(_)
      | Self::HashFixFailed { .. }
      | Self::ProcessExit { .. }
      | Self::CommandFailed { .. } => None,
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
      EhError::CommandFailed {
        command: "x".into(),
      }
      .exit_code(),
      3
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
      EhError::InvalidInput {
        input:  "x".into(),
        reason: "y".into(),
      }
      .exit_code(),
      7
    );
    assert_eq!(
      EhError::Timeout {
        command:  "build".into(),
        duration: Duration::from_secs(300),
      }
      .exit_code(),
      11
    );
    assert_eq!(
      EhError::PreEvalFailed {
        expression: "x".into(),
        stderr:     "y".into(),
      }
      .exit_code(),
      12
    );
    assert_eq!(EhError::ProcessExit { code: 42 }.exit_code(), 42);
  }

  #[test]
  fn test_display_messages() {
    let err = EhError::Timeout {
      command:  "build".into(),
      duration: Duration::from_secs(300),
    };
    assert_eq!(
      err.to_string(),
      "Command 'build' timed out after 300 seconds"
    );

    let err = EhError::PreEvalFailed {
      expression: "nixpkgs#hello".into(),
      stderr:     "attribute not found".into(),
    };
    assert!(err.to_string().contains("nixpkgs#hello"));
    assert!(err.to_string().contains("attribute not found"));

    let err = EhError::HashExtractionFailed {
      stderr: "some output".into(),
    };
    assert!(err.to_string().contains("could not parse hash"));
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
    assert!(
      EhError::Timeout {
        command:  "x".into(),
        duration: Duration::from_secs(1),
      }
      .hint()
      .is_some()
    );
    assert!(
      EhError::InvalidInput {
        input:  "x".into(),
        reason: "y".into(),
      }
      .hint()
      .is_some()
    );
    // Variants with hints
    assert!(
      EhError::NixCommandFailed {
        command: "build".into(),
      }
      .hint()
      .is_some()
    );
    // Variants without hints
    assert!(
      EhError::CommandFailed {
        command: "x".into(),
      }
      .hint()
      .is_none()
    );
    assert!(EhError::ProcessExit { code: 1 }.hint().is_none());
  }
}
