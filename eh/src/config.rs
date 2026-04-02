use std::{
  collections::HashMap,
  env,
  fs,
  path::{Path, PathBuf},
};

use serde::Deserialize;

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Config {
  /// When `Some(true)`, pass `--impure` to every Nix command.
  /// When `Some(false)`, block automatic impure retries for every command.
  /// When absent (`None`), retry behaviour is automatic (default).
  #[serde(default)]
  pub impure:   Option<bool>,
  #[serde(default)]
  pub commands: HashMap<String, CommandConfig>,
}

/// Per-command configuration.
#[derive(Debug, Deserialize, Default, Clone)]
#[serde(deny_unknown_fields)]
pub struct CommandConfig {
  /// When `Some(true)`, pass `--impure` to the underlying Nix command.
  /// When `Some(false)`, block automatic impure retries for this command.
  /// When absent (`None`), the global setting is used; if that is also absent,
  /// retry behaviour is automatic (default).
  #[serde(default)]
  pub impure: Option<bool>,
  /// Additional environment variables to set for the Nix command.
  #[serde(default)]
  pub env:    HashMap<String, String>,
}

impl Config {
  /// Return the [`CommandConfig`] for `command`.
  ///
  /// Resolution order: per-command `impure` takes precedence over the global
  /// `impure`.  Neither being set means automatic retry behaviour.
  pub fn for_command(&self, command: &str) -> CommandConfig {
    let mut cmd = self.commands.get(command).cloned().unwrap_or_default();
    // Per-command setting wins; fall back to global.
    if cmd.impure.is_none() {
      cmd.impure = self.impure;
    }
    cmd
  }
}

/// Load configuration from the first `.eh.toml` found by walking up from the
/// current directory, or from `~/.config/eh/config.toml` as a global
/// fallback.  Returns a default (empty) config if no file is found or if
/// parsing fails.
pub fn load() -> Config {
  if let Some(path) = find_project_config()
    && let Some(cfg) = load_from_file(&path)
  {
    return cfg;
  }

  if let Some(path) = global_config_path()
    && let Some(cfg) = load_from_file(&path)
  {
    return cfg;
  }

  Config::default()
}

fn find_project_config() -> Option<PathBuf> {
  let mut dir = env::current_dir().ok()?;
  loop {
    let candidate = dir.join(".eh.toml");
    if candidate.exists() {
      return Some(candidate);
    }
    if !dir.pop() {
      return None;
    }
  }
}

fn global_config_path() -> Option<PathBuf> {
  let home = env::var("HOME").ok()?;
  Some(
    PathBuf::from(home)
      .join(".config")
      .join("eh")
      .join("config.toml"),
  )
}

fn load_from_file(path: &Path) -> Option<Config> {
  let content = fs::read_to_string(path).ok()?;
  match toml::de::from_str::<Config>(&content) {
    Ok(cfg) => Some(cfg),
    Err(e) => {
      eprintln!(
        "eh: warning: failed to parse config file {}: {}",
        path.display(),
        e
      );
      None
    },
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_empty_config_defaults() {
    let cfg: Config = toml::from_str("").unwrap();
    assert!(cfg.impure.is_none());
    assert!(cfg.commands.is_empty());
  }

  #[test]
  fn test_command_config_impure_true() {
    let cfg: Config = toml::from_str(
      r#"
      [commands.build]
      impure = true
      "#,
    )
    .unwrap();
    assert_eq!(cfg.for_command("build").impure, Some(true));
    assert_eq!(cfg.for_command("run").impure, None);
  }

  #[test]
  fn test_command_config_impure_false() {
    let cfg: Config = toml::from_str(
      r#"
      [commands.build]
      impure = false
      "#,
    )
    .unwrap();
    assert_eq!(cfg.for_command("build").impure, Some(false));
    assert_eq!(cfg.for_command("run").impure, None);
  }

  #[test]
  fn test_global_impure_propagates_to_unconfigured_commands() {
    let cfg: Config = toml::from_str("impure = true").unwrap();
    // Commands with no per-command entry inherit global.
    assert_eq!(cfg.for_command("build").impure, Some(true));
    assert_eq!(cfg.for_command("nonexistent").impure, Some(true));
  }

  #[test]
  fn test_global_impure_false_propagates_to_unconfigured_commands() {
    let cfg: Config = toml::from_str("impure = false").unwrap();
    assert_eq!(cfg.for_command("build").impure, Some(false));
  }

  #[test]
  fn test_per_command_impure_overrides_global() {
    // Per-command setting wins over global.
    let cfg: Config = toml::from_str(
      r#"
      impure = false

      [commands.build]
      impure = true
      "#,
    )
    .unwrap();
    assert_eq!(cfg.for_command("build").impure, Some(true));
    // Command without per-command entry falls back to global false.
    assert_eq!(cfg.for_command("run").impure, Some(false));
  }

  #[test]
  fn test_command_config_env() {
    let cfg: Config = toml::from_str(
      r#"
      [commands.develop]
      env = { FOO = "bar", BAZ = "1" }
      "#,
    )
    .unwrap();
    let dev = cfg.for_command("develop");
    assert_eq!(dev.env.get("FOO").map(String::as_str), Some("bar"));
    assert_eq!(dev.env.get("BAZ").map(String::as_str), Some("1"));
  }

  #[test]
  fn test_command_config_env_table_syntax() {
    let cfg: Config = toml::from_str(
      r#"
      [commands.shell]
      impure = true

      [commands.shell.env]
      MY_VAR = "hello"
      "#,
    )
    .unwrap();
    let shell = cfg.for_command("shell");
    assert_eq!(shell.impure, Some(true));
    assert_eq!(shell.env.get("MY_VAR").map(String::as_str), Some("hello"));
  }

  #[test]
  fn test_for_command_missing_returns_default() {
    let cfg = Config::default();
    let cc = cfg.for_command("nonexistent");
    assert_eq!(cc.impure, None);
    assert!(cc.env.is_empty());
  }

  #[test]
  fn test_unknown_top_level_key_is_rejected() {
    let result = toml::de::from_str::<Config>("unknown_key = true");
    assert!(result.is_err(), "unknown top-level keys should be rejected");
  }

  #[test]
  fn test_unknown_command_key_is_rejected() {
    let result = toml::de::from_str::<Config>(
      r#"
      [commands.build]
      typo_key = true
      "#,
    );
    assert!(
      result.is_err(),
      "unknown per-command keys should be rejected"
    );
  }
}
