use std::collections::HashMap;

use eh_error::{EhError, Result};
use eh_log::{log_error, log_info};
use nix_command::{CommandKind, NixCommand};
use serde::Deserialize;
use yansi::Paint;

use crate::{
  eval::make_eval_expr,
  nix_config::ApplyCommandConfig,
  suggestions::print_error_suggestions,
};

const UNKNOWN_LICENSE: &str = "Unknown";

#[derive(Debug, Deserialize)]
struct PackageMeta {
  name:             String,
  version:          Option<String>,
  description:      Option<String>,
  long_description: Option<String>,
  license:          Option<serde_json::Value>,
  homepage:         Option<String>,
  platforms:        Option<Vec<String>>,
  broken:           Option<bool>,
  insecure:         Option<bool>,
  #[serde(rename = "unfree")]
  unfree:           Option<bool>,
}

#[derive(Debug, Deserialize)]
struct PackageOutputs {
  #[serde(flatten)]
  outputs: HashMap<String, serde_json::Value>,
}

pub fn handle_info(
  args: &[String],
  cfg: &eh_config::CommandConfig,
) -> Result<i32> {
  let pkg = args
    .iter()
    .find(|arg| !arg.starts_with('-'))
    .cloned()
    .unwrap_or_else(|| ".".to_string());

  let eval_arg = make_eval_expr(&pkg)?;
  let pkg_name = package_name_from_eval_expr(&eval_arg);

  log_info!("Fetching info for {}", pkg_name.bold());

  let meta_cmd = NixCommand::new(CommandKind::Eval)
    .arg("--json")
    .arg(&eval_arg)
    .print_build_logs(false)
    .apply_config(cfg);

  let meta_output = meta_cmd.output()?;

  if !meta_output.status.success() {
    log_error!("Failed to fetch package info");
    print_error_suggestions(&meta_output.stderr);
    return Err(EhError::NixCommandFailed {
      command: "eval".to_string(),
    });
  }

  let meta: PackageMeta =
    serde_json::from_slice(&meta_output.stdout).map_err(|e| {
      EhError::Io(std::io::Error::other(format!(
        "Failed to parse package metadata: {}",
        e
      )))
    })?;

  let outputs_expr = eval_arg
    .strip_suffix(".meta")
    .unwrap_or(&eval_arg)
    .to_string();
  let outputs_cmd = NixCommand::new(CommandKind::Eval)
    .arg("--json")
    .arg(format!("{}.outputs", outputs_expr))
    .print_build_logs(false)
    .apply_config(cfg);

  let outputs_output = outputs_cmd.output()?;
  let outputs: Option<PackageOutputs> = if outputs_output.status.success() {
    serde_json::from_slice(&outputs_output.stdout).ok()
  } else {
    None
  };

  print_package_info(&meta, outputs.as_ref(), &pkg);

  Ok(0)
}

fn package_name_from_eval_expr(eval_arg: &str) -> String {
  let name = eval_arg
    .rsplit_once('#')
    .map_or(eval_arg, |(_, name)| name)
    .trim_end_matches(".meta");
  if name.is_empty() { "default" } else { name }.to_string()
}

fn license_name(license: &serde_json::Value) -> Option<String> {
  match license {
    serde_json::Value::String(s) => Some(s.clone()),
    serde_json::Value::Object(obj) => {
      obj
        .get("spdxId")
        .and_then(|v| v.as_str())
        .or_else(|| obj.get("shortName").and_then(|v| v.as_str()))
        .map(str::to_string)
    },
    _ => None,
  }
}

fn format_license(license: &serde_json::Value) -> String {
  match license {
    serde_json::Value::Array(licenses) => {
      let names = licenses.iter().filter_map(license_name).collect::<Vec<_>>();
      if names.is_empty() {
        UNKNOWN_LICENSE.to_string()
      } else {
        names.join(", ")
      }
    },
    license => {
      license_name(license).unwrap_or_else(|| UNKNOWN_LICENSE.to_string())
    },
  }
}

fn print_package_info(
  meta: &PackageMeta,
  outputs: Option<&PackageOutputs>,
  pkg_ref: &str,
) {
  println!();

  println!("  {} {}", "Package:".bold(), meta.name);

  if let Some(ref version) = meta.version {
    println!("  {} {}", "Version:".bold(), version);
  }

  if let Some(ref desc) = meta.description {
    println!("  {} {}", "Description:".bold(), desc);
  }

  if let Some(ref long_desc) = meta.long_description {
    let should_show = meta
      .description
      .as_ref()
      .map(|d| d != long_desc)
      .unwrap_or(true);
    if should_show {
      println!();
      let wrapped = textwrap::fill(long_desc, 70);
      for line in wrapped.lines() {
        println!("    {}", line);
      }
    }
  }

  if let Some(ref license) = meta.license {
    println!("  {} {}", "License:".bold(), format_license(license));
  }

  if let Some(ref homepage) = meta.homepage {
    println!("  {} {}", "Homepage:".bold(), homepage);
  }

  println!();
  println!("  {}", "Meta:".bold());

  let mut status_parts = Vec::new();
  if meta.broken == Some(true) {
    status_parts.push("Broken".red().to_string());
  }
  if meta.insecure == Some(true) {
    status_parts.push("Insecure".red().to_string());
  }
  if meta.unfree == Some(true) {
    status_parts.push("Unfree".yellow().to_string());
  }

  if status_parts.is_empty() {
    println!("    {} {}", "Status:".bold(), "✓ Available".green());
  } else {
    println!("    {} {}", "Status:".bold(), status_parts.join(", "));
  }

  if let Some(ref platforms) = meta.platforms {
    let platform_list: Vec<_> = platforms.iter().take(4).cloned().collect();
    let platform_str = if platforms.len() > 4 {
      format!(
        "{} + {} more",
        platform_list.join(", "),
        platforms.len() - 4
      )
    } else {
      platform_list.join(", ")
    };
    println!("    {} {}", "Platforms:".bold(), platform_str);
  }

  if let Some(outputs) = outputs {
    println!();
    println!("  {}", "Outputs:".bold());
    let output_names: Vec<_> = outputs.outputs.keys().cloned().collect();
    for name in output_names {
      let marker = if name == "out" { " (default)" } else { "" };
      println!("    • {}{}", name, marker.dim());
    }
  }

  println!();
  println!("  {}", "Usage:".bold());
  println!(
    "    {} {} {}",
    "eh run".dim(),
    pkg_ref,
    "# Run the package".dim()
  );
  println!(
    "    {} {} {}",
    "eh shell".dim(),
    pkg_ref,
    "# Enter shell with package".dim()
  );
  println!();
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_package_meta_deserialization() {
    let json = r#"{
      "name": "hello",
      "version": "2.12.1",
      "description": "A greeting program",
      "license": "GPL-3.0",
      "homepage": "https://example.com",
      "platforms": ["x86_64-linux"],
      "broken": false,
      "insecure": false,
      "unfree": false
    }"#;

    let meta: PackageMeta = serde_json::from_str(json).unwrap();
    assert_eq!(meta.name, "hello");
    assert_eq!(meta.version, Some("2.12.1".to_string()));
  }

  #[test]
  fn test_license_object_parsing() {
    let json = r#"{
      "name": "test",
      "license": {"spdxId": "MIT", "fullName": "MIT License"}
    }"#;

    let meta: PackageMeta = serde_json::from_str(json).unwrap();
    assert!(meta.license.is_some());
  }
}
