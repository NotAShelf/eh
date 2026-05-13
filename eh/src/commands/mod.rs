use crate::{
  error::Result,
  hash::{HashExtractor, NixFileFixer},
  retry::{NixErrorClassifier, handle_nix_with_retry},
};

pub mod info;
pub mod update;

pub fn handle_nix_command(
  command: &str,
  args: &[String],
  hash_extractor: &dyn HashExtractor,
  fixer: &dyn NixFileFixer,
  classifier: &dyn NixErrorClassifier,
  cfg: &eh_config::CommandConfig,
  ask: bool,
) -> Result<i32> {
  handle_nix_with_retry(
    command,
    args,
    hash_extractor,
    fixer,
    classifier,
    matches!(command, "run" | "shell" | "develop"),
    cfg,
    ask,
  )
}
