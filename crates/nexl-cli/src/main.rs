//! `nexl` — compile a Nexl source file to a WebAssembly binary.
//!
//! Usage: `nexl build <input.nexl> [output.wasm]`
//!
//! If no output path is given, the output file is derived from the input
//! by replacing the extension with `.wasm`.

use clap::{Parser, Subcommand};
use meta::{Atom, Node, NodeKind};
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
        Command::Check { input } => {
            if let Err(message) = command_check(input) {
                eprintln!("nexl: {message}");
                process::exit(1);
            }
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
    let mut buffer = String::new();

    loop {
        let prompt = if buffer.is_empty() { "nexl> " } else { "...> " };
        output.write_all(prompt.as_bytes())?;
        output.flush()?;

        let mut line = String::new();
        let bytes = input.read_line(&mut line)?;
        if bytes == 0 {
            writeln!(output)?;
            break;
        }

        buffer.push_str(&line);
        if !delimiters_balanced(&buffer) {
            continue;
        }

        let source = buffer.trim_end_matches('\n');
        for result in nexl_eval::repl::eval_line(source, &env) {
            match result {
                Ok(value) => writeln!(output, "{value}")?,
                Err(message) => writeln!(output, "error: {message}")?,
            }
        }

        buffer.clear();
    }

    Ok(())
}

fn delimiters_balanced(source: &str) -> bool {
    let mut paren = 0i32;
    let mut bracket = 0i32;
    let mut brace = 0i32;
    let mut in_string = false;
    let mut escaped = false;
    let mut in_comment = false;

    for ch in source.chars() {
        if in_comment {
            if ch == '\n' {
                in_comment = false;
            }
            continue;
        }

        if in_string {
            if escaped {
                escaped = false;
                continue;
            }
            match ch {
                '\\' => escaped = true,
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }

        match ch {
            ';' => in_comment = true,
            '"' => in_string = true,
            '(' => paren += 1,
            ')' => {
                paren -= 1;
                if paren < 0 {
                    return true;
                }
            }
            '[' => bracket += 1,
            ']' => {
                bracket -= 1;
                if bracket < 0 {
                    return true;
                }
            }
            '{' => brace += 1,
            '}' => {
                brace -= 1;
                if brace < 0 {
                    return true;
                }
            }
            _ => {}
        }
    }

    paren == 0 && bracket == 0 && brace == 0
}

fn command_repl() -> Result<(), String> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    repl_loop(stdin.lock(), stdout.lock()).map_err(|e| format!("repl error: {e}"))
}

fn command_check(input_path: PathBuf) -> Result<(), String> {
    let source = std::fs::read_to_string(&input_path)
        .map_err(|e| format!("cannot read {:?}: {e}", input_path))?;
    let nodes = nexl_reader::read(&source, meta::FileId::SYNTHETIC)
        .map_err(|e| format!("parse error: {e}"))?;

    let mut env = nexl_infer::Env::new();
    let mut state = nexl_infer::InferState::new();
    for node in &nodes {
        env = check_top_level(node, env, &mut state)?;
    }

    if !state.errors.is_empty() {
        let message = state
            .errors
            .iter()
            .map(|error| error.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        return Err(message);
    }

    Ok(())
}

fn check_top_level(
    node: &Node,
    env: nexl_infer::Env,
    state: &mut nexl_infer::InferState,
) -> Result<nexl_infer::Env, String> {
    if list_head_is(node, "def") {
        let (_name, _ty, new_env) =
            nexl_infer::infer_def(node, &env, state).map_err(|e| format!("type error: {e}"))?;
        return Ok(new_env);
    }
    if list_head_is(node, "defn") {
        let (_name, _ty, new_env) =
            nexl_infer::infer_defn(node, &env, state).map_err(|e| format!("type error: {e}"))?;
        return Ok(new_env);
    }
    nexl_infer::synth(node, &env, state).map_err(|e| format!("type error: {e}"))?;
    Ok(env)
}

fn list_head_is(node: &Node, name: &str) -> bool {
    let NodeKind::List(items) = &node.kind else {
        return false;
    };
    let Some(first) = items.first() else {
        return false;
    };
    match &first.kind {
        NodeKind::Atom(Atom::Symbol { ns: None, name: head }) => head == name,
        _ => false,
    }
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

    #[test]
    fn repl_loop_continues_for_unbalanced_input() {
        let input = Cursor::new(b"(+ 1\n 2)\n");
        let mut output = Vec::new();
        repl_loop(input, &mut output).expect("repl loop");
        let output = String::from_utf8(output).expect("utf8");
        assert!(output.contains("nexl> "));
        assert!(output.contains("...> "));
        assert!(output.contains('3'));
        assert!(!output.contains("error:"));
    }

    #[test]
    fn check_type_checks_file() {
        let path = write_temp_file("(def x 1) x", "check_ok");
        let result = command_check(path.clone());
        let _ = std::fs::remove_file(&path);
        assert!(result.is_ok(), "check should succeed, got: {result:?}");
    }
}
