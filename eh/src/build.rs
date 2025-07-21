use crate::util::run_nix_cmd;

pub fn handle_nix_build(args: &[String]) {
    run_nix_cmd("build", args);
}
