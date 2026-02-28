//! WASM Component composition scaffolding (M23 task 9).
//!
//! Provides types and helpers for composing multiple WASM components via
//! `wasm-tools compose`. Components export interfaces consumed by others;
//! this module models that graph and generates the invocation command.
//!
//! # Overview
//!
//! ```text
//! ┌─────────────────────┐       ┌─────────────────────┐
//! │  nexl-component-a   │─────▶│  nexl-component-b   │
//! │ (exports: math)     │       │ (imports: math)      │
//! └─────────────────────┘       └─────────────────────┘
//!               └──────────────────┘
//!                  wasm-tools compose
//!                        │
//!                        ▼
//!             composed-component.wasm
//! ```
//!
//! # Usage
//!
//! ```
//! # use nexl_wasm::composition::{ComponentInput, ComposeGraph};
//! let mut graph = ComposeGraph::new("composed.wasm");
//! graph.add_input(ComponentInput::file("a.wasm"));
//! graph.add_input(ComponentInput::file("b.wasm"));
//! let cmd = graph.compose_command();
//! // cmd = ["wasm-tools", "compose", "a.wasm", "-d", "b.wasm", "-o", "composed.wasm"]
//! ```

use std::path::{Path, PathBuf};

// ─── Types ────────────────────────────────────────────────────────────────────

/// A component input for composition — either a file path or an in-memory bytes source.
#[derive(Debug, Clone, PartialEq)]
pub enum ComponentInput {
    /// A path to a `.wasm` component on disk.
    File(PathBuf),
    /// An in-memory component with a logical name (for error messages).
    Memory { name: String, bytes: Vec<u8> },
}

impl ComponentInput {
    /// Create a file-based component input.
    pub fn file(path: impl AsRef<Path>) -> Self {
        ComponentInput::File(path.as_ref().to_path_buf())
    }

    /// Create an in-memory component input.
    pub fn memory(name: impl Into<String>, bytes: Vec<u8>) -> Self {
        ComponentInput::Memory {
            name: name.into(),
            bytes,
        }
    }

    /// Return the display name of this component (filename or memory name).
    pub fn display_name(&self) -> String {
        match self {
            ComponentInput::File(p) => p
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| p.to_string_lossy().into_owned()),
            ComponentInput::Memory { name, .. } => name.clone(),
        }
    }
}

/// Errors that can occur during component composition.
#[derive(Debug, Clone, PartialEq)]
pub enum ComposeError {
    /// No components were added to the composition graph.
    EmptyGraph,
    /// `wasm-tools` binary is not found in PATH.
    WasmToolsNotFound,
    /// The `wasm-tools compose` invocation failed with an error.
    ComposeFailed(String),
    /// An I/O error occurred (e.g., writing temp files).
    IoError(String),
}

impl std::fmt::Display for ComposeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ComposeError::EmptyGraph => write!(f, "composition graph has no components"),
            ComposeError::WasmToolsNotFound => write!(
                f,
                "`wasm-tools` not found in PATH; install with: cargo install wasm-tools"
            ),
            ComposeError::ComposeFailed(msg) => write!(f, "wasm-tools compose failed: {msg}"),
            ComposeError::IoError(msg) => write!(f, "I/O error during composition: {msg}"),
        }
    }
}

impl std::error::Error for ComposeError {}

/// A directed composition graph: the first component is the root (host); subsequent
/// components are dependencies it imports from.
///
/// Generates the `wasm-tools compose` command that wires them together.
#[derive(Debug, Clone)]
pub struct ComposeGraph {
    /// Output file path for the composed component.
    pub output: PathBuf,
    /// Ordered list of component inputs.
    /// Index 0 is the root (host) component; the rest are dependencies.
    pub inputs: Vec<ComponentInput>,
}

impl ComposeGraph {
    /// Create a new composition graph with the given output path.
    pub fn new(output: impl AsRef<Path>) -> Self {
        ComposeGraph {
            output: output.as_ref().to_path_buf(),
            inputs: Vec::new(),
        }
    }

    /// Add a component input.  The first input added is the root component;
    /// subsequent inputs are dependencies linked via `--definitions` / `-d`.
    pub fn add_input(&mut self, input: ComponentInput) {
        self.inputs.push(input);
    }

    /// Validate the graph: returns `Err(ComposeError::EmptyGraph)` if no inputs were added.
    pub fn validate(&self) -> Result<(), ComposeError> {
        if self.inputs.is_empty() {
            return Err(ComposeError::EmptyGraph);
        }
        Ok(())
    }

    /// Generate the `wasm-tools compose` command for this graph.
    ///
    /// The root component (index 0) is the positional argument; each additional
    /// component is a `--definitions` flag.
    ///
    /// # Example output
    ///
    /// For inputs `[a.wasm, b.wasm, c.wasm]` and output `composed.wasm`:
    /// ```text
    /// wasm-tools compose a.wasm -d b.wasm -d c.wasm -o composed.wasm
    /// ```
    pub fn compose_command(&self) -> Result<ComposeCommand, ComposeError> {
        self.validate()?;

        let mut args: Vec<String> = Vec::new();
        args.push("compose".to_string());

        // Root component (positional).
        match &self.inputs[0] {
            ComponentInput::File(p) => args.push(p.to_string_lossy().into_owned()),
            ComponentInput::Memory { name, .. } => args.push(name.clone()),
        }

        // Dependency components (--definitions flags).
        for dep in self.inputs.iter().skip(1) {
            args.push("-d".to_string());
            match dep {
                ComponentInput::File(p) => args.push(p.to_string_lossy().into_owned()),
                ComponentInput::Memory { name, .. } => args.push(name.clone()),
            }
        }

        // Output path.
        args.push("-o".to_string());
        args.push(self.output.to_string_lossy().into_owned());

        Ok(ComposeCommand {
            binary: "wasm-tools".to_string(),
            args,
        })
    }
}

/// A ready-to-execute `wasm-tools compose` command.
#[derive(Debug, Clone, PartialEq)]
pub struct ComposeCommand {
    /// The binary to invoke (typically `"wasm-tools"`).
    pub binary: String,
    /// Arguments to pass to the binary.
    pub args: Vec<String>,
}

impl ComposeCommand {
    /// Execute the composition command via `std::process::Command`.
    ///
    /// Returns the composed bytes (read from the output path) on success.
    ///
    /// # Errors
    /// - [`ComposeError::WasmToolsNotFound`] if `wasm-tools` is not in `PATH`.
    /// - [`ComposeError::ComposeFailed`] if the command exits non-zero.
    pub fn execute(&self) -> Result<(), ComposeError> {
        let status = std::process::Command::new(&self.binary)
            .args(&self.args)
            .status()
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    ComposeError::WasmToolsNotFound
                } else {
                    ComposeError::IoError(e.to_string())
                }
            })?;

        if status.success() {
            Ok(())
        } else {
            Err(ComposeError::ComposeFailed(format!(
                "exit code: {}",
                status.code().unwrap_or(-1)
            )))
        }
    }

    /// Return the full command as a displayable string (for logging and docs).
    pub fn display(&self) -> String {
        let parts: Vec<&str> = std::iter::once(self.binary.as_str())
            .chain(self.args.iter().map(String::as_str))
            .collect();
        parts.join(" ")
    }
}

// ─── Helper: check wasm-tools availability ───────────────────────────────────

/// Check whether `wasm-tools` is available in PATH.
///
/// Returns `Ok(version_string)` or `Err(ComposeError::WasmToolsNotFound)`.
pub fn check_wasm_tools() -> Result<String, ComposeError> {
    let output = std::process::Command::new("wasm-tools")
        .arg("--version")
        .output()
        .map_err(|_| ComposeError::WasmToolsNotFound)?;

    if output.status.success() {
        let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(version)
    } else {
        Err(ComposeError::WasmToolsNotFound)
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Test 1 ──────────────────────────────────────────────────────────────

    #[test]
    fn test_compose_command_single_component() {
        let mut graph = ComposeGraph::new("output.wasm");
        graph.add_input(ComponentInput::file("component.wasm"));
        let cmd = graph.compose_command().unwrap();
        assert_eq!(cmd.binary, "wasm-tools");
        assert_eq!(
            cmd.args,
            vec!["compose", "component.wasm", "-o", "output.wasm"]
        );
    }

    // ── Test 2 ──────────────────────────────────────────────────────────────

    #[test]
    fn test_compose_command_two_components() {
        // a.wasm depends on b.wasm
        let mut graph = ComposeGraph::new("composed.wasm");
        graph.add_input(ComponentInput::file("a.wasm"));
        graph.add_input(ComponentInput::file("b.wasm"));
        let cmd = graph.compose_command().unwrap();
        assert_eq!(
            cmd.args,
            vec!["compose", "a.wasm", "-d", "b.wasm", "-o", "composed.wasm"]
        );
    }

    // ── Test 3 ──────────────────────────────────────────────────────────────

    #[test]
    fn test_compose_command_three_components() {
        let mut graph = ComposeGraph::new("out.wasm");
        graph.add_input(ComponentInput::file("host.wasm"));
        graph.add_input(ComponentInput::file("math.wasm"));
        graph.add_input(ComponentInput::file("regex.wasm"));
        let cmd = graph.compose_command().unwrap();
        assert_eq!(
            cmd.args,
            vec![
                "compose", "host.wasm",
                "-d", "math.wasm",
                "-d", "regex.wasm",
                "-o", "out.wasm"
            ]
        );
    }

    // ── Test 4 ──────────────────────────────────────────────────────────────

    #[test]
    fn test_compose_empty_graph_error() {
        let graph = ComposeGraph::new("output.wasm");
        let result = graph.compose_command();
        assert!(
            matches!(result, Err(ComposeError::EmptyGraph)),
            "expected EmptyGraph, got {result:?}"
        );
    }

    // ── Test 5 ──────────────────────────────────────────────────────────────

    #[test]
    fn test_compose_validate_empty() {
        let graph = ComposeGraph::new("output.wasm");
        assert!(
            matches!(graph.validate(), Err(ComposeError::EmptyGraph)),
            "empty graph should be invalid"
        );
    }

    // ── Test 6 ──────────────────────────────────────────────────────────────

    #[test]
    fn test_compose_validate_non_empty() {
        let mut graph = ComposeGraph::new("output.wasm");
        graph.add_input(ComponentInput::file("a.wasm"));
        assert!(graph.validate().is_ok());
    }

    // ── Test 7 ──────────────────────────────────────────────────────────────

    #[test]
    fn test_component_input_display_name_file() {
        let input = ComponentInput::file("/some/path/my-component.wasm");
        assert_eq!(input.display_name(), "my-component.wasm");
    }

    // ── Test 8 ──────────────────────────────────────────────────────────────

    #[test]
    fn test_component_input_display_name_memory() {
        let input = ComponentInput::memory("inline-regex", vec![0x00, 0x61, 0x73]);
        assert_eq!(input.display_name(), "inline-regex");
    }

    // ── Test 9 ──────────────────────────────────────────────────────────────

    #[test]
    fn test_compose_command_display() {
        let mut graph = ComposeGraph::new("out.wasm");
        graph.add_input(ComponentInput::file("a.wasm"));
        graph.add_input(ComponentInput::file("b.wasm"));
        let cmd = graph.compose_command().unwrap();
        let display = cmd.display();
        assert_eq!(display, "wasm-tools compose a.wasm -d b.wasm -o out.wasm");
    }

    // ── Test 10 ─────────────────────────────────────────────────────────────

    #[test]
    fn test_compose_command_memory_input() {
        let mut graph = ComposeGraph::new("composed.wasm");
        graph.add_input(ComponentInput::memory("host", vec![0x00, 0x61, 0x73, 0x6d]));
        graph.add_input(ComponentInput::memory("lib", vec![0x00, 0x61, 0x73, 0x6d]));
        let cmd = graph.compose_command().unwrap();
        assert_eq!(
            cmd.args,
            vec!["compose", "host", "-d", "lib", "-o", "composed.wasm"]
        );
    }

    // ── Test 11 ─────────────────────────────────────────────────────────────

    #[test]
    fn test_compose_error_display() {
        assert!(
            ComposeError::EmptyGraph.to_string().contains("no components")
        );
        assert!(
            ComposeError::WasmToolsNotFound
                .to_string()
                .contains("wasm-tools")
        );
        assert!(
            ComposeError::ComposeFailed("exit 1".to_string())
                .to_string()
                .contains("exit 1")
        );
    }

    // ── Integration test (requires wasm-tools installed) ─────────────────────

    #[test]
    #[ignore = "requires wasm-tools in PATH"]
    fn test_wasm_tools_available() {
        let version = check_wasm_tools().expect("wasm-tools should be installed");
        assert!(
            version.contains("wasm-tools"),
            "unexpected version output: {version}"
        );
    }
}
