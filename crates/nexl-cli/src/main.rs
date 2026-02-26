//! `nexl` — compile a Nexl source file to a WebAssembly binary.
//!
//! Usage: `nexl build <input.nexl> [output.wasm]`
//!
//! If no output path is given, the output file is derived from the input
//! by replacing the extension with `.wasm`.

use clap::{Parser, Subcommand};
use std::io::{self, BufRead, Write};
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
        Command::Run { input } => {
            if let Err(message) = command_run(input) {
                eprintln!("nexl: {message}");
                process::exit(1);
            }
        }
        Command::Repl => {
            if let Err(message) = command_repl() {
                eprintln!("nexl: {message}");
                process::exit(1);
            }
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

fn command_run(input_path: PathBuf) -> Result<(), String> {
    let source = std::fs::read_to_string(&input_path)
        .map_err(|e| format!("cannot read {:?}: {e}", input_path))?;

    let nodes = nexl_reader::read(&source, meta::FileId::SYNTHETIC)
        .map_err(|e| format!("parse error: {e}"))?;

    let env = nexl_eval::stdlib::standard_env();
    for node in &nodes {
        nexl_eval::eval::eval(node, &env).map_err(|e| format!("eval error: {e}"))?;
    }

    Ok(())
}

fn repl_loop<R: BufRead, W: Write>(mut input: R, mut output: W) -> io::Result<()> {
    let env = nexl_eval::stdlib::standard_env();

    loop {
        output.write_all(b"nexl> ")?;
        output.flush()?;

        let mut line = String::new();
        let bytes = input.read_line(&mut line)?;
        if bytes == 0 {
            writeln!(output)?;
            break;
        }

        for result in nexl_eval::repl::eval_line(line.trim_end_matches('\n'), &env) {
            match result {
                Ok(value) => writeln!(output, "{value}")?,
                Err(message) => writeln!(output, "error: {message}")?,
            }
        }
    }

    Ok(())
}

fn command_repl() -> Result<(), String> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    repl_loop(stdin.lock(), stdout.lock()).map_err(|e| format!("repl error: {e}"))
}


#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn write_temp_file(contents: &str, label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be available")
            .as_nanos();
        let mut path = std::env::temp_dir();
        path.push(format!("nexl_cli_{label}_{nanos}.nexl"));
        std::fs::write(&path, contents).expect("write temp file");
        path
    }

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

    #[test]
    fn run_executes_file() {
        let path = write_temp_file("(+ 1 2)", "run_ok");
        let result = command_run(path.clone());
        let _ = std::fs::remove_file(&path);
        assert!(result.is_ok(), "run should succeed, got: {result:?}");
    }

    #[test]
    fn repl_loop_evaluates_line() {
        let input = Cursor::new(b"(+ 1 2)\n");
        let mut output = Vec::new();
        repl_loop(input, &mut output).expect("repl loop");
        let output = String::from_utf8(output).expect("utf8");
        assert!(output.contains("nexl> "));
        assert!(output.contains('3'));
    }
}
