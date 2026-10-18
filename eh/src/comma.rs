use std::{
  collections::HashMap,
  env,
  fs,
  io::{self, Write},
  os::unix::process::{CommandExt, ExitStatusExt},
  path::{Path, PathBuf},
  process::{Command, ExitStatus, Stdio},
};

use clap::{
  Args,
  CommandFactory,
  FromArgMatches,
  Parser,
  Subcommand,
  ValueHint,
  parser::ValueSource,
};
use clap_complete::Shell;
use eh_error::{EhError, Result};
use serde::{Deserialize, Serialize};

/// The opt-in comma frontend. Its database is SPAM, not nix-index.
#[derive(Parser)]
#[command(
  name = "comma",
  version,
  about = "Run programs without installing them, using the SPAM package index",
  subcommand_negates_reqs = true,
  disable_help_subcommand = true
)]
struct Options {
  /// Generate the man page, then exit
  #[arg(long, hide = true)]
  mangen: bool,

  /// Install the package containing the executable
  #[arg(short, long, conflicts_with_all = ["shell", "print_packages", "print_path"])]
  install: bool,

  /// Open a shell containing the package containing the executable
  #[arg(short, long, conflicts_with_all = ["print_packages", "print_path"])]
  shell: bool,

  /// Picker executable to use when multiple packages provide the command
  #[arg(short = 'P', long, env = "COMMA_PICKER", default_value = "fzy")]
  picker: String,

  /// Nixpkgs flake to use (an explicit value overrides NIX_PATH channels)
  #[arg(
    short = 'F',
    long,
    env = "COMMA_NIXPKGS_FLAKE",
    default_value = "nixpkgs"
  )]
  nixpkgs_flake: String,

  /// Ask before running the selected program
  #[arg(short, long, env = "COMMA_ASK_TO_CONFIRM")]
  ask: bool,

  /// Print completions for the given shell and exit
  #[arg(short = 'c', long = "print-completions")]
  print_completions: Option<Shell>,

  /// Print all packages containing the executable
  #[arg(short = 'p', long = "print-packages", conflicts_with = "print_path")]
  print_packages: bool,

  /// Print the absolute path to the executable in the Nix store
  #[arg(short = 'x', long = "print-path")]
  print_path: bool,

  /// Cache level: 0 disables caching, 1 caches choices, 2 also caches paths
  #[arg(long, env = "COMMA_CACHING", default_value_t = 2, value_parser = clap::value_parser!(u8).range(0..=2))]
  cache_level: u8,

  /// Empty the compatibility frontend's cache
  #[arg(short, long)]
  empty_cache: bool,

  /// Delete the command's cached choice before resolving it again
  #[arg(short, long)]
  delete_entry: bool,

  /// Command and its arguments (all flags after the command are passed
  /// through)
  #[arg(
    name = "cmd",
    required_unless_present_any = ["empty_cache", "mangen", "print_completions"],
    value_hint = ValueHint::Other,
    trailing_var_arg = true,
    allow_hyphen_values = true
  )]
  cmd: Vec<String>,

  #[command(subcommand)]
  subcmds: Option<Subcommands>,
}

#[derive(Subcommand)]
enum Subcommands {
  /// Show a program's manual using all outputs of its package
  Man(ManArgs),
}

#[derive(Args)]
struct ManArgs {
  /// Program whose manual should be shown, followed by man arguments
  #[arg(required = true, trailing_var_arg = true, allow_hyphen_values = true)]
  cmd: Vec<String>,
}

/// The standalone compatibility parser, also used for completion generation.
pub fn command() -> clap::Command {
  Options::command()
}

/// Run the comma-compatible frontend with arguments excluding argv[0].
pub fn run(args: impl IntoIterator<Item = String>) -> Result<i32> {
  let matches = match command()
    .try_get_matches_from(std::iter::once(String::from("comma")).chain(args))
  {
    Ok(matches) => matches,
    Err(error) => {
      let code = error.exit_code();
      error.print()?;
      return Ok(code);
    },
  };
  let explicit_flake =
    matches.value_source("nixpkgs_flake") != Some(ValueSource::DefaultValue);
  let options = Options::from_arg_matches(&matches)
    .map_err(|error| io::Error::other(error.to_string()))?;

  if options.mangen {
    clap_mangen::Man::new(command()).render(&mut io::stdout())?;
    return Ok(0);
  }
  if let Some(shell) = options.print_completions {
    let name = env::args_os()
      .next()
      .and_then(|arg| Path::new(&arg).file_name().map(|name| name.to_owned()))
      .and_then(|name| name.into_string().ok())
      .unwrap_or_else(|| String::from("comma"));
    clap_complete::generate(shell, &mut command(), name, &mut io::stdout());
    return Ok(0);
  }

  let source = Source::select(&options.nixpkgs_flake, explicit_flake);
  let mut cache = if options.cache_level > 0
    || options.empty_cache
    || options.delete_entry
  {
    match Cache::open(options.empty_cache) {
      Ok(cache) => Some(cache),
      Err(error) => {
        // Cache management must not claim success if nothing was changed.
        if options.empty_cache || options.delete_entry {
          return Err(error);
        }
        eprintln!("comma: cannot initialize cache; caching disabled: {error}");
        None
      },
    }
  } else {
    None
  };
  if options.empty_cache
    && let Some(cache) = &mut cache
  {
    cache.entries.clear();
    cache.save()?;
  }

  let (arguments, manual) = match &options.subcmds {
    Some(Subcommands::Man(args)) => (&args.cmd, true),
    None => (&options.cmd, false),
  };
  let Some((binary, trail)) = arguments.split_first() else {
    return Ok(if options.empty_cache { 0 } else { 1 });
  };
  if binary.trim().is_empty() || binary.contains('/') {
    return Err(EhError::InvalidBinaryName {
      binary: binary.clone(),
    });
  }
  let key = source.cache_key(binary)?;
  if options.delete_entry
    && let Some(cache) = &mut cache
  {
    cache.entries.remove(&key);
    cache.save()?;
  }
  if options.cache_level == 0 {
    cache = None;
  }

  if options.print_packages {
    let providers = providers(binary)?;
    println!("Packages that contain /bin/{binary}:");
    for provider in providers {
      println!("- {provider}");
    }
    return Ok(0);
  }

  let cached = cache
    .as_ref()
    .and_then(|cache| cache.entries.get(&key))
    .cloned();
  let mut entry = match cached {
    Some(entry) => entry,
    None => {
      let providers = providers(binary)?;
      let Some(derivation) = pick(&options.picker, &providers)? else {
        return Ok(1);
      };
      CacheEntry {
        derivation,
        path: None,
      }
    },
  };
  if options.cache_level < 2 {
    entry.path = None;
  }
  if let Some(cache) = &mut cache {
    cache.update(&key, &entry)?;
  }

  let mut child = if options.install {
    source.install(&entry.derivation)
  } else if options.shell {
    let mut child = source.shell(&entry.derivation);
    child.arg("--command").arg(select_shell()).args(trail);
    child
  } else if manual {
    let mut child = source.shell(&all_outputs(&entry.derivation));
    child.args(["--command", "man", binary]).args(trail);
    child
  } else {
    let path = command_path(&source, binary, &mut entry)?;
    if options.cache_level >= 2
      && let Some(cache) = &mut cache
    {
      cache.update(&key, &entry)?;
    }
    if options.print_path {
      println!("{}", path.display());
      return Ok(0);
    }
    let mut child = Command::new(path);
    child.args(trail);
    child
  };

  if options.ask && !confirm(&child)? {
    return Ok(0);
  }
  // All cache writes precede exec: destructors do not run after a successful
  // exec.
  Err(EhError::Execute {
    source:  child.exec(),
    program: PathBuf::from(child.get_program()),
  })
}

fn providers(binary: &str) -> Result<Vec<String>> {
  let providers = eh_locate::providers(binary)?;
  if providers.is_empty() {
    return Err(EhError::BinaryNotFound {
      binary: binary.to_owned(),
    });
  }
  Ok(providers)
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
struct CacheEntry {
  derivation: String,
  path:       Option<PathBuf>,
}

#[derive(Debug)]
struct Cache {
  path:    PathBuf,
  entries: HashMap<String, CacheEntry>,
}

impl Cache {
  fn open(reset: bool) -> Result<Self> {
    let path = dirs::cache_dir()
      .ok_or_else(|| io::Error::other("could not locate cache directory"))?
      .join("eh/comma/choices-v1.json");
    Self::at(path, reset)
  }

  fn at(path: PathBuf, reset: bool) -> Result<Self> {
    let entries = if reset {
      HashMap::new()
    } else {
      match fs::read(&path) {
        Ok(bytes) => {
          serde_json::from_slice(&bytes).map_err(|error| {
            io::Error::new(io::ErrorKind::InvalidData, error)
          })?
        },
        Err(error) if error.kind() == io::ErrorKind::NotFound => HashMap::new(),
        Err(error) => return Err(error.into()),
      }
    };
    Ok(Self { path, entries })
  }

  fn update(&mut self, key: &str, entry: &CacheEntry) -> Result<()> {
    if self.entries.get(key) != Some(entry) {
      self.entries.insert(key.to_owned(), entry.clone());
      self.save()?;
    }
    Ok(())
  }

  fn save(&self) -> Result<()> {
    let parent = self
      .path
      .parent()
      .ok_or_else(|| io::Error::other("cache path has no parent directory"))?;
    fs::create_dir_all(parent)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    serde_json::to_writer(temporary.as_file_mut(), &self.entries)
      .map_err(|error| io::Error::other(error.to_string()))?;
    temporary.as_file_mut().flush()?;
    temporary.persist(&self.path).map_err(|error| error.error)?;
    Ok(())
  }
}

#[derive(Debug, PartialEq, Eq)]
enum Source {
  Flake(String),
  Channel(String),
}

impl Source {
  fn select(flake: &str, explicit_flake: bool) -> Self {
    let nix_path = env::var("NIX_PATH").unwrap_or_default();
    // Keep the complete search path as cache identity. NIX_PATH entries can
    // contain URI colons, so Nix itself must resolve the <nixpkgs> lookup.
    if !explicit_flake
      && nix_path.split(':').any(|part| part.starts_with("nixpkgs="))
    {
      return Self::Channel(nix_path);
    }
    Self::Flake(flake.to_owned())
  }

  fn cache_key(&self, binary: &str) -> Result<String> {
    let (kind, source) = match self {
      Self::Flake(flake) => ("flake", flake),
      Self::Channel(path) => ("channel", path),
    };
    // JSON encoding keeps arbitrary flake references and command names
    // distinct.
    serde_json::to_string(&(kind, source, binary))
      .map_err(|error| io::Error::other(error.to_string()).into())
  }

  fn nix(&self, operation: &str) -> Command {
    let mut command = Command::new("nix");
    command.args([
      "--extra-experimental-features",
      "nix-command flakes",
      operation,
    ]);
    command
  }

  fn package(&self, command: &mut Command, derivation: &str) {
    match self {
      Self::Flake(flake) => {
        command.arg(format!("{flake}#{derivation}"));
      },
      Self::Channel(_) => {
        command.args(["--file", "<nixpkgs>", derivation]);
      },
    }
  }

  fn shell(&self, derivation: &str) -> Command {
    let mut command = self.nix("shell");
    self.package(&mut command, derivation);
    command
  }

  fn install(&self, derivation: &str) -> Command {
    match self {
      Self::Flake(flake) => {
        let mut command = self.nix("profile");
        command.args(["install", &format!("{flake}#{derivation}")]);
        command
      },
      Self::Channel(_) => {
        let mut command = Command::new("nix-env");
        command.args([
          "--file",
          "<nixpkgs>",
          "--install",
          "--attr",
          derivation,
        ]);
        command
      },
    }
  }
}

fn pick(picker: &str, providers: &[String]) -> Result<Option<String>> {
  if let [provider] = providers {
    return Ok(Some(provider.clone()));
  }
  let mut process = Command::new(picker)
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .spawn()
    .map_err(|source| {
      EhError::Picker {
        picker: picker.to_owned(),
        source,
      }
    })?;
  let mut input = process
    .stdin
    .take()
    .ok_or_else(|| io::Error::other("picker stdin was not piped"))?;
  let written = (|| -> io::Result<()> {
    for provider in providers {
      writeln!(input, "{provider}")?;
    }
    Ok(())
  })();
  drop(input);
  let output = process.wait_with_output()?;
  if !output.status.success() || output.stdout.is_empty() {
    return Ok(None);
  }
  written?;
  let selection = String::from_utf8(output.stdout)?.trim().to_owned();
  if !providers.contains(&selection) {
    return Err(
      io::Error::new(
        io::ErrorKind::InvalidData,
        "picker returned a package not in the provider list",
      )
      .into(),
    );
  }
  Ok(Some(selection))
}

fn cached_path(entry: &CacheEntry) -> Option<&Path> {
  entry.path.as_deref().filter(|path| path.is_file())
}

fn command_path(
  source: &Source,
  binary: &str,
  entry: &mut CacheEntry,
) -> Result<PathBuf> {
  if let Some(path) = cached_path(entry) {
    return Ok(path.to_owned());
  }
  let mut build = source.nix("build");
  build.args(["--print-out-paths", "--no-link"]);
  source.package(&mut build, &entry.derivation);
  let output = build.stderr(Stdio::inherit()).output().map_err(|source| {
    EhError::Execute {
      program: PathBuf::from(build.get_program()),
      source,
    }
  })?;
  check_status(output.status)?;
  let paths = String::from_utf8(output.stdout)?;
  // A package can have multiple outputs; use the one actually containing the
  // binary.
  let path = paths
    .lines()
    .filter(|line| !line.trim().is_empty())
    .map(|line| Path::new(line.trim()).join("bin").join(binary))
    .find(|path| path.is_file())
    .ok_or_else(|| {
      io::Error::new(
        io::ErrorKind::NotFound,
        format!(
          "selected package '{}' does not contain /bin/{binary}",
          entry.derivation
        ),
      )
    })?;
  entry.path = Some(path.clone());
  Ok(path)
}

fn check_status(status: ExitStatus) -> Result<()> {
  if status.success() {
    Ok(())
  } else {
    Err(EhError::ProcessExit {
      code: status
        .code()
        .unwrap_or_else(|| 128 + status.signal().unwrap_or(1)),
    })
  }
}

fn all_outputs(derivation: &str) -> String {
  let attribute = derivation
    .split_once('^')
    .map_or(derivation, |(attribute, _)| attribute);
  format!("{attribute}^*")
}

fn confirm(command: &Command) -> Result<bool> {
  loop {
    eprint!("Run '{}'? [Y/n]: ", command.get_program().to_string_lossy());
    io::stderr().flush()?;
    let mut answer = String::new();
    if io::stdin().read_line(&mut answer)? == 0 {
      return Ok(false);
    }
    match answer.trim().to_ascii_lowercase().as_str() {
      "" | "y" | "yes" => return Ok(true),
      "n" | "no" => return Ok(false),
      _ => eprintln!("Please enter 'y' or 'n'."),
    }
  }
}

fn select_shell() -> String {
  const SHELLS: &[&str] =
    &["ash", "bash", "elvish", "fish", "nu", "pwsh", "tcsh", "zsh"];
  let mut pid = std::process::id();
  // Follow the process tree on Linux so an interactive shell wins over $SHELL.
  while let Ok(status) = fs::read_to_string(format!("/proc/{pid}/status")) {
    let parent = status
      .lines()
      .find_map(|line| line.strip_prefix("PPid:"))
      .and_then(|parent| parent.trim().parse::<u32>().ok());
    let Some(parent) = parent.filter(|parent| *parent != 0 && *parent != pid)
    else {
      break;
    };
    pid = parent;
    if let Ok(status) = fs::read_to_string(format!("/proc/{pid}/comm")) {
      let name = status.trim();
      if SHELLS.contains(&name) {
        return name.to_owned();
      }
    }
  }
  env::var("SHELL")
    .ok()
    .filter(|shell| !shell.is_empty())
    .unwrap_or_else(|| String::from("bash"))
}

#[cfg(test)]
mod tests {
  use super::*;

  fn parse(args: &[&str]) -> Options {
    let matches = command()
      .try_get_matches_from(
        std::iter::once("comma").chain(args.iter().copied()),
      )
      .unwrap();
    Options::from_arg_matches(&matches).unwrap()
  }

  #[test]
  fn missing_picker_is_identified_only_when_selection_is_needed() {
    let directory = tempfile::tempdir().unwrap();
    let picker = directory.path().join("missing-picker");
    let picker = picker.to_str().unwrap();
    assert_eq!(pick(picker, &["htop".into()]).unwrap(), Some("htop".into()));

    let error = pick(picker, &["htop".into(), "htop-alt".into()]).unwrap_err();
    let EhError::Picker {
      picker: failed_picker,
      source,
    } = error
    else {
      panic!("expected a picker error")
    };
    assert_eq!(failed_picker, picker);
    assert_eq!(source.kind(), io::ErrorKind::NotFound);
  }

  #[test]
  fn child_flags_are_never_frontend_options() {
    let options =
      parse(&["hello", "--help", "-q", "--install", "--cache-level", "0"]);
    assert_eq!(options.cmd, [
      "hello",
      "--help",
      "-q",
      "--install",
      "--cache-level",
      "0"
    ]);
    assert!(!options.install);
    assert_eq!(options.cache_level, 2);
  }

  #[test]
  fn special_operations_need_no_binary() {
    assert!(parse(&["--empty-cache"]).empty_cache);
    assert!(parse(&["--mangen"]).mangen);
    assert_eq!(
      parse(&["--print-completions", "bash"]).print_completions,
      Some(Shell::Bash)
    );
  }

  #[test]
  fn manual_arguments_are_preserved() {
    let options = parse(&["man", "hello", "--help"]);
    let Some(Subcommands::Man(manual)) = options.subcmds else {
      panic!("expected man subcommand")
    };
    assert_eq!(manual.cmd, ["hello", "--help"]);
  }

  #[test]
  fn cache_level_is_bounded() {
    assert!(
      command()
        .try_get_matches_from(["comma", "--cache-level", "3", "hello"])
        .is_err()
    );
  }

  #[test]
  fn cache_keys_separate_sources_and_binary_names() {
    let flake = Source::Flake("nixpkgs".into());
    let custom = Source::Flake("github:owner/repo".into());
    let channel = Source::Channel("nixpkgs".into());
    assert_ne!(
      flake.cache_key("hello").unwrap(),
      custom.cache_key("hello").unwrap()
    );
    assert_ne!(
      flake.cache_key("hello").unwrap(),
      channel.cache_key("hello").unwrap()
    );
    assert_ne!(
      flake.cache_key("hello").unwrap(),
      flake.cache_key("other").unwrap()
    );
  }

  #[test]
  fn cache_roundtrips_and_can_reset_corrupt_data() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("nested/choices.json");
    let mut cache = Cache::at(path.clone(), false).unwrap();
    cache.entries.insert("key".into(), CacheEntry {
      derivation: "hello".into(),
      path:       None,
    });
    cache.save().unwrap();
    assert_eq!(
      Cache::at(path.clone(), false).unwrap().entries["key"].derivation,
      "hello"
    );
    fs::write(&path, "not JSON").unwrap();
    assert!(Cache::at(path.clone(), false).is_err());
    let cache = Cache::at(path.clone(), true).unwrap();
    cache.save().unwrap();
    assert!(Cache::at(path, false).unwrap().entries.is_empty());
  }

  #[test]
  fn garbage_collected_and_directory_paths_are_not_reused() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("hello");
    fs::write(&path, "binary").unwrap();
    let mut entry = CacheEntry {
      derivation: "hello".into(),
      path:       Some(path.clone()),
    };
    assert_eq!(cached_path(&entry), Some(path.as_path()));
    fs::remove_file(&path).unwrap();
    assert!(cached_path(&entry).is_none());
    entry.path = Some(directory.path().to_owned());
    assert!(cached_path(&entry).is_none());
  }

  #[test]
  fn man_output_selector_keeps_nested_attributes() {
    assert_eq!(all_outputs("python3Packages.foo"), "python3Packages.foo^*");
    assert_eq!(all_outputs("hello^out"), "hello^*");
  }
}
