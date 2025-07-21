use regex::Regex;
use std::fs;
use std::path::{Path, PathBuf};

pub trait HashExtractor {
    fn extract_hash(&self, stderr: &str) -> Option<String>;
}

pub struct RegexHashExtractor;

impl HashExtractor for RegexHashExtractor {
    fn extract_hash(&self, stderr: &str) -> Option<String> {
        let patterns = [
            r"got:\s+([a-zA-Z0-9+/=]+)",
            r"actual:\s+([a-zA-Z0-9+/=]+)",
            r"have:\s+([a-zA-Z0-9+/=]+)",
        ];
        for pattern in &patterns {
            if let Ok(re) = Regex::new(pattern) {
                if let Some(captures) = re.captures(stderr) {
                    if let Some(hash) = captures.get(1) {
                        return Some(hash.as_str().to_string());
                    }
                }
            }
        }
        None
    }
}

pub trait NixFileFixer {
    fn fix_hash_in_files(&self, new_hash: &str) -> bool;
    fn find_nix_files(&self) -> Vec<PathBuf>;
    fn fix_hash_in_file(&self, file_path: &Path, new_hash: &str) -> bool;
}

pub struct DefaultNixFileFixer;

impl NixFileFixer for DefaultNixFileFixer {
    fn fix_hash_in_files(&self, new_hash: &str) -> bool {
        let nix_files = self.find_nix_files();
        let mut fixed = false;
        for file_path in nix_files {
            if self.fix_hash_in_file(&file_path, new_hash) {
                println!("Updated hash in {}", file_path.display());
                fixed = true;
            }
        }
        fixed
    }

    fn find_nix_files(&self) -> Vec<PathBuf> {
        let mut files = Vec::new();
        let candidates = [
            "default.nix",
            "package.nix",
            "shell.nix",
            "flake.nix",
            "nix/default.nix",
            "nix/package.nix",
            "nix/site.nix",
        ];
        for candidate in &candidates {
            let path = Path::new(candidate);
            if path.exists() {
                files.push(path.to_path_buf());
            }
        }
        if let Ok(entries) = fs::read_dir(".") {
            for entry in entries.flatten() {
                let path = entry.path();
                if let Some(ext) = path.extension() {
                    if ext.eq_ignore_ascii_case("nix") && !files.contains(&path) {
                        files.push(path);
                    }
                }
            }
        }
        files
    }

    fn fix_hash_in_file(&self, file_path: &Path, new_hash: &str) -> bool {
        if let Ok(content) = fs::read_to_string(file_path) {
            let patterns = [
                (r#"hash\s*=\s*"[^"]*""#, format!(r#"hash = "{new_hash}""#)),
                (
                    r#"sha256\s*=\s*"[^"]*""#,
                    format!(r#"sha256 = "{new_hash}""#),
                ),
                (
                    r#"outputHash\s*=\s*"[^"]*""#,
                    format!(r#"outputHash = "{new_hash}""#),
                ),
            ];
            for (pattern, replacement) in &patterns {
                if let Ok(re) = Regex::new(pattern) {
                    if re.is_match(&content) {
                        let new_content = re.replace_all(&content, replacement);
                        if fs::write(file_path, new_content.as_ref()).is_ok() {
                            return true;
                        }
                    }
                }
            }
        }
        false
    }
}

pub trait NixErrorClassifier {
    fn should_retry(&self, stderr: &str) -> bool;
}

pub struct DefaultNixErrorClassifier;

impl NixErrorClassifier for DefaultNixErrorClassifier {
    fn should_retry(&self, stderr: &str) -> bool {
        RegexHashExtractor.extract_hash(stderr).is_some()
            || (stderr.contains("unfree") && stderr.contains("refusing"))
            || (stderr.contains("insecure") && stderr.contains("refusing"))
            || (stderr.contains("broken") && stderr.contains("refusing"))
    }
}
