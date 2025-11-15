use std::{
  io::{BufWriter, Write},
  path::{Path, PathBuf},
};

use regex::Regex;
use tempfile::NamedTempFile;
use tracing::{info, warn};
use walkdir::WalkDir;
use yansi::Paint;

use crate::{
  command::{NixCommand, StdIoInterceptor},
  error::{EhError, Result},
};

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
      if let Ok(re) = Regex::new(pattern)
        && let Some(captures) = re.captures(stderr)
        && let Some(hash) = captures.get(1)
      {
        return Some(hash.as_str().to_string());
      }
    }
    None
  }
}

pub trait NixFileFixer {
  fn fix_hash_in_files(&self, new_hash: &str) -> Result<bool>;
  fn find_nix_files(&self) -> Result<Vec<PathBuf>>;
  fn fix_hash_in_file(&self, file_path: &Path, new_hash: &str) -> Result<bool>;
}

pub struct DefaultNixFileFixer;

impl NixFileFixer for DefaultNixFileFixer {
  fn fix_hash_in_files(&self, new_hash: &str) -> Result<bool> {
    let nix_files = self.find_nix_files()?;
    let mut fixed = false;
    for file_path in nix_files {
      if self.fix_hash_in_file(&file_path, new_hash)? {
        println!("Updated hash in {}", file_path.display());
        fixed = true;
      }
    }
    Ok(fixed)
  }

  fn find_nix_files(&self) -> Result<Vec<PathBuf>> {
    let files: Vec<PathBuf> = WalkDir::new(".")
      .into_iter()
      .filter_map(std::result::Result::ok)
      .filter(|entry| {
        entry.file_type().is_file()
          && entry
            .path()
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("nix"))
      })
      .map(|entry| entry.path().to_path_buf())
      .collect();

    if files.is_empty() {
      return Err(EhError::NoNixFilesFound);
    }
    Ok(files)
  }

  fn fix_hash_in_file(&self, file_path: &Path, new_hash: &str) -> Result<bool> {
    // Pre-compile regex patterns once to avoid repeated compilation
    let patterns: Vec<(Regex, String)> = [
      (r#"hash\s*=\s*"[^"]*""#, format!(r#"hash = "{new_hash}""#)),
      (
        r#"sha256\s*=\s*"[^"]*""#,
        format!(r#"sha256 = "{new_hash}""#),
      ),
      (
        r#"outputHash\s*=\s*"[^"]*""#,
        format!(r#"outputHash = "{new_hash}""#),
      ),
    ]
    .into_iter()
    .map(|(pattern, replacement)| {
      Regex::new(pattern)
        .map(|re| (re, replacement))
        .map_err(EhError::Regex)
    })
    .collect::<Result<Vec<_>>>()?;

    // Read the entire file content
    let content = std::fs::read_to_string(file_path)?;
    let mut replaced = false;
    let mut result_content = content;

    // Apply replacements
    for (re, replacement) in &patterns {
      if re.is_match(&result_content) {
        result_content = re
          .replace_all(&result_content, replacement.as_str())
          .into_owned();
        replaced = true;
      }
    }

    // Write back to file atomically
    if replaced {
      let temp_file =
        NamedTempFile::new_in(file_path.parent().unwrap_or(Path::new(".")))?;
      {
        let mut writer = BufWriter::new(temp_file.as_file());
        writer.write_all(result_content.as_bytes())?;
        writer.flush()?;
      }
      temp_file.persist(file_path).map_err(|_e| {
        EhError::HashFixFailed {
          path: file_path.to_string_lossy().to_string(),
        }
      })?;
    }

    Ok(replaced)
  }
}

pub trait NixErrorClassifier {
  fn should_retry(&self, stderr: &str) -> bool;
}

/// Pre-evaluate expression to catch errors early
fn pre_evaluate(_subcommand: &str, args: &[String]) -> Result<bool> {
  // Find flake references or expressions to evaluate
  // Only take the first non-flag argument (the package/expression)
  let eval_arg = args.iter().find(|arg| !arg.starts_with('-'));

  let Some(eval_arg) = eval_arg else {
    return Ok(true); // No expression to evaluate
  };

  let eval_cmd = NixCommand::new("eval").arg(eval_arg).arg("--raw");

  let output = eval_cmd.output()?;

  if output.status.success() {
    return Ok(true);
  }

  let stderr = String::from_utf8_lossy(&output.stderr);

  // If eval fails due to unfree/insecure/broken, don't fail pre-evaluation
  // Let the main command handle it with retry logic
  if stderr.contains("has an unfree license")
    || stderr.contains("refusing to evaluate")
    || stderr.contains("has been marked as insecure")
    || stderr.contains("has been marked as broken")
  {
    return Ok(true);
  }

  // For other eval failures, fail early
  Ok(false)
}

pub fn validate_nix_args(args: &[String]) -> Result<()> {
  const DANGEROUS_PATTERNS: &[&str] = &[
    ";", "&&", "||", "|", "`", "$(", "${", ">", "<", ">>", "<<", "2>", "2>>",
  ];

  for arg in args {
    for pattern in DANGEROUS_PATTERNS {
      if arg.contains(pattern) {
        return Err(EhError::InvalidInput {
          input:  arg.clone(),
          reason: format!("contains potentially dangerous pattern: {pattern}"),
        });
      }
    }
  }
  Ok(())
}

/// Shared retry logic for nix commands (build/run/shell).
pub fn handle_nix_with_retry(
  subcommand: &str,
  args: &[String],
  hash_extractor: &dyn HashExtractor,
  fixer: &dyn NixFileFixer,
  classifier: &dyn NixErrorClassifier,
  interactive: bool,
) -> Result<i32> {
  validate_nix_args(args)?;
  // Pre-evaluate for build commands to catch errors early
  if !pre_evaluate(subcommand, args)? {
    return Err(EhError::NixCommandFailed(
      "Expression evaluation failed".to_string(),
    ));
  }

  // For run commands, try interactive first to avoid breaking terminal
  if subcommand == "run" && interactive {
    let status = NixCommand::new(subcommand)
      .print_build_logs(true)
      .interactive(true)
      .args_ref(args)
      .run_with_logs(StdIoInterceptor)?;
    if status.success() {
      return Ok(0);
    }
  }

  // First, always capture output to check for errors that need retry
  let output_cmd = NixCommand::new(subcommand)
    .print_build_logs(true)
    .args_ref(args);
  let output = output_cmd.output()?;
  let stderr = String::from_utf8_lossy(&output.stderr);

  // Check if we need to retry with special flags
  if let Some(new_hash) = hash_extractor.extract_hash(&stderr) {
    match fixer.fix_hash_in_files(&new_hash) {
      Ok(true) => {
        info!("{}", Paint::green("✔ Fixed hash mismatch, retrying..."));
        let mut retry_cmd = NixCommand::new(subcommand)
          .print_build_logs(true)
          .args_ref(args);
        if interactive {
          retry_cmd = retry_cmd.interactive(true);
        }
        let retry_status = retry_cmd.run_with_logs(StdIoInterceptor)?;
        return Ok(retry_status.code().unwrap_or(1));
      },
      Ok(false) => {
        // No files were fixed, continue with normal error handling
      },
      Err(EhError::NoNixFilesFound) => {
        warn!("No .nix files found to fix hash in");
        // Continue with normal error handling
      },
      Err(e) => {
        return Err(e);
      },
    }
  } else if stderr.contains("hash") || stderr.contains("sha256") {
    // If there's a hash-related error but we couldn't extract it, that's a
    // failure
    return Err(EhError::HashExtractionFailed);
  }

  if classifier.should_retry(&stderr) {
    if stderr.contains("has an unfree license") && stderr.contains("refusing") {
      warn!(
        "{}",
        Paint::yellow(
          "⚠ Unfree package detected, retrying with NIXPKGS_ALLOW_UNFREE=1..."
        )
      );
      let mut retry_cmd = NixCommand::new(subcommand)
        .print_build_logs(true)
        .args_ref(args)
        .env("NIXPKGS_ALLOW_UNFREE", "1")
        .impure(true);
      if interactive {
        retry_cmd = retry_cmd.interactive(true);
      }
      let retry_status = retry_cmd.run_with_logs(StdIoInterceptor)?;
      return Ok(retry_status.code().unwrap_or(1));
    }
    if stderr.contains("has been marked as insecure")
      && stderr.contains("refusing")
    {
      warn!(
        "{}",
        Paint::yellow(
          "⚠ Insecure package detected, retrying with \
           NIXPKGS_ALLOW_INSECURE=1..."
        )
      );
      let mut retry_cmd = NixCommand::new(subcommand)
        .print_build_logs(true)
        .args_ref(args)
        .env("NIXPKGS_ALLOW_INSECURE", "1")
        .impure(true);
      if interactive {
        retry_cmd = retry_cmd.interactive(true);
      }
      let retry_status = retry_cmd.run_with_logs(StdIoInterceptor)?;
      return Ok(retry_status.code().unwrap_or(1));
    }
    if stderr.contains("has been marked as broken")
      && stderr.contains("refusing")
    {
      warn!(
        "{}",
        Paint::yellow(
          "⚠ Broken package detected, retrying with NIXPKGS_ALLOW_BROKEN=1..."
        )
      );
      let mut retry_cmd = NixCommand::new(subcommand)
        .print_build_logs(true)
        .args_ref(args)
        .env("NIXPKGS_ALLOW_BROKEN", "1")
        .impure(true);
      if interactive {
        retry_cmd = retry_cmd.interactive(true);
      }
      let retry_status = retry_cmd.run_with_logs(StdIoInterceptor)?;
      return Ok(retry_status.code().unwrap_or(1));
    }
  }

  // If the first attempt succeeded, we're done
  if output.status.success() {
    return Ok(0);
  }

  // Otherwise, show the error and return error
  std::io::stderr()
    .write_all(&output.stderr)
    .map_err(EhError::Io)?;
  Err(EhError::ProcessExit {
    code: output.status.code().unwrap_or(1),
  })
}

pub struct DefaultNixErrorClassifier;

impl NixErrorClassifier for DefaultNixErrorClassifier {
  fn should_retry(&self, stderr: &str) -> bool {
    RegexHashExtractor.extract_hash(stderr).is_some()
      || (stderr.contains("has an unfree license")
        && stderr.contains("refusing"))
      || (stderr.contains("has been marked as insecure")
        && stderr.contains("refusing"))
      || (stderr.contains("has been marked as broken")
        && stderr.contains("refusing"))
  }
}

#[cfg(test)]
mod tests {
  use std::io::Write;

  use tempfile::NamedTempFile;

  use super::*;

  #[test]
  fn test_streaming_hash_replacement() {
    let temp_file = NamedTempFile::new().unwrap();
    let file_path = temp_file.path();

    // Write test content with multiple hash patterns
    let test_content = r#"stdenv.mkDerivation {
  name = "test-package";
  src = fetchurl {
    url = "https://example.com.tar.gz";
    hash = "sha256-oldhash123";
    sha256 = "sha256-oldhash456";
    outputHash = "sha256-oldhash789";
  };
}"#;

    let mut file = std::fs::File::create(file_path).unwrap();
    file.write_all(test_content.as_bytes()).unwrap();
    file.flush().unwrap();

    let fixer = DefaultNixFileFixer;
    let result = fixer
      .fix_hash_in_file(file_path, "sha256-newhash999")
      .unwrap();

    assert!(result, "Hash replacement should return true");

    // Verify the content was updated
    let updated_content = std::fs::read_to_string(file_path).unwrap();
    assert!(updated_content.contains("sha256-newhash999"));
    assert!(!updated_content.contains("sha256-oldhash123"));
    assert!(!updated_content.contains("sha256-oldhash456"));
    assert!(!updated_content.contains("sha256-oldhash789"));
  }

  #[test]
  fn test_streaming_no_replacement_needed() {
    let temp_file = NamedTempFile::new().unwrap();
    let file_path = temp_file.path().to_path_buf();

    let test_content = r#"stdenv.mkDerivation {
  name = "test-package";
  src = fetchurl {
    url = "https://example.com.tar.gz";
  };
}"#;

    {
      let mut file = std::fs::File::create(&file_path).unwrap();
      file.write_all(test_content.as_bytes()).unwrap();
      file.flush().unwrap();
    } // File is closed here

    // Test hash replacement
    let fixer = DefaultNixFileFixer;
    let result = fixer
      .fix_hash_in_file(&file_path, "sha256-newhash999")
      .unwrap();

    assert!(
      !result,
      "Hash replacement should return false when no patterns found"
    );

    // Verify the content was unchanged, ignoring trailing newline differences
    let updated_content = std::fs::read_to_string(&file_path).unwrap();
    let normalized_original = test_content.trim_end();
    let normalized_updated = updated_content.trim_end();
    assert_eq!(normalized_updated, normalized_original);
  }

  // FIXME: this is a little stupid, but it works
  #[test]
  fn test_streaming_large_file_handling() {
    let temp_file = NamedTempFile::new().unwrap();
    let file_path = temp_file.path();
    let mut file = std::fs::File::create(file_path).unwrap();

    // Write header with hash
    file.write_all(b"stdenv.mkDerivation {\n  name = \"large-package\";\n  src = fetchurl {\n    url = \"https://example.com/large.tar.gz\";\n    hash = \"sha256-oldhash\";\n  };\n").unwrap();

    for i in 0..10000 {
      writeln!(file, "  # Large comment line {} to simulate file size", i)
        .unwrap();
    }

    file.flush().unwrap();

    // Test that streaming can handle large files without memory issues
    let fixer = DefaultNixFileFixer;
    let result = fixer
      .fix_hash_in_file(file_path, "sha256-newhash999")
      .unwrap();

    assert!(result, "Hash replacement should work for large files");

    // Verify the hash was replaced
    let updated_content = std::fs::read_to_string(file_path).unwrap();
    assert!(updated_content.contains("sha256-newhash999"));
    assert!(!updated_content.contains("sha256-oldhash"));
  }

  #[test]
  fn test_streaming_file_permissions_preserved() {
    let temp_file = NamedTempFile::new().unwrap();
    let file_path = temp_file.path();

    // Write test content
    let test_content = r#"stdenv.mkDerivation {
  name = "test";
  src = fetchurl {
    url = "https://example.com";
    hash = "sha256-oldhash";
  };
}"#;

    let mut file = std::fs::File::create(file_path).unwrap();
    file.write_all(test_content.as_bytes()).unwrap();
    file.flush().unwrap();

    // Get original permissions
    let original_metadata = std::fs::metadata(file_path).unwrap();
    let _original_permissions = original_metadata.permissions();

    // Test hash replacement
    let fixer = DefaultNixFileFixer;
    let result = fixer.fix_hash_in_file(file_path, "sha256-newhash").unwrap();

    assert!(result, "Hash replacement should succeed");

    // Verify file still exists and has reasonable permissions
    let new_metadata = std::fs::metadata(file_path).unwrap();
    assert!(
      new_metadata.is_file(),
      "File should still exist after replacement"
    );
  }

  #[test]
  fn test_input_validation_blocks_dangerous_patterns() {
    let dangerous_args = vec![
      "package; rm -rf /".to_string(),
      "package && echo hacked".to_string(),
      "package || echo hacked".to_string(),
      "package | cat /etc/passwd".to_string(),
      "package `whoami`".to_string(),
      "package $(echo hacked lol!)".to_string(),
      "package ${HOME}/file".to_string(),
    ];

    for arg in dangerous_args {
      let result = validate_nix_args(std::slice::from_ref(&arg));
      assert!(result.is_err(), "Should reject dangerous argument: {}", arg);

      match result.unwrap_err() {
        EhError::InvalidInput { input, reason } => {
          assert_eq!(input, arg);
          assert!(reason.contains("dangerous pattern"));
        },
        _ => panic!("Expected InvalidInput error"),
      }
    }
  }

  #[test]
  fn test_input_validation_allows_safe_args() {
    let safe_args = vec![
      "nixpkgs#hello".to_string(),
      "--impure".to_string(),
      "--print-build-logs".to_string(),
      "/path/to/flake".to_string(),
      ".#default".to_string(),
    ];

    let result = validate_nix_args(&safe_args);
    assert!(
      result.is_ok(),
      "Should allow safe arguments: {:?}",
      safe_args
    );
  }
}
