use std::{
  collections::HashMap,
  env,
  fs,
  path::{Path, PathBuf},
};

use eh_log::log_warn;
use serde::Deserialize;

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Config {
  #[serde(default)]
  pub impure:   Option<bool>,
  #[serde(default)]
  pub commands: HashMap<String, CommandConfig>,
}

#[derive(Debug, Deserialize, Default, Clone)]
#[serde(deny_unknown_fields)]
pub struct CommandConfig {
  #[serde(default)]
  pub impure: Option<bool>,
  #[serde(default)]
  pub env:    HashMap<String, String>,
}

impl Config {
  #[must_use]
  pub fn for_command(&self, command: &str) -> CommandConfig {
    let mut cmd = self.commands.get(command).cloned().unwrap_or_default();
    cmd.impure = cmd.impure.or(self.impure);
    cmd
  }
}

#[must_use]
pub fn load() -> Config {
  find_project_config()
    .and_then(|path| load_from_file(&path))
    .or_else(|| global_config_path().and_then(|path| load_from_file(&path)))
    .unwrap_or_default()
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
  dirs::config_dir().map(|dir| dir.join("eh").join("config.toml"))
}

fn load_from_file(path: &Path) -> Option<Config> {
  let content = fs::read_to_string(path).ok()?;
  match toml::de::from_str::<Config>(&content) {
    Ok(cfg) => Some(cfg),
    Err(e) => {
      log_warn!("failed to parse config file {}: {}", path.display(), e);
      None
    },
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn empty_config_defaults() {
    let cfg: Config = toml::from_str("").unwrap();
    assert!(cfg.impure.is_none());
    assert!(cfg.commands.is_empty());
  }

  #[test]
  fn command_impure_overrides_global() {
    let cfg: Config = toml::from_str(
      r#"
      impure = false

      [commands.build]
      impure = true
      "#,
    )
    .unwrap();
    assert_eq!(cfg.for_command("build").impure, Some(true));
    assert_eq!(cfg.for_command("run").impure, Some(false));
  }

  #[test]
  fn command_env_supports_inline_and_table_syntax() {
    let inline: Config = toml::from_str(
      r#"
      [commands.develop]
      env = { FOO = "bar" }
      "#,
    )
    .unwrap();
    assert_eq!(
      inline.for_command("develop").env.get("FOO"),
      Some(&"bar".into())
    );

    let table: Config = toml::from_str(
      r#"
      [commands.shell.env]
      MY_VAR = "hello"
      "#,
    )
    .unwrap();
    assert_eq!(
      table.for_command("shell").env.get("MY_VAR"),
      Some(&"hello".into())
    );
  }

  #[test]
  fn rejects_unknown_fields() {
    assert!(toml::de::from_str::<Config>("unknown_key = true").is_err());
    assert!(
      toml::de::from_str::<Config>("[commands.build]\ntypo = true").is_err()
    );
  }
}
