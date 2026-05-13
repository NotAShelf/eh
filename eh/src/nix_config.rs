use nix_command::NixCommand;

pub trait ApplyCommandConfig {
  fn apply_config(self, cfg: &eh_config::CommandConfig) -> Self;
}

impl ApplyCommandConfig for NixCommand {
  fn apply_config(mut self, cfg: &eh_config::CommandConfig) -> Self {
    if cfg.impure == Some(true) {
      self = self.impure(true);
    }
    self.envs(cfg.env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
  }
}
