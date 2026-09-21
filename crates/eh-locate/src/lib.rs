use std::{collections::BTreeSet, env::var_os, io::IsTerminal, path::PathBuf};

use eh_error::{EhError, Result};
use spam_db::{FileKind, FileRecord, PackagesDb};

pub fn get_package_from_index(binary: &str) -> Result<String> {
  if binary.trim().is_empty() || binary.contains('/') {
    return Err(EhError::InvalidBinaryName {
      binary: binary.to_string(),
    });
  }

  let db = PackagesDb::open(database_path()?)?;
  let records = db.query(&format!("/bin/{binary}"))?;
  select_candidate(binary, extract_candidates(binary, records))
}

fn database_path() -> Result<PathBuf> {
  if let Some(path) = var_os("SPAM_DATABASE") {
    return Ok(PathBuf::from(path));
  }
  dirs::cache_dir()
    .map(|path| path.join("spam").join("spam.db"))
    .ok_or_else(|| std::io::Error::other("could not locate cache directory"))
    .map_err(EhError::from)
}

fn extract_candidates(binary: &str, records: Vec<FileRecord>) -> Vec<String> {
  let path = format!("/bin/{binary}");
  records
    .into_iter()
    .filter(|record| record.path == path)
    .filter(|record| {
      record.kind == FileKind::Symlink
        || (record.kind == FileKind::Regular && record.executable)
    })
    .flat_map(|record| record.packages)
    .collect::<BTreeSet<_>>()
    .into_iter()
    .collect()
}

fn select_candidate(binary: &str, candidates: Vec<String>) -> Result<String> {
  match candidates.as_slice() {
    [] => {
      Err(EhError::BinaryNotFound {
        binary: binary.into(),
      })
    },
    [candidate] => Ok(candidate.clone()),
    _ => {
      if let Some(candidate) = candidates
        .iter()
        .find(|candidate| candidate.as_str() == binary)
      {
        return Ok(candidate.clone());
      }
      select_ambiguous_candidate(binary, candidates)
    },
  }
}

fn select_ambiguous_candidate(
  binary: &str,
  candidates: Vec<String>,
) -> Result<String> {
  if !std::io::stdin().is_terminal() {
    return Err(EhError::AmbiguousBinary {
      binary: binary.into(),
      candidates,
    });
  }

  dialoguer::Select::new()
    .with_prompt(format!("Multiple packages provide `{binary}`"))
    .items(&candidates)
    .default(0)
    .interact()
    .map(|idx| candidates[idx].clone())
    .map_err(|e| EhError::Io(std::io::Error::other(e)))
}

#[cfg(test)]
mod tests {
  use super::*;

  fn record(
    path: &str,
    package: &str,
    kind: FileKind,
    executable: bool,
  ) -> FileRecord {
    FileRecord {
      path: path.into(),
      packages: vec![package.into()],
      size: 0,
      kind,
      executable,
      target: String::new(),
    }
  }

  #[test]
  fn extracts_exact_runnable_package_candidates() {
    let records = vec![
      record("/bin/hello", "hello", FileKind::Regular, true),
      record("/bin/hello", "hello", FileKind::Symlink, false),
      record("/bin/hello", "docs", FileKind::Regular, false),
      record("/bin/hello-extra", "other", FileKind::Regular, true),
    ];
    assert_eq!(extract_candidates("hello", records), ["hello"]);
  }
}
