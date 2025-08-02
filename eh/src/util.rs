use crate::command::{NixCommand, StdIoInterceptor};
use regex::Regex;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use tracing::{info, warn};
use yansi::Paint;

pub trait HashExtractor {
    fn extract_hash(&self, stderr: &str) -> Option<String>;
}

pub struct RegexHashExtractor;

impl HashExtractor for RegexHashExtractor {
    fn extract_hash(&self, stderr: &str) -> Option<String> {
        let patterns = [
            r"got:\s+(sha256-[a-zA-Z0-9+/=]+)",
            r"actual:\s+(sha256-[a-zA-Z0-9+/=]+)",
            r"have:\s+(sha256-[a-zA-Z0-9+/=]+)",
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
        let mut stack = vec![PathBuf::from(".")];
        while let Some(dir) = stack.pop() {
            if let Ok(entries) = fs::read_dir(&dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() {
                        stack.push(path);
                    } else if let Some(ext) = path.extension() {
                        if ext.eq_ignore_ascii_case("nix") {
                            files.push(path);
                        }
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
            let mut new_content = content.clone();
            let mut replaced = false;
            for (pattern, replacement) in &patterns {
                if let Ok(re) = Regex::new(pattern) {
                    if re.is_match(&new_content) {
                        new_content = re
                            .replace_all(&new_content, replacement.as_str())
                            .into_owned();
                        replaced = true;
                    }
                }
            }
            if replaced && fs::write(file_path, new_content).is_ok() {
                return true;
            }
        }
        false
    }
}

pub trait NixErrorClassifier {
    fn should_retry(&self, stderr: &str) -> bool;
}

/// Pre-evaluate expression to catch errors early
fn pre_evaluate(_subcommand: &str, args: &[String]) -> bool {
    // Find flake references or expressions to evaluate
    // Only take the first non-flag argument (the package/expression)
    let eval_arg = args.iter().find(|arg| !arg.starts_with('-'));

    let Some(eval_arg) = eval_arg else {
        return true; // No expression to evaluate
    };

    let eval_cmd = NixCommand::new("eval").arg(eval_arg).arg("--raw");

    let output = match eval_cmd.output() {
        Ok(output) => output,
        Err(_) => return false,
    };

    if output.status.success() {
        return true;
    }

    let stderr = String::from_utf8_lossy(&output.stderr);

    // If eval fails due to unfree/insecure/broken, don't fail pre-evaluation
    // Let the main command handle it with retry logic
    if stderr.contains("has an unfree license")
        || stderr.contains("refusing to evaluate")
        || stderr.contains("has been marked as insecure")
        || stderr.contains("has been marked as broken")
    {
        return true;
    }

    // For other eval failures, fail early
    false
}

/// Shared retry logic for nix commands (build/run/shell).
pub fn handle_nix_with_retry(
    subcommand: &str,
    args: &[String],
    hash_extractor: &dyn HashExtractor,
    fixer: &dyn NixFileFixer,
    classifier: &dyn NixErrorClassifier,
    interactive: bool,
) -> ! {
    // Pre-evaluate for build commands to catch errors early
    if !pre_evaluate(subcommand, args) {
        eprintln!("Error: Expression evaluation failed");
        std::process::exit(1);
    }

    // For run commands, try interactive first to avoid breaking terminal
    if subcommand == "run" && interactive {
        let mut cmd = NixCommand::new(subcommand)
            .print_build_logs(true)
            .interactive(true);
        for arg in args {
            cmd = cmd.arg(arg);
        }
        let status = cmd
            .run_with_logs(StdIoInterceptor)
            .expect("failed to run nix command");
        if status.success() {
            std::process::exit(0);
        }
    }

    // First, always capture output to check for errors that need retry
    let output_cmd = NixCommand::new(subcommand)
        .print_build_logs(true)
        .args(args.iter().cloned());
    let output = output_cmd.output().expect("failed to capture output");
    let stderr = String::from_utf8_lossy(&output.stderr);

    // Check if we need to retry with special flags
    if let Some(new_hash) = hash_extractor.extract_hash(&stderr) {
        if fixer.fix_hash_in_files(&new_hash) {
            info!("{}", Paint::green("✔ Fixed hash mismatch, retrying..."));
            let mut retry_cmd = NixCommand::new(subcommand)
                .print_build_logs(true)
                .args(args.iter().cloned());
            if interactive {
                retry_cmd = retry_cmd.interactive(true);
            }
            let retry_status = retry_cmd.run_with_logs(StdIoInterceptor).unwrap();
            std::process::exit(retry_status.code().unwrap_or(1));
        }
    }

    if classifier.should_retry(&stderr) {
        if stderr.contains("has an unfree license") && stderr.contains("refusing") {
            warn!(
                "{}",
                Paint::yellow("⚠ Unfree package detected, retrying with NIXPKGS_ALLOW_UNFREE=1...")
            );
            let mut retry_cmd = NixCommand::new(subcommand)
                .print_build_logs(true)
                .args(args.iter().cloned())
                .env("NIXPKGS_ALLOW_UNFREE", "1")
                .impure(true);
            if interactive {
                retry_cmd = retry_cmd.interactive(true);
            }
            let retry_status = retry_cmd.run_with_logs(StdIoInterceptor).unwrap();
            std::process::exit(retry_status.code().unwrap_or(1));
        }
        if stderr.contains("has been marked as insecure") && stderr.contains("refusing") {
            warn!(
                "{}",
                Paint::yellow(
                    "⚠ Insecure package detected, retrying with NIXPKGS_ALLOW_INSECURE=1..."
                )
            );
            let mut retry_cmd = NixCommand::new(subcommand)
                .print_build_logs(true)
                .args(args.iter().cloned())
                .env("NIXPKGS_ALLOW_INSECURE", "1")
                .impure(true);
            if interactive {
                retry_cmd = retry_cmd.interactive(true);
            }
            let retry_status = retry_cmd.run_with_logs(StdIoInterceptor).unwrap();
            std::process::exit(retry_status.code().unwrap_or(1));
        }
        if stderr.contains("has been marked as broken") && stderr.contains("refusing") {
            warn!(
                "{}",
                Paint::yellow("⚠ Broken package detected, retrying with NIXPKGS_ALLOW_BROKEN=1...")
            );
            let mut retry_cmd = NixCommand::new(subcommand)
                .print_build_logs(true)
                .args(args.iter().cloned())
                .env("NIXPKGS_ALLOW_BROKEN", "1")
                .impure(true);
            if interactive {
                retry_cmd = retry_cmd.interactive(true);
            }
            let retry_status = retry_cmd.run_with_logs(StdIoInterceptor).unwrap();
            std::process::exit(retry_status.code().unwrap_or(1));
        }
    }

    // If the first attempt succeeded, we're done
    if output.status.success() {
        std::process::exit(0);
    }

    // Otherwise, show the error and exit
    std::io::stderr().write_all(output.stderr.as_ref()).unwrap();
    std::process::exit(output.status.code().unwrap_or(1));
}
pub struct DefaultNixErrorClassifier;

impl NixErrorClassifier for DefaultNixErrorClassifier {
    fn should_retry(&self, stderr: &str) -> bool {
        RegexHashExtractor.extract_hash(stderr).is_some()
            || (stderr.contains("has an unfree license") && stderr.contains("refusing"))
            || (stderr.contains("has been marked as insecure") && stderr.contains("refusing"))
            || (stderr.contains("has been marked as broken") && stderr.contains("refusing"))
    }
}
