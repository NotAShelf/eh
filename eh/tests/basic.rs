use std::{fs, process::Command};

use eh::util::{
  DefaultNixErrorClassifier,
  DefaultNixFileFixer,
  HashExtractor,
  NixErrorClassifier,
  NixFileFixer,
  RegexHashExtractor,
};
use tempfile::TempDir;

#[test]
fn test_hash_extraction_from_real_nix_errors() {
  // Test hash extraction from actual Nix error messages
  let extractor = RegexHashExtractor;

  let test_cases = [
    (
      r#"error: hash mismatch in fixed-output derivation '/nix/store/xxx-foo.drv':
  specified: sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=
       got:    sha256-BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB="#,
      Some("sha256-BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB=".to_string()),
    ),
    (
      "actual: sha256-abc123def456",
      Some("sha256-abc123def456".to_string()),
    ),
    ("have: sha256-xyz789", Some("sha256-xyz789".to_string())),
    ("no hash here", None),
  ];

  for (input, expected) in test_cases {
    assert_eq!(extractor.extract_hash(input), expected);
  }
}

#[test]
fn test_error_classification_for_retry_logic() {
  // Test that the classifier correctly identifies errors that should be retried
  let classifier = DefaultNixErrorClassifier;

  // These should trigger retries
  let retry_cases = [
    "Package 'discord-1.0.0' has an unfree license ('unfree'), refusing to \
     evaluate.",
    "Package 'openssl-1.1.1' has been marked as insecure, refusing to \
     evaluate.",
    "Package 'broken-1.0' has been marked as broken, refusing to evaluate.",
    "hash mismatch in fixed-output derivation\ngot: sha256-newhash",
  ];

  for error in retry_cases {
    assert!(classifier.should_retry(error), "Should retry: {}", error);
  }

  // These should NOT trigger retries
  let no_retry_cases = [
    "build failed",
    "random error",
    "permission denied",
    "network error",
  ];

  for error in no_retry_cases {
    assert!(
      !classifier.should_retry(error),
      "Should not retry: {}",
      error
    );
  }
}

#[test]
fn test_hash_fixing_in_nix_files() {
  // Test that hash fixing actually works on real Nix files
  let temp_dir = TempDir::new().expect("Failed to create temp dir");
  let fixer = DefaultNixFileFixer;

  // Create a mock Nix file with various hash formats
  let nix_content = r#"
stdenv.mkDerivation {
  name = "test-package";
  src = fetchurl {
    url = "https://example.com.tar.gz";
    hash = "sha256-oldhash123";
  };

  buildInputs = [ fetchurl {
    url = "https://deps.com.tar.gz";
    sha256 = "sha256-oldhash456";
  }];

  outputHash = "sha256-oldhash789";
}
"#;

  let file_path = temp_dir.path().join("test.nix");
  fs::write(&file_path, nix_content).expect("Failed to write test file");

  // Test hash replacement
  let new_hash = "sha256-newhashabc";
  let was_fixed = fixer
    .fix_hash_in_file(&file_path, new_hash)
    .expect("Failed to fix hash");

  assert!(was_fixed, "File should have been modified");

  let updated_content =
    fs::read_to_string(&file_path).expect("Failed to read updated file");

  // All hash formats should be updated
  assert!(updated_content.contains(&format!(r#"hash = "{}""#, new_hash)));
  assert!(updated_content.contains(&format!(r#"sha256 = "{}""#, new_hash)));
  assert!(updated_content.contains(&format!(r#"outputHash = "{}""#, new_hash)));

  // Old hashes should be gone
  assert!(!updated_content.contains("oldhash123"));
  assert!(!updated_content.contains("oldhash456"));
  assert!(!updated_content.contains("oldhash789"));
}

#[test]
fn test_multicall_binary_dispatch() {
  // Test that multicall binaries work without needing actual Nix evaluation
  let commands = [("nb", "build"), ("nr", "run"), ("ns", "shell")];

  for (binary_name, _expected_command) in &commands {
    // Test that the binary starts and handles invalid arguments gracefully
    let output = Command::new("timeout")
      .args(["5", "cargo", "run", "--bin", "eh", "--"])
      .env("CARGO_BIN_NAME", binary_name)
      .arg("invalid-package-ref")
      .output()
      .expect("Failed to execute command");

    // Should fail gracefully (not panic or hang)
    assert!(
      output.status.code().is_some(),
      "{} should exit with a code",
      binary_name
    );

    // Should show an error message, not crash
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
      stderr.contains("Error:")
        || stderr.contains("error:")
        || stderr.contains("failed"),
      "{} should show error for invalid package",
      binary_name
    );
  }
}

#[test]
fn test_invalid_expression_handling() {
  // Test that invalid Nix expressions fail fast with proper error messages
  let invalid_refs = [
    "invalid-flake-ref",
    "nonexistent-package",
    "file:///nonexistent/path",
  ];

  for invalid_ref in invalid_refs {
    let output = Command::new("timeout")
      .args([
        "10",
        "cargo",
        "run",
        "--bin",
        "eh",
        "--",
        "build",
        invalid_ref,
      ])
      .output()
      .expect("Failed to execute command");

    // Should fail with a proper error, not hang or crash
    assert!(
      !output.status.success(),
      "Invalid ref '{}' should fail",
      invalid_ref
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
      stderr.contains("Error:")
        || stderr.contains("error:")
        || stderr.contains("failed"),
      "Should show error message for invalid ref '{}': {}",
      invalid_ref,
      stderr
    );
  }
}

#[test]
fn test_nix_file_discovery() {
  // Test that the fixer can find Nix files in a directory structure
  let temp_dir = TempDir::new().expect("Failed to create temp dir");
  let fixer = DefaultNixFileFixer;

  // Create directory structure with Nix files
  fs::create_dir_all(temp_dir.path().join("subdir"))
    .expect("Failed to create subdir");

  let files = [
    ("test.nix", "stdenv.mkDerivation { name = \"test\"; }"),
    ("subdir/other.nix", "pkgs.hello"),
    ("not-nix.txt", "not a nix file"),
    ("default.nix", "import ./test.nix"),
  ];

  for (path, content) in files {
    fs::write(temp_dir.path().join(path), content)
      .expect("Failed to write file");
  }

  // Change to temp dir for file discovery
  let original_dir =
    std::env::current_dir().expect("Failed to get current dir");
  std::env::set_current_dir(temp_dir.path())
    .expect("Failed to change directory");

  let found_files = fixer.find_nix_files().expect("Failed to find Nix files");

  // Should find 3 .nix files (not the .txt file)
  assert_eq!(found_files.len(), 3, "Should find exactly 3 .nix files");

  // Restore original directory
  std::env::set_current_dir(original_dir).expect("Failed to restore directory");
}
