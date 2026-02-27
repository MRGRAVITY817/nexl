//! `nexl` — compile a Nexl source file to a WebAssembly binary.
//!
//! Usage: `nexl build <input.nexl> [output.wasm]`
//!
//! If no output path is given, the output file is derived from the input
//! by replacing the extension with `.wasm`.

mod repl_protocol;

use clap::{Parser, Subcommand};
use meta::{Atom, Node, NodeKind};
use nexl_doc::{extract_module_doc, render_module_pages};
use nexl_pkg::{
    build_lockfile, parse_manifest, serialize_lockfile, serialize_manifest, DependencySpec,
    PackageManifest,
};
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
        /// Target: "wasm" (default) or "native"
        #[arg(short = 't', long = "target", default_value = "wasm")]
        target: String,
        /// GC mode: "rc" (default), "gc" (WASM GC), or "none" (arena/no GC)
        #[arg(long = "gc", default_value = "rc")]
        gc: String,
        /// Disable optimizations
        #[arg(long = "no-opt")]
        no_opt: bool,
    },
    Run {
        #[arg(value_name = "FILE")]
        input: PathBuf,
    },
    Repl {
        /// Use structured JSON protocol mode (§14.3) instead of interactive REPL.
        #[arg(long = "protocol")]
        protocol: bool,
    },
    Check {
        #[arg(value_name = "FILE")]
        input: PathBuf,
    },
    Doc {
        #[arg(value_name = "FILE")]
        input: PathBuf,
        #[arg(value_name = "OUT")]
        output: Option<PathBuf>,
    },
    Lsp,
    Sandbox {
        #[arg(value_name = "FILE")]
        input: PathBuf,
        /// Allow console I/O (stdout, stderr).
        #[arg(long = "allow-console")]
        allow_console: bool,
        /// Allow file-system access.
        #[arg(long = "allow-fs")]
        allow_fs: bool,
        /// Allow network access.
        #[arg(long = "allow-net")]
        allow_net: bool,
        /// Allow wall-clock time access.
        #[arg(long = "allow-time")]
        allow_time: bool,
        /// Allow random number generation.
        #[arg(long = "allow-random")]
        allow_random: bool,
        /// Allow concurrency primitives.
        #[arg(long = "allow-concurrent")]
        allow_concurrent: bool,
        /// Allow unsafe FFI operations.
        #[arg(long = "allow-unsafe")]
        allow_unsafe: bool,
        /// Allow all capabilities (unrestricted).
        #[arg(long = "allow-all")]
        allow_all: bool,
    },
    Audit {
        #[arg(value_name = "FILE")]
        input: PathBuf,
    },
    Pkg {
        #[command(subcommand)]
        command: PkgCommand,
    },
}

#[derive(Debug, Subcommand, PartialEq, Eq)]
enum PkgCommand {
    Add {
        #[arg(value_name = "DEP")]
        dep: String,
        /// Add to dev-dependencies instead of dependencies.
        #[arg(long = "dev")]
        dev: bool,
        /// Explicit version requirement (if DEP does not include @version).
        #[arg(long = "version")]
        version: Option<String>,
    },
    Remove {
        #[arg(value_name = "DEP")]
        dep: String,
    },
    Lock,
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Command::Build { input, output, target, gc, no_opt } => {
            if let Err(message) = command_build(input, output, &target, &gc, no_opt) {
                print_error(&message);
                process::exit(1);
            }
        }
        Command::Run { input } => {
            if let Err(message) = command_run(input) {
                print_error(&message);
                process::exit(1);
            }
        }
        Command::Repl { protocol } => {
            if protocol {
                if let Err(message) = command_repl_protocol() {
                    print_error(&message);
                    process::exit(1);
                }
            } else if let Err(message) = command_repl() {
                print_error(&message);
                process::exit(1);
            }
        }
        Command::Check { input } => {
            if let Err(message) = command_check(input) {
                print_error(&message);
                process::exit(1);
            }
        }
        Command::Doc { input, output } => {
            if let Err(message) = command_doc(input, output) {
                print_error(&message);
                process::exit(1);
            }
        }
        Command::Lsp => {
            let rt = tokio::runtime::Runtime::new()
                .expect("failed to create tokio runtime");
            rt.block_on(nexl_lsp::run_server());
        }
        Command::Sandbox {
            input,
            allow_console,
            allow_fs,
            allow_net,
            allow_time,
            allow_random,
            allow_concurrent,
            allow_unsafe,
            allow_all,
        } => {
            if let Err(message) = command_sandbox(
                input,
                allow_console,
                allow_fs,
                allow_net,
                allow_time,
                allow_random,
                allow_concurrent,
                allow_unsafe,
                allow_all,
            ) {
                print_error(&message);
                process::exit(1);
            }
        }
        Command::Audit { input } => {
            if let Err(message) = command_audit(input) {
                print_error(&message);
                process::exit(1);
            }
        }
        Command::Pkg { command } => {
            if let Err(message) = command_pkg(command) {
                print_error(&message);
                process::exit(1);
            }
        }
    }
}

fn print_error(message: &str) {
    if message.contains('\n') {
        eprintln!("{message}");
    } else {
        eprintln!("nexl: {message}");
    }
}

fn command_build(
    input_path: PathBuf,
    output_override: Option<PathBuf>,
    target: &str,
    gc: &str,
    no_opt: bool,
) -> Result<(), String> {
    // Validate gc mode.
    match gc {
        "rc" | "gc" | "none" => {}
        other => return Err(format!("unknown gc mode: {other} (expected \"rc\", \"gc\", or \"none\")")),
    }

    let default_ext = match target {
        "wasm" => "wasm",
        "native" => "o",
        other => return Err(format!("unknown target: {other} (expected \"wasm\" or \"native\")")),
    };
    let output_path = output_override.unwrap_or_else(|| input_path.with_extension(default_ext));

    let source = std::fs::read_to_string(&input_path)
        .map_err(|e| format!("cannot read {:?}: {e}", input_path))?;

    let module_name = input_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("module")
        .to_string();

    let nodes = nexl_reader::read(&source, meta::FileId::SYNTHETIC).map_err(|diag| {
        format_reader_report(*diag, &source, &input_path.display().to_string())
    })?;

    let ir_module = nexl_ir::Lowerer::new(&module_name)
        .lower_module(&nodes)
        .map_err(|e| format!("lowering error: {e}"))?;

    // Run optimization passes unless --no-opt.
    let ir_module = if no_opt {
        ir_module
    } else {
        nexl_ir::optimize::optimize(&ir_module)
    };

    if gc == "none" {
        // Arena mode: log that GC is disabled.
        eprintln!("nexl: arena mode (--gc none) — no GC, no reference counting");
    }

    let bytes = match target {
        "wasm" => nexl_wasm::Emitter::new()
            .emit(&ir_module)
            .map_err(|e| format!("codegen error: {e}"))?,
        "native" => {
            let mut compiler = nexl_native::compile::Compiler::new();
            compiler
                .compile_module(&ir_module)
                .map_err(|e| format!("native codegen error: {e}"))?;
            compiler.finish()
        }
        _ => unreachable!(),
    };

    std::fs::write(&output_path, &bytes)
        .map_err(|e| format!("cannot write {:?}: {e}", output_path))?;

    println!("nexl: wrote {} bytes to {:?}", bytes.len(), output_path);
    Ok(())
}

fn command_run(input_path: PathBuf) -> Result<(), String> {
    let source = std::fs::read_to_string(&input_path)
        .map_err(|e| format!("cannot read {:?}: {e}", input_path))?;

    let nodes = nexl_reader::read(&source, meta::FileId::SYNTHETIC).map_err(|diag| {
        format_reader_report(*diag, &source, &input_path.display().to_string())
    })?;

    let env = nexl_eval::stdlib::standard_env();
    for node in &nodes {
        nexl_eval::eval::eval(node, &env).map_err(|e| format!("eval error: {e}"))?;
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn command_sandbox(
    input_path: PathBuf,
    allow_console: bool,
    allow_fs: bool,
    allow_net: bool,
    allow_time: bool,
    allow_random: bool,
    allow_concurrent: bool,
    allow_unsafe: bool,
    allow_all: bool,
) -> Result<(), String> {
    use nexl_runtime::sandbox::{Capability, SandboxPolicy};
    use std::collections::HashSet;

    let policy = if allow_all {
        SandboxPolicy::unrestricted()
    } else {
        let mut caps = HashSet::new();
        if allow_console {
            caps.insert(Capability::Console);
        }
        if allow_fs {
            caps.insert(Capability::FileSystem);
        }
        if allow_net {
            caps.insert(Capability::Net);
        }
        if allow_time {
            caps.insert(Capability::Time);
        }
        if allow_random {
            caps.insert(Capability::Random);
        }
        if allow_concurrent {
            caps.insert(Capability::Concurrent);
        }
        if allow_unsafe {
            caps.insert(Capability::Unsafe);
        }
        SandboxPolicy::sandbox(caps)
    };

    nexl_runtime::sandbox::set_policy(policy);
    command_run(input_path)
}

fn repl_loop<R: BufRead, W: Write>(mut input: R, mut output: W) -> io::Result<()> {
    let env = nexl_eval::stdlib::standard_env();
    let mut type_env = nexl_infer::Env::new();
    let mut type_state = nexl_infer::InferState::new();
    let mut buffer = String::new();

    writeln!(
        output,
        "nexl {} | :help for commands",
        env!("CARGO_PKG_VERSION")
    )?;

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

        if buffer.is_empty() {
            let trimmed = line.trim_end_matches('\n');
            if let Some(command) = trimmed.strip_prefix(':') {
                match handle_repl_command(command, &type_env, &mut output)? {
                    ReplControl::Continue => continue,
                    ReplControl::Quit => break,
                }
            }
        }

        buffer.push_str(&line);
        if !delimiters_balanced(&buffer) {
            continue;
        }

        let source = buffer.trim_end_matches('\n');
        let nodes = match nexl_reader::read(source, meta::FileId::SYNTHETIC) {
            Ok(nodes) => nodes,
            Err(diag) => {
                let report = format_reader_report(*diag, source, "<repl>");
                writeln!(output, "{report}")?;
                buffer.clear();
                continue;
            }
        };

        let type_errors = update_repl_type_env(&nodes, &mut type_env, &mut type_state);
        for error in type_errors {
            writeln!(output, "type error: {error}")?;
        }

        for node in &nodes {
            match nexl_eval::eval::eval(node, &env) {
                Ok(value) => writeln!(output, "{value}")?,
                Err(message) => writeln!(output, "error: {message}")?,
            }
        }

        buffer.clear();
    }

    Ok(())
}

enum ReplControl {
    Continue,
    Quit,
}

fn handle_repl_command<W: Write>(
    command_line: &str,
    type_env: &nexl_infer::Env,
    output: &mut W,
) -> io::Result<ReplControl> {
    let command_line = command_line.trim();
    if command_line == "help" {
        write_repl_help(output)?;
        return Ok(ReplControl::Continue);
    }
    if command_line == "quit" {
        return Ok(ReplControl::Quit);
    }
    if let Some(rest) = command_line.strip_prefix("type") {
        let expr = rest.trim();
        if expr.is_empty() {
            writeln!(output, "error: :type requires an expression")?;
            return Ok(ReplControl::Continue);
        }
        match infer_repl_type(expr, type_env) {
            Ok(ty) => writeln!(output, "{ty}")?,
            Err(message) => writeln!(output, "error: {message}")?,
        }
        return Ok(ReplControl::Continue);
    }

    writeln!(output, "error: unknown command :{command_line}")?;
    Ok(ReplControl::Continue)
}

fn write_repl_help<W: Write>(output: &mut W) -> io::Result<()> {
    writeln!(output, "Commands:")?;
    writeln!(output, "  :help         Show this help")?;
    writeln!(output, "  :quit         Exit the REPL")?;
    writeln!(output, "  :type <expr>  Show inferred type")?;
    Ok(())
}

pub(crate) fn infer_repl_type(expr: &str, env: &nexl_infer::Env) -> Result<String, String> {
    let nodes =
        nexl_reader::read(expr, meta::FileId::SYNTHETIC).map_err(|e| format!("{e}"))?;
    if nodes.len() != 1 {
        return Err("expected a single form".to_string());
    }
    let mut state = nexl_infer::InferState::new();
    let ty =
        nexl_infer::synth(&nodes[0], env, &mut state).map_err(|e| format!("{e}"))?;
    if !state.errors.is_empty() {
        let message = state
            .errors
            .iter()
            .map(|error| error.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        return Err(message);
    }
    Ok(state.subst.apply(&ty).to_string())
}

pub(crate) fn update_repl_type_env(
    nodes: &[Node],
    env: &mut nexl_infer::Env,
    state: &mut nexl_infer::InferState,
) -> Vec<String> {
    let mut errors = Vec::new();
    for node in nodes {
        if list_head_is(node, "def") {
            match nexl_infer::infer_def(node, env, state) {
                Ok((_name, _ty, new_env)) => *env = new_env,
                Err(err) => errors.push(err.to_string()),
            }
            continue;
        }
        if list_head_is(node, "defn") {
            match nexl_infer::infer_defn(node, env, state) {
                Ok((_name, _ty, new_env)) => *env = new_env,
                Err(err) => errors.push(err.to_string()),
            }
            continue;
        }
        if list_head_is(node, "defpattern") {
            match nexl_infer::infer_defpattern(node, env) {
                Ok(new_env) => *env = new_env,
                Err(err) => errors.push(err.to_string()),
            }
            continue;
        }
        if list_head_is(node, "impl") {
            match nexl_infer::infer_impl(node, env, state) {
                Ok(new_env) => *env = new_env,
                Err(err) => errors.push(err.to_string()),
            }
        }
    }

    if !state.errors.is_empty() {
        errors.extend(state.errors.drain(..).map(|error| error.to_string()));
    }

    errors
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

fn format_reader_report(
    mut diag: nexl_errors::Diagnostic,
    source: &str,
    name: &str,
) -> String {
    diag.attach_source(miette::NamedSource::new(name, source.to_string()));
    let report = miette::Report::new(diag);
    format!("{report:?}")
}

fn command_repl() -> Result<(), String> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    repl_loop(stdin.lock(), stdout.lock()).map_err(|e| format!("repl error: {e}"))
}

fn command_repl_protocol() -> Result<(), String> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    repl_protocol::protocol_loop(stdin.lock(), stdout.lock())
        .map_err(|e| format!("protocol error: {e}"))
}

fn command_check(input_path: PathBuf) -> Result<(), String> {
    let source = std::fs::read_to_string(&input_path)
        .map_err(|e| format!("cannot read {:?}: {e}", input_path))?;
    let nodes = nexl_reader::read(&source, meta::FileId::SYNTHETIC).map_err(|diag| {
        format_reader_report(*diag, &source, &input_path.display().to_string())
    })?;

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

fn command_doc(input_path: PathBuf, output_override: Option<PathBuf>) -> Result<(), String> {
    let source = std::fs::read_to_string(&input_path)
        .map_err(|e| format!("cannot read {:?}: {e}", input_path))?;
    let doc = extract_module_doc(&source).map_err(|e| format!("doc error: {e}"))?;
    let pages = render_module_pages(&[doc]);
    let output_dir = output_override.unwrap_or_else(|| PathBuf::from("docs"));
    std::fs::create_dir_all(&output_dir)
        .map_err(|e| format!("cannot create {:?}: {e}", output_dir))?;
    for page in pages {
        let path = output_dir.join(page.filename);
        std::fs::write(&path, page.html)
            .map_err(|e| format!("cannot write {:?}: {e}", path))?;
    }
    Ok(())
}

fn command_audit(input_path: PathBuf) -> Result<(), String> {
    let source = std::fs::read_to_string(&input_path)
        .map_err(|e| format!("cannot read {:?}: {e}", input_path))?;

    let nodes = nexl_reader::read(&source, meta::FileId::SYNTHETIC).map_err(|diag| {
        format_reader_report(*diag, &source, &input_path.display().to_string())
    })?;

    let filename = input_path.display().to_string();
    let mut ffi_entries: Vec<AuditEntry> = Vec::new();
    let mut module_effects: Vec<String> = Vec::new();

    for node in &nodes {
        let NodeKind::List(items) = &node.kind else {
            continue;
        };
        let Some(head) = items.first() else {
            continue;
        };

        match symbol_name(head) {
            Some("defextern") => {
                if let Some(entry) = parse_defextern_entry(items, &source, node) {
                    ffi_entries.push(entry);
                }
            }
            Some("module") => {
                // Extract :performs from module declaration
                for (i, item) in items.iter().enumerate() {
                    if keyword_name_ref(item) == Some("performs")
                        && let Some(effects_node) = items.get(i + 1)
                        && let NodeKind::Vector(effects) = &effects_node.kind
                    {
                        for eff in effects {
                            if let Some(name) = symbol_name(eff) {
                                module_effects.push(name.to_string());
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    // Print report
    println!("== FFI Trust Boundaries ({filename}) ==");
    if ffi_entries.is_empty() {
        println!("  (none)");
    } else {
        for entry in &ffi_entries {
            let line = byte_offset_to_line(&source, entry.offset);
            let mut flags = Vec::new();
            if entry.is_unsafe {
                flags.push("UNSAFE".to_string());
            }
            if !entry.performs.is_empty() {
                flags.push(format!("performs [{}]", entry.performs.join(", ")));
            }
            if flags.is_empty() {
                flags.push("pure (declared)".to_string());
            }
            println!("  {}:{} defextern {} — {}", filename, line, entry.name, flags.join(", "));
            if entry.is_unsafe {
                println!("    ⚠ requires Unsafe capability");
            }
        }
    }
    println!();

    // Module effects summary
    println!("== Module Effects ==");
    if module_effects.is_empty() && ffi_entries.is_empty() {
        println!("  (none declared)");
    } else {
        let mut all_effects: Vec<String> = module_effects;
        for entry in &ffi_entries {
            for eff in &entry.performs {
                if !all_effects.contains(eff) {
                    all_effects.push(eff.clone());
                }
            }
            if entry.is_unsafe && !all_effects.iter().any(|e| e == "Unsafe") {
                all_effects.push("Unsafe".to_string());
            }
        }
        all_effects.sort();
        all_effects.dedup();
        if all_effects.is_empty() {
            println!("  (none declared)");
        } else {
            println!("  {}: {}", filename, all_effects.join(", "));
        }
    }

    Ok(())
}

struct AuditEntry {
    name: String,
    performs: Vec<String>,
    is_unsafe: bool,
    offset: usize,
}

fn parse_defextern_entry(items: &[Node], _source: &str, node: &Node) -> Option<AuditEntry> {
    // (defextern name : type "c-name" [:performs [...]] [:unsafe])
    let name = items.get(1).and_then(symbol_name)?;
    let mut performs = Vec::new();
    let mut is_unsafe = false;

    for (i, item) in items.iter().enumerate() {
        match keyword_name_ref(item) {
            Some("performs") => {
                if let Some(effects_node) = items.get(i + 1)
                    && let NodeKind::Vector(effects) = &effects_node.kind
                {
                    for eff in effects {
                        if let Some(eff_name) = symbol_name(eff) {
                            performs.push(eff_name.to_string());
                        }
                    }
                }
            }
            Some("unsafe") => {
                is_unsafe = true;
            }
            _ => {}
        }
    }

    Some(AuditEntry {
        name: name.to_string(),
        performs,
        is_unsafe,
        offset: node.span.start as usize,
    })
}

fn symbol_name(node: &Node) -> Option<&str> {
    match &node.kind {
        NodeKind::Atom(Atom::Symbol { ns: None, name }) => Some(name.as_str()),
        _ => None,
    }
}

fn keyword_name_ref(node: &Node) -> Option<&str> {
    match &node.kind {
        NodeKind::Atom(Atom::Keyword { ns: None, name }) => Some(name.as_str()),
        _ => None,
    }
}

fn byte_offset_to_line(source: &str, offset: usize) -> usize {
    source[..offset.min(source.len())]
        .chars()
        .filter(|c| *c == '\n')
        .count()
        + 1
}

fn command_pkg(command: PkgCommand) -> Result<(), String> {
    match command {
        PkgCommand::Add { dep, dev, version } => command_pkg_add(dep, version, dev),
        PkgCommand::Remove { dep } => command_pkg_remove(dep),
        PkgCommand::Lock => command_pkg_lock(),
    }
}

fn command_pkg_add(dep: String, version: Option<String>, dev: bool) -> Result<(), String> {
    let (name, version) = parse_dep_spec(&dep, version)?;
    let manifest_path = manifest_path();
    let mut manifest = read_manifest(&manifest_path)?;

    if let Some(existing) = manifest.dependencies.get(&name) {
        if !spec_matches(existing, &version) {
            return Err(format!(
                "dependency `{name}` already exists with {}",
                spec_label(existing)
            ));
        }
        return Ok(());
    }

    if let Some(existing) = manifest.dev_dependencies.get(&name) {
        if !spec_matches(existing, &version) {
            return Err(format!(
                "dependency `{name}` already exists with {}",
                spec_label(existing)
            ));
        }
        if dev {
            return Ok(());
        }
    }

    let target = if dev {
        &mut manifest.dev_dependencies
    } else {
        &mut manifest.dependencies
    };
    target.insert(name, DependencySpec::Version(version));
    write_manifest(&manifest_path, &manifest)
}

fn command_pkg_remove(dep: String) -> Result<(), String> {
    let name = dep.split_once('@').map(|(name, _)| name).unwrap_or(&dep);
    if name.is_empty() {
        return Err("dependency name cannot be empty".to_string());
    }
    let manifest_path = manifest_path();
    let mut manifest = read_manifest(&manifest_path)?;
    let removed = manifest.dependencies.remove(name).is_some()
        || manifest.dev_dependencies.remove(name).is_some();
    if !removed {
        return Err(format!("dependency `{name}` not found"));
    }
    write_manifest(&manifest_path, &manifest)
}

fn command_pkg_lock() -> Result<(), String> {
    let manifest_path = manifest_path();
    let manifest = read_manifest(&manifest_path)?;
    let lockfile = build_lockfile(&manifest).map_err(|e| format!("resolve error: {e}"))?;
    let lock_path = lockfile_path();
    write_lockfile(&lock_path, &lockfile)
}

fn manifest_path() -> PathBuf {
    PathBuf::from("project.nexl")
}

fn lockfile_path() -> PathBuf {
    PathBuf::from("nexl.lock")
}

fn parse_dep_spec(dep: &str, version: Option<String>) -> Result<(String, String), String> {
    match dep.split_once('@') {
        Some((name, ver)) => {
            if name.is_empty() || ver.is_empty() {
                return Err("dependency spec must be NAME@VERSION".to_string());
            }
            Ok((name.to_string(), ver.to_string()))
        }
        None => match version {
            Some(ver) if !dep.is_empty() => Ok((dep.to_string(), ver)),
            _ => Err("dependency version required (use NAME@VERSION or --version)".to_string()),
        },
    }
}

fn spec_matches(spec: &DependencySpec, version: &str) -> bool {
    let (existing_version, existing_registry) = normalize_spec(spec);
    existing_registry.is_none() && existing_version == version
}

fn spec_label(spec: &DependencySpec) -> String {
    let (version, registry) = normalize_spec(spec);
    match registry {
        Some(registry) => format!("{version} (registry {registry})"),
        None => version,
    }
}

fn normalize_spec(spec: &DependencySpec) -> (String, Option<String>) {
    match spec {
        DependencySpec::Version(version) => (version.clone(), None),
        DependencySpec::Detailed(detail) => (detail.version.clone(), detail.registry.clone()),
    }
}

fn read_manifest(path: &PathBuf) -> Result<PackageManifest, String> {
    let source = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    parse_manifest(&source).map_err(|e| format!("invalid manifest {}: {e}", path.display()))
}

fn write_manifest(path: &PathBuf, manifest: &PackageManifest) -> Result<(), String> {
    let output = serialize_manifest(manifest);
    std::fs::write(path, output)
        .map_err(|e| format!("cannot write {}: {e}", path.display()))
}

fn write_lockfile(path: &PathBuf, lockfile: &nexl_pkg::Lockfile) -> Result<(), String> {
    let output = serialize_lockfile(lockfile);
    std::fs::write(path, output)
        .map_err(|e| format!("cannot write {}: {e}", path.display()))
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

    fn write_temp_dir(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be available")
            .as_nanos();
        let mut path = std::env::temp_dir();
        path.push(format!("nexl_cli_{label}_{nanos}"));
        std::fs::create_dir_all(&path).expect("create temp dir");
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
                target: "wasm".to_string(),
                gc: "rc".to_string(),
                no_opt: false,
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
                target: "wasm".to_string(),
                gc: "rc".to_string(),
                no_opt: false,
            }
        );
    }

    #[test]
    fn parse_build_with_native_target() {
        let cli =
            Cli::try_parse_from(["nexl", "build", "main.nexl", "-t", "native"]).expect("parse");
        assert_eq!(
            cli.command,
            Command::Build {
                input: PathBuf::from("main.nexl"),
                output: None,
                target: "native".to_string(),
                gc: "rc".to_string(),
                no_opt: false,
            }
        );
    }

    #[test]
    fn build_native_produces_object_file() {
        let path = write_temp_file("(defn f [x] x)", "build_native");
        let out = path.with_extension("o");
        let result = command_build(path.clone(), Some(out.clone()), "native", "rc", false);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&out);
        assert!(result.is_ok(), "native build should succeed, got: {result:?}");
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
        assert_eq!(
            cli.command,
            Command::Repl {
                protocol: false,
            }
        );
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
    fn repl_help_command_prints_usage() {
        let input = Cursor::new(b":help\n");
        let mut output = Vec::new();
        repl_loop(input, &mut output).expect("repl loop");
        let output = String::from_utf8(output).expect("utf8");
        assert!(output.contains(":help"));
        assert!(output.contains(":quit"));
        assert!(output.contains(":type"));
    }

    #[test]
    fn repl_quit_exits_immediately() {
        let input = Cursor::new(b":quit\n(+ 1 2)\n");
        let mut output = Vec::new();
        repl_loop(input, &mut output).expect("repl loop");
        let output = String::from_utf8(output).expect("utf8");
        assert!(!output.contains('3'));
    }

    #[test]
    fn repl_type_command_prints_type() {
        let input = Cursor::new(b":type 1\n");
        let mut output = Vec::new();
        repl_loop(input, &mut output).expect("repl loop");
        let output = String::from_utf8(output).expect("utf8");
        assert!(output.contains("Int"));
    }

    #[test]
    fn repl_banner_printed_on_start() {
        let input = Cursor::new(b":quit\n");
        let mut output = Vec::new();
        repl_loop(input, &mut output).expect("repl loop");
        let output = String::from_utf8(output).expect("utf8");
        let banner = format!("nexl {} | :help for commands", env!("CARGO_PKG_VERSION"));
        assert!(output.contains(&banner));
    }

    #[test]
    fn reader_error_report_includes_source() {
        let source = "(";
        let diag = nexl_reader::read(source, meta::FileId::SYNTHETIC)
            .expect_err("expected parse error");
        let report = format_reader_report(*diag, source, "test.nxl");
        assert!(report.contains("unclosed `(`"));
        assert!(report.contains("test.nxl"));
        assert!(report.contains('('));
    }

    #[test]
    fn check_type_checks_file() {
        let path = write_temp_file("(def x 1) x", "check_ok");
        let result = command_check(path.clone());
        let _ = std::fs::remove_file(&path);
        assert!(result.is_ok(), "check should succeed, got: {result:?}");
    }

    #[test]
    fn parse_lsp_without_args() {
        let cli = Cli::try_parse_from(["nexl", "lsp"]).expect("parse");
        assert_eq!(cli.command, Command::Lsp);
    }

    #[test]
    fn parse_doc_with_input() {
        let cli = Cli::try_parse_from(["nexl", "doc", "mod.nexl"]).expect("parse");
        assert_eq!(
            cli.command,
            Command::Doc {
                input: PathBuf::from("mod.nexl"),
                output: None,
            }
        );
    }

    #[test]
    fn parse_doc_with_output() {
        let cli = Cli::try_parse_from(["nexl", "doc", "mod.nexl", "out"])
            .expect("parse");
        assert_eq!(
            cli.command,
            Command::Doc {
                input: PathBuf::from("mod.nexl"),
                output: Some(PathBuf::from("out")),
            }
        );
    }

    #[test]
    fn parse_pkg_add_with_version_in_spec() {
        let cli = Cli::try_parse_from(["nexl", "pkg", "add", "json@^1.0.0"])
            .expect("parse");
        assert_eq!(
            cli.command,
            Command::Pkg {
                command: PkgCommand::Add {
                    dep: "json@^1.0.0".to_string(),
                    dev: false,
                    version: None,
                }
            }
        );
    }

    #[test]
    fn parse_pkg_add_with_dev_flag() {
        let cli = Cli::try_parse_from([
            "nexl",
            "pkg",
            "add",
            "test-utils",
            "--dev",
            "--version",
            "^0.1.0",
        ])
        .expect("parse");
        assert_eq!(
            cli.command,
            Command::Pkg {
                command: PkgCommand::Add {
                    dep: "test-utils".to_string(),
                    dev: true,
                    version: Some("^0.1.0".to_string()),
                }
            }
        );
    }

    #[test]
    fn parse_pkg_remove() {
        let cli = Cli::try_parse_from(["nexl", "pkg", "remove", "json"]).expect("parse");
        assert_eq!(
            cli.command,
            Command::Pkg {
                command: PkgCommand::Remove {
                    dep: "json".to_string(),
                }
            }
        );
    }

    #[test]
    fn parse_pkg_lock() {
        let cli = Cli::try_parse_from(["nexl", "pkg", "lock"]).expect("parse");
        assert_eq!(
            cli.command,
            Command::Pkg {
                command: PkgCommand::Lock
            }
        );
    }

    #[test]
    fn doc_command_writes_html() {
        let source = "(module demo)\n(defn ident [x] x)";
        let path = write_temp_file(source, "doc");
        let out_dir = write_temp_dir("doc_out");
        let result = command_doc(path.clone(), Some(out_dir.clone()));
        let _ = std::fs::remove_file(&path);
        let html_path = out_dir.join("demo.html");
        let html_exists = html_path.exists();
        let _ = std::fs::remove_file(&html_path);
        let _ = std::fs::remove_dir_all(&out_dir);
        assert!(result.is_ok(), "doc command should succeed, got: {result:?}");
        assert!(html_exists, "doc command should write html output");
    }

    #[test]
    fn parse_sandbox_no_flags() {
        let cli = Cli::try_parse_from(["nexl", "sandbox", "app.nexl"]).expect("parse");
        match cli.command {
            Command::Sandbox {
                input,
                allow_console,
                allow_fs,
                allow_all,
                ..
            } => {
                assert_eq!(input, PathBuf::from("app.nexl"));
                assert!(!allow_console);
                assert!(!allow_fs);
                assert!(!allow_all);
            }
            other => panic!("expected Sandbox, got {other:?}"),
        }
    }

    #[test]
    fn parse_sandbox_with_flags() {
        let cli = Cli::try_parse_from([
            "nexl",
            "sandbox",
            "app.nexl",
            "--allow-console",
            "--allow-fs",
            "--allow-time",
        ])
        .expect("parse");
        match cli.command {
            Command::Sandbox {
                allow_console,
                allow_fs,
                allow_time,
                allow_net,
                ..
            } => {
                assert!(allow_console);
                assert!(allow_fs);
                assert!(allow_time);
                assert!(!allow_net);
            }
            other => panic!("expected Sandbox, got {other:?}"),
        }
    }

    #[test]
    fn sandbox_denies_console_by_default() {
        let source = r#"(io/println "hello")"#;
        let path = write_temp_file(source, "sandbox_deny");
        let result = command_sandbox(
            path.clone(),
            false, false, false, false, false, false, false, false,
        );
        let _ = std::fs::remove_file(&path);
        assert!(result.is_err(), "sandbox should deny console");
        let err = result.unwrap_err();
        assert!(err.contains("Console"), "error should mention Console: {err}");
    }

    #[test]
    fn sandbox_allows_granted_capability() {
        let source = r#"(io/println "hello")"#;
        let path = write_temp_file(source, "sandbox_allow");
        let result = command_sandbox(
            path.clone(),
            true, false, false, false, false, false, false, false,
        );
        let _ = std::fs::remove_file(&path);
        assert!(result.is_ok(), "sandbox should allow console: {result:?}");
    }

    #[test]
    fn sandbox_allow_all_permits_everything() {
        let source = r#"(io/println "hello")"#;
        let path = write_temp_file(source, "sandbox_all");
        let result = command_sandbox(
            path.clone(),
            false, false, false, false, false, false, false, true,
        );
        let _ = std::fs::remove_file(&path);
        assert!(result.is_ok(), "sandbox --allow-all should permit: {result:?}");
    }

    #[test]
    fn sandbox_pure_code_runs_without_flags() {
        let source = "(+ 1 2)";
        let path = write_temp_file(source, "sandbox_pure");
        let result = command_sandbox(
            path.clone(),
            false, false, false, false, false, false, false, false,
        );
        let _ = std::fs::remove_file(&path);
        assert!(result.is_ok(), "pure code should run in sandbox: {result:?}");
    }

    #[test]
    fn parse_audit_with_input() {
        let cli = Cli::try_parse_from(["nexl", "audit", "main.nexl"]).expect("parse");
        match cli.command {
            Command::Audit { input } => assert_eq!(input, PathBuf::from("main.nexl")),
            other => panic!("expected Audit, got {other:?}"),
        }
    }

    #[test]
    fn audit_reports_defextern() {
        let source = r#"
(defextern puts : (Fn [Str] -> Int) "puts" :performs [Console])
(defextern malloc : (Fn [Int] -> Int) "malloc" :unsafe)
(defextern sin : (Fn [Float] -> Float) "sin")
"#;
        let path = write_temp_file(source, "audit_ffi");
        let result = command_audit(path.clone());
        let _ = std::fs::remove_file(&path);
        assert!(result.is_ok(), "audit should succeed: {result:?}");
    }

    #[test]
    fn audit_no_ffi_succeeds() {
        let source = "(def x 1)";
        let path = write_temp_file(source, "audit_clean");
        let result = command_audit(path.clone());
        let _ = std::fs::remove_file(&path);
        assert!(result.is_ok(), "audit should succeed with no FFI: {result:?}");
    }

    #[test]
    fn audit_reports_module_effects() {
        let source = r#"
(module demo :exports [f] :performs [Console FileSystem])
(defn f [x] x)
"#;
        let path = write_temp_file(source, "audit_effects");
        let result = command_audit(path.clone());
        let _ = std::fs::remove_file(&path);
        assert!(result.is_ok(), "audit should succeed: {result:?}");
    }

    // --- Kernel subset bootstrap tests ---

    #[test]
    fn kernel_bootstrap_parses_successfully() {
        // Verify Stage 0 can parse the kernel-subset bootstrap POC.
        let source = include_str!("../../../docs/kernel-bootstrap.nxl");
        let nodes = nexl_reader::read(source, meta::FileId::SYNTHETIC);
        assert!(
            nodes.is_ok(),
            "kernel-bootstrap.nxl must parse: {:?}",
            nodes.err()
        );
        let nodes = nodes.unwrap();
        assert!(
            !nodes.is_empty(),
            "kernel-bootstrap.nxl should contain definitions"
        );
    }

    #[test]
    fn kernel_bootstrap_evaluates() {
        // Verify Stage 0 evaluator can run the kernel-subset bootstrap POC.
        let source = include_str!("../../../docs/kernel-bootstrap.nxl");
        let nodes =
            nexl_reader::read(source, meta::FileId::SYNTHETIC).expect("parse");
        let env = nexl_eval::stdlib::standard_env();
        let mut last_result = None;
        for node in &nodes {
            match nexl_eval::eval::eval(node, &env) {
                Ok(value) => last_result = Some(value),
                Err(err) => panic!("kernel-bootstrap eval error: {err}"),
            }
        }
        assert!(
            last_result.is_some(),
            "kernel-bootstrap should produce a result"
        );
    }
}
