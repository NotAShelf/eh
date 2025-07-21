use crate::util::run_nix_cmd;

pub fn handle_nix_run(args: &[String]) {
    run_nix_cmd("run", args);
}
