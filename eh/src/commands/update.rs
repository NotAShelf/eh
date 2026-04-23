use crate::{
  commands::{NixCommand, StdIoInterceptor},
  error::{EhError, Result},
};

/// Parse flake input names from `nix flake metadata --json` output.
pub fn parse_flake_inputs(stdout: &str) -> Result<Vec<String>> {
  let value: serde_json::Value = serde_json::from_str(stdout).map_err(|e| {
    EhError::JsonParse {
      detail: e.to_string(),
    }
  })?;

  let inputs = value
    .get("locks")
    .and_then(|l| l.get("nodes"))
    .and_then(|n| n.get("root"))
    .and_then(|r| r.get("inputs"))
    .and_then(|i| i.as_object())
    .ok_or(EhError::NoFlakeInputs)?;

  let mut names: Vec<String> = inputs.keys().cloned().collect();
  names.sort();
  Ok(names)
}

/// Fetch flake input names by running `nix flake metadata --json`.
fn fetch_flake_inputs() -> Result<Vec<String>> {
  let output = NixCommand::new("flake")
    .arg("metadata")
    .arg("--json")
    .print_build_logs(false)
    .output()?;

  let stdout = String::from_utf8(output.stdout)?;
  parse_flake_inputs(&stdout)
}

/// Prompt the user to select inputs via a multi-select dialog.
fn prompt_input_selection(inputs: &[String]) -> Result<Vec<String>> {
  let selections = dialoguer::MultiSelect::new()
    .with_prompt("Select inputs to update")
    .items(inputs)
    .interact()
    .map_err(|e| EhError::Io(std::io::Error::other(e)))?;

  if selections.is_empty() {
    return Err(EhError::UpdateCancelled);
  }

  Ok(selections.iter().map(|&i| inputs[i].clone()).collect())
}

/// Entry point for the `update` subcommand.
///
/// If `args` is non-empty, use them as explicit input names.
/// Otherwise, fetch inputs interactively and prompt for selection.
pub fn handle_update(
  args: &[String],
  cfg: &crate::config::CommandConfig,
) -> Result<i32> {
  let selected = if args.is_empty() {
    let inputs = fetch_flake_inputs()?;
    if inputs.is_empty() {
      return Err(EhError::NoFlakeInputs);
    }
    prompt_input_selection(&inputs)?
  } else {
    args.to_vec()
  };

  let mut cmd = NixCommand::new("flake").arg("lock").with_config(cfg);
  for name in &selected {
    cmd = cmd.arg("--update-input").arg(name);
  }

  eh_log::log_info!("updating inputs: {}", selected.join(", "));

  let status = cmd.run_with_logs(StdIoInterceptor)?;
  Ok(status.code().unwrap_or(1))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_parse_flake_inputs() {
    let json = r#"{
      "locks": {
        "nodes": {
          "root": {
            "inputs": {
              "nixpkgs": "nixpkgs_2",
              "home-manager": "home-manager_2",
              "flake-utils": "flake-utils_2"
            }
          }
        }
      }
    }"#;

    let inputs = parse_flake_inputs(json).unwrap();
    assert_eq!(inputs, vec!["flake-utils", "home-manager", "nixpkgs"]);
  }

  #[test]
  fn test_parse_flake_inputs_invalid_json() {
    let result = parse_flake_inputs("not json");
    assert!(result.is_err());
  }

  #[test]
  fn test_parse_flake_inputs_no_inputs() {
    let json = r#"{"locks": {"nodes": {"root": {}}}}"#;
    let result = parse_flake_inputs(json);
    assert!(matches!(result, Err(EhError::NoFlakeInputs)));
  }
}
