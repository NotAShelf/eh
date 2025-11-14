use std::{
  collections::VecDeque,
  io::{self, Read, Write},
  process::{Command, ExitStatus, Output, Stdio},
};

use crate::error::{EhError, Result};

/// Trait for log interception and output handling.
pub trait LogInterceptor: Send {
  fn on_stderr(&mut self, chunk: &[u8]);
  fn on_stdout(&mut self, chunk: &[u8]);
}

/// Default log interceptor that just writes to stdio.
pub struct StdIoInterceptor;

impl LogInterceptor for StdIoInterceptor {
  fn on_stderr(&mut self, chunk: &[u8]) {
    let _ = io::stderr().write_all(chunk);
  }
  fn on_stdout(&mut self, chunk: &[u8]) {
    let _ = io::stdout().write_all(chunk);
  }
}

/// Default buffer size for reading command output
const DEFAULT_BUFFER_SIZE: usize = 4096;

/// Builder and executor for Nix commands.
pub struct NixCommand {
  subcommand:       String,
  args:             Vec<String>,
  env:              Vec<(String, String)>,
  impure:           bool,
  print_build_logs: bool,
  interactive:      bool,
}

impl NixCommand {
  pub fn new<S: Into<String>>(subcommand: S) -> Self {
    Self {
      subcommand:       subcommand.into(),
      args:             Vec::new(),
      env:              Vec::new(),
      impure:           false,
      print_build_logs: true,
      interactive:      false,
    }
  }

  pub fn arg<S: Into<String>>(mut self, arg: S) -> Self {
    self.args.push(arg.into());
    self
  }

  pub fn args<I, S>(mut self, args: I) -> Self
  where
    I: IntoIterator<Item = S>,
    S: Into<String>,
  {
    self.args.extend(args.into_iter().map(Into::into));
    self
  }

  pub fn env<K: Into<String>, V: Into<String>>(
    mut self,
    key: K,
    value: V,
  ) -> Self {
    self.env.push((key.into(), value.into()));
    self
  }

  #[must_use]
  pub const fn impure(mut self, yes: bool) -> Self {
    self.impure = yes;
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

  /// Run the command, streaming output to the provided interceptor.
  pub fn run_with_logs<I: LogInterceptor + 'static>(
    &self,
    mut interceptor: I,
  ) -> Result<ExitStatus> {
    let mut cmd = Command::new("nix");
    cmd.arg(&self.subcommand);

    if self.print_build_logs
      && !self.args.iter().any(|a| a == "--no-build-output")
    {
      cmd.arg("--print-build-logs");
    }
    if self.impure {
      cmd.arg("--impure");
    }
    for (k, v) in &self.env {
      cmd.env(k, v);
    }
    cmd.args(&self.args);

    if self.interactive {
      cmd.stdout(Stdio::inherit());
      cmd.stderr(Stdio::inherit());
      cmd.stdin(Stdio::inherit());
      return Ok(cmd.status()?);
    }

    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let mut child = cmd.spawn()?;
    let child_stdout = child.stdout.take().ok_or_else(|| {
      EhError::CommandFailed {
        command: format!("nix {}", self.subcommand),
      }
    })?;
    let child_stderr = child.stderr.take().ok_or_else(|| {
      EhError::CommandFailed {
        command: format!("nix {}", self.subcommand),
      }
    })?;
    let mut stdout = child_stdout;
    let mut stderr = child_stderr;

    let mut out_buf = [0u8; DEFAULT_BUFFER_SIZE];
    let mut err_buf = [0u8; DEFAULT_BUFFER_SIZE];

    let mut out_queue = VecDeque::new();
    let mut err_queue = VecDeque::new();

    loop {
      let mut did_something = false;

      match stdout.read(&mut out_buf) {
        Ok(0) => {},
        Ok(n) => {
          interceptor.on_stdout(&out_buf[..n]);
          out_queue.push_back(Vec::from(&out_buf[..n]));
          did_something = true;
        },
        Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {},
        Err(e) => return Err(EhError::Io(e)),
      }

      match stderr.read(&mut err_buf) {
        Ok(0) => {},
        Ok(n) => {
          interceptor.on_stderr(&err_buf[..n]);
          err_queue.push_back(Vec::from(&err_buf[..n]));
          did_something = true;
        },
        Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {},
        Err(e) => return Err(EhError::Io(e)),
      }

      if !did_something && child.try_wait()?.is_some() {
        break;
      }
    }

    let status = child.wait()?;
    Ok(status)
  }

  /// Run the command and capture all output.
  pub fn output(&self) -> Result<Output> {
    let mut cmd = Command::new("nix");
    cmd.arg(&self.subcommand);

    if self.print_build_logs
      && !self.args.iter().any(|a| a == "--no-build-output")
    {
      cmd.arg("--print-build-logs");
    }
    if self.impure {
      cmd.arg("--impure");
    }
    for (k, v) in &self.env {
      cmd.env(k, v);
    }
    cmd.args(&self.args);

    if self.interactive {
      cmd.stdout(Stdio::inherit());
      cmd.stderr(Stdio::inherit());
      cmd.stdin(Stdio::inherit());
    } else {
      cmd.stdout(Stdio::piped());
      cmd.stderr(Stdio::piped());
    }

    Ok(cmd.output()?)
  }
}
