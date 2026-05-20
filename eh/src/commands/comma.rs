use std::{collections::BTreeSet, env::var_os, io::IsTerminal, path::PathBuf};

use dirs;
use nix_index::{
  database::Reader,
  files::{FileTreeEntry, FileType},
  package::StorePath,
};
use regex::bytes::Regex;

use crate::{
  commands::handle_default_nix_command,
  error::{EhError, Result},
};

pub fn handle_comma(
  args: &[String],
  cfg: &eh_config::CommandConfig,
  ask: bool,
) -> crate::error::Result<i32> {
  let binary = args.first().ok_or(EhError::MissingBinary)?;
  let installable = resolve_binary(binary)?;
  let mut shell_args = Vec::with_capacity(args.len() + 2);
  shell_args.push(installable);
  shell_args.push("-c".to_string());
  shell_args.push(binary.clone());
  shell_args.extend(args.iter().skip(1).cloned());
  handle_default_nix_command("shell", &shell_args, cfg, ask)
}

fn get_index_db() -> Result<Reader> {
  let db_dir = if let Some(path) = var_os("NIX_INDEX_DATABASE") {
    PathBuf::from(path)
  } else {
    dirs::cache_dir()
      .ok_or_else(|| std::io::Error::other("could not locate cache directory"))?
      .join("nix-index")
  };
  Ok(Reader::open(db_dir.join("files"))?)
}

fn resolve_binary(binary: &str) -> Result<String> {
  if binary.trim().is_empty() || binary.contains('/') {
    return Err(EhError::InvalidBinaryName {
      binary: binary.to_string(),
    });
  }
  let db = get_index_db()?;
  let candidates = extract_candidates(db, binary)?;
  let attr = select_candidate(binary, candidates)?;
  Ok(format!("nixpkgs#{attr}"))
}

fn binary_pattern(binary: &str) -> Result<Regex> {
  let path = format!("/bin/{binary}");
  let escaped = regex::escape(&path);
  Ok(Regex::new(&format!("^{escaped}$"))?)
}

/// Reject a candidate if it's not runnable or if it's not toplevel
fn reject_candidate(store_path: &StorePath, entry: &FileTreeEntry) -> bool {
  !&store_path.origin().toplevel
    || !matches!(
      &entry.node.get_type(),
      FileType::Regular { executable: true } | FileType::Symlink
    )
}

fn extract_candidates(db: Reader, binary: &str) -> Result<Vec<String>> {
  let pattern = binary_pattern(binary)?;
  let mut attrs = BTreeSet::new();

  for result in db.query(&pattern).run()? {
    let (store_path, entry): (StorePath, FileTreeEntry) = result?;

    if reject_candidate(&store_path, &entry) {
      continue;
    }

    attrs.insert(store_path.origin().attr.clone());
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
