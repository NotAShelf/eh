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
) -> Result<i32> {
  let intercept_env = matches!(command, "run" | "shell");
  handle_nix_with_retry(
    command,
    args,
    hash_extractor,
    fixer,
    classifier,
    intercept_env,
  )
}
