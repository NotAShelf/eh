use std::{
  collections::BTreeSet,
  env::var_os,
  io::IsTerminal,
  path::PathBuf,
  str::FromStr,
};

use eh_error::{EhError, Result};
use spam_db::{FileKind, FileRecord, PackagesDb};

/// Find every package shipping an exact runnable `/bin/<binary>` entry.
/// Package identifiers come directly from SPAM, without output suffix
/// rewriting.
pub fn providers(binary: &str) -> Result<Vec<String>> {
  validate_binary(binary)?;
  let db = PackagesDb::open(database_path()?)?;
  let records = db.query(&format!("/bin/{binary}"))?;
  Ok(extract_candidates(binary, records))
}

pub fn get_package_from_index(binary: &str) -> Result<String> {
  select_candidate(binary, providers(binary)?)
}

fn validate_binary(binary: &str) -> Result<()> {
  if binary.trim().is_empty() || binary.contains('/') || binary.contains('\0') {
    return Err(EhError::InvalidBinaryName {
      binary: binary.to_string(),
    });
  }
  Ok(())
}

/// File types use the same one-letter representation as nix-locate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum FileType {
  Regular,
  Executable,
  Directory,
  Symlink,
}

impl FileType {
  pub const fn as_str(self) -> &'static str {
    match self {
      Self::Regular => "r",
      Self::Executable => "x",
      Self::Directory => "d",
      Self::Symlink => "s",
    }
  }

  fn of(record: &FileRecord) -> Self {
    match record.kind {
      FileKind::Regular if record.executable => Self::Executable,
      FileKind::Regular => Self::Regular,
      FileKind::Directory => Self::Directory,
      FileKind::Symlink => Self::Symlink,
    }
  }
}

impl FromStr for FileType {
  type Err = String;

  fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
    match value {
      "r" => Ok(Self::Regular),
      "x" => Ok(Self::Executable),
      "d" => Ok(Self::Directory),
      "s" => Ok(Self::Symlink),
      _ => Err("expected one of r, x, d, s".into()),
    }
  }
}

#[derive(Debug, Default)]
pub struct LocateOptions {
  pub pattern:    String,
  pub regex:      bool,
  pub package:    Option<String>,
  pub file_types: Vec<FileType>,
  pub at_root:    bool,
  pub whole_name: bool,
  pub no_group:   bool,
  pub database:   Option<PathBuf>,
}

/// A package file as indexed by SPAM. `path` is relative to an output root,
/// not a store path: the database does not provide store hashes.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct LocatedFile {
  pub package:   String,
  pub path:      String,
  pub size:      u64,
  pub file_type: FileType,
}

pub fn locate(options: &LocateOptions) -> Result<Vec<LocatedFile>> {
  // Compile before opening the database, so malformed patterns are reported
  // independently of whether an index has been generated yet.
  let matcher = LocateMatcher::new(options)?;
  let db = match &options.database {
    Some(path) => PackagesDb::open(path)?,
    None => PackagesDb::open(database_path()?)?,
  };
  // spam-db only supports literal substring queries. All indexed output paths
  // contain `/`; querying it scans every path, including bucketed databases
  // where an empty query would only scan bucket zero.
  let query = if options.regex || options.pattern.is_empty() {
    "/"
  } else {
    &options.pattern
  };
  Ok(matcher.filter(db.query(query)?))
}

struct LocateMatcher<'a> {
  options: &'a LocateOptions,
  path:    regex::Regex,
  package: Option<regex::Regex>,
}

impl<'a> LocateMatcher<'a> {
  fn new(options: &'a LocateOptions) -> Result<Self> {
    let make_pattern = |pattern: &str| {
      if options.regex {
        pattern.to_owned()
      } else {
        regex::escape(pattern)
      }
    };
    let pattern = make_pattern(&options.pattern);
    let start = if options.at_root { "^" } else { "" };
    let end = if options.whole_name { "$" } else { "" };
    let path = regex::Regex::new(&format!("{start}(?:{pattern}){end}"))?;
    let package = options
      .package
      .as_ref()
      .map(|pattern| regex::Regex::new(&make_pattern(pattern)))
      .transpose()?;
    Ok(Self {
      options,
      path,
      package,
    })
  }

  fn filter(&self, records: Vec<FileRecord>) -> Vec<LocatedFile> {
    let mut matches = BTreeSet::new();
    for record in records {
      let Some(last_match) = self.path.find_iter(&record.path).last() else {
        continue;
      };
      // Like nix-locate, a matching directory stands for its descendants by
      // default. --no-group keeps matches with further path components.
      if !self.options.no_group && record.path[last_match.end()..].contains('/')
      {
        continue;
      }
      let file_type = FileType::of(&record);
      if !self.options.file_types.is_empty()
        && !self.options.file_types.contains(&file_type)
      {
        continue;
      }
      for package in record.packages {
        if self
          .package
          .as_ref()
          .is_some_and(|pattern| !pattern.is_match(&package))
        {
          continue;
        }
        matches.insert(LocatedFile {
          package,
          path: record.path.clone(),
          size: record.size,
          file_type,
        });
      }
    }
    matches.into_iter().collect()
  }
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

  #[test]
  fn providers_are_sorted_deduplicated_and_exact() {
    let records = vec![
      record("/bin/tool", "z.tools", FileKind::Regular, true),
      record("/bin/tool", "a.tools", FileKind::Symlink, false),
      record("/bin/tool", "z.tools", FileKind::Regular, true),
      record("/sbin/tool", "wrong-root", FileKind::Regular, true),
      record("/bin/tool/child", "wrong-suffix", FileKind::Regular, true),
      record("/bin/tool", "directory", FileKind::Directory, true),
    ];
    assert_eq!(extract_candidates("tool", records), ["a.tools", "z.tools"]);
  }

  #[test]
  fn rejects_invalid_binary_names_before_opening_database() {
    for binary in ["", "  ", "/bin/hello", "foo/bar", "hello\0"] {
      assert!(matches!(
        providers(binary),
        Err(EhError::InvalidBinaryName { .. })
      ));
    }
  }

  #[test]
  fn selection_handles_missing_single_and_exact_package_names() {
    assert!(matches!(
      select_candidate("tool", vec![]),
      Err(EhError::BinaryNotFound { .. })
    ));
    assert_eq!(
      select_candidate("tool", vec!["suite".into()]).unwrap(),
      "suite"
    );
    assert_eq!(
      select_candidate("tool", vec!["suite".into(), "tool".into()]).unwrap(),
      "tool"
    );
  }

  fn query_records(
    options: &LocateOptions,
    records: Vec<FileRecord>,
  ) -> Vec<LocatedFile> {
    LocateMatcher::new(options).unwrap().filter(records)
  }

  #[test]
  fn literal_default_and_regex_apply_to_path_and_package() {
    let records = vec![
      record("/lib/a.so", "nested.attr", FileKind::Regular, false),
      record("/lib/axso", "nestedXattr", FileKind::Regular, false),
    ];
    let mut options = LocateOptions {
      pattern: "a.so".into(),
      package: Some("nested.attr".into()),
      ..Default::default()
    };
    let literal = query_records(&options, records.clone());
    assert_eq!(literal.len(), 1);
    assert_eq!(literal[0].path, "/lib/a.so");
    options.regex = true;
    assert_eq!(query_records(&options, records).len(), 2);
  }

  #[test]
  fn root_and_whole_name_anchor_the_entire_regex() {
    let records = vec![
      record("/bin/hello", "hello", FileKind::Regular, true),
      record("/bin/hello-extra", "extra", FileKind::Regular, true),
      record("/local/bin/hello", "local", FileKind::Regular, true),
    ];
    let options = LocateOptions {
      pattern: "/bin/hello|/bin/missing".into(),
      regex: true,
      at_root: true,
      whole_name: true,
      ..Default::default()
    };
    let found = query_records(&options, records);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].package, "hello");
  }

  #[test]
  fn grouping_suppresses_descendants_unless_requested() {
    let records = vec![
      record("/share/docs", "docs", FileKind::Directory, false),
      record("/share/docs/manual", "docs", FileKind::Regular, false),
      record("/share/docs/other/docs", "docs", FileKind::Directory, false),
    ];
    let mut options = LocateOptions {
      pattern: "docs".into(),
      ..Default::default()
    };
    // Grouping is based on the last match, as with nix-locate.
    assert_eq!(query_records(&options, records.clone()).len(), 2);
    options.no_group = true;
    assert_eq!(query_records(&options, records).len(), 3);
  }

  #[test]
  fn repeated_type_filters_are_a_union_and_regular_excludes_executable() {
    let records = vec![
      record("/bin/tool", "executable", FileKind::Regular, true),
      record("/bin/tool", "regular", FileKind::Regular, false),
      record("/bin/tool", "symlink", FileKind::Symlink, false),
      record("/bin/tool", "directory", FileKind::Directory, false),
    ];
    let mut options = LocateOptions {
      pattern: "tool".into(),
      file_types: vec![FileType::Executable, FileType::Symlink],
      ..Default::default()
    };
    let found = query_records(&options, records.clone());
    assert_eq!(
      found
        .iter()
        .map(|entry| entry.package.as_str())
        .collect::<Vec<_>>(),
      ["executable", "symlink"]
    );
    options.file_types = vec![FileType::Regular];
    let found = query_records(&options, records);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].package, "regular");
  }

  #[test]
  fn malformed_regex_is_reported_before_opening_database_override() {
    let options = LocateOptions {
      pattern: "[".into(),
      regex: true,
      database: Some("/missing/spam.db".into()),
      ..Default::default()
    };
    assert!(matches!(locate(&options), Err(EhError::Regex(_))));
  }
}
