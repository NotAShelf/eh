use crate::error::{EhError, Result};

pub fn package_arg(args: &[String]) -> Option<&str> {
  args
    .iter()
    .find(|arg| !arg.starts_with('-'))
    .map(String::as_str)
}

pub fn make_eval_expr(eval_arg: &str) -> Result<String> {
  let eval_arg = eval_arg.trim();
  if eval_arg.is_empty() {
    return Err(EhError::InvalidEvalInput);
  }
  let eval_arg = if eval_arg == "." { ".#" } else { eval_arg };
  if eval_arg.ends_with('#') {
    Ok(format!("{eval_arg}default.meta"))
  } else if eval_arg.contains('#') {
    Ok(format!("{eval_arg}.meta"))
  } else {
    Ok(format!("nixpkgs#{eval_arg}.meta"))
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn builds_metadata_eval_expressions() {
    assert_eq!(make_eval_expr("hello").unwrap(), "nixpkgs#hello.meta");
    assert_eq!(make_eval_expr(".").unwrap(), ".#default.meta");
    assert_eq!(make_eval_expr(".#").unwrap(), ".#default.meta");
    assert_eq!(
      make_eval_expr("github:nixos/nixpkgs#hello").unwrap(),
      "github:nixos/nixpkgs#hello.meta"
    );
  }

  #[test]
  fn rejects_empty_eval_expression() {
    assert!(matches!(make_eval_expr(""), Err(EhError::InvalidEvalInput)));
    assert!(matches!(
      make_eval_expr("   "),
      Err(EhError::InvalidEvalInput)
    ));
  }
}
