use clap::{CommandFactory, Parser};
use regex::Regex;
use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::process::{Command as StdCommand, Stdio};

#[derive(Parser)]
#[command(name = "eh")]
#[command(about = "Ergonomic Nix helper", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(clap::Subcommand)]
enum Command {
    /// Run a Nix derivation
    Run {
        #[arg(trailing_var_arg = true)]
        args: Vec<String>,
    },

    /// Enter a Nix shell
    Shell {
        #[arg(trailing_var_arg = true)]
        args: Vec<String>,
    },

    /// Build a Nix derivation
    Build {
        #[arg(trailing_var_arg = true)]
        args: Vec<String>,
    },
}

fn main() {
    let path = env::args().next();
    let app_name = path
        .as_ref()
        .and_then(|p| Path::new(p).file_name())
        .and_then(|name| name.to_str())
        .unwrap_or("eh");

    // Multicall logic
    if app_name == "nr" {
        let args: Vec<String> = env::args().skip(1).collect();
        handle_nix_run(&args);
        return;
    }
    if app_name == "ns" {
        let args: Vec<String> = env::args().skip(1).collect();
        handle_nix_shell(&args);
        return;
    }
    if app_name == "nb" {
        let args: Vec<String> = env::args().skip(1).collect();
        handle_nix_build(&args);
        return;
    }

    let cli = Cli::parse();

    match cli.command {
        Some(Command::Run { args }) => handle_nix_run(&args),
        Some(Command::Shell { args }) => handle_nix_shell(&args),
        Some(Command::Build { args }) => handle_nix_build(&args),
        None => {
            Cli::command().print_help().unwrap();
            println!();
            std::process::exit(0);
        }
    }
}

fn handle_nix_run(args: &[String]) {
    run_nix_cmd("run", args);
}

fn handle_nix_shell(args: &[String]) {
    run_nix_cmd("shell", args);
}

fn handle_nix_build(args: &[String]) {
    run_nix_cmd("build", args);
}

fn run_nix_cmd(subcommand: &str, args: &[String]) {
    let mut cmd = StdCommand::new("nix");
    cmd.arg(subcommand);

    if !args.iter().any(|arg| arg == "--no-build-output") {
        cmd.arg("--print-build-logs");
    }

    cmd.args(args);
    cmd.stderr(Stdio::piped());
    cmd.stdout(Stdio::inherit());

    let mut child = cmd.spawn().expect("Failed to start nix command");
    let stderr = child.stderr.take().unwrap();

    let stderr_handle = std::thread::spawn(move || {
        let mut buffer = Vec::new();
        std::io::copy(&mut std::io::BufReader::new(stderr), &mut buffer).unwrap();
        buffer
    });

    let exit_status = child.wait().expect("Failed to wait for nix command");
    let stderr_output = stderr_handle.join().unwrap();

    let stderr_str = String::from_utf8_lossy(&stderr_output);

    if !exit_status.success() {
        if !should_retry_nix_error(&stderr_str) {
            io::stderr().write_all(&stderr_output).unwrap();
        }
        handle_nix_error(subcommand, args, &stderr_str);
    }
}

fn handle_nix_error(subcommand: &str, args: &[String], stderr: &str) {
    if let Some(new_hash) = extract_hash_from_error(stderr) {
        if fix_hash_in_files(&new_hash) {
            println!("Fixed hash mismatch, retrying...");
            retry_nix_command(subcommand, args);
            return;
        }
    }

    if stderr.contains("unfree") && stderr.contains("refusing") {
        println!("Unfree package detected, retrying with NIXPKGS_ALLOW_UNFREE=1...");
        retry_nix_command_with_env(subcommand, args, "NIXPKGS_ALLOW_UNFREE", "1");
        return;
    }

    if stderr.contains("insecure") && stderr.contains("refusing") {
        println!("Insecure package detected, retrying with NIXPKGS_ALLOW_INSECURE=1...");
        retry_nix_command_with_env(subcommand, args, "NIXPKGS_ALLOW_INSECURE", "1");
        return;
    }

    if stderr.contains("broken") && stderr.contains("refusing") {
        println!("Broken package detected, retrying with NIXPKGS_ALLOW_BROKEN=1...");
        retry_nix_command_with_env(subcommand, args, "NIXPKGS_ALLOW_BROKEN", "1");
        return;
    }

    io::stderr().write_all(stderr.as_bytes()).unwrap();
    std::process::exit(1);
}

fn extract_hash_from_error(stderr: &str) -> Option<String> {
    let patterns = [
        r"got:\s+([a-zA-Z0-9+/=]+)",
        r"actual:\s+([a-zA-Z0-9+/=]+)",
        r"have:\s+([a-zA-Z0-9+/=]+)",
    ];

    for pattern in &patterns {
        if let Ok(re) = Regex::new(pattern) {
            if let Some(captures) = re.captures(stderr) {
                if let Some(hash) = captures.get(1) {
                    return Some(hash.as_str().to_string());
                }
            }
        }
    }
    None
}

fn fix_hash_in_files(new_hash: &str) -> bool {
    let nix_files = find_nix_files();
    let mut fixed = false;

    for file_path in nix_files {
        if fix_hash_in_file(&file_path, new_hash) {
            println!("Updated hash in {file_path}");
            fixed = true;
        }
    }

    fixed
}

fn find_nix_files() -> Vec<String> {
    let mut files = Vec::new();

    let candidates = [
        "default.nix",
        "package.nix",
        "shell.nix",
        "flake.nix",
        "nix/default.nix",
        "nix/package.nix",
        "nix/site.nix",
    ];

    for candidate in &candidates {
        if Path::new(candidate).exists() {
            files.push((*candidate).to_string());
        }
    }

    if let Ok(entries) = fs::read_dir(".") {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                if std::path::Path::new(name)
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("nix"))
                    && !files.contains(&name.to_string())
                {
                    files.push(name.to_string());
                }
            }
        }
    }

    files
}

fn fix_hash_in_file(file_path: &str, new_hash: &str) -> bool {
    if let Ok(content) = fs::read_to_string(file_path) {
        let patterns = [
            (r#"hash\s*=\s*"[^"]*""#, format!(r#"hash = "{new_hash}""#)),
            (
                r#"sha256\s*=\s*"[^"]*""#,
                format!(r#"sha256 = "{new_hash}""#),
            ),
            (
                r#"outputHash\s*=\s*"[^"]*""#,
                format!(r#"outputHash = "{new_hash}""#),
            ),
        ];

        for (pattern, replacement) in &patterns {
            if let Ok(re) = Regex::new(pattern) {
                if re.is_match(&content) {
                    let new_content = re.replace_all(&content, replacement);
                    if fs::write(file_path, new_content.as_ref()).is_ok() {
                        return true;
                    }
                }
            }
        }
    }
    false
}

fn retry_nix_command(subcommand: &str, args: &[String]) {
    let mut cmd = StdCommand::new("nix");
    cmd.arg(subcommand);

    if !args.iter().any(|arg| arg == "--no-build-output") {
        cmd.arg("--print-build-logs");
    }

    cmd.args(args);

    let exit_status = cmd.status().expect("Failed to retry nix command");
    std::process::exit(exit_status.code().unwrap_or(1));
}

fn retry_nix_command_with_env(subcommand: &str, args: &[String], env_key: &str, env_value: &str) {
    let mut cmd = StdCommand::new("nix");
    cmd.env(env_key, env_value);
    cmd.arg(subcommand);

    // Add --impure for env var to take effect
    cmd.arg("--impure");

    if !args.iter().any(|arg| arg == "--no-build-output") {
        cmd.arg("--print-build-logs");
    }

    cmd.args(args);

    let exit_status = cmd.status().expect("Failed to retry nix command");
    std::process::exit(exit_status.code().unwrap_or(1));
}

fn should_retry_nix_error(stderr: &str) -> bool {
    if extract_hash_from_error(stderr).is_some() {
        return true;
    }
    (stderr.contains("unfree") && stderr.contains("refusing"))
        || (stderr.contains("insecure") && stderr.contains("refusing"))
        || (stderr.contains("broken") && stderr.contains("refusing"))
}
