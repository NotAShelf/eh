use std::{
  io::{self, Read, Write},
  process::{Command, ExitStatus, Output, Stdio},
  sync::mpsc,
  thread,
  time::{Duration, Instant},
};

use thiserror::Error;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Debug, Error)]
pub enum Error {
  #[error("io: {0}")]
  Io(#[from] io::Error),
  #[error("command '{command}' failed")]
  CommandFailed { command: String },
  #[error("nix {command} timed out after {} seconds", duration.as_secs())]
  Timeout {
    command:  String,
    duration: Duration,
  },
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommandKind {
  Build,
  Develop,
  Eval,
  Flake,
  Run,
  Shell,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CommandSpec {
  pub name:             &'static str,
  pub print_build_logs: bool,
  pub interactive:      bool,
}

pub const COMMAND_SPECS: &[CommandSpec] = &[
  CommandSpec {
    name:             "build",
    print_build_logs: true,
    interactive:      false,
  },
  CommandSpec {
    name:             "develop",
    print_build_logs: true,
    interactive:      true,
  },
  CommandSpec {
    name:             "eval",
    print_build_logs: false,
    interactive:      false,
  },
  CommandSpec {
    name:             "flake",
    print_build_logs: false,
    interactive:      false,
  },
  CommandSpec {
    name:             "run",
    print_build_logs: true,
    interactive:      true,
  },
  CommandSpec {
    name:             "shell",
    print_build_logs: true,
    interactive:      true,
  },
];

impl CommandKind {
  #[must_use]
  pub const fn as_str(self) -> &'static str {
    self.spec().name
  }

  #[must_use]
  pub const fn spec(self) -> CommandSpec {
    match self {
      Self::Build => COMMAND_SPECS[0],
      Self::Develop => COMMAND_SPECS[1],
      Self::Eval => COMMAND_SPECS[2],
      Self::Flake => COMMAND_SPECS[3],
      Self::Run => COMMAND_SPECS[4],
      Self::Shell => COMMAND_SPECS[5],
    }
  }
}

impl TryFrom<&str> for CommandKind {
  type Error = UnknownCommand;

  fn try_from(value: &str) -> std::result::Result<Self, Self::Error> {
    match value {
      "build" => Ok(Self::Build),
      "develop" => Ok(Self::Develop),
      "eval" => Ok(Self::Eval),
      "flake" => Ok(Self::Flake),
      "run" => Ok(Self::Run),
      "shell" => Ok(Self::Shell),
      command => {
        Err(UnknownCommand {
          command: command.to_string(),
        })
      },
    }
  }
}

#[derive(Debug, Error, Eq, PartialEq)]
#[error("unknown nix command '{command}'")]
pub struct UnknownCommand {
  command: String,
}

pub struct StdIo;

impl StdIo {
  fn on_stderr(&mut self, chunk: &[u8]) {
    let _ = io::stderr().write_all(chunk);
  }

  fn on_stdout(&mut self, chunk: &[u8]) {
    let _ = io::stdout().write_all(chunk);
  }
}

#[derive(Debug)]
enum PipeEvent {
  Stdout(Vec<u8>),
  Stderr(Vec<u8>),
  Error(io::Error),
}

fn read_pipe<R: Read>(
  mut reader: R,
  tx: mpsc::Sender<PipeEvent>,
  is_stderr: bool,
) {
  let mut buf = [0u8; 4096];
  loop {
    match reader.read(&mut buf) {
      Ok(0) => break,
      Ok(n) => {
        let event = if is_stderr {
          PipeEvent::Stderr(buf[..n].to_vec())
        } else {
          PipeEvent::Stdout(buf[..n].to_vec())
        };
        if tx.send(event).is_err() {
          break;
        }
      },
      Err(e) => {
        let _ = tx.send(PipeEvent::Error(e));
        break;
      },
    }
  }
}

pub struct NixCommand {
  kind:                    CommandKind,
  binary:                  Option<String>,
  args:                    Vec<String>,
  env:                     Vec<(String, String)>,
  impure:                  bool,
  print_build_logs:        bool,
  interactive:             bool,
  eval_profiler_mode:      Option<String>,
  eval_profiler_frequency: Option<u32>,
  eval_profile_file:       Option<String>,
}

impl NixCommand {
  #[must_use]
  pub fn new(kind: CommandKind) -> Self {
    let spec = kind.spec();
    Self {
      kind,
      binary: None,
      args: Vec::new(),
      env: Vec::new(),
      impure: false,
      print_build_logs: spec.print_build_logs,
      interactive: spec.interactive,
      eval_profiler_mode: None,
      eval_profiler_frequency: None,
      eval_profile_file: None,
    }
  }

  #[must_use]
  pub fn arg<S: Into<String>>(mut self, arg: S) -> Self {
    self.args.push(arg.into());
    self
  }

  #[must_use]
  pub fn args_ref(mut self, args: &[String]) -> Self {
    self.args.extend(args.iter().cloned());
    self
  }

  #[must_use]
  pub fn env<K: Into<String>, V: Into<String>>(
    mut self,
    key: K,
    value: V,
  ) -> Self {
    self.env.push((key.into(), value.into()));
    self
  }

  #[must_use]
  pub fn envs<I, K, V>(mut self, env: I) -> Self
  where
    I: IntoIterator<Item = (K, V)>,
    K: Into<String>,
    V: Into<String>,
  {
    self
      .env
      .extend(env.into_iter().map(|(k, v)| (k.into(), v.into())));
    self
  }

  #[must_use]
  pub const fn impure(mut self, yes: bool) -> Self {
    self.impure = yes;
    self
  }

  #[must_use]
  pub fn binary<S: Into<String>>(mut self, path: S) -> Self {
    self.binary = Some(path.into());
    self
  }

  #[must_use]
  pub const fn interactive(mut self, yes: bool) -> Self {
    self.interactive = yes;
    self
  }

  #[must_use]
  pub const fn print_build_logs(mut self, yes: bool) -> Self {
    self.print_build_logs = yes;
    self
  }

  #[must_use]
  pub fn eval_profiler<S: Into<String>>(mut self, mode: S) -> Self {
    self.eval_profiler_mode = Some(mode.into());
    self
  }

  #[must_use]
  pub const fn eval_profiler_frequency(mut self, hz: u32) -> Self {
    self.eval_profiler_frequency = Some(hz);
    self
  }

  #[must_use]
  pub fn eval_profile_file<S: Into<String>>(mut self, path: S) -> Self {
    self.eval_profile_file = Some(path.into());
    self
  }

  #[must_use]
  pub fn argv(&self) -> Vec<String> {
    let nix = self.binary.as_deref().unwrap_or("nix").to_string();
    let mut argv = vec![nix, self.kind.as_str().to_string()];
    if self.print_build_logs
      && !self.args.iter().any(|a| a == "--no-build-output")
    {
      argv.push("--print-build-logs".to_string());
    }
    if self.impure {
      argv.push("--impure".to_string());
    }
    if let Some(ref mode) = self.eval_profiler_mode {
      argv.push("--eval-profiler".to_string());
      argv.push(mode.clone());
    }
    if let Some(hz) = self.eval_profiler_frequency {
      argv.push("--eval-profiler-frequency".to_string());
      argv.push(hz.to_string());
    }
    if let Some(ref path) = self.eval_profile_file {
      argv.push("--eval-profile-file".to_string());
      argv.push(path.clone());
    }
    argv.extend(self.args.iter().cloned());
    argv
  }

  fn build_command(&self) -> Command {
    let argv = self.argv();
    let mut cmd = Command::new(&argv[0]);
    cmd.args(&argv[1..]);
    for (k, v) in &self.env {
      cmd.env(k, v);
    }
    cmd
  }

  pub fn run_with_logs(&self, mut interceptor: StdIo) -> Result<ExitStatus> {
    let mut cmd = self.build_command();

    if self.interactive {
      return Ok(
        cmd
          .stdout(Stdio::inherit())
          .stderr(Stdio::inherit())
          .stdin(Stdio::inherit())
          .status()?,
      );
    }

    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn()?;
    let stdout = child.stdout.take().ok_or_else(|| self.command_failed())?;
    let stderr = child.stderr.take().ok_or_else(|| self.command_failed())?;
    let (tx, rx) = mpsc::channel();
    let stdout_thread = thread::spawn({
      let tx = tx.clone();
      move || read_pipe(stdout, tx, false)
    });
    let stderr_thread = thread::spawn(move || read_pipe(stderr, tx, true));
    let start = Instant::now();

    loop {
      if start.elapsed() > DEFAULT_TIMEOUT {
        self.kill_wait_join(&mut child, stdout_thread, stderr_thread)?;
        return Err(self.timeout());
      }

      match rx.recv_timeout(Duration::from_millis(100)) {
        Ok(PipeEvent::Stdout(data)) => interceptor.on_stdout(&data),
        Ok(PipeEvent::Stderr(data)) => interceptor.on_stderr(&data),
        Ok(PipeEvent::Error(e)) => {
          self.kill_wait_join(&mut child, stdout_thread, stderr_thread)?;
          return Err(Error::Io(e));
        },
        Err(mpsc::RecvTimeoutError::Timeout) => {},
        Err(mpsc::RecvTimeoutError::Disconnected) => break,
      }
    }

    let _ = stdout_thread.join();
    let _ = stderr_thread.join();
    Ok(child.wait()?)
  }

  pub fn output(&self) -> Result<Output> {
    let mut cmd = self.build_command();
    if self.interactive {
      return Ok(
        cmd
          .stdout(Stdio::inherit())
          .stderr(Stdio::inherit())
          .stdin(Stdio::inherit())
          .output()?,
      );
    }
    Ok(cmd.output()?)
  }

  fn kill_wait_join(
    &self,
    child: &mut std::process::Child,
    stdout_thread: thread::JoinHandle<()>,
    stderr_thread: thread::JoinHandle<()>,
  ) -> Result<()> {
    let _ = child.kill();
    let _ = stdout_thread.join();
    let _ = stderr_thread.join();
    let _ = child.wait()?;
    Ok(())
  }

  fn command_failed(&self) -> Error {
    Error::CommandFailed {
      command: self.kind.as_str().to_string(),
    }
  }

  fn timeout(&self) -> Error {
    Error::Timeout {
      command:  self.kind.as_str().to_string(),
      duration: DEFAULT_TIMEOUT,
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn schema_parses_supported_commands() {
    for spec in COMMAND_SPECS {
      let kind = CommandKind::try_from(spec.name).unwrap();
      assert_eq!(kind.as_str(), spec.name);
    }
  }

  #[test]
  fn schema_rejects_unknown_commands() {
    assert_eq!(
      CommandKind::try_from("repl"),
      Err(UnknownCommand {
        command: "repl".to_string(),
      })
    );
  }

  #[test]
  fn argv_is_deterministic_and_schema_driven() {
    let argv = NixCommand::new(CommandKind::Build)
      .arg("nixpkgs#hello")
      .impure(true)
      .argv();
    assert_eq!(argv, [
      "nix",
      "build",
      "--print-build-logs",
      "--impure",
      "nixpkgs#hello"
    ]);
  }

  #[test]
  fn no_build_output_suppresses_print_build_logs() {
    let argv = NixCommand::new(CommandKind::Build)
      .arg("--no-build-output")
      .argv();
    assert_eq!(argv, ["nix", "build", "--no-build-output"]);
  }

  #[test]
  fn eval_defaults_to_quiet_schema() {
    assert_eq!(NixCommand::new(CommandKind::Eval).argv(), ["nix", "eval"]);
  }

  #[test]
  fn interactive_defaults_come_from_schema() {
    assert!(NixCommand::new(CommandKind::Run).interactive);
    assert!(NixCommand::new(CommandKind::Shell).interactive);
    assert!(NixCommand::new(CommandKind::Develop).interactive);
    assert!(!NixCommand::new(CommandKind::Build).interactive);
  }

  #[test]
  fn eval_profiler_flags_are_added_to_argv() {
    let argv = NixCommand::new(CommandKind::Eval)
      .arg("nixpkgs#hello")
      .impure(true)
      .eval_profiler("flamegraph")
      .eval_profiler_frequency(9999)
      .eval_profile_file("/tmp/nix.profile")
      .argv();
    assert_eq!(argv, [
      "nix",
      "eval",
      "--impure",
      "--eval-profiler",
      "flamegraph",
      "--eval-profiler-frequency",
      "9999",
      "--eval-profile-file",
      "/tmp/nix.profile",
      "nixpkgs#hello"
    ]);
  }
}
