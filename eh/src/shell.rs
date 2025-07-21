use crate::util::run_nix_cmd;

pub fn handle_nix_shell(args: &[String]) {
    run_nix_cmd("shell", args);
}
