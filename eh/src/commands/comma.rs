use crate::{
  commands::handle_default_nix_command,
};
use eh_error::{EhError, Result};
use eh_locate::get_package_from_index;


pub fn handle_comma(
  args: &[String],
  cfg: &eh_config::CommandConfig,
  ask: bool,
) -> Result<i32> {
  let binary = args.first().ok_or(EhError::MissingBinary)?;
  let installable = get_package_from_index(binary)?;
  let mut shell_args = Vec::with_capacity(args.len() + 2);
  shell_args.push(format!("nixpkgs#{installable}"));
  shell_args.push("-c".to_string());
  shell_args.push(binary.clone());
  shell_args.extend(args.iter().skip(1).cloned());
  handle_default_nix_command("shell", &shell_args, cfg, ask)
}
