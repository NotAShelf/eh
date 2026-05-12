use std::io::{IsTerminal, Write};

use eh_log::{log_debug, log_info, log_warn};
use nix_command::{CommandKind, NixCommand, StdIo};
use yansi::Paint;

use crate::{
  error::{EhError, Result},
  eval::{make_eval_expr, package_arg},
  hash::{HashExtractor, NixFileFixer, is_hash_mismatch_error},
  nix_config::ApplyCommandConfig,
  suggestions::print_error_suggestions,
};

pub trait NixErrorClassifier {
  fn should_retry(&self, stderr: &str) -> bool;
}

#[derive(Debug, PartialEq, Eq)]
pub enum RetryAction {
  AllowUnfree,
  AllowInsecure,
  AllowBroken,
  None,
}

impl RetryAction {
  const fn env_override(&self) -> Option<(&str, &str)> {
    match self {
      Self::AllowUnfree => {
        Some(("NIXPKGS_ALLOW_UNFREE", "has an unfree license"))
      },
      Self::AllowInsecure => {
        Some(("NIXPKGS_ALLOW_INSECURE", "has been marked as insecure"))
      },
      Self::AllowBroken => {
        Some(("NIXPKGS_ALLOW_BROKEN", "has been marked as broken"))
      },
      Self::None => None,
    }
  }
}

pub fn classify_retry_action(stderr: &str) -> RetryAction {
  if stderr.contains("refusing") && stderr.contains("has an unfree license") {
    RetryAction::AllowUnfree
  } else if stderr.contains("refusing")
    && stderr.contains("has been marked as insecure")
  {
    RetryAction::AllowInsecure
  } else if stderr.contains("refusing")
    && stderr.contains("has been marked as broken")
  {
    RetryAction::AllowBroken
  } else {
    RetryAction::None
  }
}

fn command_kind(subcommand: &str) -> Result<CommandKind> {
  CommandKind::try_from(subcommand).map_err(|_| {
    EhError::NixCommandFailed {
      command: subcommand.to_string(),
    }
  })
}

fn nix_command(
  subcommand: &str,
  args: &[String],
  cfg: &eh_config::CommandConfig,
  interactive: bool,
) -> Result<NixCommand> {
  Ok(
    NixCommand::new(command_kind(subcommand)?)
      .args_ref(args)
      .apply_config(cfg)
      .interactive(interactive),
  )
}

fn run_nix_command(
  subcommand: &str,
  args: &[String],
  cfg: &eh_config::CommandConfig,
  interactive: bool,
  env_override: Option<&str>,
) -> Result<i32> {
  let mut cmd = nix_command(subcommand, args, cfg, interactive)?;
  if let Some(env_var) = env_override {
    cmd = cmd.env(env_var, "1").impure(true);
  }
  if interactive {
    log_debug!("entering {}", command_display(subcommand, args));
  }
  Ok(cmd.run_with_logs(StdIo)?.code().unwrap_or(1))
}

fn command_display(subcommand: &str, args: &[String]) -> String {
  if args.is_empty() {
    format!("nix {subcommand}")
  } else {
    format!("nix {} {}", subcommand, args.join(" "))
  }
}

fn ensure_impure_allowed(
  cfg: &eh_config::CommandConfig,
  subcommand: &str,
  reason: &str,
) -> Result<()> {
  if cfg.impure == Some(false) {
    return Err(EhError::ImpureRequired {
      command: subcommand.to_string(),
      reason:  reason.to_string(),
    });
  }
  Ok(())
}

fn check_package_flags(args: &[String]) -> Result<RetryAction> {
  let eval_arg = package_arg(args).unwrap_or(".");
  let eval_expr = make_eval_expr(eval_arg)?;
  let output = match NixCommand::new(CommandKind::Eval)
    .arg("--json")
    .arg(eval_expr)
    .output()
  {
    Ok(output) if output.status.success() => output,
    Ok(output) => {
      let stderr = String::from_utf8_lossy(&output.stderr);
      if !stderr.contains("does not provide attribute") {
        log_warn!(
          "failed to check package flags for '{}': {}",
          eval_arg,
          stderr.trim()
        );
      }
      return Ok(RetryAction::None);
    },
    Err(e) => {
      log_warn!("failed to check package flags for '{}': {}", eval_arg, e);
      return Ok(RetryAction::None);
    },
  };

  let meta = match serde_json::from_slice::<serde_json::Value>(&output.stdout) {
    Ok(meta) => meta,
    Err(e) => {
      log_warn!("failed to parse package metadata for '{}': {}", eval_arg, e);
      return Ok(RetryAction::None);
    },
  };

  [
    ("unfree", RetryAction::AllowUnfree),
    ("insecure", RetryAction::AllowInsecure),
    ("broken", RetryAction::AllowBroken),
  ]
  .into_iter()
  .find_map(|(key, action)| {
    meta
      .get(key)
      .and_then(serde_json::Value::as_bool)
      .unwrap_or(false)
      .then_some(action)
  })
  .ok_or(EhError::ProcessExit { code: 0 })
  .or(Ok(RetryAction::None))
}

fn pre_evaluate(args: &[String]) -> Result<RetryAction> {
  let action = check_package_flags(args)?;
  if action != RetryAction::None {
    return Ok(action);
  }

  let eval_arg = package_arg(args).unwrap_or(".");
  let output = NixCommand::new(CommandKind::Eval).arg(eval_arg).output()?;
  if output.status.success() {
    return Ok(RetryAction::None);
  }

  let stderr = String::from_utf8_lossy(&output.stderr);
  let action = classify_retry_action(&stderr);
  if action != RetryAction::None {
    return Ok(action);
  }

  let stderr = stderr
    .trim()
    .strip_prefix("error:")
    .unwrap_or(stderr.trim())
    .trim();
  Err(EhError::PreEvalFailed {
    expression: eval_arg.to_string(),
    stderr:     stderr.to_string(),
  })
}

pub fn handle_nix_with_retry(
  subcommand: &str,
  args: &[String],
  hash_extractor: &dyn HashExtractor,
  fixer: &dyn NixFileFixer,
  classifier: &dyn NixErrorClassifier,
  interactive: bool,
  cfg: &eh_config::CommandConfig,
  ask: bool,
) -> Result<i32> {
  let pkg = package_arg(args).unwrap_or("<unknown>");
  log_debug!("checking {}", command_display(subcommand, args));
  if let Some((env_var, reason)) = pre_evaluate(args)?.env_override() {
    ensure_impure_allowed(cfg, subcommand, reason)?;
    confirm_impure_retry(pkg, reason, ask)?;
    print_retry_msg(pkg, reason, env_var);
    return run_nix_command(subcommand, args, cfg, interactive, Some(env_var));
  }

  if interactive {
    let code = run_nix_command(subcommand, args, cfg, true, None)?;
    if code == 0 {
      return Ok(0);
    }
  }

  log_debug!("running {}", command_display(subcommand, args));
  let output = nix_command(subcommand, args, cfg, false)?.output()?;
  let stderr = String::from_utf8_lossy(&output.stderr);

  if let Some(new_hash) = hash_extractor.extract_hash(&stderr) {
    let ctx = HashFixContext {
      subcommand,
      args,
      cfg,
      interactive,
      ask,
      pkg,
      fixer,
    };
    if let Some(code) = handle_hash_mismatch(
      ctx,
      hash_extractor.extract_old_hash(&stderr),
      &new_hash,
    )? {
      return Ok(code);
    }
  } else if is_hash_mismatch_error(&stderr) {
    return Err(EhError::HashExtractionFailed {
      stderr: stderr.to_string(),
    });
  }

  if classifier.should_retry(&stderr)
    && let Some((env_var, reason)) =
      classify_retry_action(&stderr).env_override()
  {
    ensure_impure_allowed(cfg, subcommand, reason)?;
    confirm_impure_retry(pkg, reason, ask)?;
    print_retry_msg(pkg, reason, env_var);
    return run_nix_command(subcommand, args, cfg, interactive, Some(env_var));
  }

  if output.status.success() {
    return Ok(0);
  }

  std::io::stderr()
    .write_all(&output.stderr)
    .map_err(EhError::Io)?;
  print_error_suggestions(&output.stderr);
  output.status.code().map_or_else(
    || {
      Err(EhError::NixCommandFailed {
        command: subcommand.to_string(),
      })
    },
    |code| Err(EhError::ProcessExit { code }),
  )
}

fn confirm_impure_retry(pkg: &str, reason: &str, ask: bool) -> Result<()> {
  if !ask {
    return Ok(());
  }

  if !std::io::stdin().is_terminal() {
    return Err(EhError::Io(std::io::Error::other(
      "cannot prompt for retry confirmation in non-interactive mode (no TTY)",
    )));
  }

  let choices = ["Yes, retry with --impure", "No, cancel"];
  let idx = dialoguer::Select::new()
    .with_prompt(format!(
      "Package {} requires `--impure` ({}). Retry?",
      pkg.bold(),
      reason.bold()
    ))
    .items(choices)
    .default(0)
    .interact()
    .map_err(|e| EhError::Io(std::io::Error::other(e)))?;

  if idx == 0 {
    Ok(())
  } else {
    Err(EhError::ProcessExit { code: 1 })
  }
}

struct HashFixContext<'a> {
  subcommand:  &'a str,
  args:        &'a [String],
  cfg:         &'a eh_config::CommandConfig,
  interactive: bool,
  ask:         bool,
  pkg:         &'a str,
  fixer:       &'a dyn NixFileFixer,
}

fn handle_hash_mismatch(
  ctx: HashFixContext<'_>,
  old_hash: Option<String>,
  new_hash: &str,
) -> Result<Option<i32>> {
  if !std::io::stdin().is_terminal() {
    if ctx.ask {
      return Err(EhError::Io(std::io::Error::other(
        "cannot prompt for hash fix confirmation in non-interactive mode (no TTY)",
      )));
    }

    log_info!(
      "{}: skipping hash fix in non-interactive mode",
      ctx.pkg.bold()
    );
    return Ok(None);
  }

  let should_fix = dialoguer::Confirm::new()
    .with_prompt(format!(
      "Hash mismatch detected for {}. Update hash in local .nix files?",
      ctx.pkg.bold()
    ))
    .default(true)
    .interact()
    .map_err(|e| EhError::Io(std::io::Error::other(e)))?;

  if !should_fix {
    log_warn!("{}: hash fix cancelled", ctx.pkg.bold());
    return Err(EhError::ProcessExit { code: 1 });
  }

  match ctx.fixer.fix_hash_in_files(old_hash.as_deref(), new_hash) {
    Ok(true) => {
      log_info!(
        "{}: hash mismatch corrected in local files, rebuilding",
        ctx.pkg.bold()
      );
      run_nix_command(ctx.subcommand, ctx.args, ctx.cfg, ctx.interactive, None)
        .map(Some)
    },
    Ok(false) | Err(EhError::NoNixFilesFound) => Ok(None),
    Err(e) => Err(e),
  }
}

pub struct DefaultNixErrorClassifier;

impl NixErrorClassifier for DefaultNixErrorClassifier {
  fn should_retry(&self, stderr: &str) -> bool {
    classify_retry_action(stderr) != RetryAction::None
  }
}

fn print_retry_msg(pkg: &str, reason: &str, env_var: &str) {
  log_warn!(
    "{}: {}, setting {}",
    pkg.bold(),
    reason,
    format!("{env_var}=1").bold()
  );
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn classifies_retryable_errors() {
    assert_eq!(
      classify_retry_action("refusing because it has an unfree license"),
      RetryAction::AllowUnfree
    );
    assert_eq!(
      classify_retry_action("refusing because it has been marked as insecure"),
      RetryAction::AllowInsecure
    );
    assert_eq!(
      classify_retry_action("refusing because it has been marked as broken"),
      RetryAction::AllowBroken
    );
    assert_eq!(classify_retry_action("ordinary error"), RetryAction::None);
  }
}
