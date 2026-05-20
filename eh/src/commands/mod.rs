use nix_command::CommandKind;

use crate::{
  error::{EhError, Result},
  hash::{HashExtractor, NixFileFixer},
  retry::{NixErrorClassifier, handle_nix_with_retry},
};

pub mod comma;
pub mod info;
pub mod update;

pub fn handle_default_nix_command(
  command: &str,
  args: &[String],
  cfg: &eh_config::CommandConfig,
  ask: bool,
) -> Result<i32> {
  let hash_extractor = crate::hash::RegexHashExtractor;
  let fixer = crate::hash::DefaultNixFileFixer;
  let classifier = crate::retry::DefaultNixErrorClassifier;

  handle_nix_command(
    command,
    args,
    &hash_extractor,
    &fixer,
    &classifier,
    cfg,
    ask,
  )
}

pub fn handle_nix_command(
  command: &str,
  args: &[String],
  hash_extractor: &dyn HashExtractor,
  fixer: &dyn NixFileFixer,
  classifier: &dyn NixErrorClassifier,
  cfg: &eh_config::CommandConfig,
  ask: bool,
) -> Result<i32> {
  let kind = CommandKind::try_from(command).map_err(|_| {
    EhError::NixCommandFailed {
      command: command.to_string(),
    }
  })?;
  handle_nix_with_retry(kind, args, hash_extractor, fixer, classifier, cfg, ask)
}
