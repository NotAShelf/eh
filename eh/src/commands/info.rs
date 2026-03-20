use std::collections::HashMap;

use eh_log::{log_error, log_info};
use serde::Deserialize;
use yansi::Paint;

use crate::{
  commands::NixCommand,
  error::{EhError, Result},
  util::{make_eval_expr, print_error_suggestions},
};

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

pub fn handle_info(args: &[String]) -> Result<i32> {
  // Get the package argument (skip flags)
  let pkg = args
    .iter()
    .find(|arg| !arg.starts_with('-'))
    .cloned()
    .unwrap_or_else(|| ".".to_string());

  let eval_arg = make_eval_expr(&pkg);
  let pkg_name: String = if eval_arg.contains("#") {
    eval_arg
      .split("#")
      .last()
      .unwrap_or(&eval_arg)
      .trim_end_matches(".meta")
      .to_string()
  } else {
    eval_arg.trim_end_matches(".meta").to_string()
  };
  // Handle .# case - show "default" as the package name
  let pkg_name = if pkg_name.is_empty() {
    "default".to_string()
  } else {
    pkg_name
  };

  log_info!("Fetching info for {}", pkg_name.bold());

  // Fetch metadata
  let meta_cmd = NixCommand::new("eval")
    .arg("--json")
    .arg(&eval_arg)
    .print_build_logs(false);

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

  // Fetch outputs
  let outputs_expr = eval_arg
    .strip_suffix(".meta")
    .unwrap_or(&eval_arg)
    .to_string();
  let outputs_cmd = NixCommand::new("eval")
    .arg("--json")
    .arg(format!("{}.outputs", outputs_expr))
    .print_build_logs(false);

  let outputs_output = outputs_cmd.output()?;
  let outputs: Option<PackageOutputs> = if outputs_output.status.success() {
    serde_json::from_slice(&outputs_output.stdout).ok()
  } else {
    None
  };

  // Print formatted info
  print_package_info(&meta, outputs.as_ref(), &pkg);

  Ok(0)
}

fn print_package_info(
  meta: &PackageMeta,
  outputs: Option<&PackageOutputs>,
  pkg_ref: &str,
) {
  println!();

  // Header
  println!("  {} {}", "Package:".bold(), meta.name);

  if let Some(ref version) = meta.version {
    println!("  {} {}", "Version:".bold(), version);
  }

  if let Some(ref desc) = meta.description {
    println!("  {} {}", "Description:".bold(), desc);
  }

  // Show long description if available and different from short description
  if let Some(ref long_desc) = meta.long_description {
    let should_show = meta
      .description
      .as_ref()
      .map(|d| d != long_desc)
      .unwrap_or(true);
    if should_show {
      println!();
      // Wrap long description to 70 chars for readability
      let wrapped = textwrap::fill(long_desc, 70);
      for line in wrapped.lines() {
        println!("    {}", line);
      }
    }
  }

  // License
  if let Some(ref license) = meta.license {
    let license_str = match license {
      serde_json::Value::String(s) => s.clone(),
      serde_json::Value::Object(obj) => {
        obj
          .get("spdxId")
          .and_then(|v| v.as_str())
          .or_else(|| obj.get("shortName").and_then(|v| v.as_str()))
          .unwrap_or("Unknown")
          .to_string()
      },
      serde_json::Value::Array(licenses) => {
        // Handle multiple licenses (e.g., neovim has Apache-2.0 AND Vim)
        let license_names: Vec<String> = licenses
          .iter()
          .filter_map(|lic| {
            match lic {
              serde_json::Value::Object(obj) => {
                obj
                  .get("spdxId")
                  .and_then(|v| v.as_str())
                  .or_else(|| obj.get("shortName").and_then(|v| v.as_str()))
                  .map(|s| s.to_string())
              },
              serde_json::Value::String(s) => Some(s.clone()),
              _ => None,
            }
          })
          .collect();

        if license_names.is_empty() {
          "Unknown".to_string()
        } else {
          license_names.join(", ")
        }
      },
      _ => "Unknown".to_string(),
    };
    println!("  {} {}", "License:".bold(), license_str);
  }

  // Homepage
  if let Some(ref homepage) = meta.homepage {
    println!("  {} {}", "Homepage:".bold(), homepage);
  }

  // Meta section
  println!();
  println!("  {}", "Meta:".bold());

  // Status indicators
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

  // Platforms
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

  // Outputs section
  if let Some(outputs) = outputs {
    println!();
    println!("  {}", "Outputs:".bold());
    let output_names: Vec<_> = outputs.outputs.keys().cloned().collect();
    for name in output_names {
      let marker = if name == "out" { " (default)" } else { "" };
      println!("    • {}{}", name, marker.dim());
    }
  }

  // Usage section
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
