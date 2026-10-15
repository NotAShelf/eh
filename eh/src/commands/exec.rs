use eh_error::{EhError, Result};
use eh_locate::get_package_from_index;

use crate::commands::handle_default_nix_command;

pub fn handle_exec(
  args: &[String],
  cfg: &eh_config::CommandConfig,
  ask: bool,
) -> Result<i32> {
  let binary = args.first().ok_or(EhError::MissingBinary)?;
  let installable = get_package_from_index(binary)?;
  handle_default_nix_command("shell", &shell_args(args, &installable), cfg, ask)
}

fn shell_args(args: &[String], installable: &str) -> Vec<String> {
  let mut shell_args = Vec::with_capacity(args.len() + 2);
  shell_args.push(format!("nixpkgs#{installable}"));
  shell_args.push("-c".to_string());
  shell_args.extend_from_slice(args);
  shell_args
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn forwards_all_child_arguments_after_shell_command_boundary() {
    let args = ["hello", "--help", "-v", "--", "--version", "two words"]
      .map(String::from);
    assert_eq!(shell_args(&args, "nested.hello"), [
      "nixpkgs#nested.hello",
      "-c",
      "hello",
      "--help",
      "-v",
      "--",
      "--version",
      "two words",
    ]);
  }

  #[test]
  fn missing_binary_does_not_open_the_database() {
    assert!(matches!(
      handle_exec(&[], &eh_config::CommandConfig::default(), false),
      Err(EhError::MissingBinary)
    ));
  }
}
