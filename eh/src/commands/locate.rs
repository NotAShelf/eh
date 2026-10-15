use std::io::{self, Write};

use eh::LocateArgs;
use eh_error::Result;
use eh_locate::{LocateOptions, LocatedFile};

pub fn handle_locate(args: LocateArgs) -> Result<i32> {
  let matches = eh_locate::locate(&LocateOptions {
    pattern:    args.pattern,
    regex:      args.regex,
    package:    args.package,
    file_types: args.file_types,
    at_root:    args.at_root,
    whole_name: args.whole_name,
    no_group:   args.no_group,
    database:   args.db,
  })?;
  print_matches(&mut io::stdout().lock(), &matches, args.minimal)?;
  Ok(0)
}

fn print_matches(
  output: &mut impl Write,
  matches: &[LocatedFile],
  minimal: bool,
) -> io::Result<()> {
  if minimal {
    // The shared query returns entries ordered by package, then path.
    let mut previous = None;
    for entry in matches {
      if previous != Some(entry.package.as_str()) {
        writeln!(output, "{}", entry.package)?;
        previous = Some(entry.package.as_str());
      }
    }
  } else {
    for entry in matches {
      writeln!(
        output,
        "{}\t{}\t{}\t{}",
        entry.package,
        entry.size,
        entry.file_type.as_str(),
        entry.path
      )?;
    }
  }
  Ok(())
}

#[cfg(test)]
mod tests {
  use eh_locate::FileType;

  use super::*;

  #[test]
  fn prints_indexed_paths_and_metadata_without_fabricating_store_paths() {
    let matches = vec![LocatedFile {
      package:   "nested.attr".into(),
      size:      1234,
      file_type: FileType::Executable,
      path:      "/bin/tool".into(),
    }];
    let mut output = Vec::new();
    print_matches(&mut output, &matches, false).unwrap();
    assert_eq!(
      String::from_utf8(output).unwrap(),
      "nested.attr\t1234\tx\t/bin/tool\n"
    );
  }

  #[test]
  fn minimal_output_deduplicates_packages() {
    let matches = vec![
      LocatedFile {
        package:   "tools".into(),
        path:      "/bin/a".into(),
        size:      1,
        file_type: FileType::Executable,
      },
      LocatedFile {
        package:   "tools".into(),
        path:      "/bin/b".into(),
        size:      2,
        file_type: FileType::Executable,
      },
    ];
    let mut output = Vec::new();
    print_matches(&mut output, &matches, true).unwrap();
    assert_eq!(output, b"tools\n");
  }
}
