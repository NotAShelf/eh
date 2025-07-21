use crate::util::{HashExtractor, NixErrorClassifier, NixFileFixer, handle_nix_with_retry};

pub fn handle_nix_run(
    args: &[String],
    hash_extractor: &dyn HashExtractor,
    fixer: &dyn NixFileFixer,
    classifier: &dyn NixErrorClassifier,
) {
    handle_nix_with_retry("run", args, hash_extractor, fixer, classifier, false);
}
