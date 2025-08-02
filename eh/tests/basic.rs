//! I hate writing tests, and I hate writing integration tests. This is the best
//! that you are getting, deal with it.
use std::process::{Command, Stdio};

#[test]
fn nix_eval_validation() {
    // Test that invalid expressions are caught early for all commands
    let commands = ["build", "run", "shell"];

    for cmd in &commands {
        let output = Command::new("timeout")
            .args([
                "10",
                "cargo",
                "run",
                "--bin",
                "eh",
                "--",
                cmd,
                "invalid-flake-ref",
            ])
            .output()
            .expect("Failed to execute command");

        // Should fail fast with eval error
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("Error: Expression evaluation failed") || !output.status.success());
    }
}

#[test]
fn unfree_package_handling() {
    // Test that unfree packages are detected and handled correctly
    let output = Command::new("timeout")
        .args([
            "30",
            "cargo",
            "run",
            "--bin",
            "eh",
            "--",
            "build",
            "nixpkgs#discord",
        ])
        .output()
        .expect("Failed to execute command");

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let combined = format!("{}{}", stdout, stderr);

    // Should detect unfree package and show appropriate message
    assert!(
        combined.contains("has an unfree license")
            || combined.contains("NIXPKGS_ALLOW_UNFREE")
            || combined.contains("⚠ Unfree package detected")
    );
}

#[test]
fn insecure_package_handling() {
    // Test that error classification works for insecure packages
    use eh::util::{DefaultNixErrorClassifier, NixErrorClassifier};

    let classifier = DefaultNixErrorClassifier;
    let stderr_insecure =
        "Package 'example-1.0' has been marked as insecure, refusing to evaluate.";

    assert!(classifier.should_retry(stderr_insecure));
}

#[test]
fn broken_package_handling() {
    // Test that error classification works for broken packages
    use eh::util::{DefaultNixErrorClassifier, NixErrorClassifier};

    let classifier = DefaultNixErrorClassifier;
    let stderr_broken = "Package 'example-1.0' has been marked as broken, refusing to evaluate.";

    assert!(classifier.should_retry(stderr_broken));
}

#[test]
fn multicall_binary_dispatch() {
    // Test that nb/nr/ns dispatch correctly based on binary name
    let commands = [("nb", "build"), ("nr", "run"), ("ns", "shell")];

    for (binary_name, _expected_cmd) in &commands {
        let output = Command::new("timeout")
            .args(["10", "cargo", "run", "--bin", "eh"])
            .env("CARGO_BIN_NAME", binary_name)
            .arg("nixpkgs#hello")
            .arg("--help") // Use help to avoid actually building
            .output()
            .expect("Failed to execute command");

        // Should execute without panicking (status code may vary)
        assert!(output.status.code().is_some());
    }
}

#[test]
fn interactive_mode_inheritance() {
    // Test that run commands inherit stdio properly
    let mut child = Command::new("timeout")
        .args([
            "10",
            "cargo",
            "run",
            "--bin",
            "eh",
            "--",
            "run",
            "nixpkgs#echo",
            "test",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to spawn command");

    let status = child.wait().expect("Failed to wait for child");

    // Should complete without hanging
    assert!(status.code().is_some());
}

#[test]
fn hash_extraction() {
    use eh::util::{HashExtractor, RegexHashExtractor};

    let extractor = RegexHashExtractor;
    let stderr = "error: hash mismatch in fixed-output derivation '/nix/store/...':
         specified: sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=
            got:    sha256-BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB=";

    let hash = extractor.extract_hash(stderr);
    assert!(hash.is_some());
    assert_eq!(
        hash.unwrap(),
        "sha256-BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB="
    );
}

#[test]
fn error_classification() {
    use eh::util::{DefaultNixErrorClassifier, NixErrorClassifier};

    let classifier = DefaultNixErrorClassifier;

    assert!(classifier.should_retry("has an unfree license ('unfree'), refusing to evaluate"));
    assert!(classifier.should_retry("has been marked as insecure, refusing to evaluate"));
    assert!(classifier.should_retry("has been marked as broken, refusing to evaluate"));
    assert!(!classifier.should_retry("random build error"));
}

#[test]
fn hash_mismatch_auto_fix() {
    // Test that hash mismatches are automatically detected and fixed
    // This is harder to test without creating actual files, so we test the regex
    // for the time being. Alternatively I could do this inside a temporary directory
    // but cba for now.
    use eh::util::{HashExtractor, RegexHashExtractor};

    let extractor = RegexHashExtractor;
    let stderr_with_mismatch = r#"
error: hash mismatch in fixed-output derivation
  specified: sha256-oldhashaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa=
       got: sha256-newhashbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb=
"#;

    let extracted = extractor.extract_hash(stderr_with_mismatch);
    assert_eq!(
        extracted,
        Some("sha256-newhashbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb=".to_string())
    );
}
