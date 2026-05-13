use std::{
  io::{BufWriter, Write},
  path::{Path, PathBuf},
  sync::LazyLock,
};

use eh_log::log_info;
use regex::Regex;
use tempfile::NamedTempFile;
use walkdir::WalkDir;
use yansi::Paint;

use crate::error::{EhError, Result};

const MAX_DIR_DEPTH: usize = 3;

static HASH_EXTRACT_PATTERNS: LazyLock<[Regex; 3]> = LazyLock::new(|| {
  [
    Regex::new(r"got:\s+(sha256-[a-zA-Z0-9+/=]+)").unwrap(),
    Regex::new(r"actual:\s+(sha256-[a-zA-Z0-9+/=]+)").unwrap(),
    Regex::new(r"have:\s+(sha256-[a-zA-Z0-9+/=]+)").unwrap(),
  ]
});

static HASH_OLD_EXTRACT_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
  Regex::new(r"specified:\s+(sha256-[a-zA-Z0-9+/=]+)").unwrap()
});

static HASH_FIX_PATTERNS: LazyLock<[Regex; 3]> = LazyLock::new(|| {
  [
    Regex::new(r#"hash\s*=\s*"[^"]*""#).unwrap(),
    Regex::new(r#"sha256\s*=\s*"[^"]*""#).unwrap(),
    Regex::new(r#"outputHash\s*=\s*"[^"]*""#).unwrap(),
  ]
});

pub trait HashExtractor {
  fn extract_hash(&self, stderr: &str) -> Option<String>;
  fn extract_old_hash(&self, stderr: &str) -> Option<String>;
}

pub struct RegexHashExtractor;

impl HashExtractor for RegexHashExtractor {
  fn extract_hash(&self, stderr: &str) -> Option<String> {
    HASH_EXTRACT_PATTERNS.iter().find_map(|re| {
      re.captures(stderr)
        .and_then(|captures| captures.get(1))
        .map(|hash| hash.as_str().to_string())
    })
  }

  fn extract_old_hash(&self, stderr: &str) -> Option<String> {
    HASH_OLD_EXTRACT_PATTERN
      .captures(stderr)
      .and_then(|c| c.get(1))
      .map(|m| m.as_str().to_string())
  }
}

pub trait NixFileFixer {
  fn fix_hash_in_files(
    &self,
    old_hash: Option<&str>,
    new_hash: &str,
  ) -> Result<bool>;
  fn find_nix_files(&self) -> Result<Vec<PathBuf>>;
  fn fix_hash_in_file(
    &self,
    file_path: &Path,
    old_hash: Option<&str>,
    new_hash: &str,
  ) -> Result<bool>;
}

pub struct DefaultNixFileFixer;

impl NixFileFixer for DefaultNixFileFixer {
  fn fix_hash_in_files(
    &self,
    old_hash: Option<&str>,
    new_hash: &str,
  ) -> Result<bool> {
    let mut fixed = false;
    for file_path in self.find_nix_files()? {
      if self.fix_hash_in_file(&file_path, old_hash, new_hash)? {
        log_info!("updated hash in {}", file_path.display().bold());
        fixed = true;
      }
    }
    Ok(fixed)
  }

  fn find_nix_files(&self) -> Result<Vec<PathBuf>> {
    let files = WalkDir::new(".")
      .max_depth(MAX_DIR_DEPTH)
      .into_iter()
      .filter_entry(|entry| !should_skip(entry))
      .filter_map(std::result::Result::ok)
      .filter(|entry| {
        entry.file_type().is_file()
          && entry.path().extension().is_some_and(|ext| ext == "nix")
      })
      .map(|entry| entry.path().to_path_buf())
      .collect::<Vec<_>>();

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
    let content = std::fs::read_to_string(file_path)?;
    let result_content = if let Some(old) = old_hash {
      replace_target_hash(&content, old, new_hash)?
    } else {
      replace_any_hash(&content, new_hash)
    };

    if result_content == content {
      return Ok(false);
    }

    let temp_file =
      NamedTempFile::new_in(file_path.parent().unwrap_or(Path::new(".")))?;
    {
      let mut writer = BufWriter::new(temp_file.as_file());
      writer.write_all(result_content.as_bytes())?;
      writer.flush()?;
    }
    temp_file.persist(file_path).map_err(|_| {
      EhError::HashFixFailed {
        path: file_path.to_string_lossy().to_string(),
      }
    })?;
    Ok(true)
  }
}

fn should_skip(entry: &walkdir::DirEntry) -> bool {
  if entry.depth() == 0 || !entry.file_type().is_dir() {
    return false;
  }
  let name = entry.file_name().to_string_lossy();
  name.starts_with('.')
    || matches!(name.as_ref(), "node_modules" | "target" | "result")
}

fn replace_target_hash(
  content: &str,
  old_hash: &str,
  new_hash: &str,
) -> Result<String> {
  let old = regex::escape(old_hash);
  let replacements = [
    (
      Regex::new(&format!(r#"hash\s*=\s*"{old}""#))?,
      format!(r#"hash = "{new_hash}""#),
    ),
    (
      Regex::new(&format!(r#"sha256\s*=\s*"{old}""#))?,
      format!(r#"sha256 = "{new_hash}""#),
    ),
    (
      Regex::new(&format!(r#"outputHash\s*=\s*"{old}""#))?,
      format!(r#"outputHash = "{new_hash}""#),
    ),
  ];
  Ok(replace_with_patterns(
    content,
    replacements.iter().map(|(re, value)| (re, value.as_str())),
  ))
}

fn replace_any_hash(content: &str, new_hash: &str) -> String {
  let replacements = [
    format!(r#"hash = "{new_hash}""#),
    format!(r#"sha256 = "{new_hash}""#),
    format!(r#"outputHash = "{new_hash}""#),
  ];
  replace_with_patterns(
    content,
    HASH_FIX_PATTERNS
      .iter()
      .zip(replacements.iter().map(String::as_str)),
  )
}

fn replace_with_patterns<'a>(
  content: &str,
  patterns: impl Iterator<Item = (&'a Regex, &'a str)>,
) -> String {
  patterns.fold(content.to_string(), |acc, (re, replacement)| {
    re.replace_all(&acc, replacement).into_owned()
  })
}

pub fn is_hash_mismatch_error(stderr: &str) -> bool {
  stderr.contains("hash mismatch")
    || (stderr.contains("specified:") && stderr.contains("got:"))
}

#[cfg(test)]
mod tests {
  use std::io::Write;

  use tempfile::NamedTempFile;

  use super::*;

  #[test]
  fn extracts_new_and_old_hashes() {
    let stderr = "specified: sha256-OLD\n got: sha256-NEW=";
    let extractor = RegexHashExtractor;
    assert_eq!(
      extractor.extract_hash(stderr),
      Some("sha256-NEW=".to_string())
    );
    assert_eq!(
      extractor.extract_old_hash(stderr),
      Some("sha256-OLD".to_string())
    );
  }

  #[test]
  fn replaces_only_matching_old_hash() {
    let file = NamedTempFile::new().unwrap();
    let path = file.path();
    std::fs::write(
      path,
      r#"hash = "sha256-old";
sha256 = "sha256-other";
"#,
    )
    .unwrap();

    assert!(
      DefaultNixFileFixer
        .fix_hash_in_file(path, Some("sha256-old"), "sha256-new")
        .unwrap()
    );
    let updated = std::fs::read_to_string(path).unwrap();
    assert!(updated.contains(r#"hash = "sha256-new""#));
    assert!(updated.contains(r#"sha256 = "sha256-other""#));
  }

  #[test]
  fn replaces_all_hash_attributes_without_old_hash() {
    let file = NamedTempFile::new().unwrap();
    let path = file.path();
    let mut writer = std::fs::File::create(path).unwrap();
    writer
      .write_all(
        br#"hash = "a";
sha256 = "b";
outputHash = "c";
"#,
      )
      .unwrap();

    assert!(
      DefaultNixFileFixer
        .fix_hash_in_file(path, None, "sha256-new")
        .unwrap()
    );
    let updated = std::fs::read_to_string(path).unwrap();
    assert_eq!(updated.matches("sha256-new").count(), 3);
  }
}
