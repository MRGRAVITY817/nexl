//! `nexl` — compile a Nexl source file to a WebAssembly binary.
//!
//! Usage: `nexl build <input.nexl> [output.wasm]`
//!
//! If no output path is given, the output file is derived from the input
//! by replacing the extension with `.wasm`.

mod functions;
mod repl_protocol;
mod wasm_runner;

use clap::{Parser, Subcommand};
use meta::{Atom, Node, NodeKind};
use nexl_doc::{extract_module_doc, render_module_pages};
use nexl_pkg::{
    DependencySpec, PackageManifest, build_lockfile, parse_manifest, serialize_lockfile,
    serialize_manifest,
};
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
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
    /// Run a Nexl source file. If no FILE is given, runs the project entry point
    /// (src/main.nx by default, or {source-dir}/main.nx from project.nx).
    Run {
        /// File to run. If omitted, discovers main.nx from project.nx source-dir.
        #[arg(value_name = "FILE")]
        input: Option<PathBuf>,
        /// Compile to WASM and execute via wasmtime instead of the tree-walk evaluator.
        #[arg(long = "wasm")]
        wasm: bool,
        /// Enable experimental WASI Preview 3 async features (design only; no runtime effect).
        #[arg(long = "experimental-wasi3")]
        experimental_wasi3: bool,
        /// Arguments to pass to the program via sys/args
        #[arg(trailing_var_arg = true)]
        args: Vec<String>,
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
    /// Format a Nexl source file.
    Fmt {
        /// File to format (use `-` for stdin).
        #[arg(value_name = "FILE")]
        input: String,
        /// Format file in-place.
        #[arg(short = 'i', long = "in-place")]
        in_place: bool,
        /// Maximum line width (default: 80).
        #[arg(long = "width", default_value = "80")]
        width: usize,
        /// Indent width in spaces (default: 2).
        #[arg(long = "indent", default_value = "2")]
        indent: usize,
        /// Disable vertical column alignment.
        #[arg(long = "no-align")]
        no_align: bool,
    },
    Pkg {
        #[command(subcommand)]
        command: PkgCommand,
    },
    /// Run tests in a Nexl source file, or discover all *_test.nx files.
    Test {
        /// File to test. If omitted, discovers all *_test.nx files under tests/.
        #[arg(value_name = "FILE")]
        input: Option<PathBuf>,
        /// Only run tests whose names contain this substring.
        #[arg(long = "filter")]
        filter: Option<String>,
        /// Only run tests that have all of these tags (comma-separated, e.g. "db,fast").
        #[arg(long = "tags")]
        tags: Option<String>,
        /// Output format: "text" (default) or "json" (JSON Lines).
        #[arg(long = "format", default_value = "text")]
        format: String,
        /// Accept all new/changed snapshots (overwrites .snap files).
        #[arg(long = "accept")]
        accept: bool,
        /// Do not stop after the first file with failures (run all files).
        #[arg(long = "no-fail-fast")]
        no_fail_fast: bool,
    },
    /// Run benchmarks in a Nexl file.
    Bench {
        /// File containing bench forms.
        #[arg(value_name = "FILE")]
        input: PathBuf,
    },
    /// Check for updates and print upgrade instructions.
    Upgrade,
    /// Create a new Nexl project.
    New {
        /// Project name (also used as directory name).
        #[arg(value_name = "NAME")]
        name: String,
        /// Project template: "default" or "web".
        #[arg(long = "template", default_value = "default")]
        template: String,
    },
    /// Manage and serve effect-sandboxed Nexl functions.
    Functions {
        #[command(subcommand)]
        command: FunctionsCommand,
    },
}

#[derive(Debug, Subcommand, PartialEq, Eq)]
enum FunctionsCommand {
    /// Deploy a .nx handler file as a function.
    Deploy {
        /// Path to the .nx file defining `(defn handle [req] ...)`.
        #[arg(value_name = "FILE")]
        file: PathBuf,
        /// Function name (defaults to file stem).
        #[arg(long = "name")]
        name: Option<String>,
        /// Capability level: pure, read-only, or full (default: full).
        #[arg(long = "capability", default_value = "full")]
        capability: String,
        /// URL route pattern (default: /<name>).
        #[arg(long = "route")]
        route: Option<String>,
    },
    /// List all deployed functions.
    List,
    /// Serve deployed functions over HTTP.
    Serve {
        /// Port to listen on (default: 8080).
        #[arg(long = "port", default_value = "8080")]
        port: u16,
    },
    /// Show recent invocation logs for a function.
    Logs {
        /// Function name.
        #[arg(value_name = "NAME")]
        name: String,
        /// Number of recent entries to show (default: 20).
        #[arg(short = 'n', default_value = "20")]
        n: usize,
    },
    /// Invoke a function directly (without HTTP).
    Invoke {
        /// Function name.
        #[arg(value_name = "NAME")]
        name: String,
        /// HTTP method (default: GET).
        #[arg(long = "method", default_value = "GET")]
        method: String,
        /// URL path (defaults to the function's registered route).
        #[arg(long = "path")]
        path: Option<String>,
        /// Request body as a string.
        #[arg(long = "body")]
        body: Option<String>,
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
        Command::Build {
            input,
            output,
            target,
            gc,
            no_opt,
        } => {
            if let Err(message) = command_build(input, output, &target, &gc, no_opt) {
                print_error(&message);
                process::exit(1);
            }
        }
        Command::Run {
            input,
            args,
            wasm,
            experimental_wasi3,
        } => {
            let input = match input {
                Some(path) => path,
                None => match discover_run_file() {
                    Some(path) => path,
                    None => {
                        eprintln!("nexl: no FILE given and no main.nx found.\n\nLooking for: src/main.nx (or {{source-dir}}/main.nx from project.nx).\n\nUsage: nexl run [FILE]");
                        process::exit(1);
                    }
                },
            };
            if experimental_wasi3 {
                eprintln!(
                    "{}",
                    nexl_wasm::wasi3::Wasi3Config::experimental_notice()
                );
            }
            nexl_runtime::sys::set_program_args(args.clone());
            let result = if wasm {
                command_run_wasm(input, args)
            } else {
                command_run(input)
            };
            if let Err(message) = result {
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
            let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
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
        Command::Fmt {
            input,
            in_place,
            width,
            indent,
            no_align,
        } => {
            if let Err(message) = command_fmt(&input, in_place, width, indent, no_align) {
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
        Command::Test { input, filter, tags, format, accept, no_fail_fast } => {
            let tag_list: Vec<String> = tags
                .as_deref()
                .map(|s| s.split(',').map(|t| t.trim().to_string()).collect())
                .unwrap_or_default();
            nexl_stdlib::test::set_accept_mode(accept);
            let files = match input {
                Some(path) => vec![path],
                None => discover_test_files(),
            };
            if files.is_empty() {
                eprintln!("No test files found. Create *_test.nx files under tests/.");
                process::exit(1);
            }
            let mut all_passed = true;
            for file in files {
                let passed = match command_test(file, filter.as_deref(), &tag_list, &format) {
                    Ok(true) => true,
                    Ok(false) => false,
                    Err(message) => {
                        print_error(&message);
                        false
                    }
                };
                if !passed {
                    all_passed = false;
                    if !no_fail_fast {
                        break;
                    }
                }
            }
            if !all_passed {
                process::exit(1);
            }
        }
        Command::Bench { input } => {
            match command_bench(input) {
                Ok(()) => {}
                Err(message) => {
                    print_error(&message);
                    process::exit(1);
                }
            }
        }
        Command::Upgrade => {
            command_upgrade();
        }
        Command::New { name, template } => {
            if let Err(message) = command_new(&name, &template) {
                print_error(&message);
                process::exit(1);
            }
        }
        Command::Functions { command } => {
            if let Err(message) = command_functions(command) {
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
        other => {
            return Err(format!(
                "unknown gc mode: {other} (expected \"rc\", \"gc\", or \"none\")"
            ));
        }
    }

    let default_ext = match target {
        "wasm" => "wasm",
        "native" => "o",
        other => {
            return Err(format!(
                "unknown target: {other} (expected \"wasm\" or \"native\")"
            ));
        }
    };
    let output_path = output_override.unwrap_or_else(|| input_path.with_extension(default_ext));

    let source = std::fs::read_to_string(&input_path)
        .map_err(|e| format!("cannot read {:?}: {e}", input_path))?;

    let module_name = input_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("module")
        .to_string();

    let nodes = nexl_reader::read(&source, meta::FileId::SYNTHETIC)
        .map_err(|diag| format_reader_report(*diag, &source, &input_path.display().to_string()))?;

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

/// Walk up from `start` looking for a `project.nx` file.
/// Returns the directory containing it, or `None`.
fn find_project_root(start: &Path) -> Option<PathBuf> {
    let mut dir = if start.is_file() {
        start.parent()?.to_path_buf()
    } else {
        start.to_path_buf()
    };
    loop {
        if dir.join("project.nx").is_file() {
            return Some(dir);
        }
        if !dir.pop() {
            return None;
        }
    }
}

/// Check if the first form in parsed nodes is a `(module ...)` declaration.
fn has_module_decl(nodes: &[Node]) -> bool {
    matches!(
        nodes.first(),
        Some(Node {
            kind: NodeKind::List(items),
            ..
        }) if matches!(
            items.first(),
            Some(Node {
                kind: NodeKind::Atom(Atom::Symbol { ns: None, name }),
                ..
            }) if name == "module"
        )
    )
}

/// Macro-expand a sequence of top-level nodes.
///
/// Creates a fresh [`nexl_macros::Expander`] pre-loaded with the stdlib macro
/// definitions embedded from `nexl-stdlib/nexl/test.nx`. Any `defmacro-syntax`
/// forms in that file are registered in the expander; non-macro forms (`defn`
/// etc.) are discarded since they are already live via `standard_env()`.
///
/// Call this before `eval` so that `deftest`, `is`, `throws?`, etc. expand.
/// Expand macros in `nodes` using the test.nx stdlib prelude.
/// Returns `(expanded_user_nodes, expanded_prelude_defns)`.
/// The caller should evaluate `expanded_prelude_defns` before `expanded_user_nodes`
/// so that Nexl-defined helpers (`test/check-run!`, `gen/seed-seq`, etc.) are in scope.
fn macro_expand(nodes: &[Node]) -> Result<(Vec<Node>, Vec<Node>), String> {
    const STDLIB_TEST: &str = include_str!("../../nexl-stdlib/nexl/test.nx");
    let mut expander = nexl_macros::Expander::new();
    let prelude = nexl_reader::read(STDLIB_TEST, meta::FileId::SYNTHETIC)
        .map_err(|e| format!("stdlib macro parse error: {e}"))?;
    let prelude_forms = expander
        .expand_forms(&prelude)
        .map_err(|e| format!("stdlib macro expand error: {e}"))?;
    let expanded = expander
        .expand_forms(nodes)
        .map_err(|e| format!("macro error: {e}"))?;
    Ok((expanded, prelude_forms))
}

/// Discover and load all modules transitively starting from the entry file.
///
/// Returns the loaded `ModuleSource` values ready for `eval_modules`.
fn discover_and_load_modules(
    entry_path: &Path,
) -> Result<Vec<nexl_eval::modules::ModuleSource>, String> {
    use std::collections::{HashMap, HashSet, VecDeque};

    let entry_path = entry_path
        .canonicalize()
        .map_err(|e| format!("cannot resolve {:?}: {e}", entry_path))?;

    // Find project root and read manifest for prefix
    let project_root = find_project_root(&entry_path)
        .ok_or("no project.nx found; multi-file modules require a project manifest")?;

    let manifest_source = std::fs::read_to_string(project_root.join("project.nx"))
        .map_err(|e| format!("cannot read project.nx: {e}"))?;
    let manifest =
        parse_manifest(&manifest_source).map_err(|e| format!("invalid project.nx: {e}"))?;
    let prefix = &manifest.package.prefix;
    let source_root = project_root.join(&manifest.package.source_dir);

    // Build dependency prefix→source_root map for cross-project resolution.
    // Each path dependency's project.nx is loaded to discover its prefix and source_dir.
    let mut dep_roots: HashMap<String, std::path::PathBuf> = HashMap::new();
    let all_deps = manifest
        .dependencies
        .iter()
        .chain(manifest.dev_dependencies.iter());
    for (dep_name, spec) in all_deps {
        if let nexl_pkg::DependencySpec::Detailed(detail) = spec {
            if let Some(dep_path) = &detail.path {
                let dep_project_root = project_root.join(dep_path);
                let dep_manifest_path = dep_project_root.join("project.nx");
                let dep_manifest_source =
                    std::fs::read_to_string(&dep_manifest_path).map_err(|e| {
                        format!(
                            "cannot read dependency `{dep_name}` manifest at {:?}: {e}",
                            dep_manifest_path
                        )
                    })?;
                let dep_manifest = parse_manifest(&dep_manifest_source).map_err(|e| {
                    format!("invalid project.nx for dependency `{dep_name}`: {e}")
                })?;
                let dep_source_root =
                    dep_project_root.join(&dep_manifest.package.source_dir);
                dep_roots.insert(dep_manifest.package.prefix.clone(), dep_source_root);
            }
        }
    }

    // Parse entry file
    let entry_source = std::fs::read_to_string(&entry_path)
        .map_err(|e| format!("cannot read {:?}: {e}", entry_path))?;
    let entry_nodes =
        nexl_reader::read(&entry_source, meta::FileId::SYNTHETIC).map_err(|diag| {
            format_reader_report(*diag, &entry_source, &entry_path.display().to_string())
        })?;
    let entry_module = nexl_eval::modules::parse_module_source(&entry_nodes)
        .map_err(|e| format!("module parse error: {e}"))?;

    // BFS to discover all imported modules
    let mut loaded: Vec<nexl_eval::modules::ModuleSource> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut queue: VecDeque<nexl_eval::modules::ModuleSource> = VecDeque::new();

    seen.insert(entry_module.decl.name.clone());
    queue.push_back(entry_module);

    while let Some(module) = queue.pop_front() {
        for import in &module.imports {
            if seen.contains(&import.module_path) {
                continue;
            }
            seen.insert(import.module_path.clone());

            // Try resolving with the current project's prefix first.
            // If prefix doesn't match, check path dependencies.
            let abs_path = match nexl_modules::module_name_to_path(
                &import.module_path,
                prefix,
            ) {
                Ok(rel_path) => source_root.join(&rel_path),
                Err(nexl_modules::ModulePathError::PrefixMismatch { .. }) => {
                    // Try each dependency's prefix
                    resolve_dep_module(&import.module_path, &dep_roots)?
                }
                Err(e) => {
                    return Err(format!(
                        "cannot resolve module `{}`: {e}",
                        import.module_path
                    ));
                }
            };

            let source = std::fs::read_to_string(&abs_path).map_err(|e| {
                format!(
                    "cannot read module `{}` at {:?}: {e}",
                    import.module_path, abs_path
                )
            })?;
            let nodes = nexl_reader::read(&source, meta::FileId::SYNTHETIC).map_err(|diag| {
                format_reader_report(*diag, &source, &abs_path.display().to_string())
            })?;
            let dep_module = nexl_eval::modules::parse_module_source(&nodes)
                .map_err(|e| format!("module parse error in `{}`: {e}", import.module_path))?;

            queue.push_back(dep_module);
        }
        loaded.push(module);
    }

    Ok(loaded)
}

/// Resolve a module path against path dependencies.
///
/// Given `frond.render` and dep_roots `{"frond" => "/path/to/frond/src"}`,
/// this resolves to `/path/to/frond/src/frond/render.nx`.
fn resolve_dep_module(
    module_path: &str,
    dep_roots: &std::collections::HashMap<String, std::path::PathBuf>,
) -> Result<std::path::PathBuf, String> {
    // Extract the first component as the potential dependency prefix
    let first_dot = module_path.find('.');
    let candidates: Vec<&str> = if let Some(pos) = first_dot {
        // Try progressively longer prefixes: "frond", then "frond.sub", etc.
        // Most deps use a single-segment prefix, so try that first.
        vec![&module_path[..pos]]
    } else {
        vec![module_path]
    };

    for candidate in &candidates {
        if let Some(dep_source_root) = dep_roots.get(*candidate) {
            let rel_path = nexl_modules::module_name_to_path(module_path, candidate)
                .map_err(|e| format!("cannot resolve dep module `{module_path}`: {e}"))?;
            return Ok(dep_source_root.join(&rel_path));
        }
    }

    let available: Vec<&String> = dep_roots.keys().collect();
    Err(format!(
        "module `{module_path}` does not match project prefix or any dependency. \
         Available prefixes: {:?}",
        available
    ))
}

fn command_run_wasm(input_path: PathBuf, args: Vec<String>) -> Result<(), String> {
    let source = std::fs::read_to_string(&input_path)
        .map_err(|e| format!("cannot read {:?}: {e}", input_path))?;

    let module_name = input_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("module")
        .to_string();

    let nodes = nexl_reader::read(&source, meta::FileId::SYNTHETIC)
        .map_err(|diag| format_reader_report(*diag, &source, &input_path.display().to_string()))?;

    let ir_module = nexl_ir::Lowerer::new(&module_name)
        .lower_module(&nodes)
        .map_err(|e| format!("lowering error: {e}"))?;

    let ir_module = nexl_ir::optimize::optimize(&ir_module);

    let bytes = nexl_wasm::Emitter::new()
        .emit(&ir_module)
        .map_err(|e| format!("codegen error: {e}"))?;

    let args_ref: Vec<&str> = args.iter().map(String::as_str).collect();
    // Preopen the current working directory as "." so the WASM module can access
    // the filesystem via WASI (capability-model sandboxing).
    let cwd = std::env::current_dir().map_err(|e| format!("cannot get cwd: {e}"))?;
    let cwd_str = cwd
        .to_str()
        .ok_or_else(|| "cwd path is not valid UTF-8".to_string())?;
    wasm_runner::WasmRunner::new()
        .run_wasm_with_fs(&bytes, &args_ref, &[(cwd_str, ".")])
        .map_err(|e| format!("wasm error: {e}"))
}

fn command_run(input_path: PathBuf) -> Result<(), String> {
    let source = std::fs::read_to_string(&input_path)
        .map_err(|e| format!("cannot read {:?}: {e}", input_path))?;

    let nodes = nexl_reader::read(&source, meta::FileId::SYNTHETIC)
        .map_err(|diag| format_reader_report(*diag, &source, &input_path.display().to_string()))?;

    if has_module_decl(&nodes) {
        // Multi-file module mode: discover modules, macro-expand each module's forms
        // (eval_modules skips macro expansion), then evaluate.
        let mut modules = discover_and_load_modules(&input_path)?;
        let mut expander = nexl_macros::Expander::new();
        for module in &mut modules {
            module.forms = expander
                .expand_forms(&module.forms)
                .map_err(|e| format!("macro error: {e}"))?;
        }
        nexl_eval::modules::eval_modules(modules).map_err(|e| format!("eval error: {e}"))?;
        return Ok(());
    }

    // Single-file fallback
    let env = nexl_eval::stdlib::standard_env();
    let (expanded, prelude_forms) = macro_expand(&nodes)?;
    for node in &prelude_forms {
        let _ = nexl_eval::eval::eval(node, &env);
    }
    for node in &expanded {
        nexl_eval::eval::eval(node, &env).map_err(|e| format!("eval error: {e}"))?;
    }

    Ok(())
}

/// Discover test files for `nexl test` (no argument).
///
/// Looks for `*_test.nx` files in `tests/` relative to the current directory,
/// sorted alphabetically for a deterministic run order.
fn discover_test_files() -> Vec<PathBuf> {
    let tests_dir = PathBuf::from("tests");
    if !tests_dir.is_dir() {
        return Vec::new();
    }
    let mut files: Vec<PathBuf> = std::fs::read_dir(&tests_dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.extension().and_then(|e| e.to_str()) == Some("nx")
                && p.file_stem()
                    .and_then(|s| s.to_str())
                    .is_some_and(|s| s.ends_with("_test") || s.ends_with("-test"))
        })
        .collect();
    files.sort();
    files
}

/// Discover the entry point for `nexl run` (no argument).
///
/// Checks project.nx for `:source-dir` (defaults to `src`), then looks for
/// `main.nx` inside that directory.  Falls back to `src/main.nx` if no
/// `project.nx` is found.
fn discover_run_file() -> Option<PathBuf> {
    let cwd = std::env::current_dir().ok()?;
    let manifest_path = cwd.join("project.nx");

    let source_dir = if manifest_path.is_file() {
        // Read source-dir from project.nx
        if let Ok(contents) = std::fs::read_to_string(&manifest_path) {
            if let Ok(manifest) = parse_manifest(&contents) {
                manifest.package.source_dir.clone()
            } else {
                "src".to_string()
            }
        } else {
            "src".to_string()
        }
    } else {
        "src".to_string()
    };

    let main_path = cwd.join(&source_dir).join("main.nx");
    if main_path.is_file() {
        return Some(main_path);
    }

    None
}

/// `nexl test [file]` — evaluate source file and run all registered tests.
///
/// Returns `Ok(true)` if all tests pass, `Ok(false)` if any fail,
/// or `Err` if the file fails to evaluate.
fn command_test(input_path: PathBuf, filter: Option<&str>, tags: &[String], format: &str) -> Result<bool, String> {
    use nexl_stdlib::test as test_mod;

    // Clear any tests from a previous run.
    test_mod::registry_clear();
    test_mod::focus_drain();
    test_mod::tags_drain();
    // Enable test mode so (submodule test ...) blocks are evaluated (spec §8).
    test_mod::set_test_mode(true);

    // Load persisted failing seeds from `.test-seeds` next to the test file (spec §12.6).
    let seeds_path = input_path.with_file_name(".test-seeds");
    let prior_seeds = load_test_seeds(&seeds_path);
    if !prior_seeds.is_empty() {
        test_mod::set_seed_overrides(prior_seeds);
    }

    // Evaluate the source file. This will call (test/register! ...) for each test.
    let source = std::fs::read_to_string(&input_path)
        .map_err(|e| format!("cannot read {:?}: {e}", input_path))?;

    let nodes = nexl_reader::read(&source, meta::FileId::SYNTHETIC)
        .map_err(|diag| format_reader_report(*diag, &source, &input_path.display().to_string()))?;

    if has_module_decl(&nodes) {
        // Module-mode: discover all imports via project.nx, macro-expand every module's
        // forms (eval_modules skips macro expansion), then eval — tests register as a
        // side effect of evaluating deftest-expanded forms.
        let mut modules = discover_and_load_modules(&input_path)
            .map_err(|e| format!("eval error: {e}"))?;

        // Prime the expander with the test stdlib so deftest/is/check macros are known,
        // then expand all module forms (source modules need -> threading, etc.).
        const STDLIB_TEST: &str = include_str!("../../nexl-stdlib/nexl/test.nx");
        let mut expander = nexl_macros::Expander::new();
        let prelude_nodes = nexl_reader::read(STDLIB_TEST, meta::FileId::SYNTHETIC)
            .map_err(|e| format!("stdlib macro parse error: {e}"))?;
        expander
            .expand_forms(&prelude_nodes)
            .map_err(|e| format!("stdlib macro expand error: {e}"))?;
        for module in &mut modules {
            module.forms = expander
                .expand_forms(&module.forms)
                .map_err(|e| format!("macro error: {e}"))?;
        }

        nexl_eval::modules::eval_modules(modules)
            .map_err(|e| format!("eval error: {e}"))?;
    } else {
        let env = nexl_eval::stdlib::standard_env();
        let (expanded, prelude_forms) = macro_expand(&nodes)?;
        for node in &prelude_forms {
            let _ = nexl_eval::eval::eval(node, &env);
        }
        for node in &expanded {
            nexl_eval::eval::eval(node, &env).map_err(|e| format!("eval error: {e}"))?;
        }
    }

    // Drain the registry and optionally filter by name.
    let mut tests = test_mod::registry_drain();
    // Focus mode: when any test has :focus, run only focused tests.
    let focused = test_mod::focus_drain();
    let all_tags = test_mod::tags_drain();
    let flaky_map = test_mod::flaky_registry_drain();
    if !focused.is_empty() {
        tests.retain(|(name, _)| focused.contains(name));
    } else if !tags.is_empty() {
        // Tags filter: retain only tests whose tag list contains all requested tags
        tests.retain(|(name, _)| {
            if let Some(test_tags) = all_tags.get(name) {
                tags.iter().all(|t| test_tags.contains(t))
            } else {
                false
            }
        });
    } else if let Some(f) = filter {
        tests.retain(|(name, _thunk): &(String, _)| name.contains(f));
    }

    // Run setup-all hooks once before all tests
    for hook in test_mod::setup_all_drain() {
        nexl_runtime::call_value(&hook, &[]).map_err(|e| format!("setup-all error: {e}"))?;
    }
    // Collect teardown-all hooks to run after all tests
    let teardown_all_hooks = test_mod::teardown_all_drain();

    let total = tests.len();
    let file_name = input_path.display().to_string();
    let use_json = format == "json";

    if !use_json {
        println!("running {total} tests in {file_name}");
    }

    let mut passed: usize = 0;
    let mut failed: usize = 0;
    let mut skipped: usize = 0;
    let mut failures: Vec<String> = Vec::new();

    for (name, thunk) in tests {
        // For flaky tests: retry up to N times before reporting failure
        let max_retries = flaky_map.get(&name).copied().unwrap_or(0);
        let mut result = nexl_runtime::call_value(&thunk, &[]);
        for _ in 0..max_retries {
            if result.is_ok() { break; }
            result = nexl_runtime::call_value(&thunk, &[]);
        }

        match result {
            Ok(nexl_runtime::Value::Adt { ctor, fields, .. }) if ctor.as_ref() == "Skip" => {
                skipped += 1;
                let reason = fields.first().map(|v| format!("{v}")).unwrap_or_default();
                if use_json {
                    println!(
                        "{{\"type\":\"test\",\"name\":{},\"status\":\"skip\",\"message\":{}}}",
                        json_string(&name),
                        json_string(&reason)
                    );
                } else if reason.is_empty() {
                    println!("  SKIP  {name}");
                } else {
                    println!("  SKIP  {name} ({reason})");
                }
            }
            Ok(_) => {
                passed += 1;
                if use_json {
                    println!(
                        "{{\"type\":\"test\",\"name\":{},\"status\":\"pass\"}}",
                        json_string(&name)
                    );
                } else {
                    println!("  PASS  {name}");
                }
            }
            Err(msg) => {
                failed += 1;
                if use_json {
                    println!(
                        "{{\"type\":\"test\",\"name\":{},\"status\":\"fail\",\"message\":{}}}",
                        json_string(&name),
                        json_string(&msg)
                    );
                } else {
                    println!("  FAIL  {name}");
                    failures.push(format!("  {name}: {msg}"));
                }
            }
        }
    }

    // Run teardown-all hooks once after all tests
    for hook in teardown_all_hooks {
        let _ = nexl_runtime::call_value(&hook, &[]);
    }

    if use_json {
        println!(
            "{{\"type\":\"summary\",\"passed\":{passed},\"failed\":{failed},\"skipped\":{skipped},\"total\":{total}}}"
        );
    } else {
        println!();
        if !failures.is_empty() {
            println!("failures:");
            for f in &failures {
                println!("{f}");
            }
            println!();
        }
        let skip_note = if skipped > 0 { format!("; {skipped} skipped") } else { String::new() };
        if failed == 0 {
            println!("test result: ok. {passed} passed; 0 failed{skip_note}");
        } else {
            println!("test result: FAILED. {passed} passed; {failed} failed{skip_note}");
        }
    }

    // Write any newly-discovered failing seeds to `.test-seeds` (spec §12.6).
    let new_seeds = test_mod::failed_seeds_drain();
    if !new_seeds.is_empty() {
        save_test_seeds(&seeds_path, &new_seeds);
    } else {
        // If no failures, remove stale seeds file so next run starts clean.
        let _ = std::fs::remove_file(&seeds_path);
    }

    Ok(failed == 0)
}

/// Load integer seeds from a `.test-seeds` file (one per line).
fn load_test_seeds(path: &std::path::Path) -> Vec<i64> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| l.trim().parse::<i64>().ok())
        .collect()
}

/// Write failing seeds to a `.test-seeds` file (one per line).
fn save_test_seeds(path: &std::path::Path, seeds: &[i64]) {
    let content = seeds.iter().map(|s| s.to_string()).collect::<Vec<_>>().join("\n");
    let _ = std::fs::write(path, content);
}

/// Escape a string for JSON output (basic escaping: `"`, `\`, and control chars).
fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {}
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// `nexl bench <file>` — run benchmarks registered via `(bench ...)` forms.
fn command_bench(input_path: PathBuf) -> Result<(), String> {
    use nexl_stdlib::test as test_mod;
    test_mod::bench_registry_clear();
    test_mod::set_bench_mode(true);

    let source = std::fs::read_to_string(&input_path)
        .map_err(|e| format!("cannot read {:?}: {e}", input_path))?;
    let nodes = nexl_reader::read(&source, meta::FileId::SYNTHETIC)
        .map_err(|diag| format_reader_report(*diag, &source, &input_path.display().to_string()))?;
    let env = nexl_eval::stdlib::standard_env();
    let (expanded, prelude_forms) = macro_expand(&nodes)?;
    for node in &prelude_forms {
        let _ = nexl_eval::eval::eval(node, &env);
    }
    for node in &expanded {
        nexl_eval::eval::eval(node, &env).map_err(|e| format!("eval error: {e}"))?;
    }

    test_mod::set_bench_mode(false);
    let benches = test_mod::bench_registry_drain();
    if benches.is_empty() {
        println!("no benchmarks found in {}", input_path.display());
        return Ok(());
    }

    println!("running {} benchmarks", benches.len());
    for (name, thunk, warmup, iterations) in benches {
        // Warmup
        for _ in 0..warmup {
            let _ = nexl_runtime::call_value(&thunk, &[]);
        }
        // Timed run
        let start = std::time::Instant::now();
        for _ in 0..iterations {
            let _ = nexl_runtime::call_value(&thunk, &[]);
        }
        let elapsed = start.elapsed();
        let per_iter_ns = elapsed.as_nanos() / iterations as u128;
        let (value, unit) = if per_iter_ns < 1_000 {
            (per_iter_ns, "ns")
        } else if per_iter_ns < 1_000_000 {
            (per_iter_ns / 1_000, "us")
        } else {
            (per_iter_ns / 1_000_000, "ms")
        };
        println!("  {name:<40} {value}{unit}/iter  ({iterations} iterations, {warmup} warmup)");
    }
    Ok(())
}

/// `nexl upgrade` — print current version and upgrade instructions.
fn command_upgrade() {
    let version = env!("CARGO_PKG_VERSION");
    println!("nexl {version} (current)");
    println!();
    println!("Self-update is not yet available.");
    println!("To upgrade, rebuild from source:");
    println!();
    println!("  cargo install --path crates/nexl-cli");
}

/// `nexl new <name> [--template <template>]` — create a new Nexl project.
fn command_new(name: &str, template: &str) -> Result<(), String> {
    let project_dir = Path::new(name);
    if project_dir.exists() {
        return Err(format!("directory `{name}` already exists"));
    }

    // Validate template.
    match template {
        "default" | "web" => {}
        other => return Err(format!("unknown template: `{other}` (expected \"default\" or \"web\")")),
    }

    // Create directory structure.
    let src_dir = project_dir.join("src");
    let tests_dir = project_dir.join("tests");
    std::fs::create_dir_all(&src_dir).map_err(|e| format!("cannot create src/: {e}"))?;
    std::fs::create_dir_all(&tests_dir).map_err(|e| format!("cannot create tests/: {e}"))?;

    // Write project.nx
    let manifest = PackageManifest {
        package: nexl_pkg::PackageSection {
            name: name.to_string(),
            version: "0.1.0".to_string(),
            description: None,
            prefix: name.to_string(),
            source_dir: "src".to_string(),
        },
        dependencies: std::collections::BTreeMap::new(),
        dev_dependencies: std::collections::BTreeMap::new(),
        registries: std::collections::BTreeMap::new(),
        sandbox: None,
        profiles: std::collections::BTreeMap::new(),
    };
    let manifest_str = serialize_manifest(&manifest);
    std::fs::write(project_dir.join("project.nx"), &manifest_str)
        .map_err(|e| format!("cannot write project.nx: {e}"))?;

    // Write source files based on template.
    let (main_content, test_content) = match template {
        "web" => (
            scaffold_web_main(name),
            scaffold_web_test(name),
        ),
        _ => (
            scaffold_default_main(name),
            scaffold_default_test(name),
        ),
    };

    std::fs::write(src_dir.join("main.nx"), &main_content)
        .map_err(|e| format!("cannot write src/main.nx: {e}"))?;
    std::fs::write(tests_dir.join("main_test.nx"), &test_content)
        .map_err(|e| format!("cannot write tests/main_test.nx: {e}"))?;

    // Write .gitignore
    std::fs::write(project_dir.join(".gitignore"), "target/\n*.wasm\n")
        .map_err(|e| format!("cannot write .gitignore: {e}"))?;

    println!("Created project `{name}` in ./{name}");
    println!();
    println!("  cd {name}");
    println!("  nexl run src/main.nx");
    println!();

    Ok(())
}

fn command_functions(cmd: FunctionsCommand) -> Result<(), String> {
    match cmd {
        FunctionsCommand::Deploy { file, name, capability, route } => {
            functions::cmd_deploy(&file, name.as_deref(), &capability, route.as_deref())
        }
        FunctionsCommand::List => {
            functions::cmd_list();
            Ok(())
        }
        FunctionsCommand::Serve { port } => functions::cmd_serve(port),
        FunctionsCommand::Logs { name, n } => {
            functions::cmd_logs(&name, n);
            Ok(())
        }
        FunctionsCommand::Invoke { name, method, path, body } => {
            functions::cmd_invoke(&name, &method, path.as_deref(), body.as_deref())
        }
    }
}

fn scaffold_default_main(name: &str) -> String {
    format!(
        r#"(module {name}.main
  :exports [main])

(defn main []
  (io/println "Hello from {name}!"))

(main)
"#
    )
}

fn scaffold_default_test(_name: &str) -> String {
    r#"(deftest "hello works"
  (assert (= 1 1)))
"#
    .to_string()
}

fn scaffold_web_main(name: &str) -> String {
    format!(
        r#"(module {name}.main
  :exports [main]
  :performs [Net Log])

(defn handle-request [req]
  (log/info "request received" {{:path (http/body req)}})
  (let body (json/encode {{:message "Hello from {name}!"}}))
  (http/response 200 body))

(defn main []
  (log/info "starting server" {{:port 8080}})
  (http/serve handle-request 8080))

(main)
"#
    )
}

fn scaffold_web_test(_name: &str) -> String {
    r#"(deftest "json encode works"
  (assert (= (json/encode {:a 1}) "{\"a\":1}")))

(deftest "http response"
  (let resp (http/response 200 "ok"))
  (assert (= (http/status resp) 200)))
"#
    .to_string()
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
    let nodes = nexl_reader::read(expr, meta::FileId::SYNTHETIC).map_err(|e| format!("{e}"))?;
    if nodes.len() != 1 {
        return Err("expected a single form".to_string());
    }
    let mut state = nexl_infer::InferState::new();
    let ty = nexl_infer::synth(&nodes[0], env, &mut state).map_err(|e| format!("{e}"))?;
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

fn format_reader_report(mut diag: nexl_errors::Diagnostic, source: &str, name: &str) -> String {
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
    let nodes = nexl_reader::read(&source, meta::FileId::SYNTHETIC)
        .map_err(|diag| format_reader_report(*diag, &source, &input_path.display().to_string()))?;

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

fn command_fmt(
    input: &str,
    in_place: bool,
    width: usize,
    indent_width: usize,
    no_align: bool,
) -> Result<(), String> {
    use meta::printer::{PrettyPrinter, PrintConfig};
    use std::io::Read;

    let config = PrintConfig {
        indent_width,
        max_line_width: width,
        align_columns: !no_align,
    };
    let printer = PrettyPrinter::new(config);

    let (source, filename) = if input == "-" {
        let mut buf = String::new();
        io::stdin()
            .read_to_string(&mut buf)
            .map_err(|e| format!("cannot read stdin: {e}"))?;
        (buf, "<stdin>".to_string())
    } else {
        let path = PathBuf::from(input);
        let text =
            std::fs::read_to_string(&path).map_err(|e| format!("cannot read {:?}: {e}", path))?;
        let name = path.display().to_string();
        (text, name)
    };

    let nodes = nexl_reader::read(&source, meta::FileId::SYNTHETIC)
        .map_err(|diag| format_reader_report(*diag, &source, &filename))?;

    let formatted = printer.print_file(&nodes);

    if in_place {
        if input == "-" {
            return Err("cannot use --in-place with stdin".to_string());
        }
        std::fs::write(input, &formatted).map_err(|e| format!("cannot write {:?}: {e}", input))?;
    } else {
        print!("{formatted}");
    }

    Ok(())
}

fn command_doc(input_path: PathBuf, output_override: Option<PathBuf>) -> Result<(), String> {
    let source = std::fs::read_to_string(&input_path)
        .map_err(|e| format!("cannot read {:?}: {e}", input_path))?;
    let doc = extract_module_doc(&source).map_err(|e| format!("doc error: {e}"))?;
    let modules = vec![doc];
    let mut pages = render_module_pages(&modules);
    // Generate index page for navigation.
    pages.push(nexl_doc::render_index_page(&modules));

    let output_dir = output_override.unwrap_or_else(|| PathBuf::from("docs"));
    std::fs::create_dir_all(&output_dir)
        .map_err(|e| format!("cannot create {:?}: {e}", output_dir))?;
    for page in &pages {
        let path = output_dir.join(&page.filename);
        std::fs::write(&path, &page.html)
            .map_err(|e| format!("cannot write {:?}: {e}", path))?;
    }
    println!(
        "nexl: wrote {} pages to {:?}",
        pages.len(),
        output_dir
    );
    Ok(())
}

fn command_audit(input_path: PathBuf) -> Result<(), String> {
    let source = std::fs::read_to_string(&input_path)
        .map_err(|e| format!("cannot read {:?}: {e}", input_path))?;

    let nodes = nexl_reader::read(&source, meta::FileId::SYNTHETIC)
        .map_err(|diag| format_reader_report(*diag, &source, &input_path.display().to_string()))?;

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
            println!(
                "  {}:{} defextern {} — {}",
                filename,
                line,
                entry.name,
                flags.join(", ")
            );
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
    PathBuf::from("project.nx")
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
    std::fs::write(path, output).map_err(|e| format!("cannot write {}: {e}", path.display()))
}

fn write_lockfile(path: &PathBuf, lockfile: &nexl_pkg::Lockfile) -> Result<(), String> {
    let output = serialize_lockfile(lockfile);
    std::fs::write(path, output).map_err(|e| format!("cannot write {}: {e}", path.display()))
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
        NodeKind::Atom(Atom::Symbol {
            ns: None,
            name: head,
        }) => head == name,
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
        let cli = Cli::try_parse_from(["nexl", "build", "main.nexl", "out.wasm"]).expect("parse");
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
        assert!(
            result.is_ok(),
            "native build should succeed, got: {result:?}"
        );
    }

    #[test]
    fn parse_run_with_input() {
        let cli = Cli::try_parse_from(["nexl", "run", "main.nexl"]).expect("parse");
        assert_eq!(
            cli.command,
            Command::Run {
                input: Some(PathBuf::from("main.nexl")),
                wasm: false,
                experimental_wasi3: false,
                args: vec![],
            }
        );
    }

    #[test]
    fn parse_run_without_input() {
        let cli = Cli::try_parse_from(["nexl", "run"]).expect("parse");
        assert_eq!(
            cli.command,
            Command::Run {
                input: None,
                wasm: false,
                experimental_wasi3: false,
                args: vec![],
            }
        );
    }

    #[test]
    fn parse_repl_without_args() {
        let cli = Cli::try_parse_from(["nexl", "repl"]).expect("parse");
        assert_eq!(cli.command, Command::Repl { protocol: false });
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
        let diag =
            nexl_reader::read(source, meta::FileId::SYNTHETIC).expect_err("expected parse error");
        let report = format_reader_report(*diag, source, "test.nx");
        assert!(report.contains("unclosed `(`"));
        assert!(report.contains("test.nx"));
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
    fn parse_fmt_basic() {
        let cli = Cli::try_parse_from(["nexl", "fmt", "main.nexl"]).expect("parse");
        assert_eq!(
            cli.command,
            Command::Fmt {
                input: "main.nexl".to_string(),
                in_place: false,
                width: 80,
                indent: 2,
                no_align: false,
            }
        );
    }

    #[test]
    fn parse_fmt_in_place() {
        let cli = Cli::try_parse_from(["nexl", "fmt", "-i", "main.nexl"]).expect("parse");
        match cli.command {
            Command::Fmt { in_place, .. } => assert!(in_place),
            other => panic!("expected Fmt, got {other:?}"),
        }
    }

    #[test]
    fn parse_fmt_with_options() {
        let cli = Cli::try_parse_from([
            "nexl",
            "fmt",
            "--width",
            "100",
            "--indent",
            "4",
            "--no-align",
            "main.nexl",
        ])
        .expect("parse");
        assert_eq!(
            cli.command,
            Command::Fmt {
                input: "main.nexl".to_string(),
                in_place: false,
                width: 100,
                indent: 4,
                no_align: true,
            }
        );
    }

    #[test]
    fn parse_fmt_stdin() {
        let cli = Cli::try_parse_from(["nexl", "fmt", "-"]).expect("parse");
        match cli.command {
            Command::Fmt { input, .. } => assert_eq!(input, "-"),
            other => panic!("expected Fmt, got {other:?}"),
        }
    }

    #[test]
    fn fmt_formats_file() {
        let source = "(def x 42)";
        let path = write_temp_file(source, "fmt_basic");
        let result = command_fmt(&path.display().to_string(), false, 80, 2, false);
        let _ = std::fs::remove_file(&path);
        assert!(result.is_ok(), "fmt should succeed: {result:?}");
    }

    #[test]
    fn fmt_in_place_writes_file() {
        let source = "(def   x   42)";
        let path = write_temp_file(source, "fmt_inplace");
        let path_str = path.display().to_string();
        let result = command_fmt(&path_str, true, 80, 2, false);
        let contents = std::fs::read_to_string(&path).unwrap_or_default();
        let _ = std::fs::remove_file(&path);
        assert!(result.is_ok(), "fmt should succeed: {result:?}");
        assert!(
            contents.contains("(def x 42)"),
            "file should be reformatted: {contents}"
        );
    }

    #[test]
    fn fmt_rejects_inplace_stdin() {
        let result = command_fmt("-", true, 80, 2, false);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("stdin"));
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
        let cli = Cli::try_parse_from(["nexl", "doc", "mod.nexl", "out"]).expect("parse");
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
        let cli = Cli::try_parse_from(["nexl", "pkg", "add", "json@^1.0.0"]).expect("parse");
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
        assert!(
            result.is_ok(),
            "doc command should succeed, got: {result:?}"
        );
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
            false,
            false,
            false,
            false,
            false,
            false,
            false,
            false,
        );
        let _ = std::fs::remove_file(&path);
        assert!(result.is_err(), "sandbox should deny console");
        let err = result.unwrap_err();
        assert!(
            err.contains("Console"),
            "error should mention Console: {err}"
        );
    }

    #[test]
    fn sandbox_allows_granted_capability() {
        let source = r#"(io/println "hello")"#;
        let path = write_temp_file(source, "sandbox_allow");
        let result = command_sandbox(
            path.clone(),
            true,
            false,
            false,
            false,
            false,
            false,
            false,
            false,
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
            false,
            false,
            false,
            false,
            false,
            false,
            false,
            true,
        );
        let _ = std::fs::remove_file(&path);
        assert!(
            result.is_ok(),
            "sandbox --allow-all should permit: {result:?}"
        );
    }

    #[test]
    fn sandbox_pure_code_runs_without_flags() {
        let source = "(+ 1 2)";
        let path = write_temp_file(source, "sandbox_pure");
        let result = command_sandbox(
            path.clone(),
            false,
            false,
            false,
            false,
            false,
            false,
            false,
            false,
        );
        let _ = std::fs::remove_file(&path);
        assert!(
            result.is_ok(),
            "pure code should run in sandbox: {result:?}"
        );
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
    fn parse_test_with_input() {
        let cli = Cli::try_parse_from(["nexl", "test", "tests.nx"]).expect("parse");
        match cli.command {
            Command::Test { input, filter, tags, .. } => {
                assert_eq!(input, Some(PathBuf::from("tests.nx")));
                assert_eq!(filter, None);
                assert_eq!(tags, None);
            }
            other => panic!("expected Test, got {other:?}"),
        }
    }

    #[test]
    fn parse_test_with_filter() {
        let cli = Cli::try_parse_from(["nexl", "test", "tests.nx", "--filter", "my-"]).expect("parse");
        match cli.command {
            Command::Test { filter, .. } => assert_eq!(filter, Some("my-".to_string())),
            other => panic!("expected Test, got {other:?}"),
        }
    }

    #[test]
    fn parse_test_with_tags() {
        let cli = Cli::try_parse_from(["nexl", "test", "tests.nx", "--tags", "db"]).expect("parse");
        match cli.command {
            Command::Test { tags, .. } => assert_eq!(tags, Some("db".to_string())),
            other => panic!("expected Test, got {other:?}"),
        }
    }

    #[test]
    fn deftest_tags_filter_runs_only_tagged() {
        let source = r#"
(deftest "untagged" (is (= 1 1)))
(deftest "tagged-db" {:tags [:db]} (is (= 2 2)))
"#;
        let path = write_temp_file(source, "test_tags");
        let result = command_test(path.clone(), None, &["db".to_string()], "text");
        let _ = std::fs::remove_file(&path);
        assert!(result.is_ok(), "command_test should succeed: {result:?}");
        assert!(result.unwrap(), "tagged test should pass");
    }

    #[test]
    fn test_command_runs_empty_file() {
        let source = "(def x 1)";
        let path = write_temp_file(source, "test_cmd_empty");
        let result = command_test(path.clone(), None, &[], "text");
        let _ = std::fs::remove_file(&path);
        // Should succeed with 0 tests
        assert!(result.is_ok(), "command_test should succeed: {result:?}");
        assert!(result.unwrap(), "empty registry should return true (0 failures)");
    }

    #[test]
    fn test_command_runs_registered_tests() {
        let source = r#"
(test/register! "pass-test" (fn [] (test/is true)))
"#;
        let path = write_temp_file(source, "test_cmd_reg");
        let result = command_test(path.clone(), None, &[], "text");
        let _ = std::fs::remove_file(&path);
        assert!(result.is_ok(), "command_test should succeed: {result:?}");
        assert!(result.unwrap(), "passing test should return true");
    }

    #[test]
    fn deftest_skip_counts_as_skipped_not_failed() {
        let source = r#"
(deftest "always-passes" (is (= 1 1)))
(deftest "skip-me" {:skip "not ready"} (is false))
"#;
        let path = write_temp_file(source, "test_skip");
        let result = command_test(path.clone(), None, &[], "text");
        let _ = std::fs::remove_file(&path);
        assert!(result.is_ok(), "command_test should succeed: {result:?}");
        assert!(result.unwrap(), "skipped test should not count as failure");
    }

    #[test]
    fn deftest_focus_runs_only_focused_tests() {
        let source = r#"
(deftest "regular" (is false))
(deftest "focused" {:focus true} (is (= 1 1)))
"#;
        let path = write_temp_file(source, "test_focus");
        let result = command_test(path.clone(), None, &[], "text");
        let _ = std::fs::remove_file(&path);
        assert!(result.is_ok(), "command_test should succeed: {result:?}");
        assert!(result.unwrap(), "only focused test should run (and pass)");
    }

    #[test]
    fn backward_compat_register_and_deftest_share_registry() {
        let source = r#"
(test/register! "old-style" (fn [] (is (= 1 1))))
(deftest "new-style" (is (= 2 2)))
"#;
        let path = write_temp_file(source, "test_compat");
        let result = command_test(path.clone(), None, &[], "text");
        let _ = std::fs::remove_file(&path);
        assert!(result.is_ok(), "command_test should succeed: {result:?}");
        assert!(result.unwrap(), "both old and new style tests should pass");
    }

    #[test]
    fn setup_runs_before_each_test() {
        let source = r#"
(describe "hooks"
  (setup (fn [] (def x 1)))
  (deftest "t1" (is (= 1 1)))
  (deftest "t2" (is (= 2 2))))
"#;
        let path = write_temp_file(source, "test_setup");
        let result = command_test(path.clone(), None, &[], "text");
        let _ = std::fs::remove_file(&path);
        assert!(result.is_ok(), "command_test should succeed: {result:?}");
        assert!(result.unwrap(), "setup tests should pass");
    }

    #[test]
    fn teardown_runs_after_each_test() {
        let source = r#"
(describe "td"
  (teardown (fn [] (def y 1)))
  (deftest "t1" (is (= 1 1))))
"#;
        let path = write_temp_file(source, "test_teardown");
        let result = command_test(path.clone(), None, &[], "text");
        let _ = std::fs::remove_file(&path);
        assert!(result.is_ok(), "teardown test should succeed: {result:?}");
        assert!(result.unwrap(), "teardown test should pass");
    }

    #[test]
    fn setup_all_and_teardown_all_run_once() {
        let source = r#"
(setup-all (fn [] (def n 0)))
(teardown-all (fn [] (def n 0)))
(deftest "t1" (is (= 1 1)))
(deftest "t2" (is (= 2 2)))
"#;
        let path = write_temp_file(source, "test_setup_all");
        let result = command_test(path.clone(), None, &[], "text");
        let _ = std::fs::remove_file(&path);
        assert!(result.is_ok(), "setup-all/teardown-all test should succeed: {result:?}");
        assert!(result.unwrap(), "tests should pass");
    }

    // ── Phase 2: throws? macro ──────────────────────────────────────────────

    #[test]
    fn throws_macro_passes_when_exception_raised() {
        let source = r#"
(deftest "should-throw" (throws? (panic "err")))
"#;
        let path = write_temp_file(source, "throws_pass");
        let result = command_test(path.clone(), None, &[], "text");
        let _ = std::fs::remove_file(&path);
        assert!(result.is_ok(), "command_test should succeed: {result:?}");
        assert!(result.unwrap(), "throws? should pass when exception is raised");
    }

    #[test]
    fn throws_macro_fails_when_no_exception() {
        let source = r#"
(deftest "no-throw" (throws? (+ 1 1)))
"#;
        let path = write_temp_file(source, "throws_fail");
        let result = command_test(path.clone(), None, &[], "text");
        let _ = std::fs::remove_file(&path);
        assert!(result.is_ok(), "command_test should succeed: {result:?}");
        assert!(!result.unwrap(), "throws? should fail when no exception is raised");
    }

    #[test]
    fn throws_macro_with_type_hint_passes() {
        let source = r#"
(deftest "type-match" (throws? [TypeError] (panic "TypeError: boom")))
"#;
        let path = write_temp_file(source, "throws_type_pass");
        let result = command_test(path.clone(), None, &[], "text");
        let _ = std::fs::remove_file(&path);
        assert!(result.is_ok(), "command_test should succeed: {result:?}");
        assert!(result.unwrap(), "throws? with type hint should pass when message matches");
    }

    #[test]
    fn throws_macro_with_type_hint_wrong_type_fails() {
        let source = r#"
(deftest "type-mismatch" (throws? [TypeError] (panic "OtherError: boom")))
"#;
        let path = write_temp_file(source, "throws_type_fail");
        let result = command_test(path.clone(), None, &[], "text");
        let _ = std::fs::remove_file(&path);
        assert!(result.is_ok(), "command_test should succeed: {result:?}");
        assert!(!result.unwrap(), "throws? with type hint should fail when error message doesn't match");
    }

    // ── Phase 2: is-match macro ─────────────────────────────────────────────

    #[test]
    fn is_match_macro_passes_on_literal_match() {
        let source = r#"
(deftest "match-42" (is-match 42 42))
"#;
        let path = write_temp_file(source, "is_match_pass");
        let result = command_test(path.clone(), None, &[], "text");
        let _ = std::fs::remove_file(&path);
        assert!(result.is_ok(), "command_test should succeed: {result:?}");
        assert!(result.unwrap(), "is-match on matching literal should pass");
    }

    #[test]
    fn is_match_macro_fails_on_mismatch() {
        let source = r#"
(deftest "no-match" (is-match 99 42))
"#;
        let path = write_temp_file(source, "is_match_fail");
        let result = command_test(path.clone(), None, &[], "text");
        let _ = std::fs::remove_file(&path);
        assert!(result.is_ok(), "command_test should succeed: {result:?}");
        assert!(!result.unwrap(), "is-match on mismatched values should fail");
    }

    #[test]
    fn is_match_macro_destructures_and_runs_body() {
        let source = r#"
(deftest "destructure" (is-match [a b] [1 2] (is (= a 1)) (is (= b 2))))
"#;
        let path = write_temp_file(source, "is_match_destruct");
        let result = command_test(path.clone(), None, &[], "text");
        let _ = std::fs::remove_file(&path);
        assert!(result.is_ok(), "command_test should succeed: {result:?}");
        assert!(result.unwrap(), "is-match should destructure and run body forms");
    }

    #[test]
    fn is_match_macro_guard_passes() {
        let source = r#"
(deftest "guard-pass" (is-match x 5 :when (> x 0)))
"#;
        let path = write_temp_file(source, "is_match_guard_pass");
        let result = command_test(path.clone(), None, &[], "text");
        let _ = std::fs::remove_file(&path);
        assert!(result.is_ok(), "command_test should succeed: {result:?}");
        assert!(result.unwrap(), "is-match with satisfied guard should pass");
    }

    #[test]
    fn is_match_macro_guard_fails() {
        let source = r#"
(deftest "guard-fail" (is-match x -1 :when (> x 0)))
"#;
        let path = write_temp_file(source, "is_match_guard_fail");
        let result = command_test(path.clone(), None, &[], "text");
        let _ = std::fs::remove_file(&path);
        assert!(result.is_ok(), "command_test should succeed: {result:?}");
        assert!(!result.unwrap(), "is-match with failed guard should fail the test");
    }

    // ── Phase 3: describe macro ─────────────────────────────────────────────

    #[test]
    fn describe_macro_scopes_test_names() {
        let source = r#"
(describe "suite"
  (deftest "t" (is (= 1 1))))
"#;
        let path = write_temp_file(source, "describe_scope");
        let result = command_test(path.clone(), None, &[], "text");
        let _ = std::fs::remove_file(&path);
        assert!(result.is_ok(), "command_test should succeed: {result:?}");
        assert!(result.unwrap(), "scoped test should pass");
    }

    #[test]
    fn describe_macro_with_let_provides_fixture() {
        let source = r#"
(describe "d"
  :let [x 42]
  (deftest "t" (is (= x 42))))
"#;
        let path = write_temp_file(source, "describe_let");
        let result = command_test(path.clone(), None, &[], "text");
        let _ = std::fs::remove_file(&path);
        assert!(result.is_ok(), "command_test should succeed: {result:?}");
        assert!(result.unwrap(), ":let fixture should be visible in deftest");
    }

    #[test]
    fn describe_macro_cleans_up_setup_hooks() {
        // setup hooks from inside describe should not affect tests outside
        let source = r#"
(describe "inner"
  (setup (fn [] unit))
  (deftest "t1" (is (= 1 1))))
(deftest "t2" (is (= 2 2)))
"#;
        let path = write_temp_file(source, "describe_cleanup");
        let result = command_test(path.clone(), None, &[], "text");
        let _ = std::fs::remove_file(&path);
        assert!(result.is_ok(), "command_test should succeed: {result:?}");
        assert!(result.unwrap(), "both tests should pass, no hook leakage");
    }

    // ── Phase 5: is macro ────────────────────────────────────────────────────

    #[test]
    fn is_macro_simple_pass() {
        let source = r#"(deftest "t" (is true))"#;
        let path = write_temp_file(source, "is_pass");
        let result = command_test(path.clone(), None, &[], "text");
        let _ = std::fs::remove_file(&path);
        assert!(result.is_ok() && result.unwrap(), "is true should pass");
    }

    #[test]
    fn is_macro_simple_fail() {
        let source = r#"(deftest "t" (is false))"#;
        let path = write_temp_file(source, "is_fail");
        let result = command_test(path.clone(), None, &[], "text");
        let _ = std::fs::remove_file(&path);
        assert!(result.is_ok() && !result.unwrap(), "is false should fail");
    }

    #[test]
    fn is_macro_eq_pass() {
        let source = r#"(deftest "t" (is (= 1 1)))"#;
        let path = write_temp_file(source, "is_eq_pass");
        let result = command_test(path.clone(), None, &[], "text");
        let _ = std::fs::remove_file(&path);
        assert!(result.is_ok() && result.unwrap(), "is (= 1 1) should pass");
    }

    #[test]
    fn is_macro_eq_fail() {
        let source = r#"(deftest "t" (is (= 1 2)))"#;
        let path = write_temp_file(source, "is_eq_fail");
        let result = command_test(path.clone(), None, &[], "text");
        let _ = std::fs::remove_file(&path);
        assert!(result.is_ok() && !result.unwrap(), "is (= 1 2) should fail");
    }

    #[test]
    fn is_macro_neq_pass() {
        let source = r#"(deftest "t" (is (not= 1 2)))"#;
        let path = write_temp_file(source, "is_neq_pass");
        let result = command_test(path.clone(), None, &[], "text");
        let _ = std::fs::remove_file(&path);
        assert!(result.is_ok() && result.unwrap(), "is (not= 1 2) should pass");
    }

    #[test]
    fn is_macro_predicate_pass() {
        let source = r#"(deftest "t" (is (= (count []) 0)))"#;
        let path = write_temp_file(source, "is_pred_pass");
        let result = command_test(path.clone(), None, &[], "text");
        let _ = std::fs::remove_file(&path);
        assert!(result.is_ok() && result.unwrap(), "is (odd? 3) should pass");
    }

    #[test]
    fn is_macro_with_message() {
        // (is expr "msg") — user message appears in failure
        let source = r#"(deftest "t" (is (= 1 2) "one does not equal two"))"#;
        let path = write_temp_file(source, "is_msg");
        let result = command_test(path.clone(), None, &[], "text");
        let _ = std::fs::remove_file(&path);
        assert!(result.is_ok() && !result.unwrap(), "is with message should fail");
    }

    // ── Phase 4: deftest macro ──────────────────────────────────────────────

    #[test]
    fn deftest_macro_simple() {
        // (deftest "t" (is (= 1 1))) — registers a test that passes
        let source = r#"(deftest "t" (is (= 1 1)))"#;
        let path = write_temp_file(source, "deftest_simple");
        let result = command_test(path.clone(), None, &[], "text");
        let _ = std::fs::remove_file(&path);
        assert!(result.is_ok(), "command_test should succeed: {result:?}");
        assert!(result.unwrap(), "test should pass");
    }

    #[test]
    fn deftest_macro_with_describe_prefix() {
        // Inside describe, test gets prefixed name
        let source = r#"
(describe "Group"
  (deftest "t" (is (= 1 1))))
"#;
        let path = write_temp_file(source, "deftest_prefix");
        let result = command_test(path.clone(), None, &[], "text");
        let _ = std::fs::remove_file(&path);
        assert!(result.is_ok(), "command_test should succeed: {result:?}");
        assert!(result.unwrap(), "test should pass with describe prefix");
    }

    #[test]
    fn deftest_macro_skip() {
        // (deftest "t" {:skip "reason"} (is false)) — test skips, body not evaluated
        let source = r#"(deftest "skip-me" {:skip "not ready"} (is false))"#;
        let path = write_temp_file(source, "deftest_skip");
        let result = command_test(path.clone(), None, &[], "text");
        let _ = std::fs::remove_file(&path);
        assert!(result.is_ok(), "command_test should succeed: {result:?}");
    }

    #[test]
    fn deftest_macro_focus() {
        // (deftest "t" {:focus true} body) — focus registered
        let source = r#"
(deftest "focused" {:focus true} (is (= 1 1)))
(deftest "not-focused" (is (= 2 2)))
"#;
        let path = write_temp_file(source, "deftest_focus");
        let result = command_test(path.clone(), None, &[], "text");
        let _ = std::fs::remove_file(&path);
        assert!(result.is_ok(), "command_test should succeed: {result:?}");
        assert!(result.unwrap(), "focused test should pass");
    }

    #[test]
    fn deftest_macro_tags() {
        // (deftest "t" {:tags [:db]} body) — tags registered (no filter, all run)
        let source = r#"(deftest "tagged-db" {:tags [:db]} (is (= 2 2)))"#;
        let path = write_temp_file(source, "deftest_tags");
        let result = command_test(path.clone(), None, &[], "text");
        let _ = std::fs::remove_file(&path);
        assert!(result.is_ok(), "command_test should succeed: {result:?}");
        assert!(result.unwrap(), "tagged test should pass");
    }

    #[test]
    fn deftest_macro_flaky() {
        // (deftest "t" {:flaky 5} body) — flaky registered, test still runs
        let source = r#"(deftest "flaky-t" {:flaky 5} (is (= 1 1)))"#;
        let path = write_temp_file(source, "deftest_flaky");
        let result = command_test(path.clone(), None, &[], "text");
        let _ = std::fs::remove_file(&path);
        assert!(result.is_ok(), "command_test should succeed: {result:?}");
        assert!(result.unwrap(), "flaky test should pass");
    }

    #[test]
    fn deftest_macro_runs_setup_teardown() {
        // setup hook runs before test body; teardown after
        let source = r#"
(def log (atom []))
(describe "d"
  (setup (fn [] (swap! log (fn [v] (append v "setup")))))
  (deftest "t" (is (= 1 1))))
"#;
        let path = write_temp_file(source, "deftest_hooks");
        let result = command_test(path.clone(), None, &[], "text");
        let _ = std::fs::remove_file(&path);
        assert!(result.is_ok(), "command_test should succeed: {result:?}");
        assert!(result.unwrap(), "test with setup should pass");
    }

    // ── Phase 3: bench macro ────────────────────────────────────────────────

    #[test]
    fn bench_macro_registers_in_bench_mode() {
        // This is already tested by the existing bench test, but verify the macro version
        let source = r#"(bench "perf" (+ 1 2))"#;
        let path = write_temp_file(source, "bench_macro");
        let result = command_bench(path.clone());
        let _ = std::fs::remove_file(&path);
        assert!(result.is_ok(), "bench macro should work: {result:?}");
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
        assert!(
            result.is_ok(),
            "audit should succeed with no FFI: {result:?}"
        );
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
        let source = include_str!("../../../docs/kernel-bootstrap.nx");
        let nodes = nexl_reader::read(source, meta::FileId::SYNTHETIC);
        assert!(
            nodes.is_ok(),
            "kernel-bootstrap.nx must parse: {:?}",
            nodes.err()
        );
        let nodes = nodes.unwrap();
        assert!(
            !nodes.is_empty(),
            "kernel-bootstrap.nx should contain definitions"
        );
    }

    #[test]
    fn kernel_bootstrap_evaluates() {
        // Verify Stage 0 evaluator can run the kernel-subset bootstrap POC.
        let source = include_str!("../../../docs/kernel-bootstrap.nx");
        let nodes = nexl_reader::read(source, meta::FileId::SYNTHETIC).expect("parse");
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

    // --- M21: Multi-file module loading tests ---

    #[test]
    fn find_project_root_finds_manifest() {
        let root = write_temp_dir("proj_root");
        let sub = root.join("src").join("app");
        std::fs::create_dir_all(&sub).expect("create subdirs");
        std::fs::write(
            root.join("project.nx"),
            "(project :name \"demo\" :version \"0.1.0\" :prefix \"demo\")",
        )
        .expect("write manifest");

        let found = find_project_root(&sub);
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(found, Some(root));
    }

    #[test]
    fn find_project_root_none_when_missing() {
        let dir = write_temp_dir("no_manifest");
        let found = find_project_root(&dir);
        let _ = std::fs::remove_dir_all(&dir);
        // The temp dir has no project.nx anywhere up the tree (well, it might
        // find one in the real filesystem, but /tmp shouldn't have one).
        // We just verify the function doesn't panic and returns a path or None.
        // In a controlled env without project.nx in /tmp, this is None.
        assert!(
            found.is_none(),
            "expected None in temp dir without manifest"
        );
    }

    #[test]
    fn has_module_decl_true() {
        let nodes = nexl_reader::read(
            "(module demo :exports [f])\n(defn f [x] x)",
            meta::FileId::SYNTHETIC,
        )
        .expect("parse");
        assert!(has_module_decl(&nodes));
    }

    #[test]
    fn has_module_decl_false() {
        let nodes =
            nexl_reader::read("(def x 1)\n(+ x 2)", meta::FileId::SYNTHETIC).expect("parse");
        assert!(!has_module_decl(&nodes));
    }

    #[test]
    fn discover_modules_loads_transitive_deps() {
        // Create a project structure:
        // root/project.nx
        // root/demo/app.nx     — imports demo.util
        // root/demo/util.nx    — imports demo.math
        // root/demo/math.nx    — no imports
        let root = write_temp_dir("discover");
        let demo = root.join("demo");
        std::fs::create_dir_all(&demo).expect("create demo dir");
        std::fs::write(
            root.join("project.nx"),
            "{:package {:name \"demo\" :version \"0.1.0\" :prefix \"demo\"}}",
        )
        .expect("write manifest");
        std::fs::write(
            demo.join("app.nx"),
            "(module demo.app :exports [main])\n(import demo.util :refer [add1])\n(defn main [] (add1 41))",
        )
        .expect("write app");
        std::fs::write(
            demo.join("util.nx"),
            "(module demo.util :exports [add1])\n(import demo.math :refer [inc])\n(defn add1 [x] (inc x))",
        )
        .expect("write util");
        std::fs::write(
            demo.join("math.nx"),
            "(module demo.math :exports [inc])\n(defn inc [x] (+ x 1))",
        )
        .expect("write math");

        let entry = demo.join("app.nx");
        let modules = discover_and_load_modules(&entry).expect("discover");
        let _ = std::fs::remove_dir_all(&root);

        let names: Vec<&str> = modules.iter().map(|m| m.decl.name.as_str()).collect();
        assert!(names.contains(&"demo.app"), "should contain app: {names:?}");
        assert!(
            names.contains(&"demo.util"),
            "should contain util: {names:?}"
        );
        assert!(
            names.contains(&"demo.math"),
            "should contain math: {names:?}"
        );
        assert_eq!(names.len(), 3);
    }

    #[test]
    fn discover_modules_circular_error() {
        let root = write_temp_dir("circular");
        let demo = root.join("demo");
        std::fs::create_dir_all(&demo).expect("create demo dir");
        std::fs::write(
            root.join("project.nx"),
            "{:package {:name \"demo\" :version \"0.1.0\" :prefix \"demo\"}}",
        )
        .expect("write manifest");
        std::fs::write(
            demo.join("a.nx"),
            "(module demo.a :exports [x])\n(import demo.b :refer [y])\n(def x 1)",
        )
        .expect("write a");
        std::fs::write(
            demo.join("b.nx"),
            "(module demo.b :exports [y])\n(import demo.a :refer [x])\n(def y 2)",
        )
        .expect("write b");

        let entry = demo.join("a.nx");
        let modules = discover_and_load_modules(&entry).expect("discover should succeed");
        // eval_modules should detect the cycle
        let result = nexl_eval::modules::eval_modules(modules);
        let _ = std::fs::remove_dir_all(&root);

        assert!(result.is_err(), "should detect circular dependency");
        let err = format!("{}", result.unwrap_err());
        assert!(
            err.contains("circular") || err.contains("cycle"),
            "error should mention cycle: {err}"
        );
    }

    #[test]
    fn command_run_multifile() {
        // Create a two-module project that evaluates successfully.
        let root = write_temp_dir("run_multi");
        let demo = root.join("demo");
        std::fs::create_dir_all(&demo).expect("create demo dir");
        std::fs::write(
            root.join("project.nx"),
            "{:package {:name \"demo\" :version \"0.1.0\" :prefix \"demo\"}}",
        )
        .expect("write manifest");
        std::fs::write(
            demo.join("app.nx"),
            "(module demo.app)\n(import demo.lib :refer [double])\n(double 21)",
        )
        .expect("write app");
        std::fs::write(
            demo.join("lib.nx"),
            "(module demo.lib :exports [double])\n(defn double [x] (* x 2))",
        )
        .expect("write lib");

        let entry = demo.join("app.nx");
        let result = command_run(entry);
        let _ = std::fs::remove_dir_all(&root);
        assert!(result.is_ok(), "multi-file run should succeed: {result:?}");
    }

    #[test]
    fn command_run_multifile_source_dir() {
        // Modules live under src/ with :source-dir "src" in manifest.
        let root = write_temp_dir("run_multi_src");
        let src_demo = root.join("src").join("demo");
        std::fs::create_dir_all(&src_demo).expect("create src/demo dir");
        std::fs::write(
            root.join("project.nx"),
            "{:package {:name \"demo\" :version \"0.1.0\" :prefix \"demo\" :source-dir \"src\"}}",
        )
        .expect("write manifest");
        std::fs::write(
            src_demo.join("app.nx"),
            "(module demo.app)\n(import demo.lib :refer [double])\n(double 21)",
        )
        .expect("write app");
        std::fs::write(
            src_demo.join("lib.nx"),
            "(module demo.lib :exports [double])\n(defn double [x] (* x 2))",
        )
        .expect("write lib");

        let entry = src_demo.join("app.nx");
        let result = command_run(entry);
        let _ = std::fs::remove_dir_all(&root);
        assert!(
            result.is_ok(),
            "multi-file run with source-dir should succeed: {result:?}"
        );
    }

    #[test]
    fn command_run_singlefile_fallback() {
        // A plain script without (module ...) should still work.
        let path = write_temp_file("(+ 40 2)", "run_single");
        let result = command_run(path.clone());
        let _ = std::fs::remove_file(&path);
        assert!(result.is_ok(), "single-file run should succeed: {result:?}");
    }

    // ---- nexl new tests ----

    #[test]
    fn parse_new_command() {
        let cli = Cli::try_parse_from(["nexl", "new", "my-app"]).expect("parse");
        assert_eq!(
            cli.command,
            Command::New {
                name: "my-app".to_string(),
                template: "default".to_string(),
            }
        );
    }

    #[test]
    fn parse_new_with_template() {
        let cli = Cli::try_parse_from(["nexl", "new", "my-app", "--template", "web"]).expect("parse");
        assert_eq!(
            cli.command,
            Command::New {
                name: "my-app".to_string(),
                template: "web".to_string(),
            }
        );
    }

    #[test]
    fn scaffold_creates_directory() {
        let root = write_temp_dir("new_scaffold");
        let name = root.join("test-proj");
        let name_str = name.to_str().expect("utf8 path");
        let result = command_new(name_str, "default");
        assert!(result.is_ok(), "scaffold should succeed: {result:?}");
        assert!(name.join("src").is_dir(), "src/ should exist");
        assert!(name.join("tests").is_dir(), "tests/ should exist");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn scaffold_creates_project_nx() {
        let root = write_temp_dir("new_project_nx");
        let name = root.join("my-proj");
        let name_str = name.to_str().expect("utf8 path");
        command_new(name_str, "default").expect("scaffold");
        let content = std::fs::read_to_string(name.join("project.nx")).expect("read project.nx");
        assert!(content.contains(":name"), "should have :name");
        assert!(content.contains(":version"), "should have :version");
        assert!(content.contains("\"0.1.0\""), "should have version 0.1.0");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn scaffold_creates_main_nx() {
        let root = write_temp_dir("new_main_nx");
        let name = root.join("hello");
        let name_str = name.to_str().expect("utf8 path");
        command_new(name_str, "default").expect("scaffold");
        let content = std::fs::read_to_string(name.join("src/main.nx")).expect("read main.nx");
        assert!(content.contains("Hello from"), "should contain hello message");
        assert!(content.contains("io/println"), "should use io/println");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn scaffold_creates_gitignore() {
        let root = write_temp_dir("new_gitignore");
        let name = root.join("proj");
        let name_str = name.to_str().expect("utf8 path");
        command_new(name_str, "default").expect("scaffold");
        let content = std::fs::read_to_string(name.join(".gitignore")).expect("read .gitignore");
        assert!(content.contains("target/"), "should ignore target/");
        assert!(content.contains("*.wasm"), "should ignore *.wasm");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn scaffold_creates_test_file() {
        let root = write_temp_dir("new_testfile");
        let name = root.join("proj");
        let name_str = name.to_str().expect("utf8 path");
        command_new(name_str, "default").expect("scaffold");
        let content = std::fs::read_to_string(name.join("tests/main_test.nx")).expect("read test");
        assert!(content.contains("deftest"), "should contain deftest");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn scaffold_web_template() {
        let root = write_temp_dir("new_web");
        let name = root.join("web-app");
        let name_str = name.to_str().expect("utf8 path");
        command_new(name_str, "web").expect("scaffold");
        let main = std::fs::read_to_string(name.join("src/main.nx")).expect("read main.nx");
        assert!(main.contains("http/serve"), "should contain http/serve");
        assert!(main.contains("json/encode"), "should contain json/encode");
        assert!(main.contains("log/info"), "should contain log/info");
        let test = std::fs::read_to_string(name.join("tests/main_test.nx")).expect("read test");
        assert!(test.contains("json/encode"), "test should use json/encode");
        assert!(test.contains("http/response"), "test should use http/response");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn scaffold_existing_dir_errors() {
        let root = write_temp_dir("new_existing");
        // root already exists, so passing it as the project name should fail.
        let name_str = root.to_str().expect("utf8 path");
        let result = command_new(name_str, "default");
        assert!(result.is_err(), "should error for existing dir");
        assert!(result.unwrap_err().contains("already exists"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn parse_upgrade_command() {
        let cli = Cli::try_parse_from(["nexl", "upgrade"]).expect("parse");
        assert_eq!(cli.command, Command::Upgrade);
    }

    #[test]
    fn scaffold_project_nx_roundtrip() {
        let root = write_temp_dir("new_roundtrip");
        let name = root.join("rt-proj");
        let name_str = name.to_str().expect("utf8 path");
        command_new(name_str, "default").expect("scaffold");
        let content = std::fs::read_to_string(name.join("project.nx")).expect("read");
        let parsed = parse_manifest(&content);
        assert!(parsed.is_ok(), "project.nx should be parseable: {parsed:?}");
        let manifest = parsed.unwrap();
        assert!(manifest.package.version == "0.1.0");
        assert!(manifest.package.source_dir == "src");
        let _ = std::fs::remove_dir_all(&root);
    }

    // --- --format json output ---

    #[test]
    fn test_format_json_passing() {
        let source = r#"(test/register! "pass-test" (fn [] (test/is true)))"#;
        let path = write_temp_file(source, "json_pass");
        let result = command_test(path.clone(), None, &[], "json");
        let _ = std::fs::remove_file(&path);
        assert!(result.is_ok(), "command_test json should succeed: {result:?}");
        assert!(result.unwrap(), "passing test should return true");
    }

    #[test]
    fn test_format_json_failing() {
        let source = r#"(test/register! "fail-test" (fn [] (test/is false)))"#;
        let path = write_temp_file(source, "json_fail");
        let result = command_test(path.clone(), None, &[], "json");
        let _ = std::fs::remove_file(&path);
        assert!(result.is_ok(), "command_test json should not error: {result:?}");
        assert!(!result.unwrap(), "failing test should return false");
    }

    #[test]
    fn json_string_escapes_quotes() {
        assert_eq!(json_string(r#"hello "world""#), r#""hello \"world\"""#);
    }

    #[test]
    fn json_string_escapes_backslash() {
        assert_eq!(json_string(r"a\b"), r#""a\\b""#);
    }

    // --- bench form ---

    #[test]
    fn bench_form_no_op_outside_bench_mode() {
        // bench is now a macro; use command_run (non-bench mode) — body should eval without error
        let source = r#"(bench "perf" (+ 1 2))"#;
        let path = write_temp_file(source, "bench_noop");
        let result = command_run(path.clone());
        let _ = std::fs::remove_file(&path);
        assert!(result.is_ok(), "bench should be no-op outside bench mode: {result:?}");
    }

    #[test]
    fn bench_command_runs_benchmarks() {
        let source = r#"(bench "add" (+ 1 2))"#;
        let path = write_temp_file(source, "bench_run");
        let result = command_bench(path.clone());
        let _ = std::fs::remove_file(&path);
        assert!(result.is_ok(), "command_bench should succeed: {result:?}");
    }

    #[test]
    fn bench_command_empty_file() {
        let source = "(def x 1)";
        let path = write_temp_file(source, "bench_empty");
        let result = command_bench(path.clone());
        let _ = std::fs::remove_file(&path);
        assert!(result.is_ok(), "command_bench with no benchmarks should succeed: {result:?}");
    }
}
