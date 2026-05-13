use std::sync::LazyLock;

use eh_log::log_info;
use regex::Regex;
use yansi::Paint;

static DID_YOU_MEAN_PATTERN: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r#"Did you mean (?:one of )?(.+?)\?"#).unwrap());

fn parse_nix_suggestions(did_you_mean_line: &str) -> Vec<String> {
  DID_YOU_MEAN_PATTERN
    .captures(did_you_mean_line)
    .and_then(|caps| caps.get(1))
    .map(|m| m.as_str())
    .map(|suggestions| {
      suggestions
        .split(", ")
        .flat_map(|part| part.split(" or "))
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
    })
    .unwrap_or_default()
}

pub fn print_error_suggestions(stderr: &[u8]) {
  let stderr = String::from_utf8_lossy(stderr);
  let Some(line) = stderr.lines().find(|line| line.contains("Did you mean"))
  else {
    return;
  };
  let suggestions = parse_nix_suggestions(line);
  if suggestions.is_empty() {
    return;
  }
  let formatted = suggestions
    .iter()
    .map(|s| s.bold().to_string())
    .collect::<Vec<_>>()
    .join(", ");
  log_info!("Did you mean: {}?", formatted);
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn parses_nix_suggestions() {
    assert_eq!(
      parse_nix_suggestions(
        "Did you mean one of neovim, hevi, navi, neo or neo4j?"
      ),
      ["neovim", "hevi", "navi", "neo", "neo4j"]
    );
  }
}
