use thiserror::Error;

#[derive(Error, Debug)]
pub enum EhError {
  #[error("Nix command failed: {0}")]
  NixCommandFailed(String),

  #[error("IO error: {0}")]
  Io(#[from] std::io::Error),

  #[error("Regex error: {0}")]
  Regex(#[from] regex::Error),

  #[error("UTF-8 conversion error: {0}")]
  Utf8(#[from] std::string::FromUtf8Error),

  #[error("Hash extraction failed")]
  HashExtractionFailed,

  #[error("No Nix files found")]
  NoNixFilesFound,

  #[error("Failed to fix hash in file: {path}")]
  HashFixFailed { path: String },

  #[error("Process exited with code: {code}")]
  ProcessExit { code: i32 },

  #[error("Command execution failed: {command}")]
  CommandFailed { command: String },

  #[error("Invalid input: {input} - {reason}")]
  InvalidInput { input: String, reason: String },
}

pub type Result<T> = std::result::Result<T, EhError>;

impl EhError {
  #[must_use]
  pub const fn exit_code(&self) -> i32 {
    match self {
      Self::ProcessExit { code } => *code,
      Self::NixCommandFailed(_) => 2,
      Self::CommandFailed { .. } => 3,
      Self::HashExtractionFailed => 4,
      Self::NoNixFilesFound => 5,
      Self::HashFixFailed { .. } => 6,
      Self::InvalidInput { .. } => 7,
      Self::Io(_) => 8,
      Self::Regex(_) => 9,
      Self::Utf8(_) => 10,
    }
  }
}
