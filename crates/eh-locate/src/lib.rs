use std::{collections::BTreeSet, io::IsTerminal, path::Path};

use eh_error::{EhError, Result};
use spam_db::PackagesDb;

pub fn get_package_from_index(
  binary: &str,
  dbfile: impl AsRef<Path>,
) -> Result<String> {
  let dbfile = dbfile.as_ref();
  if binary.trim().is_empty() || binary.contains('/') {
    return Err(EhError::InvalidBinaryName {
      binary: binary.to_string(),
    });
  }
  let db = spam_db::PackagesDb::open(dbfile)?;
  let candidates = extract_candidates(db, binary)?;
  select_candidate(binary, candidates)
}

fn extract_candidates(db: PackagesDb, binary: &str) -> Result<Vec<String>> {
  let pattern = format!("/bin/{binary}");
  let mut attrs = BTreeSet::new();

  for result in db.query(&pattern)? {
    if result.path != pattern {
      continue;
    }
    for package in result.packages {
      attrs.insert(package);
    }
  }
  Ok(attrs.into_iter().collect())
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
