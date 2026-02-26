//! `nexl` — compile a Nexl source file to a WebAssembly binary.
//!
//! Usage: `nexl build <input.nexl> [output.wasm]`
//!
//! If no output path is given, the output file is derived from the input
//! by replacing the extension with `.wasm`.

use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::process;

#[derive(Debug, Parser, PartialEq, Eq)]
#[command(name = "nexl")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand, PartialEq, Eq)]
enum Command {
    Build {
        #[arg(value_name = "FILE")]
        input: PathBuf,
        #[arg(value_name = "OUT")]
        output: Option<PathBuf>,
    },
    Run {
        #[arg(value_name = "FILE")]
        input: PathBuf,
    },
    Repl,
    Check {
        #[arg(value_name = "FILE")]
        input: PathBuf,
    },
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Command::Build { input, output } => {
            if let Err(message) = command_build(input, output) {
                eprintln!("nexl: {message}");
                process::exit(1);
            }
        }
        Command::Run { .. } => {
            eprintln!("nexl: run is not implemented yet");
            process::exit(1);
        }
        Command::Repl => {
            eprintln!("nexl: repl is not implemented yet");
            process::exit(1);
        }
        Command::Check { .. } => {
            eprintln!("nexl: check is not implemented yet");
            process::exit(1);
        }
    }
}

fn command_build(input_path: PathBuf, output_override: Option<PathBuf>) -> Result<(), String> {
    let output_path = output_override.unwrap_or_else(|| input_path.with_extension("wasm"));

    let source = std::fs::read_to_string(&input_path)
        .map_err(|e| format!("cannot read {:?}: {e}", input_path))?;

    let module_name = input_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("module")
        .to_string();

    let nodes = nexl_reader::read(&source, meta::FileId::SYNTHETIC)
        .map_err(|e| format!("parse error: {e}"))?;

    let ir_module = nexl_ir::Lowerer::new(&module_name)
        .lower_module(&nodes)
        .map_err(|e| format!("lowering error: {e}"))?;

    let wasm_bytes = nexl_wasm::Emitter::new()
        .emit(&ir_module)
        .map_err(|e| format!("codegen error: {e}"))?;

    std::fs::write(&output_path, &wasm_bytes)
        .map_err(|e| format!("cannot write {:?}: {e}", output_path))?;

    println!("nexl: wrote {} bytes to {:?}", wasm_bytes.len(), output_path);
    Ok(())
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_build_with_input() {
        let cli = Cli::try_parse_from(["nexl", "build", "main.nexl"]).expect("parse");
        assert_eq!(
            cli.command,
            Command::Build {
                input: PathBuf::from("main.nexl"),
                output: None,
            }
        );
    }

    #[test]
    fn parse_build_with_output() {
        let cli = Cli::try_parse_from(["nexl", "build", "main.nexl", "out.wasm"])
            .expect("parse");
        assert_eq!(
            cli.command,
            Command::Build {
                input: PathBuf::from("main.nexl"),
                output: Some(PathBuf::from("out.wasm")),
            }
        );
    }

    #[test]
    fn parse_run_with_input() {
        let cli = Cli::try_parse_from(["nexl", "run", "main.nexl"]).expect("parse");
        assert_eq!(
            cli.command,
            Command::Run {
                input: PathBuf::from("main.nexl"),
            }
        );
    }

    #[test]
    fn parse_repl_without_args() {
        let cli = Cli::try_parse_from(["nexl", "repl"]).expect("parse");
        assert_eq!(cli.command, Command::Repl);
    }

    #[test]
    fn parse_check_with_input() {
        let cli = Cli::try_parse_from(["nexl", "check", "main.nexl"]).expect("parse");
        assert_eq!(
            cli.command,
            Command::Check {
                input: PathBuf::from("main.nexl"),
            }
        );
    }
}
