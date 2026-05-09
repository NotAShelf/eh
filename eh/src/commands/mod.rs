use std::{
  io::{self, Read, Write},
  process::{Command, ExitStatus, Output, Stdio},
  sync::mpsc,
  thread,
  time::{Duration, Instant},
};

use crate::{
  error::{EhError, Result},
  util::{
    HashExtractor,
    NixErrorClassifier,
    NixFileFixer,
    handle_nix_with_retry,
  },
};

pub mod info;
pub mod update;

const DEFAULT_BUFFER_SIZE: usize = 4096;
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(300);

pub trait LogInterceptor: Send {
  fn on_stderr(&mut self, chunk: &[u8]);
  fn on_stdout(&mut self, chunk: &[u8]);
}

pub struct StdIoInterceptor;

impl LogInterceptor for StdIoInterceptor {
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
  let mut buf = [0u8; DEFAULT_BUFFER_SIZE];
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

  #[must_use]
  pub fn args_ref(mut self, args: &[String]) -> Self {
    self.args.extend(args.iter().cloned());
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

  /// Apply per-command configuration: sets `--impure` (when explicitly enabled)
  /// and any extra environment variables declared in the config file.  Call
  /// this before any retry-specific overrides so that retry logic can still
  /// force `impure(true)` afterwards.
  #[must_use]
  pub fn with_config(mut self, cfg: &crate::config::CommandConfig) -> Self {
    if cfg.impure == Some(true) {
      self = self.impure(true);
    }
    for (k, v) in &cfg.env {
      self = self.env(k, v);
    }
    self
  }

  fn build_command(&self) -> Command {
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
    cmd
  }

  pub fn run_with_logs<I: LogInterceptor + 'static>(
    &self,
    mut interceptor: I,
  ) -> Result<ExitStatus> {
    let mut cmd = self.build_command();

    if self.interactive {
      cmd.stdout(Stdio::inherit());
      cmd.stderr(Stdio::inherit());
      cmd.stdin(Stdio::inherit());
      return Ok(cmd.status()?);
    }

    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let mut child = cmd.spawn()?;
    let stdout = child.stdout.take().ok_or_else(|| {
      EhError::CommandFailed {
        command: format!("nix {}", self.subcommand),
      }
    })?;
    let stderr = child.stderr.take().ok_or_else(|| {
      EhError::CommandFailed {
        command: format!("nix {}", self.subcommand),
      }
    })?;

    let (tx, rx) = mpsc::channel();

    let tx_out = tx.clone();
    let stdout_thread = thread::spawn(move || read_pipe(stdout, tx_out, false));

    let tx_err = tx;
    let stderr_thread = thread::spawn(move || read_pipe(stderr, tx_err, true));

    let start_time = Instant::now();

    loop {
      if start_time.elapsed() > DEFAULT_TIMEOUT {
        let _ = child.kill();
        let _ = stdout_thread.join();
        let _ = stderr_thread.join();
        let _ = child.wait();
        return Err(EhError::Timeout {
          command:  format!("nix {}", self.subcommand),
          duration: DEFAULT_TIMEOUT,
        });
      }

      match rx.recv_timeout(Duration::from_millis(100)) {
        Ok(PipeEvent::Stdout(data)) => interceptor.on_stdout(&data),
        Ok(PipeEvent::Stderr(data)) => interceptor.on_stderr(&data),
        Ok(PipeEvent::Error(e)) => {
          let _ = child.kill();
          let _ = stdout_thread.join();
          let _ = stderr_thread.join();
          let _ = child.wait();
          return Err(EhError::Io(e));
        },
        Err(mpsc::RecvTimeoutError::Timeout) => {},
        Err(mpsc::RecvTimeoutError::Disconnected) => break,
      }
    }

    let _ = stdout_thread.join();
    let _ = stderr_thread.join();

    let status = child.wait()?;
    Ok(status)
  }

  pub fn output(&self) -> Result<Output> {
    let mut cmd = self.build_command();

    if self.interactive {
      cmd.stdout(Stdio::inherit());
      cmd.stderr(Stdio::inherit());
      cmd.stdin(Stdio::inherit());
      return Ok(cmd.output()?);
    }

    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let mut child = cmd.spawn()?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    let (tx, rx) = mpsc::channel();

    let tx_out = tx.clone();
    let stdout_thread = thread::spawn(move || {
      let mut buf = Vec::new();
      if let Some(mut r) = stdout {
        let _ = r.read_to_end(&mut buf);
      }
      let _ = tx_out.send((false, buf));
    });

    let tx_err = tx;
    let stderr_thread = thread::spawn(move || {
      let mut buf = Vec::new();
      if let Some(mut r) = stderr {
        let _ = r.read_to_end(&mut buf);
      }
      let _ = tx_err.send((true, buf));
    });

    let start_time = Instant::now();
    let mut stdout_buf = Vec::new();
    let mut stderr_buf = Vec::new();
    let mut received = 0;

    while received < 2 {
      let remaining = DEFAULT_TIMEOUT
        .checked_sub(start_time.elapsed())
        .unwrap_or(Duration::ZERO);

      if remaining.is_zero() {
        let _ = child.kill();
        let _ = stdout_thread.join();
        let _ = stderr_thread.join();
        let _ = child.wait();
        return Err(EhError::Timeout {
          command:  format!("nix {}", self.subcommand),
          duration: DEFAULT_TIMEOUT,
        });
      }

      match rx.recv_timeout(remaining) {
        Ok((true, buf)) => {
          stderr_buf = buf;
          received += 1;
        },
        Ok((false, buf)) => {
          stdout_buf = buf;
          received += 1;
        },
        Err(mpsc::RecvTimeoutError::Timeout) => {
          let _ = child.kill();
          let _ = stdout_thread.join();
          let _ = stderr_thread.join();
          let _ = child.wait();
          return Err(EhError::Timeout {
            command:  format!("nix {}", self.subcommand),
            duration: DEFAULT_TIMEOUT,
          });
        },
        Err(mpsc::RecvTimeoutError::Disconnected) => break,
      }
    }

    let _ = stdout_thread.join();
    let _ = stderr_thread.join();

    let status = child.wait()?;
    Ok(Output {
      status,
      stdout: stdout_buf,
      stderr: stderr_buf,
    })
  }
}

pub fn handle_nix_command(
  command: &str,
  args: &[String],
  hash_extractor: &dyn HashExtractor,
  fixer: &dyn NixFileFixer,
  classifier: &dyn NixErrorClassifier,
  cfg: &crate::config::CommandConfig,
  ask: bool,
) -> Result<i32> {
  let intercept_env = matches!(command, "run" | "shell");
  handle_nix_with_retry(
    command,
    args,
    hash_extractor,
    fixer,
    classifier,
    intercept_env,
    cfg,
    ask,
  )
}

#[cfg(test)]
mod tests {
  use std::io::{Cursor, Error};

  use super::*;

  #[test]
  fn test_read_pipe_stdout() {
    let data = b"hello world";
    let cursor = Cursor::new(data);
    let (tx, rx) = mpsc::channel();

    let tx_clone = tx.clone();
    std::thread::spawn(move || {
      read_pipe(cursor, tx_clone, false);
    });

    drop(tx);

    let events: Vec<PipeEvent> = rx.iter().take(10).collect();
    assert!(!events.is_empty());

    let stdout_events: Vec<_> = events
      .iter()
      .filter(|e| matches!(e, PipeEvent::Stdout(_)))
      .collect();
    assert!(!stdout_events.is_empty());

    let combined: Vec<u8> = events
      .iter()
      .filter_map(|e| {
        match e {
          PipeEvent::Stdout(b) => Some(b.clone()),
          _ => None,
        }
      })
      .flatten()
      .collect();
    assert_eq!(combined, data);
  }

  #[test]
  fn test_read_pipe_stderr() {
    let data = b"error output";
    let cursor = Cursor::new(data);
    let (tx, rx) = mpsc::channel();

    let tx_clone = tx.clone();
    std::thread::spawn(move || {
      read_pipe(cursor, tx_clone, true);
    });

    drop(tx);

    let events: Vec<PipeEvent> = rx.iter().take(10).collect();

    let stderr_events: Vec<_> = events
      .iter()
      .filter(|e| matches!(e, PipeEvent::Stderr(_)))
      .collect();
    assert!(!stderr_events.is_empty());

    let combined: Vec<u8> = events
      .iter()
      .filter_map(|e| {
        match e {
          PipeEvent::Stderr(b) => Some(b.clone()),
          _ => None,
        }
      })
      .flatten()
      .collect();
    assert_eq!(combined, data);
  }

  #[test]
  fn test_read_pipe_empty() {
    let cursor = Cursor::new(b"");
    let (tx, rx) = mpsc::channel();

    let tx_clone = tx.clone();
    std::thread::spawn(move || {
      read_pipe(cursor, tx_clone, false);
    });

    drop(tx);

    let events: Vec<PipeEvent> = rx.iter().take(10).collect();
    assert!(events.is_empty());
  }

  #[test]
  fn test_read_pipe_error() {
    struct ErrorReader;
    impl Read for ErrorReader {
      fn read(&mut self, _buf: &mut [u8]) -> std::io::Result<usize> {
        Err(std::io::Error::other("test error"))
      }
    }

    let reader = ErrorReader;
    let (tx, rx) = mpsc::channel();

    let tx_clone = tx.clone();
    std::thread::spawn(move || {
      read_pipe(reader, tx_clone, false);
    });

    drop(tx);

    let events: Vec<PipeEvent> = rx.iter().take(10).collect();

    let error_events: Vec<_> = events
      .iter()
      .filter(|e| matches!(e, PipeEvent::Error(_)))
      .collect();
    assert!(!error_events.is_empty());
  }

  #[test]
  fn test_pipe_event_debug() {
    let stdout_event = PipeEvent::Stdout(b"test".to_vec());
    let stderr_event = PipeEvent::Stderr(b"error".to_vec());
    let error_event = PipeEvent::Error(Error::other("test"));

    let debug_stdout = format!("{:?}", stdout_event);
    let debug_stderr = format!("{:?}", stderr_event);
    let debug_error = format!("{:?}", error_event);

    assert!(debug_stdout.contains("Stdout"));
    assert!(debug_stderr.contains("Stderr"));
    assert!(debug_error.contains("Error"));
  }
}
