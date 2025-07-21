use crate::util::{HashExtractor, NixErrorClassifier, NixFileFixer, handle_nix_with_retry};

pub fn handle_nix_build(
    args: &[String],
    hash_extractor: &dyn HashExtractor,
    fixer: &dyn NixFileFixer,
    classifier: &dyn NixErrorClassifier,
) {
    handle_nix_with_retry("build", args, hash_extractor, fixer, classifier, false);
}
