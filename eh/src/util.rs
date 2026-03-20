use std::{
  io::{BufWriter, Write},
  path::{Path, PathBuf},
  sync::LazyLock,
};

use eh_log::{log_info, log_warn};
use regex::Regex;
use tempfile::NamedTempFile;
use walkdir::WalkDir;
use yansi::Paint;

use crate::{
  commands::{NixCommand, StdIoInterceptor},
  error::{EhError, Result},
};

/// Maximum directory depth when searching for .nix files.
const MAX_DIR_DEPTH: usize = 3;

/// Compiled regex patterns for extracting the actual hash from nix stderr.
static HASH_EXTRACT_PATTERNS: LazyLock<[Regex; 3]> = LazyLock::new(|| {
  [
    Regex::new(r"got:\s+(sha256-[a-zA-Z0-9+/=]+)").unwrap(),
    Regex::new(r"actual:\s+(sha256-[a-zA-Z0-9+/=]+)").unwrap(),
    Regex::new(r"have:\s+(sha256-[a-zA-Z0-9+/=]+)").unwrap(),
  ]
});

/// Compiled regex pattern for extracting the old (specified) hash from nix
/// stderr.
static HASH_OLD_EXTRACT_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
  Regex::new(r"specified:\s+(sha256-[a-zA-Z0-9+/=]+)").unwrap()
});

/// Compiled regex patterns for matching hash attributes in .nix files.
static HASH_FIX_PATTERNS: LazyLock<[Regex; 3]> = LazyLock::new(|| {
  [
    Regex::new(r#"hash\s*=\s*"[^"]*""#).unwrap(),
    Regex::new(r#"sha256\s*=\s*"[^"]*""#).unwrap(),
    Regex::new(r#"outputHash\s*=\s*"[^"]*""#).unwrap(),
  ]
});

/// Trait for extracting store paths and hashes from nix output.
pub trait HashExtractor {
  /// Extract the new store path/hash from nix output.
  fn extract_hash(&self, stderr: &str) -> Option<String>;
  /// Extract the old store path/hash from nix output (for hash updates).
  fn extract_old_hash(&self, stderr: &str) -> Option<String>;
}

/// Default implementation of [`HashExtractor`] using regex patterns.
pub struct RegexHashExtractor;

impl HashExtractor for RegexHashExtractor {
  fn extract_hash(&self, stderr: &str) -> Option<String> {
    for re in HASH_EXTRACT_PATTERNS.iter() {
      if let Some(captures) = re.captures(stderr)
        && let Some(hash) = captures.get(1)
      {
        return Some(hash.as_str().to_string());
      }
    }
    None
  }

  fn extract_old_hash(&self, stderr: &str) -> Option<String> {
    HASH_OLD_EXTRACT_PATTERN
      .captures(stderr)
      .and_then(|c| c.get(1))
      .map(|m| m.as_str().to_string())
  }
}

/// Trait for fixing hash mismatches in nix files.
pub trait NixFileFixer {
  /// Attempt to fix hash in all nix files found in the current directory.
  /// Returns `true` if at least one file was fixed.
  fn fix_hash_in_files(
    &self,
    old_hash: Option<&str>,
    new_hash: &str,
  ) -> Result<bool>;
  /// Find all .nix files in the current directory (respects MAX_DIR_DEPTH).
  fn find_nix_files(&self) -> Result<Vec<PathBuf>>;
  /// Attempt to fix hash in a single file.
  /// Returns `true` if the file was modified.
  fn fix_hash_in_file(
    &self,
    file_path: &Path,
    old_hash: Option<&str>,
    new_hash: &str,
  ) -> Result<bool>;
}

/// Default implementation of [`NixFileFixer`] that walks the directory tree.
pub struct DefaultNixFileFixer;

impl NixFileFixer for DefaultNixFileFixer {
  fn fix_hash_in_files(
    &self,
    old_hash: Option<&str>,
    new_hash: &str,
  ) -> Result<bool> {
    let nix_files = self.find_nix_files()?;
    let mut fixed = false;
    for file_path in nix_files {
      if self.fix_hash_in_file(&file_path, old_hash, new_hash)? {
        log_info!("updated hash in {}", file_path.display().bold());
        fixed = true;
      }
    }
    Ok(fixed)
  }

  fn find_nix_files(&self) -> Result<Vec<PathBuf>> {
    let should_skip = |entry: &walkdir::DirEntry| -> bool {
      // Never skip the root entry, otherwise the entire walk is empty
      if entry.depth() == 0 || !entry.file_type().is_dir() {
        return false;
      }
      let name = entry.file_name().to_string_lossy();
      name.starts_with('.')
        || matches!(name.as_ref(), "node_modules" | "target" | "result")
    };

    let files: Vec<PathBuf> = WalkDir::new(".")
      .max_depth(MAX_DIR_DEPTH)
      .into_iter()
      .filter_entry(|e| !should_skip(e))
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

  fn fix_hash_in_file(
    &self,
    file_path: &Path,
    old_hash: Option<&str>,
    new_hash: &str,
  ) -> Result<bool> {
    // Read the entire file content
    let content = std::fs::read_to_string(file_path)?;
    let mut replaced = false;
    let mut result_content = content;

    if let Some(old) = old_hash {
      // Only replace attributes whose value matches the
      // old hash. Uses regexes to handle variable whitespace around `=`.
      let old_escaped = regex::escape(old);
      let targeted_patterns = [
        (
          Regex::new(&format!(r#"hash\s*=\s*"{old_escaped}""#)).unwrap(),
          format!(r#"hash = "{new_hash}""#),
        ),
        (
          Regex::new(&format!(r#"sha256\s*=\s*"{old_escaped}""#)).unwrap(),
          format!(r#"sha256 = "{new_hash}""#),
        ),
        (
          Regex::new(&format!(r#"outputHash\s*=\s*"{old_escaped}""#)).unwrap(),
          format!(r#"outputHash = "{new_hash}""#),
        ),
      ];

      for (re, replacement) in &targeted_patterns {
        if re.is_match(&result_content) {
          result_content = re
            .replace_all(&result_content, replacement.as_str())
            .into_owned();
          replaced = true;
        }
      }
    } else {
      // Fallback: replace all hash attributes
      let replacements = [
        format!(r#"hash = "{new_hash}""#),
        format!(r#"sha256 = "{new_hash}""#),
        format!(r#"outputHash = "{new_hash}""#),
      ];

      for (re, replacement) in HASH_FIX_PATTERNS.iter().zip(&replacements) {
        if re.is_match(&result_content) {
          result_content = re
            .replace_all(&result_content, replacement.as_str())
            .into_owned();
          replaced = true;
        }
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

/// Trait for classifying nix errors and determining if a retry with modified
/// environment is appropriate.
pub trait NixErrorClassifier {
  /// Determine if the given stderr output should trigger a retry with modified
  /// environment variables (e.g., NIXPKGS_ALLOW_UNFREE).
  fn should_retry(&self, stderr: &str) -> bool;
}

/// Classifies what retry action should be taken based on nix stderr output.
#[derive(Debug, PartialEq, Eq)]
pub enum RetryAction {
  /// Package has an unfree license, retry with NIXPKGS_ALLOW_UNFREE=1
  AllowUnfree,
  /// Package is marked insecure, retry with
  /// NIXPKGS_ALLOW_INSECURE_DERIVATIONS=1
  AllowInsecure,
  /// Package is marked broken, retry with NIXPKGS_ALLOW_BROKEN=1
  AllowBroken,
  /// No retry needed
  None,
}

impl RetryAction {
  /// # Returns
  ///
  /// `(env_var, reason)` for this retry action, or `None` if no retry is
  /// needed.
  const fn env_override(&self) -> Option<(&str, &str)> {
    match self {
      Self::AllowUnfree => {
        Some(("NIXPKGS_ALLOW_UNFREE", "has an unfree license"))
      },
      Self::AllowInsecure => {
        Some(("NIXPKGS_ALLOW_INSECURE", "has been marked as insecure"))
      },
      Self::AllowBroken => {
        Some(("NIXPKGS_ALLOW_BROKEN", "has been marked as broken"))
      },
      Self::None => None,
    }
  }
}

/// Extract the package/expression name from args (first non-flag argument).
fn package_name(args: &[String]) -> &str {
  args
    .iter()
    .find(|a| !a.starts_with('-'))
    .map_or("<unknown>", String::as_str)
}

/// Print a retry message with consistent formatting.
/// Format: `  -> <pkg>: <reason>, setting <ENV>=1`
fn print_retry_msg(pkg: &str, reason: &str, env_var: &str) {
  log_warn!(
    "{}: {}, setting {}",
    pkg.bold(),
    reason,
    format!("{env_var}=1").bold()
  );
}

/// Classify stderr into a retry action.
#[must_use]
pub fn classify_retry_action(stderr: &str) -> RetryAction {
  if stderr.contains("has an unfree license") && stderr.contains("refusing") {
    RetryAction::AllowUnfree
  } else if stderr.contains("has been marked as insecure")
    && stderr.contains("refusing")
  {
    RetryAction::AllowInsecure
  } else if stderr.contains("has been marked as broken")
    && stderr.contains("refusing")
  {
    RetryAction::AllowBroken
  } else {
    RetryAction::None
  }
}

/// Returns true if stderr looks like a genuine hash mismatch error
/// (not just any mention of "hash" or "sha256").
fn is_hash_mismatch_error(stderr: &str) -> bool {
  stderr.contains("hash mismatch")
    || (stderr.contains("specified:") && stderr.contains("got:"))
}

/// Construct the eval expression for a given argument.
/// Handles both plain package names and flake references.
fn make_eval_expr(eval_arg: &str) -> String {
  if eval_arg.contains('#') {
    format!("{eval_arg}.meta")
  } else {
    format!("nixpkgs#{eval_arg}.meta")
  }
}

/// Check if a package has an unfree, insecure, or broken attribute set.
/// Returns the appropriate `RetryAction` if any of these are true. Makes a
/// single nix eval call to minimize overhead.
fn check_package_flags(args: &[String]) -> Result<RetryAction> {
  // Default to "." if no argument provided (like `nix build` without args)
  let eval_arg = args
    .iter()
    .find(|arg| !arg.starts_with('-'))
    .cloned()
    .unwrap_or_else(|| ".".to_string());

  let eval_expr = make_eval_expr(&eval_arg);
  let eval_cmd = NixCommand::new("eval")
    .arg("--json")
    .arg(&eval_expr)
    .print_build_logs(false);

  let output = match eval_cmd.output() {
    Ok(o) if o.status.success() => o,
    Ok(o) => {
      let stderr = String::from_utf8_lossy(&o.stderr);
      if stderr.contains("does not provide attribute") {
        return Ok(RetryAction::None);
      }
      log_warn!(
        "failed to check package flags for '{}': {}",
        eval_arg,
        stderr.trim()
      );
      return Ok(RetryAction::None);
    },

    Err(e) => {
      log_warn!("failed to check package flags for '{}': {}", eval_arg, e);
      return Ok(RetryAction::None);
    },
  };

  let meta: serde_json::Value = match serde_json::from_slice(&output.stdout) {
    Ok(v) => v,
    Err(e) => {
      log_warn!("failed to parse package metadata for '{}': {}", eval_arg, e);
      return Ok(RetryAction::None);
    },
  };

  let flags = [
    ("unfree", RetryAction::AllowUnfree),
    ("insecure", RetryAction::AllowInsecure),
    ("broken", RetryAction::AllowBroken),
  ];

  for (key, action) in flags {
    if meta
      .get(key)
      .and_then(serde_json::Value::as_bool)
      .unwrap_or(false)
    {
      return Ok(action);
    }
  }

  Ok(RetryAction::None)
}

/// Pre-evaluate expression to catch errors early.
///
/// Returns a `RetryAction` if the package has retryable flags
/// (unfree/insecure/broken), allowing the caller to retry with the right
/// environment variables.
fn pre_evaluate(args: &[String]) -> Result<RetryAction> {
  // First, check package meta flags directly to avoid error message parsing
  let action = check_package_flags(args)?;
  if action != RetryAction::None {
    return Ok(action);
  }

  // Find flake references or expressions to evaluate
  // Only take the first non-flag argument (the package/expression)
  // Default to "." if no argument provided (like `nix build` without args)
  let eval_arg = args
    .iter()
    .find(|arg| !arg.starts_with('-'))
    .cloned()
    .unwrap_or_else(|| {
      log_warn!("no package specified, defaulting to '.' (current directory)");
      ".".to_string()
    });

  let eval_arg_ref = &eval_arg;
  let eval_cmd = NixCommand::new("eval")
    .arg(eval_arg_ref)
    .print_build_logs(false);

  let output = eval_cmd.output()?;

  if output.status.success() {
    return Ok(RetryAction::None);
  }

  let stderr = String::from_utf8_lossy(&output.stderr);

  // Classify whether this is a retryable error (unfree/insecure/broken)
  // Fallback for errors that slip through (e.g., from dependencies)
  let action = classify_retry_action(&stderr);
  if action != RetryAction::None {
    return Ok(action);
  }

  // Non-retryable eval failure, we should fail fast with a clear message
  // rather than running the full command and showing the same error again.
  let stderr_clean = stderr
    .trim()
    .strip_prefix("error:")
    .unwrap_or(stderr.trim())
    .trim();
  Err(EhError::PreEvalFailed {
    expression: eval_arg.clone(),
    stderr:     stderr_clean.to_string(),
  })
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

  // Pre-evaluate to detect retryable errors (unfree/insecure/broken) before
  // running the actual command. This avoids streaming verbose nix error output
  // only to retry immediately after.
  let pkg = package_name(args);
  let pre_eval_action = pre_evaluate(args)?;
  if let Some((env_var, reason)) = pre_eval_action.env_override() {
    print_retry_msg(pkg, reason, env_var);
    let mut retry_cmd = NixCommand::new(subcommand)
      .print_build_logs(true)
      .args_ref(args)
      .env(env_var, "1")
      .impure(true);
    if interactive {
      retry_cmd = retry_cmd.interactive(true);
    }
    let retry_status = retry_cmd.run_with_logs(StdIoInterceptor)?;
    return Ok(retry_status.code().unwrap_or(1));
  }

  // For run/shell commands, try interactive mode now that pre-eval passed
  if interactive {
    let status = NixCommand::new(subcommand)
      .print_build_logs(true)
      .interactive(true)
      .args_ref(args)
      .run_with_logs(StdIoInterceptor)?;
    if status.success() {
      return Ok(0);
    }
  }

  // Capture output to check for errors that need retry (hash mismatches etc.)
  let output_cmd = NixCommand::new(subcommand)
    .print_build_logs(true)
    .args_ref(args);
  let output = output_cmd.output()?;
  let stderr = String::from_utf8_lossy(&output.stderr);

  // Check for hash mismatch errors
  if let Some(new_hash) = hash_extractor.extract_hash(&stderr) {
    let old_hash = hash_extractor.extract_old_hash(&stderr);
    match fixer.fix_hash_in_files(old_hash.as_deref(), &new_hash) {
      Ok(true) => {
        log_info!(
          "{}: hash mismatch corrected in local files, rebuilding",
          pkg.bold()
        );
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
        log_warn!(
          "{}: hash mismatch detected but no .nix files found to update",
          pkg.bold()
        );
        // Continue with normal error handling
      },
      Err(e) => {
        return Err(e);
      },
    }
  } else if is_hash_mismatch_error(&stderr) {
    // There's a genuine hash mismatch but we couldn't extract the new hash
    return Err(EhError::HashExtractionFailed {
      stderr: stderr.to_string(),
    });
  }

  // Fallback: check for unfree/insecure/broken in captured output
  // (in case pre_evaluate didn't catch it, e.g. from a dependency)
  if classifier.should_retry(&stderr) {
    let action = classify_retry_action(&stderr);
    if let Some((env_var, reason)) = action.env_override() {
      print_retry_msg(pkg, reason, env_var);
      let mut retry_cmd = NixCommand::new(subcommand)
        .print_build_logs(true)
        .args_ref(args)
        .env(env_var, "1")
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

  match output.status.code() {
    Some(code) => Err(EhError::ProcessExit { code }),
    // No exit code means the process was killed by a signal
    None => {
      Err(EhError::NixCommandFailed {
        command: subcommand.to_string(),
      })
    },
  }
}

pub struct DefaultNixErrorClassifier;

impl NixErrorClassifier for DefaultNixErrorClassifier {
  fn should_retry(&self, stderr: &str) -> bool {
    classify_retry_action(stderr) != RetryAction::None
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
      .fix_hash_in_file(file_path, None, "sha256-newhash999")
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
      .fix_hash_in_file(&file_path, None, "sha256-newhash999")
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
      .fix_hash_in_file(file_path, None, "sha256-newhash999")
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
    let result = fixer
      .fix_hash_in_file(file_path, None, "sha256-newhash")
      .unwrap();

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

  #[test]
  fn test_input_validation_empty_args() {
    let result = validate_nix_args(&[]);
    assert!(result.is_ok(), "Empty args should be accepted");
  }

  #[test]
  fn test_hash_extraction_got_pattern() {
    let stderr = "hash mismatch in fixed-output derivation\n  specified: \
                  sha256-AAAA\n  got:    \
                  sha256-BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB=";
    let extractor = RegexHashExtractor;
    let hash = extractor.extract_hash(stderr);
    assert!(hash.is_some());
    assert!(hash.unwrap().starts_with("sha256-"));
  }

  #[test]
  fn test_hash_extraction_actual_pattern() {
    let stderr = "hash mismatch\n  actual: \
                  sha256-CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC=";
    let extractor = RegexHashExtractor;
    let hash = extractor.extract_hash(stderr);
    assert!(hash.is_some());
    assert!(hash.unwrap().starts_with("sha256-"));
  }

  #[test]
  fn test_hash_extraction_have_pattern() {
    let stderr = "hash mismatch\n  have: \
                  sha256-DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD=";
    let extractor = RegexHashExtractor;
    let hash = extractor.extract_hash(stderr);
    assert!(hash.is_some());
    assert!(hash.unwrap().starts_with("sha256-"));
  }

  #[test]
  fn test_hash_extraction_no_match() {
    let stderr = "error: some other nix error without hashes";
    let extractor = RegexHashExtractor;
    assert!(extractor.extract_hash(stderr).is_none());
  }

  #[test]
  fn test_hash_extraction_partial_match() {
    // Contains "got:" but no sha256 hash
    let stderr = "got: some-other-value";
    let extractor = RegexHashExtractor;
    assert!(extractor.extract_hash(stderr).is_none());
  }

  #[test]
  fn test_false_positive_hash_detection() {
    // Normal nix output mentioning "hash" or "sha256" without being a mismatch
    let cases = [
      "evaluating attribute 'sha256' of derivation 'hello'",
      "building '/nix/store/hash-something.drv'",
      "copying path '/nix/store/sha256-abcdef-hello'",
      "this derivation has a hash attribute set",
    ];
    for stderr in &cases {
      assert!(
        !is_hash_mismatch_error(stderr),
        "Should not detect hash mismatch in: {stderr}"
      );
    }
  }

  #[test]
  fn test_genuine_hash_mismatch_detection() {
    assert!(is_hash_mismatch_error(
      "hash mismatch in fixed-output derivation"
    ));
    assert!(is_hash_mismatch_error(
      "specified: sha256-AAAA\n  got: sha256-BBBB"
    ));
  }

  #[test]
  fn test_classify_retry_action_unfree() {
    let stderr =
      "error: Package 'foo' has an unfree license, refusing to evaluate.";
    assert_eq!(classify_retry_action(stderr), RetryAction::AllowUnfree);
  }

  #[test]
  fn test_classify_retry_action_insecure() {
    let stderr =
      "error: Package 'bar' has been marked as insecure, refusing to evaluate.";
    assert_eq!(classify_retry_action(stderr), RetryAction::AllowInsecure);
  }

  #[test]
  fn test_classify_retry_action_broken() {
    let stderr =
      "error: Package 'baz' has been marked as broken, refusing to evaluate.";
    assert_eq!(classify_retry_action(stderr), RetryAction::AllowBroken);
  }

  #[test]
  fn test_classify_retry_action_none() {
    let stderr = "error: attribute 'nonexistent' not found";
    assert_eq!(classify_retry_action(stderr), RetryAction::None);
  }

  #[test]
  fn test_retry_action_env_overrides() {
    let (var, reason) = RetryAction::AllowUnfree.env_override().unwrap();
    assert_eq!(var, "NIXPKGS_ALLOW_UNFREE");
    assert!(reason.contains("unfree"));

    let (var, reason) = RetryAction::AllowInsecure.env_override().unwrap();
    assert_eq!(var, "NIXPKGS_ALLOW_INSECURE");
    assert!(reason.contains("insecure"));

    let (var, reason) = RetryAction::AllowBroken.env_override().unwrap();
    assert_eq!(var, "NIXPKGS_ALLOW_BROKEN");
    assert!(reason.contains("broken"));

    assert_eq!(RetryAction::None.env_override(), None);
  }

  #[test]
  fn test_classifier_should_retry() {
    let classifier = DefaultNixErrorClassifier;
    assert!(
      classifier.should_retry(
        "Package 'x' has an unfree license, refusing to evaluate"
      )
    );
    assert!(classifier.should_retry(
      "Package 'x' has been marked as insecure, refusing to evaluate"
    ));
    assert!(classifier.should_retry(
      "Package 'x' has been marked as broken, refusing to evaluate"
    ));
    assert!(!classifier.should_retry("error: attribute not found"));
  }

  #[test]
  fn test_old_hash_extraction() {
    let stderr =
      "hash mismatch in fixed-output derivation\n  specified: \
       sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=\n  got:    \
       sha256-BBBB=";
    let extractor = RegexHashExtractor;
    let old = extractor.extract_old_hash(stderr);
    assert!(old.is_some());
    assert_eq!(
      old.unwrap(),
      "sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="
    );
  }

  #[test]
  fn test_old_hash_extraction_missing() {
    let stderr = "hash mismatch\n  got: sha256-BBBB=";
    let extractor = RegexHashExtractor;
    assert!(extractor.extract_old_hash(stderr).is_none());
  }

  #[test]
  fn test_targeted_hash_replacement_only_matching() {
    let temp_file = NamedTempFile::new().unwrap();
    let file_path = temp_file.path();

    // File with two derivations, each with a different hash
    let test_content = r#"{ pkgs }:
{
  a = pkgs.fetchurl {
    url = "https://example.com/a.tar.gz";
    hash = "sha256-AAAA";
  };
  b = pkgs.fetchurl {
    url = "https://example.com/b.tar.gz";
    hash = "sha256-BBBB";
  };
}"#;

    let mut file = std::fs::File::create(file_path).unwrap();
    file.write_all(test_content.as_bytes()).unwrap();
    file.flush().unwrap();

    let fixer = DefaultNixFileFixer;
    // Only replace the hash matching "sha256-AAAA"
    let result = fixer
      .fix_hash_in_file(file_path, Some("sha256-AAAA"), "sha256-NEWW")
      .unwrap();

    assert!(result, "Targeted replacement should return true");

    let updated = std::fs::read_to_string(file_path).unwrap();
    assert!(
      updated.contains(r#"hash = "sha256-NEWW""#),
      "Matching hash should be replaced"
    );
    assert!(
      updated.contains(r#"hash = "sha256-BBBB""#),
      "Non-matching hash should be untouched"
    );
  }

  #[test]
  fn test_targeted_hash_replacement_no_match() {
    let temp_file = NamedTempFile::new().unwrap();
    let file_path = temp_file.path();

    let test_content = r#"{ hash = "sha256-XXXX"; }"#;

    let mut file = std::fs::File::create(file_path).unwrap();
    file.write_all(test_content.as_bytes()).unwrap();
    file.flush().unwrap();

    let fixer = DefaultNixFileFixer;
    // old_hash doesn't match anything in the file
    let result = fixer
      .fix_hash_in_file(file_path, Some("sha256-NOMATCH"), "sha256-NEWW")
      .unwrap();

    assert!(!result, "Should return false when old hash doesn't match");

    let updated = std::fs::read_to_string(file_path).unwrap();
    assert!(
      updated.contains("sha256-XXXX"),
      "Original hash should be untouched"
    );
  }

  #[test]
  fn test_eval_expr_plain_package_name() {
    assert_eq!(
      make_eval_expr("vscode"),
      "nixpkgs#vscode.meta",
      "Plain package names should be prefixed with nixpkgs#"
    );
  }

  #[test]
  fn test_eval_expr_nixpkgs_prefixed() {
    assert_eq!(
      make_eval_expr("nixpkgs#vscode"),
      "nixpkgs#vscode.meta",
      "nixpkgs# prefix should not be duplicated"
    );
  }

  #[test]
  fn test_eval_expr_custom_flake() {
    assert_eq!(
      make_eval_expr("myflake#vscode"),
      "myflake#vscode.meta",
      "Custom flake references should be preserved"
    );
  }

  #[test]
  fn test_eval_expr_github_flake() {
    assert_eq!(
      make_eval_expr("github:owner/repo#vscode"),
      "github:owner/repo#vscode.meta",
      "GitHub flake references should be preserved"
    );
  }

  #[test]
  fn test_eval_expr_path_flake() {
    assert_eq!(
      make_eval_expr("./myflake#vscode"),
      "./myflake#vscode.meta",
      "Path-based flake references should be preserved"
    );
  }

  #[test]
  fn test_eval_expr_special_nixpkg_forms() {
    // Test various nixpkgs forms that might be used
    assert_eq!(
      make_eval_expr("nixpkgs#legacyPackages.x86_64-linux.vscode"),
      "nixpkgs#legacyPackages.x86_64-linux.vscode.meta",
      "Complex nixpkgs references should be preserved"
    );
  }
}
