use clap::{CommandFactory, Parser, Subcommand};
use std::env;
use std::path::Path;

mod build;
mod run;
mod shell;
mod util;

#[derive(Parser)]
#[command(name = "eh")]
#[command(about = "Ergonomic Nix helper", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
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
    let mut args = env::args();
    let bin = args.next().unwrap_or_else(|| "eh".to_string());
    let app_name = Path::new(&bin)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("eh");

    match app_name {
        "nr" => {
            let rest: Vec<String> = args.collect();
            run::handle_nix_run(&rest);
            return;
        }
        "ns" => {
            let rest: Vec<String> = args.collect();
            shell::handle_nix_shell(&rest);
            return;
        }
        "nb" => {
            let rest: Vec<String> = args.collect();
            build::handle_nix_build(&rest);
            return;
        }
        _ => {}
    }

    let cli = Cli::parse();

    match cli.command {
        Some(Command::Run { args }) => run::handle_nix_run(&args),
        Some(Command::Shell { args }) => shell::handle_nix_shell(&args),
        Some(Command::Build { args }) => build::handle_nix_build(&args),
        None => {
            Cli::command().print_help().unwrap();
            println!();
            std::process::exit(0);
        }
    }
}
