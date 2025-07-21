use crate::command::{NixCommand, StdIoInterceptor};
use crate::util::{HashExtractor, NixErrorClassifier, NixFileFixer};
use std::io::Write;

pub fn handle_nix_build(
    args: &[String],
    hash_extractor: &dyn HashExtractor,
    fixer: &dyn NixFileFixer,
    classifier: &dyn NixErrorClassifier,
) {
    let mut cmd = NixCommand::new("build").print_build_logs(true);
    for arg in args {
        cmd = cmd.arg(arg);
    }
    let status = cmd
        .run_with_logs(StdIoInterceptor)
        .expect("failed to run nix build");
    if status.success() {
        return;
    }

    let output = NixCommand::new("build")
        .print_build_logs(true)
        .args(args.iter().cloned())
        .output()
        .expect("failed to capture output");
    let stderr = String::from_utf8_lossy(&output.stderr);

    if let Some(new_hash) = hash_extractor.extract_hash(&stderr) {
        if fixer.fix_hash_in_files(&new_hash) {
            eprintln!("\x1b[32m✔ Fixed hash mismatch, retrying...\x1b[0m");
            let retry_status = NixCommand::new("build")
                .print_build_logs(true)
                .args(args.iter().cloned())
                .run_with_logs(StdIoInterceptor)
                .unwrap();
            std::process::exit(retry_status.code().unwrap_or(1));
        }
    }

    if classifier.should_retry(&stderr) {
        if stderr.contains("unfree") {
            eprintln!(
                "\x1b[33m⚠ Unfree package detected, retrying with NIXPKGS_ALLOW_UNFREE=1...\x1b[0m"
            );
            let retry_status = NixCommand::new("build")
                .print_build_logs(true)
                .args(args.iter().cloned())
                .env("NIXPKGS_ALLOW_UNFREE", "1")
                .impure(true)
                .run_with_logs(StdIoInterceptor)
                .unwrap();
            std::process::exit(retry_status.code().unwrap_or(1));
        }
        if stderr.contains("insecure") {
            eprintln!(
                "\x1b[33m⚠ Insecure package detected, retrying with NIXPKGS_ALLOW_INSECURE=1...\x1b[0m"
            );
            let retry_status = NixCommand::new("build")
                .print_build_logs(true)
                .args(args.iter().cloned())
                .env("NIXPKGS_ALLOW_INSECURE", "1")
                .impure(true)
                .run_with_logs(StdIoInterceptor)
                .unwrap();
            std::process::exit(retry_status.code().unwrap_or(1));
        }
        if stderr.contains("broken") {
            eprintln!(
                "\x1b[33m⚠ Broken package detected, retrying with NIXPKGS_ALLOW_BROKEN=1...\x1b[0m"
            );
            let retry_status = NixCommand::new("build")
                .print_build_logs(true)
                .args(args.iter().cloned())
                .env("NIXPKGS_ALLOW_BROKEN", "1")
                .impure(true)
                .run_with_logs(StdIoInterceptor)
                .unwrap();
            std::process::exit(retry_status.code().unwrap_or(1));
        }
    }

    std::io::stderr().write_all(output.stderr.as_ref()).unwrap();
    std::process::exit(status.code().unwrap_or(1));
}
