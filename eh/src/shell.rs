use crate::{
  error::Result,
  util::{
    HashExtractor,
    NixErrorClassifier,
    NixFileFixer,
    handle_nix_with_retry,
  },
};

pub fn handle_nix_shell(
  args: &[String],
  hash_extractor: &dyn HashExtractor,
  fixer: &dyn NixFileFixer,
  classifier: &dyn NixErrorClassifier,
) -> Result<i32> {
  handle_nix_with_retry("shell", args, hash_extractor, fixer, classifier, true)
}
