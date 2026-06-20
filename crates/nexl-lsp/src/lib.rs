//! `nexl-lsp` — Language Server Protocol implementation for Nexl.
//!
//! Provides a `tower-lsp`-based LSP server with diagnostics, hover,
//! go-to-definition, and completion support for Nexl source files.

use dashmap::DashMap;
use nexl_ast::module::parse_module_decl;
use nexl_ast::printer::PrettyPrinter;
use nexl_ast::{Atom, FileId, ImportDecl, ImportKind, Node, NodeKind, Span};
use nexl_errors::{Diagnostic as NexlDiagnostic, Severity as NexlSeverity};
use nexl_infer::{Env, InferState};
use nexl_types::{EffectRow, Type, TypeError, TypeErrorKind, TypeVar};
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};

/// The LSP backend holding client handle and document state.
#[derive(Debug)]
pub struct Backend {
    /// Handle for sending notifications/requests to the client.
    client: Client,
    /// Open documents keyed by URI.
    documents: DashMap<Url, TextDocumentItem>,
}

impl Backend {
    /// Create a new backend with the given client handle.
    pub fn new(client: Client) -> Self {
        Self {
            client,
            documents: DashMap::new(),
        }
    }

    /// Get the text of an open document, if it exists.
    pub fn get_document_text(&self, uri: &Url) -> Option<String> {
        self.documents.get(uri).map(|doc| doc.text.clone())
    }

    async fn publish_diagnostics(&self, uri: &Url, source: &str, version: Option<i32>) {
        let diagnostics = collect_diagnostics(uri, source);
        self.client
            .publish_diagnostics(uri.clone(), diagnostics, version)
            .await;
    }
}

fn collect_diagnostics(uri: &Url, source: &str) -> Vec<Diagnostic> {
    // Skip type-checking for project.nx manifest files — they contain
    // pure data (heterogeneous maps) that would produce false type errors.
    if uri.path().ends_with("project.nx") {
        return Vec::new();
    }
    let file_path = uri.to_file_path().ok();
    match nexl_reader::read(source, FileId(0)) {
        Ok(nodes) => type_check_diagnostics(&nodes, source, file_path.as_deref()),
        Err(diag) => vec![reader_diagnostic_to_lsp(&diag, uri, source)],
    }
}

/// Register `deftype` declarations from an imported source file into `env`.
///
/// Loads the file at `path`, parses it, and calls `register_deftype` for every
/// `deftype` form found — making imported ADT constructors visible to
/// `check_constructor_pattern` in the importing file.
fn register_imported_deftypes(env: Env, path: &Path) -> Env {
    let Ok(source) = std::fs::read_to_string(path) else {
        return env;
    };
    let Ok(nodes) = nexl_reader::read(&source, FileId::SYNTHETIC) else {
        return env;
    };
    let mut env = env;
    for node in &nodes {
        if list_head_is(node, "deftype") {
            if let Ok(decl) = nexl_infer::parse_deftype(node) {
                env = nexl_infer::register_deftype(&env, decl);
            }
        }
    }
    env
}

/// Load deftype and defn/def types from a source file into `env`.
///
/// Used by hover to make imported names visible in the infer environment.
fn load_module_env(env: Env, path: &Path, state: &mut InferState) -> Env {
    let Ok(source) = std::fs::read_to_string(path) else {
        return env;
    };
    let Ok(nodes) = nexl_reader::read(&source, FileId::SYNTHETIC) else {
        return env;
    };
    let mut env = env;
    for node in &nodes {
        if list_head_is(node, "deftype") {
            if let Ok(decl) = nexl_infer::parse_deftype(node) {
                env = nexl_infer::register_deftype(&env, decl);
            }
        } else if defn_name_and_docstring(node).is_some() {
            let node_for_infer = defn_node_for_infer(node);
            if let Ok((_, _, new_env)) = nexl_infer::infer_defn(node_for_infer.as_ref(), &env, state) {
                env = new_env;
            }
        } else if def_name_node(node).is_some() {
            if let Ok((_, _, new_env)) = nexl_infer::infer_def(node, &env, state) {
                env = new_env;
            }
        }
    }
    env
}

/// Search for a `defn` docstring by name in a source file on disk.
fn find_defn_docstring_in_path(path: &Path, name: &str) -> Option<String> {
    let source = std::fs::read_to_string(path).ok()?;
    let nodes = nexl_reader::read(&source, FileId::SYNTHETIC).ok()?;
    find_defn_docstring(&nodes, name)
}

/// Resolve the file paths for all imports declared in `nodes`, relative to `file_path`.
fn resolve_import_paths(nodes: &[Node], file_path: &Path) -> Vec<PathBuf> {
    let Some(imports) = extract_module_imports(nodes) else {
        return Vec::new();
    };
    let Some(ctx) = resolve_project_context(file_path) else {
        return Vec::new();
    };
    imports
        .iter()
        .filter_map(|imp| resolve_module_to_file_path(&imp.module_path, &ctx))
        .collect()
}

fn type_check_diagnostics(nodes: &[Node], source: &str, file_path: Option<&Path>) -> Vec<Diagnostic> {
    let mut env = Env::new();
    let mut state = InferState::new();

    // If this file is a module with imports, load deftype declarations from
    // each imported source file so ADT constructors are visible in patterns.
    if let (Some(path), Some(imports)) = (file_path, extract_module_imports(nodes)) {
        if let Some(ctx) = resolve_project_context(path) {
            for import in &imports {
                if let Some(abs) = resolve_module_to_file_path(&import.module_path, &ctx) {
                    env = register_imported_deftypes(env, &abs);
                }
            }
        }
    }

    for node in nodes {
        // Skip module infrastructure forms.
        if list_head_is(node, "module") || list_head_is(node, "import") {
            continue;
        }
        // Register deftype declarations so record/ADT types are known.
        if list_head_is(node, "deftype") {
            if let Ok(decl) = nexl_infer::parse_deftype(node) {
                env = nexl_infer::register_deftype(&env, decl);
            }
            continue;
        }
        let result = if list_head_is(node, "def") {
            match nexl_infer::infer_def(node, &env, &mut state) {
                Ok((_name, _ty, new_env)) => {
                    env = new_env;
                    Ok(())
                }
                Err(err) => Err(err),
            }
        } else if list_head_is(node, "defn") {
            let node_for_infer = defn_node_for_infer(node);
            match nexl_infer::infer_defn(node_for_infer.as_ref(), &env, &mut state) {
                Ok((_name, _ty, new_env)) => {
                    env = new_env;
                    Ok(())
                }
                Err(err) => Err(err),
            }
        } else if list_head_is(node, "defhandler") {
            match nexl_infer::infer_defhandler(node, &env, &mut state) {
                Ok(new_env) => {
                    env = new_env;
                    Ok(())
                }
                Err(err) => Err(err),
            }
        } else {
            nexl_infer::synth(node, &env, &mut state).map(|_| ())
        };

        if let Err(err) = result {
            state.push_error(err);
        }
    }

    // The type checker doesn't have stdlib type signatures, so UnboundVariable
    // errors cascade into Mismatch/ArityMismatch noise.  Only surface
    // MalformedForm errors (structural problems the checker CAN detect
    // without stdlib) until the inference env is populated with stdlib types.
    let has_unbound = state
        .errors
        .iter()
        .chain(state.warnings.iter())
        .any(|e| matches!(e.kind, TypeErrorKind::UnboundVariable { .. }));

    let mut diagnostics = Vec::new();
    for err in &state.errors {
        if has_unbound && !matches!(err.kind, TypeErrorKind::MalformedForm { .. }) {
            continue;
        }
        diagnostics.push(type_error_to_lsp(err, DiagnosticSeverity::ERROR, source));
    }
    for warning in &state.warnings {
        if has_unbound && !matches!(warning.kind, TypeErrorKind::MalformedForm { .. }) {
            continue;
        }
        diagnostics.push(type_error_to_lsp(
            warning,
            DiagnosticSeverity::WARNING,
            source,
        ));
    }
    diagnostics
}

fn reader_diagnostic_to_lsp(diag: &NexlDiagnostic, uri: &Url, source: &str) -> Diagnostic {
    let (range, related_information) = match diag.labels.split_first() {
        Some((primary, rest)) => {
            let primary_range = span_to_range(source, primary.span);
            let related = rest
                .iter()
                .map(|label| DiagnosticRelatedInformation {
                    location: Location {
                        uri: uri.clone(),
                        range: span_to_range(source, label.span),
                    },
                    message: label.message.clone(),
                })
                .collect::<Vec<_>>();
            (primary_range, related)
        }
        None => (
            Range::new(Position::new(0, 0), Position::new(0, 0)),
            Vec::new(),
        ),
    };

    let mut message = diag.message.clone();
    if let Some(help) = &diag.help {
        message.push_str("\nhelp: ");
        message.push_str(help);
    }
    for note in &diag.notes {
        message.push_str("\nnote: ");
        message.push_str(note);
    }

    Diagnostic {
        range,
        severity: Some(map_severity(diag.severity)),
        code: diag
            .code
            .as_ref()
            .map(|code| NumberOrString::String(code.to_string())),
        source: Some("nexl-reader".to_string()),
        message,
        related_information: if related_information.is_empty() {
            None
        } else {
            Some(related_information)
        },
        ..Diagnostic::default()
    }
}

fn type_error_to_lsp(error: &TypeError, severity: DiagnosticSeverity, source: &str) -> Diagnostic {
    let range = match error.span {
        Some(span) if !span.is_synthetic() => span_to_range(source, span),
        _ => Range::new(Position::new(0, 0), Position::new(0, 0)),
    };
    let message = type_error_message(error);

    Diagnostic {
        range,
        severity: Some(severity),
        source: Some("nexl-infer".to_string()),
        message,
        related_information: None,
        ..Diagnostic::default()
    }
}

fn type_error_message(error: &TypeError) -> String {
    let base = match &error.kind {
        TypeErrorKind::Mismatch { expected, found } => {
            format!("expected {expected} but got {found}")
        }
        TypeErrorKind::InfiniteType { var, ty } => {
            format!("infinite type: {} = {ty}", Type::Var(*var))
        }
        TypeErrorKind::ArityMismatch { expected, found } => {
            format!("function arity mismatch: expected {expected} parameter(s), found {found}")
        }
        TypeErrorKind::UnboundVariable { name } => format!("unbound variable: {name}"),
        TypeErrorKind::MalformedForm { description } => format!("malformed form: {description}"),
    };
    match &error.help {
        Some(help) => format!("{base}\nhelp: {help}"),
        None => base,
    }
}

fn map_severity(severity: NexlSeverity) -> DiagnosticSeverity {
    match severity {
        NexlSeverity::Error => DiagnosticSeverity::ERROR,
        NexlSeverity::Warning => DiagnosticSeverity::WARNING,
        NexlSeverity::Note => DiagnosticSeverity::INFORMATION,
        NexlSeverity::Help => DiagnosticSeverity::HINT,
    }
}

fn span_to_range(source: &str, span: Span) -> Range {
    let start = offset_to_position(source, span.start as usize);
    let end = offset_to_position(source, span.end() as usize);
    Range::new(start, end)
}

fn offset_to_position(source: &str, offset: usize) -> Position {
    let mut line: u32 = 0;
    let mut col: u32 = 0;
    let mut idx: usize = 0;
    let offset = offset.min(source.len());
    for ch in source.chars() {
        let next = idx + ch.len_utf8();
        if next > offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 0;
        } else {
            col += ch.len_utf16() as u32;
        }
        idx = next;
    }
    Position::new(line, col)
}

fn position_to_offset(source: &str, position: Position) -> usize {
    let target_line = position.line;
    let target_col = position.character;
    let mut line: u32 = 0;
    let mut col: u32 = 0;
    let mut idx: usize = 0;
    for ch in source.chars() {
        if line > target_line {
            break;
        }
        if line == target_line {
            if col >= target_col {
                break;
            }
            if ch == '\n' {
                break;
            }
            let next_col = col + ch.len_utf16() as u32;
            if next_col > target_col {
                break;
            }
            col = next_col;
            idx += ch.len_utf8();
            continue;
        }

        if ch == '\n' {
            line += 1;
            col = 0;
        }
        idx += ch.len_utf8();
    }
    idx.min(source.len())
}

fn span_contains(span: Span, offset: usize) -> bool {
    if span.is_synthetic() {
        return false;
    }
    let start = span.start as usize;
    let end = span.end() as usize;
    offset >= start && offset < end
}

/// Return documentation for a stdlib function by looking it up in the parsed
/// Nexl declaration files (the single source of truth for stdlib docs).
fn stdlib_doc(name: &str) -> Option<&'static str> {
    stdlib_docs().get(name).map(|s| s.as_str())
}

/// Lazily build and cache the stdlib documentation map from the embedded
/// Nexl declaration files in `nexl-stdlib/nexl/`.
///
/// Keys are:
/// - Unqualified function names for builtins: `"+"`, `"map"`, `"get"`, …
/// - Qualified names for module functions: `"io/println"`, `"str/split"`, …
/// - Both forms for module functions (unqualified as fallback).
fn stdlib_docs() -> &'static HashMap<String, String> {
    static DOCS: OnceLock<HashMap<String, String>> = OnceLock::new();
    DOCS.get_or_init(|| {
        let mut map = HashMap::new();
        for (module_name, src) in nexl_stdlib::nexl_declaration_sources() {
            let nodes = match nexl_reader::read(src, FileId(0)) {
                Ok(nodes) => nodes,
                Err(_) => continue,
            };
            for node in &nodes {
                // Extract docstring from (defn name "docstring" ...) forms.
                if let Some((name_node, Some(doc))) = defn_name_and_docstring(node) {
                    if let Some(fn_name) = symbol_name(name_node) {
                        if *module_name == "builtins" {
                            // Builtins use unqualified keys only.
                            map.insert(fn_name, doc);
                        } else {
                            // Module functions: store as "module/fn" and also
                            // unqualified (fallback for imported names).
                            let qualified = format!("{module_name}/{fn_name}");
                            map.insert(fn_name, doc.clone());
                            map.insert(qualified, doc);
                        }
                    }
                }
                // Also handle (def name "docstring") for constants like math/pi.
                if let Some(name_node) = def_name_node(node) {
                    if let NodeKind::List(items) = &node.kind {
                        if let Some(doc_node) = items.get(2) {
                            if let NodeKind::Atom(Atom::Str(doc)) = &doc_node.kind {
                                if let Some(fn_name) = symbol_name(name_node) {
                                    let qualified = format!("{module_name}/{fn_name}");
                                    map.insert(fn_name, doc.clone());
                                    map.insert(qualified, doc.clone());
                                }
                            }
                        }
                    }
                }
            }
        }
        map
    })
}

/// Return documentation for a built-in type name, if known.
fn builtin_type_doc(name: &str) -> Option<&'static str> {
    Some(match name {
        "Int" => r#"**`Int`** — 64-bit signed integer.

The default integer type in Nexl. Represents whole numbers from
−9,223,372,036,854,775,808 to 9,223,372,036,854,775,807.

## Literals
```nexl
42
-7
0
1_000_000   ; underscores allowed as separators
```

## Arithmetic
```nexl
(+ 1 2)      ; => 3
(- 10 3)     ; => 7
(* 4 5)      ; => 20
(/ 10 2)     ; => 5
(mod 10 3)   ; => 1
```

## See Also
- `Float` — floating-point numbers
- `Int32`, `Int64` — fixed-width variants
- `conv/->int` — convert from Str or Float
"#,
        "Float" => r#"**`Float`** — 64-bit IEEE 754 double-precision floating-point number.

The default floating-point type. Provides ~15 significant decimal digits
of precision. Use `Ratio` when exact arithmetic is needed.

## Literals
```nexl
3.14
-0.5
1.0e10    ; scientific notation
0.001
```

## Arithmetic
```nexl
(+ 1.5 2.5)     ; => 4.0
(* 2.0 3.14)    ; => 6.28
(/ 1.0 3.0)     ; => 0.3333...
(math/sqrt 2.0) ; => 1.4142...
```

## See Also
- `Int` — integer type
- `Ratio` — exact rational arithmetic
- `F32`, `F64` — fixed-width variants
- `conv/->float` — convert from Str or Int
"#,
        "Str" => r#"**`Str`** — UTF-8 encoded string.

Strings are immutable and support full Unicode. String literals use
double quotes; escape sequences include `\n`, `\t`, `\\`, `\"`.

## Literals
```nexl
"hello"
"Hello, world!\n"
"unicode: \u00e9"
""   ; empty string
```

## Common operations
```nexl
(str/split "a,b,c" ",")    ; => ["a" "b" "c"]
(str/join ["a" "b"] ", ")  ; => "a, b"
(str/upper "hello")        ; => "HELLO"
(str/trim "  hi  ")        ; => "hi"
(count "hello")             ; => 5
(str "value: " 42)          ; coerce + concat
```

## See Also
- `str` module — full string function reference
- `Char` — individual Unicode scalar value
- `conv/->str` — convert any value to Str
"#,
        "Bool" => r#"**`Bool`** — Boolean value: `true` or `false`.

Nexl conditionals require strict `Bool` — there is no truthy/falsy
coercion (ADR-004). Comparisons and logical operators all return `Bool`.

## Literals
```nexl
true
false
```

## Operations
```nexl
(and true false)   ; => false
(or  true false)   ; => true
(not true)         ; => false

(= 1 1)    ; => true
(< 1 2)    ; => true
(> 3 1)    ; => true
```

## See Also
- `if` — requires Bool test
- `when`, `unless` — conditional forms
"#,
        "Unit" => r#"**`Unit`** — The unit type; the single value returned by side-effecting expressions.

`Unit` has exactly one value, also written `Unit`. It is the return type
of functions that are called for their side effects (e.g. `io/println`,
`each`). Nexl uses `Unit` rather than `nil` or `void` (ADR-001).

## Usage
```nexl
(io/println "hello")   ; => Unit
(each [x [1 2 3]] x)   ; => Unit

;; Type annotation
(defn greet [name] :-> Unit
  (io/println (str "Hello, " name "!")))
```

## See Also
- `Never` — the bottom type (functions that never return)
- `do` — returns the last expression's value
"#,
        "Vec" => r#"**`Vec`** — Persistent ordered vector (generic).

`(Vec a)` is a generic vector of elements of type `a`. Vectors are
immutable — operations return new vectors. Literal syntax uses `[...]`.

## Literals
```nexl
[1 2 3]
["a" "b" "c"]
[]            ; empty vector
```

## Common operations
```nexl
(count [1 2 3])            ; => 3
(get [10 20 30] 1)         ; => 20
(append [1 2] 3)           ; => [1 2 3]
(first [1 2 3])            ; => 1
(rest  [1 2 3])            ; => [2 3]
(map (fn [x] (* x 2)) v)   ; => doubled vector
(filter (fn [x] (> x 0)) v)
(slice v 1 3)              ; => subvector [1, 3)
```

## See Also
- `Set` — unordered, unique elements
- `Map` — key-value pairs
- `for` — list comprehension producing a Vec
"#,
        "Map" => r#"**`Map`** — Persistent hash map from keys to values (generic).

`(Map k v)` maps keys of type `k` to values of type `v`. Maps are
immutable. Literal syntax uses `{:key value ...}`.

## Literals
```nexl
{:name "Alice" :age 30}
{}   ; empty map
```

## Common operations
```nexl
(get m :name)              ; => "Alice"  (or nil if missing)
(put m :city "Paris")      ; => new map with :city added
(remove m :age)            ; => new map without :age
(keys m)                   ; => [:name :age]
(vals m)                   ; => ["Alice" 30]
(contains? m :name)        ; => true
(count m)                  ; => 2
```

## See Also
- `Vec` — ordered collection
- `Set` — unique-element collection
- `entries` — get `[[key val] ...]` pairs
"#,
        "Set" => r#"**`Set`** — Persistent hash set of unique elements (generic).

`(Set a)` contains unique elements of type `a`. Set membership tests
are O(log n). Literal syntax uses `#{...}`.

## Literals
```nexl
#{1 2 3}
#{"a" "b"}
#{}   ; empty set
```

## Common operations
```nexl
(add s 4)               ; => new set with 4
(remove s 2)            ; => new set without 2
(contains? s 3)         ; => true
(count s)               ; => 3
(union s1 s2)
(intersection s1 s2)
(difference s1 s2)
```

## See Also
- `Vec` — ordered, allows duplicates
- `Map` — key-value pairs
"#,
        "Option" => r#"**`Option`** — A value that may or may not be present.

`(Option a)` is either `(Some value)` or `None`. Use `Option` to
represent nullable or optional data without null pointer exceptions.

## Constructors
```nexl
(Some 42)    ; present
None         ; absent
```

## Pattern matching
```nexl
(match (db/query ...)
  (Some row)  (:name row)
  _           "unknown")
```

## Common patterns
```nexl
;; Propagate None with ?
(defn find-user [id]
  (let [row (db/query conn id)?]
    (:name row)))

;; Default value
(match (env/get "PORT")
  (Some p)  (conv/->int p)
  _         8080)
```

## See Also
- `Result` — failure with an error message
- `match` — pattern matching on constructors
- `?` — propagate None/Err out of a function
"#,
        "Result" => r#"**`Result`** — A value representing success or failure.

`(Result a e)` is either `(Ok value)` or `(Err error)`. Use `Result`
for operations that can fail with a meaningful error message.

## Constructors
```nexl
(Ok 42)          ; success
(Err "not found") ; failure
```

## Pattern matching
```nexl
(match (http/get url)
  (Ok resp)  (http/body resp)
  (Err msg)  (do (log/error msg) ""))
```

## Error propagation with `?`
```nexl
(defn load-config []
  (let [raw  (io/read-file "config.json")?
        data (json/decode raw)?]
    (Ok data)))
```

## try/catch
```nexl
(try (json/decode input)
  (catch e
    (do (log/warn e) {})))
```

## See Also
- `Option` — absence without an error reason
- `?` — propagate Err out of a function
- `try` — catch and recover from errors
"#,
        "Fn" => r#"**`Fn`** — Function type.

`(Fn [param-types] -> return-type)` describes the type of a function.
Effect rows appear as `(Fn [A] -> B ! [Eff])`.

## Syntax in type annotations
```nexl
(Fn [Int] -> Int)
(Fn [Str Str] -> Bool)
(Fn [] -> Unit)
(Fn [Int] -> Int ! [Console])   ; with effect
```

## Usage
```nexl
;; Higher-order function
(defn apply-twice [f x]
  (f (f x)))

;; Storing functions
(def transform (fn [x] (* x 2)))
(apply-twice transform 3)   ; => 12
```

## See Also
- `fn` — create an anonymous function
- `defn` — define a named function
- `partial` — partial application
"#,
        "Ratio" => r#"**`Ratio`** — Exact rational number (arbitrary precision).

Represents a fraction as `numerator/denominator` in lowest terms.
Use `Ratio` when floating-point rounding is unacceptable (financial
calculations, exact geometry, etc.).

## Literals
```nexl
1/3
22/7
-5/4
```

## Operations
```nexl
(+ 1/3 1/6)    ; => 1/2  (exact)
(* 2/3 3/4)    ; => 1/2
(= 2/4 1/2)    ; => true  (auto-reduced)
```

## See Also
- `Float` — approximate floating-point
- `Int` — whole numbers
"#,
        "Char" => r#"**`Char`** — A single Unicode scalar value.

Represents a single Unicode code point. Char literals use the `\c`
syntax (backslash prefix).

## Literals
```nexl
\a
\newline
\space
\u00e9   ; é
```

## Usage
```nexl
(str/chars "hello")   ; => [\h \e \l \l \o]
(= \a \a)             ; => true
```

## See Also
- `Str` — sequence of Chars
- `str/chars` — split a string into chars
- `str/graphemes` — Unicode grapheme clusters
"#,
        "Keyword" => r#"**`Keyword`** — An interned symbolic constant, always prefixed with `:`.

Keywords evaluate to themselves, are compared by identity (fast), and
are commonly used as map keys, enum-like flags, and option names.

## Literals
```nexl
:name
:status
:ok
:not-found
```

## Usage
```nexl
(def m {:status :active :role :admin})
(get m :status)          ; => :active
(= :ok :ok)              ; => true
(contains? m :role)      ; => true
```

## See Also
- `Map` — keyword keys are idiomatic
- `Symbol` — unquoted name in source code
"#,
        "Symbol" => r#"**`Symbol`** — A named reference in Nexl source code.

Symbols name bindings, functions, and types. At the type level, `Symbol`
is the type of first-class symbol values (rare outside macros).

## Examples
```nexl
;; In macro contexts
(defmacro-syntax quote-name [x]
  (list 'Symbol (str x)))
```

## See Also
- `Keyword` — self-evaluating constant (`:name`)
- `defn`, `def` — bind symbols to values
"#,
        "Never" => r#"**`Never`** — The bottom type; the return type of expressions that never return.

A function returning `Never` either loops forever, calls `sys/exit`, or
always throws an error. `Never` is a subtype of every type, so a
`never`-typed branch is accepted anywhere.

## Usage
```nexl
(defn abort! [msg] :-> Never
  (io/println msg)
  (sys/exit 1))

;; The `abort!` branch is valid regardless of the surrounding type:
(if ok? result (abort! "fatal"))
```

## See Also
- `Unit` — returns normally but carries no information
- `sys/exit` — one way to produce a Never value
"#,
        "Any" => r#"**`Any`** — The top type; accepts any value without type checking.

`Any` is a dynamic escape hatch. Values of type `Any` bypass static
type checking — use sparingly, only at system boundaries (FFI, dynamic
data, deserialized JSON before validation).

## Usage
```nexl
;; JSON decode returns (Map Str Any) before you validate the shape
(def raw (json/decode text))

;; Cast back to a concrete type via pattern matching or contract
(defn parse-user [m :- (Map Str Any)] :-> User
  (User {:name (get m "name") :age (get m "age")}))
```

## See Also
- `Result` — prefer typed errors over dynamic Any
- `Option` — prefer typed optionality
"#,
        "Tuple" => r#"**`Tuple`** — A fixed-length heterogeneous product type.

`(Tuple A B ...)` holds exactly 2–8 values of potentially different types.
Tuple literals use `(#a b c)` syntax. Elements are accessed positionally.

## Literals
```nexl
(#1 "hello")          ; Tuple of Int and Str
(#true 42 "ok")       ; Tuple of Bool, Int, Str
```

## Pattern matching
```nexl
(match coord
  (#x y)  (io/println (str "x=" x " y=" y)))
```

## See Also
- `Vec` — homogeneous, variable-length
- `deftype` — named records are usually preferred over tuples
"#,
        // Fixed-width numeric types — grouped short docs
        "Int8"  => "**`Int8`** — 8-bit signed integer (−128 to 127). See also: `Int`, `Int16`, `Int32`, `Int64`.",
        "Int16" => "**`Int16`** — 16-bit signed integer (−32,768 to 32,767). See also: `Int`, `Int8`, `Int32`, `Int64`.",
        "Int32" => "**`Int32`** — 32-bit signed integer (−2,147,483,648 to 2,147,483,647). See also: `Int`, `Int64`.",
        "Int64" => "**`Int64`** — 64-bit signed integer. Alias for `Int`. See also: `Int`, `Int32`.",
        "U8"    => "**`U8`** — 8-bit unsigned integer (0 to 255). Commonly used for byte buffers. See also: `U16`, `U32`, `U64`.",
        "U16"   => "**`U16`** — 16-bit unsigned integer (0 to 65,535). See also: `U8`, `U32`, `U64`.",
        "U32"   => "**`U32`** — 32-bit unsigned integer (0 to 4,294,967,295). See also: `U8`, `U64`.",
        "U64"   => "**`U64`** — 64-bit unsigned integer (0 to 18,446,744,073,709,551,615). See also: `U8`, `U32`.",
        "F32"   => "**`F32`** — 32-bit IEEE 754 single-precision float (~7 significant digits). See also: `Float` (`F64`).",
        "F64"   => "**`F64`** — 64-bit IEEE 754 double-precision float. Alias for `Float`. See also: `F32`, `Float`.",
        _ => return None,
    })
}

/// Return documentation for a special form, if known.
fn special_form_doc(name: &str) -> Option<&'static str> {
    Some(match name {
        "defn" => r#"**`defn`** — Define a named function.

## Syntax
```nexl
(defn name [params] body)
(defn name "docstring" [params] body)
(defn name "docstring" [params] :requires [guards] body)
```

## Parameters
- `name` — function name (symbol)
- `[params]` — parameter vector; use `[& rest]` for variadic
- `body` — one or more expressions; last value is returned

## Examples
```nexl
(defn square [x] (* x x))
(square 4)   ; => 16

(defn greet
  "Return a greeting string."
  [name]
  (str "Hello, " name "!"))

(defn safe-div
  "Divide a by b, guarding against zero."
  [a b]
  :requires [(not (= b 0))]
  (/ a b))
```

## See Also
- `fn` — anonymous function literal
- `def` — bind a non-function value
"#,
        "def" => r#"**`def`** — Bind a value to a name in the current scope.

## Syntax
```nexl
(def name expr)
(def name "docstring" expr)
```

## Parameters
- `name` — symbol to bind
- `expr` — value expression (evaluated once)

## Examples
```nexl
(def pi 3.14159)
(def greeting "Hello, world!")
(def items [1 2 3 4 5])

(def max-retries "Number of HTTP retry attempts." 3)
```

## See Also
- `defn` — define a function
- `let` — bind locals within an expression
"#,
        "fn" => r#"**`fn`** — Create an anonymous function.

## Syntax
```nexl
(fn [params] body)
(fn [a & rest] body)
```

## Parameters
- `[params]` — parameter vector; `& rest` captures remaining args as a vector
- `body` — one or more expressions; last is returned

## Examples
```nexl
(def double (fn [x] (* x 2)))
(double 5)   ; => 10

(map (fn [x] (* x x)) [1 2 3 4])   ; => [1 4 9 16]

;; Immediately-invoked
((fn [x y] (+ x y)) 3 4)   ; => 7
```

## See Also
- `defn` — named function definition
- `partial` — partially apply a function
"#,
        "let" => r#"**`let`** — Bind local variables within an expression.

## Syntax
```nexl
(let [name1 val1
      name2 val2
      ...] body)
```

## Parameters
- Bindings are sequential — later bindings can refer to earlier ones.
- `body` — one or more expressions; last is returned.

## Examples
```nexl
(let [x 10
      y (* x 2)]
  (+ x y))   ; => 30

(let [msg (str "Hello, " name "!")]
  (io/println msg)
  msg)
```

## See Also
- `def` — top-level binding
- `do` — sequential forms without new bindings
"#,
        "if" => r#"**`if`** — Conditional branch.

## Syntax
```nexl
(if test then else)
```

## Parameters
- `test` — must be a `Bool` (no truthy/falsy coercion)
- `then` — evaluated when test is `true`
- `else` — evaluated when test is `false`; required

## Examples
```nexl
(if (> x 0) "positive" "non-positive")

(if (str/blank? input)
  (io/println "empty")
  (io/println input))
```

## See Also
- `when` — one-armed conditional (no else branch)
- `cond` — multi-way conditional
- `match` — pattern-based dispatch
"#,
        "do" => r#"**`do`** — Evaluate a sequence of expressions, returning the last.

## Syntax
```nexl
(do expr1 expr2 ... exprN)
```

## Parameters
- `expr1 ... exprN-1` — evaluated for side effects; return values discarded
- `exprN` — return value of the whole `do` block

## Examples
```nexl
(do
  (io/println "step 1")
  (io/println "step 2")
  42)   ; => 42

(if condition
  (do (log/info "branch A")
      (update-state!))
  (log/info "branch B"))
```

## See Also
- `let` — sequence + new bindings
- `when` — conditional do-block
"#,
        "when" => r#"**`when`** — Evaluate body expressions when test is true.

## Syntax
```nexl
(when test body...)
```

## Parameters
- `test` — `Bool` condition
- `body` — one or more expressions; returns last value, or `Unit` when skipped

## Examples
```nexl
(when (> count 0)
  (io/println "items found:")
  (io/println count))

(when (not valid?)
  (log/warn "invalid input")
  (sys/exit 1))
```

## See Also
- `unless` — negated form
- `if` — two-armed conditional with required else
"#,
        "unless" => r#"**`unless`** — Evaluate body when test is false.

## Syntax
```nexl
(unless test body...)
```

## Parameters
- `test` — `Bool` condition; body runs when `false`
- `body` — one or more expressions; returns last, or `Unit` when skipped

## Examples
```nexl
(unless (file-exists? path)
  (io/println "File not found")
  (sys/exit 1))

(unless (str/blank? name)
  (greet name))
```

## See Also
- `when` — positive form
- `if` — two-armed conditional
"#,
        "cond" => r#"**`cond`** — Multi-way conditional dispatch.

## Syntax
```nexl
(cond
  test1 result1
  test2 result2
  ...
  :else  default)
```

## Parameters
- Pairs of `test` / `result` evaluated in order; first true test wins.
- `:else` is the conventional catch-all (always true).

## Examples
```nexl
(cond
  (< x 0)  "negative"
  (= x 0)  "zero"
  :else     "positive")

(cond
  (str/starts-with? s "http")  :url
  (str/starts-with? s "/")     :path
  :else                        :name)
```

## See Also
- `if` — two-armed conditional
- `match` — pattern-based dispatch
"#,
        "match" => r#"**`match`** — Pattern match an expression against a list of patterns.

## Syntax
```nexl
(match expr
  pattern1 result1
  pattern2 result2
  ...
  _        default)
```

## Patterns
- `_` — wildcard, matches anything (does not bind)
- `name` — variable pattern, binds matched value
- `(Ctor args...)` — ADT constructor pattern
- `:keyword` or literal — exact value match

## Examples
```nexl
(match status
  (Some v)  (io/println v)
  _         (io/println "nothing"))

(match order-status
  (Draft)      "draft"
  (Placed)     "placed"
  (Cancelled)  "cancelled"
  _            "other")

(match (http/get url)
  (Ok resp)  (http/body resp)
  (Err msg)  (do (log/error msg) ""))
```

## See Also
- `cond` — predicate-based dispatch
- `if` — simple boolean branch
"#,
        "loop" => r#"**`loop`** — Loop with rebindable local variables.

## Syntax
```nexl
(loop [name1 init1
       name2 init2
       ...] body)
```

## Parameters
- Bindings provide initial values for loop variables.
- `body` — call `recur` with new values to continue, or return a value to exit.

## Examples
```nexl
(loop [i 0  acc 0]
  (if (= i 10)
    acc
    (recur (+ i 1) (+ acc i))))   ; => 45

;; Countdown
(loop [n 5]
  (when (> n 0)
    (io/println n)
    (recur (- n 1))))
```

## See Also
- `recur` — tail-recursive jump back to `loop`
- `each` — iterate over a collection
"#,
        "recur" => r#"**`recur`** — Tail-recursive jump to the enclosing `loop` or function.

## Syntax
```nexl
(recur new-val1 new-val2 ...)
```

## Parameters
- Must be in tail position within a `loop` or `fn`/`defn` body.
- Argument count must match the number of loop bindings or function parameters.

## Examples
```nexl
;; Sum via loop/recur
(loop [items [1 2 3 4 5]  acc 0]
  (if (= (count items) 0)
    acc
    (recur (rest items) (+ acc (first items)))))   ; => 15

;; Fibonacci
(defn fib-iter [n a b]
  (if (= n 0) a
    (recur (- n 1) b (+ a b))))
```

## See Also
- `loop` — creates a recur target with local bindings
"#,
        "deftype" => r#"**`deftype`** — Define an algebraic data type (record or sum type).

## Syntax
```nexl
;; Record type (single constructor)
(deftype Name {:field1 Type1  :field2 Type2})

;; Sum type (multiple variants, | separators required)
(deftype Name | Variant1 | (Variant2 field) | (Variant3 f1 f2))
```

## Examples
```nexl
;; Record
(deftype Point {:x Float :y Float})
(def p (Point {:x 1.0 :y 2.0}))
(:x p)   ; => 1.0

;; Sum type
(deftype Color | Red | Green | Blue)
(deftype Shape | (Circle Float) | (Rect Float Float))

;; With match
(match shape
  (Circle r)    (* 3.14159 r r)
  (Rect w h)    (* w h))
```

## See Also
- `match` — pattern match on ADT variants
- `defprotocol` — define an interface
"#,
        "defeffect" => r#"**`defeffect`** — Define an algebraic effect type.

## Syntax
```nexl
(defeffect Name
  (operation-name [param-types] return-type)
  ...)
```

## Examples
```nexl
(defeffect Log
  (log-line [Str] Unit))

(defeffect State
  (get-state [] Int)
  (put-state [Int] Unit))
```

## See Also
- `handle` — provide an effect handler
- `defprotocol` — interface without effect semantics
"#,
        "defprotocol" => r#"**`defprotocol`** — Define a protocol (structural interface).

## Syntax
```nexl
(defprotocol Name
  (method-name [self param-types] return-type)
  ...)
```

## Examples
```nexl
(defprotocol Printable
  (print-self [self] Unit))

(defprotocol Comparable
  (compare-to [self other] Int))
```

## See Also
- `deftype` — define a concrete type
- `defeffect` — algebraic effect signature
"#,
        "handle" => r#"**`handle`** — Handle algebraic effects raised within an expression.

## Syntax
```nexl
(handle expr
  (EffectName/operation [params] k body)
  ...)
```

## Parameters
- `expr` — expression that may raise effects
- Each clause names the effect and operation, binds params and the continuation `k`
- Call `(k value)` to resume; omit to abort

## Examples
```nexl
(handle (log-something)
  (Log/log-line [msg] k
    (io/println (str "[LOG] " msg))
    (k Unit)))
```

## See Also
- `defeffect` — declare the effect type
"#,
        "module" => r#"**`module`** — Declare the current file's module identity, imports, and exports.

## Syntax
```nexl
(module name
  :imports [[mod :as alias] ...]
  :exports [sym1 sym2 ...]
  :performs [EffectName ...])
```

## Parameters
- `name` — fully-qualified module name (e.g. `market.catalog.domain`)
- `:imports` — list of `[module-path :as alias]` pairs
- `:exports` — symbols to expose to other modules
- `:performs` — effects this module may raise (for effect checking)

## Examples
```nexl
(module market.catalog.domain
  :imports [[nexl.stdlib.db   :as db]
            [nexl.stdlib.json :as json]]
  :exports [create-product get-catalog]
  :performs [Db])
```

## See Also
- `import` — standalone import form
"#,
        "import" => r#"**`import`** — Import a module (standalone form).

## Syntax
```nexl
(import module-path :as alias)
(import module-path)
```

## Parameters
- `module-path` — dotted module path or stdlib name
- `:as alias` — local alias for the module

## Examples
```nexl
(import nexl.stdlib.json :as json)
(json/encode {:a 1})

(import my.utils.string)
```

## See Also
- `module` — full module declaration with exports and effects
"#,
        "try" => r#"**`try`** — Evaluate an expression, catching any errors.

## Syntax
```nexl
(try expr
  (catch pattern body))
```

## Parameters
- `expr` — expression that may return `(Err ...)` or propagate an error
- `(catch pattern body)` — pattern matched against the error; body is the recovery expression

## Examples
```nexl
(try (db/query conn "SELECT ...")
  (catch msg
    (do (log/error msg) [])))

(try (json/decode raw)
  (catch e
    (do (log/warn (str "bad JSON: " e))
        {})))
```

## See Also
- `?` — propagate error without catching
- `match` — pattern match on `Ok`/`Err` directly
"#,
        "for" => r#"**`for`** — List comprehension over one or more collections.

## Syntax
```nexl
(for [binding source
      :when pred
      ...] body)
```

## Parameters
- `binding source` — bind each element of `source`
- `:when pred` — optional filter; skips items where pred is false
- `body` — expression producing the result element

## Examples
```nexl
(for [x [1 2 3 4 5]] (* x x))
; => [1 4 9 16 25]

(for [x [1 2 3 4 5]
      :when (= (mod x 2) 0)]
  (* x x))
; => [4 16]

(for [x [1 2 3]
      y [10 20]]
  (+ x y))
; => [11 21 12 22 13 23]
```

## See Also
- `map` — transform a single collection
- `filter` — filter without transformation
- `each` — iterate for side effects
"#,
        "each" => r#"**`each`** — Iterate over a collection for side effects.

## Syntax
```nexl
(each [binding collection] body...)
```

## Parameters
- `binding` — symbol bound to each element
- `collection` — any `Vec` or `Set`
- `body` — one or more expressions evaluated for side effects; returns `Unit`

## Examples
```nexl
(each [item [1 2 3]]
  (io/println item))
; prints 1, 2, 3

(each [user users]
  (log/info (str "processing " (:name user)))
  (process-user! user))
```

## See Also
- `for` — list comprehension (returns a collection)
- `map` — transform a collection
- `loop` — loop with mutable locals
"#,
        _ => return None,
    })
}

fn hover_for_offset(nodes: &[Node], offset: usize, source: &str, file_path: Option<&Path>) -> Option<Hover> {
    let mut env = Env::new();
    let mut state = InferState::new();

    // Pre-load types from all imported modules so imported names are hover-able.
    let import_paths: Vec<PathBuf> = file_path
        .map(|p| resolve_import_paths(nodes, p))
        .unwrap_or_default();
    for path in &import_paths {
        env = load_module_env(env, path, &mut state);
    }

    for node in nodes {
        // Register deftype declarations so record/ADT types are known.
        if list_head_is(node, "deftype") {
            if let Ok(decl) = nexl_infer::parse_deftype(node) {
                env = nexl_infer::register_deftype(&env, decl);
            }
            continue;
        }

        if let Some((name_node, docstring)) = defn_name_and_docstring(node) {
            let is_target = span_contains(name_node.span, offset);
            let node_for_infer = defn_node_for_infer(node);
            match nexl_infer::infer_defn(node_for_infer.as_ref(), &env, &mut state) {
                Ok((name, ty, new_env)) => {
                    env = new_env;
                    if is_target {
                        return Some(build_hover(
                            &name,
                            &ty,
                            docstring.as_deref(),
                            name_node.span,
                            source,
                        ));
                    }
                }
                Err(err) => {
                    state.push_error(err);
                    // Inference failed (likely missing stdlib types), but we
                    // can still show a useful hover from the AST structure.
                    if is_target {
                        return Some(build_defn_fallback_hover(
                            node,
                            docstring.as_deref(),
                            name_node.span,
                            source,
                        ));
                    }
                }
            }
            continue;
        }

        if let Some(name_node) = def_name_node(node) {
            let is_target = span_contains(name_node.span, offset);
            match nexl_infer::infer_def(node, &env, &mut state) {
                Ok((name, ty, new_env)) => {
                    env = new_env;
                    if is_target {
                        return Some(build_hover(&name, &ty, None, name_node.span, source));
                    }
                }
                Err(err) => {
                    state.push_error(err);
                    if is_target && let Some(name) = symbol_name(name_node) {
                        return Some(build_simple_hover(&name, name_node.span, source));
                    }
                }
            }
            continue;
        }

        if let Some(name_node) = defhandler_name_node(node) {
            let is_target = span_contains(name_node.span, offset);
            match nexl_infer::infer_defhandler(node, &env, &mut state) {
                Ok(new_env) => {
                    env = new_env;
                    if is_target && let Some(name) = symbol_name(name_node) {
                        return Some(build_simple_hover(&name, name_node.span, source));
                    }
                }
                Err(err) => {
                    state.push_error(err);
                    if is_target && let Some(name) = symbol_name(name_node) {
                        return Some(build_simple_hover(&name, name_node.span, source));
                    }
                }
            }
            continue;
        }

        let _ = nexl_infer::synth(node, &env, &mut state);
    }

    // Fall through: cursor is not on a definition-site name.
    // Try to provide hover for a usage-site symbol.
    let sym_node = find_symbol_at_offset(nodes, offset)?;
    let name = symbol_name(sym_node)?;
    let span = sym_node.span;

    // 1. Check inference env for user-defined bindings (includes imported names)
    if let Some(scheme) = env.lookup(&name) {
        let ty = scheme.instantiate(&mut state.supply);
        // Try to find a docstring in the current file, then imported files.
        let docstring = find_defn_docstring(nodes, &name).or_else(|| {
            import_paths
                .iter()
                .find_map(|p| find_defn_docstring_in_path(p, &name))
        });
        let doc = docstring.as_deref().or_else(|| stdlib_doc(&name));
        return Some(build_hover(&name, &ty, doc, span, source));
    }

    // 2. Check if it's a user-defined defn (inference may have failed)
    if let Some(defn_node) = find_defn_node(nodes, &name) {
        let docstring = find_defn_docstring(nodes, &name);
        return Some(build_defn_fallback_hover(
            defn_node,
            docstring.as_deref(),
            span,
            source,
        ));
    }

    // 3. Check stdlib documentation
    if let Some(doc) = stdlib_doc(&name) {
        return Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: format!("```nexl\n{name}\n```\n\n{doc}"),
            }),
            range: Some(span_to_range(source, span)),
        });
    }

    // 4. Check special form documentation
    if let Some(doc) = special_form_doc(&name) {
        return Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: format!("```nexl\n{name}\n```\n\n{doc}"),
            }),
            range: Some(span_to_range(source, span)),
        });
    }

    // 5. Check built-in type documentation
    if let Some(doc) = builtin_type_doc(&name) {
        return Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: doc.to_string(),
            }),
            range: Some(span_to_range(source, span)),
        });
    }

    None
}

/// Find the docstring for a defn with the given name.
fn find_defn_docstring(nodes: &[Node], name: &str) -> Option<String> {
    for node in nodes {
        if let Some((name_node, docstring)) = defn_name_and_docstring(node)
            && symbol_name(name_node).as_deref() == Some(name)
        {
            return docstring;
        }
    }
    None
}

/// Find the defn node that defines the given name.
fn find_defn_node<'a>(nodes: &'a [Node], name: &str) -> Option<&'a Node> {
    for node in nodes {
        if let Some((name_node, _)) = defn_name_and_docstring(node)
            && symbol_name(name_node).as_deref() == Some(name)
        {
            return Some(node);
        }
    }
    None
}

/// Build a hover for a `defn` when type inference failed.
/// Extracts name and params from the AST.
fn build_defn_fallback_hover(
    node: &Node,
    docstring: Option<&str>,
    span: Span,
    source: &str,
) -> Hover {
    let NodeKind::List(items) = &node.kind else {
        return build_simple_hover("defn", span, source);
    };
    let name = items
        .get(1)
        .and_then(symbol_name)
        .unwrap_or_else(|| "?".to_string());

    // Find the parameter vector (skip optional docstring)
    let param_idx = if matches!(
        items.get(2),
        Some(Node {
            kind: NodeKind::Atom(Atom::Str(_)),
            ..
        })
    ) {
        3
    } else {
        2
    };

    let params = items.get(param_idx).and_then(|n| {
        if let NodeKind::Vector(elems) = &n.kind {
            let names: Vec<String> = elems.iter().filter_map(symbol_name).collect();
            Some(names.join(" "))
        } else {
            None
        }
    });

    let signature = match params {
        Some(p) => format!("(defn {name} [{p}] ...)",),
        None => format!("(defn {name} ...)",),
    };

    let mut value = format!("```nexl\n{signature}\n```");
    if let Some(doc) = docstring
        && !doc.is_empty()
    {
        value.push_str("\n\n");
        value.push_str(doc);
    }

    Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value,
        }),
        range: Some(span_to_range(source, span)),
    }
}

/// Build a minimal hover showing just the name.
fn build_simple_hover(name: &str, span: Span, source: &str) -> Hover {
    Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: format!("```nexl\n{name}\n```"),
        }),
        range: Some(span_to_range(source, span)),
    }
}

/// Collect all `TypeVar`s in a type, in order of first appearance (depth-first).
fn collect_type_vars_ordered(ty: &Type, out: &mut Vec<TypeVar>) {
    match ty {
        Type::Var(tv) => {
            if !out.contains(tv) {
                out.push(*tv);
            }
        }
        Type::Fn {
            params,
            ret,
            effects,
        } => {
            for p in params {
                collect_type_vars_ordered(p, out);
            }
            collect_type_vars_ordered(ret, out);
            collect_effect_row_vars(effects, out);
        }
        Type::Adt { args, .. } => {
            for arg in args {
                collect_type_vars_ordered(arg, out);
            }
        }
        Type::Record { fields, .. } => {
            for (_, field_ty) in fields {
                collect_type_vars_ordered(field_ty, out);
            }
        }
        Type::Tuple(items) => {
            for item in items {
                collect_type_vars_ordered(item, out);
            }
        }
        Type::Vec(elem) | Type::Set(elem) => collect_type_vars_ordered(elem, out),
        Type::Map { key, val } => {
            collect_type_vars_ordered(key, out);
            collect_type_vars_ordered(val, out);
        }
        _ => {} // primitives have no vars
    }
}

fn collect_effect_row_vars(_effects: &EffectRow, _out: &mut Vec<TypeVar>) {
    // Effect rows use string names, not TypeVars — nothing to collect.
}

/// Render a type to string with clean variable names (`a`, `b`, `c`, ...).
fn prettify_type(ty: &Type) -> String {
    let mut vars = Vec::new();
    collect_type_vars_ordered(ty, &mut vars);
    if vars.is_empty() {
        return ty.to_string();
    }
    // Build a mapping: TypeVar → clean name
    let names: std::collections::HashMap<TypeVar, String> = vars
        .into_iter()
        .enumerate()
        .map(|(i, tv)| (tv, var_name(i)))
        .collect();
    render_type(ty, &names)
}

/// Generate a clean variable name: 0→"a", 1→"b", ..., 25→"z", 26→"a1", ...
fn var_name(index: usize) -> String {
    let letter = (b'a' + (index % 26) as u8) as char;
    if index < 26 {
        letter.to_string()
    } else {
        format!("{letter}{}", index / 26)
    }
}

/// Render a type using the given variable name mapping.
fn render_type(ty: &Type, names: &std::collections::HashMap<TypeVar, String>) -> String {
    match ty {
        Type::Var(tv) => names.get(tv).cloned().unwrap_or_else(|| format!("t{}", tv.0)),
        Type::Fn {
            params,
            ret,
            effects,
        } => {
            let mut out = String::from("(Fn [");
            for (i, p) in params.iter().enumerate() {
                if i > 0 {
                    out.push(' ');
                }
                out.push_str(&render_type(p, names));
            }
            out.push_str("] -> ");
            out.push_str(&render_type(ret, names));
            if !effects.is_empty() {
                out.push_str(" ! [");
                for (i, eff) in effects.effects.iter().enumerate() {
                    if i > 0 {
                        out.push(' ');
                    }
                    out.push_str(eff);
                }
                if let Some(tail) = &effects.tail {
                    if !effects.effects.is_empty() {
                        out.push(' ');
                    }
                    out.push_str("| ");
                    out.push_str(tail);
                }
                out.push(']');
            }
            out.push(')');
            out
        }
        Type::Adt { name, args } => {
            if args.is_empty() {
                name.clone()
            } else {
                let mut out = format!("({name}");
                for arg in args {
                    out.push(' ');
                    out.push_str(&render_type(arg, names));
                }
                out.push(')');
                out
            }
        }
        Type::Record { name, .. } => name.clone(),
        Type::Tuple(items) => {
            let mut out = String::from("(Tuple");
            for item in items {
                out.push(' ');
                out.push_str(&render_type(item, names));
            }
            out.push(')');
            out
        }
        Type::Vec(elem) => format!("(Vec {})", render_type(elem, names)),
        Type::Map { key, val } => {
            format!("(Map {} {})", render_type(key, names), render_type(val, names))
        }
        Type::Set(elem) => format!("(Set {})", render_type(elem, names)),
        // All primitives: delegate to Display
        _ => ty.to_string(),
    }
}

fn build_hover(name: &str, ty: &Type, docstring: Option<&str>, span: Span, source: &str) -> Hover {
    let pretty = prettify_type(ty);
    let mut value = format!("```nexl\n{name} : {pretty}\n```");
    match docstring {
        Some(doc) if !doc.is_empty() => {
            value.push_str("\n\n");
            value.push_str(doc);
        }
        _ => {}
    }
    Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value,
        }),
        range: Some(span_to_range(source, span)),
    }
}

fn defn_name_and_docstring(node: &Node) -> Option<(&Node, Option<String>)> {
    let NodeKind::List(items) = &node.kind else {
        return None;
    };
    let head = symbol_name(items.first()?)?;
    let is_defn = head == "defn";
    let is_macro = head == "defmacro-syntax" || head == "defmacro" || head == "defmacro-elab";
    if !is_defn && !is_macro {
        return None;
    }
    let name_node = items.get(1)?;
    match &name_node.kind {
        NodeKind::Atom(Atom::Symbol { .. }) => {}
        _ => return None,
    }
    let docstring = match items.get(2) {
        Some(Node {
            kind: NodeKind::Atom(Atom::Str(text)),
            ..
        }) => Some(text.clone()),
        _ => None,
    };
    Some((name_node, docstring))
}

fn defn_node_for_infer(node: &Node) -> Cow<'_, Node> {
    let NodeKind::List(items) = &node.kind else {
        return Cow::Borrowed(node);
    };
    let has_docstring = matches!(
        items.get(2),
        Some(Node {
            kind: NodeKind::Atom(Atom::Str(_)),
            ..
        })
    );
    if !has_docstring {
        return Cow::Borrowed(node);
    }
    let mut stripped = items.clone();
    stripped.remove(2);
    Cow::Owned(Node {
        kind: NodeKind::List(stripped),
        span: node.span,
        leading_comments: node.leading_comments.clone(),
        trailing_comment: node.trailing_comment.clone(),
    })
}

fn def_name_node(node: &Node) -> Option<&Node> {
    if !list_head_is(node, "def") {
        return None;
    }
    let NodeKind::List(items) = &node.kind else {
        return None;
    };
    let name_node = items.get(1)?;
    match &name_node.kind {
        NodeKind::Atom(Atom::Symbol { .. }) => Some(name_node),
        _ => None,
    }
}

/// Return the name node (items[1]) of a `(defhandler Name ...)` form.
fn defhandler_name_node(node: &Node) -> Option<&Node> {
    if !list_head_is(node, "defhandler") {
        return None;
    }
    let NodeKind::List(items) = &node.kind else {
        return None;
    };
    let name_node = items.get(1)?;
    match &name_node.kind {
        NodeKind::Atom(Atom::Symbol { .. }) => Some(name_node),
        _ => None,
    }
}

fn symbol_name(node: &Node) -> Option<String> {
    match &node.kind {
        NodeKind::Atom(Atom::Symbol { ns: None, name }) => Some(name.clone()),
        NodeKind::Atom(Atom::Symbol { ns: Some(ns), name }) => Some(format!("{ns}/{name}")),
        _ => None,
    }
}

fn find_symbol_at_offset(nodes: &[Node], offset: usize) -> Option<&Node> {
    for node in nodes {
        if let Some(found) = find_symbol_in_node(node, offset) {
            return Some(found);
        }
    }
    None
}

fn find_symbol_in_node(node: &Node, offset: usize) -> Option<&Node> {
    if !span_contains(node.span, offset) {
        return None;
    }
    match &node.kind {
        NodeKind::Atom(Atom::Symbol { .. }) => Some(node),
        NodeKind::List(items) | NodeKind::Vector(items) | NodeKind::Set(items) => {
            for item in items {
                if let Some(found) = find_symbol_in_node(item, offset) {
                    return Some(found);
                }
            }
            None
        }
        NodeKind::Map(entries) => {
            for (key, value) in entries {
                if let Some(found) = find_symbol_in_node(key, offset) {
                    return Some(found);
                }
                if let Some(found) = find_symbol_in_node(value, offset) {
                    return Some(found);
                }
            }
            None
        }
        NodeKind::Quote(inner)
        | NodeKind::Deref(inner)
        | NodeKind::Discard(inner)
        | NodeKind::Quasiquote(inner)
        | NodeKind::Unquote(inner)
        | NodeKind::UnquoteSplice(inner) => find_symbol_in_node(inner, offset),
        _ => None,
    }
}

fn find_definition_range(nodes: &[Node], name: &str, source: &str) -> Option<Range> {
    for node in nodes {
        match defn_name_and_docstring(node) {
            Some((name_node, _)) if symbol_name(name_node).as_deref() == Some(name) => {
                return Some(span_to_range(source, name_node.span));
            }
            _ => {}
        }
        match def_name_node(node) {
            Some(name_node) if symbol_name(name_node).as_deref() == Some(name) => {
                return Some(span_to_range(source, name_node.span));
            }
            _ => {}
        }
        match defhandler_name_node(node) {
            Some(name_node) if symbol_name(name_node).as_deref() == Some(name) => {
                return Some(span_to_range(source, name_node.span));
            }
            _ => {}
        }
        // deftype: match the type name (items[1]) and constructor names
        // Form: (deftype TypeName | (Ctor Field*) | ...)
        if list_head_is(node, "deftype") {
            if let NodeKind::List(items) = &node.kind {
                if let Some(name_node) = items.get(1) {
                    if symbol_name(name_node).as_deref() == Some(name) {
                        return Some(span_to_range(source, name_node.span));
                    }
                }
                for item in items.iter().skip(2) {
                    if let NodeKind::List(ctor_items) = &item.kind {
                        if let Some(ctor_name_node) = ctor_items.first() {
                            if symbol_name(ctor_name_node).as_deref() == Some(name) {
                                return Some(span_to_range(source, ctor_name_node.span));
                            }
                        }
                    }
                }
            }
        }
    }
    None
}

fn completion_items(nodes: &[Node]) -> Vec<CompletionItem> {
    let mut seen = HashSet::new();
    let mut items = Vec::new();
    for node in nodes {
        match defn_name_and_docstring(node).and_then(|(name_node, _)| symbol_name(name_node)) {
            Some(name) if seen.insert(name.clone()) => {
                items.push(CompletionItem {
                    label: name,
                    kind: Some(CompletionItemKind::FUNCTION),
                    detail: Some("defn".to_string()),
                    ..CompletionItem::default()
                });
            }
            _ => {}
        }

        match def_name_node(node).and_then(symbol_name) {
            Some(name) if seen.insert(name.clone()) => {
                items.push(CompletionItem {
                    label: name,
                    kind: Some(CompletionItemKind::VARIABLE),
                    detail: Some("def".to_string()),
                    ..CompletionItem::default()
                });
            }
            _ => {}
        }

        match defhandler_name_node(node).and_then(symbol_name) {
            Some(name) if seen.insert(name.clone()) => {
                items.push(CompletionItem {
                    label: name,
                    kind: Some(CompletionItemKind::CLASS),
                    detail: Some("defhandler".to_string()),
                    ..CompletionItem::default()
                });
            }
            _ => {}
        }
    }
    items
}

/// Return stdlib module names as completion items.
fn stdlib_module_completions() -> Vec<CompletionItem> {
    nexl_stdlib::all_modules()
        .into_iter()
        .map(|(name, _entries): (&str, _)| CompletionItem {
            label: name.to_string(),
            kind: Some(CompletionItemKind::MODULE),
            detail: Some("stdlib module".to_string()),
            ..CompletionItem::default()
        })
        .collect()
}

/// Scan a project source directory for `.nx` files and return module-path completions.
fn project_module_completions(file_path: &Path) -> Vec<CompletionItem> {
    let ctx = match resolve_project_context(file_path) {
        Some(ctx) => ctx,
        None => return Vec::new(),
    };
    let mut items = Vec::new();
    collect_nx_files(&ctx.source_root, &ctx.source_root, &ctx.prefix, &mut items);
    items
}

/// Recursively collect `.nx` files and convert to module paths.
fn collect_nx_files(
    dir: &Path,
    root: &Path,
    prefix: &str,
    items: &mut Vec<CompletionItem>,
) {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_nx_files(&path, root, prefix, items);
        } else if path.extension().is_some_and(|ext| ext == "nx") {
            if let Some(module_path) = file_to_module_path(&path, root, prefix) {
                items.push(CompletionItem {
                    label: module_path,
                    kind: Some(CompletionItemKind::MODULE),
                    detail: Some("project module".to_string()),
                    ..CompletionItem::default()
                });
            }
        }
    }
}

/// Convert a `.nx` file path to a dotted module path.
fn file_to_module_path(file: &Path, root: &Path, prefix: &str) -> Option<String> {
    let rel = file.strip_prefix(root).ok()?;
    let stem = rel.with_extension("");
    let parts: Vec<&str> = stem.iter().filter_map(|c| c.to_str()).collect();
    if parts.is_empty() {
        return None;
    }
    Some(format!("{prefix}.{}", parts.join(".")))
}

/// Return completions for stdlib function names, qualified with module prefix.
/// E.g., for "json" module: json/encode, json/decode, json/pretty, etc.
fn stdlib_function_completions() -> Vec<CompletionItem> {
    let mut items = Vec::new();
    for (module_name, entries) in nexl_stdlib::all_modules() {
        for (fn_name, _) in &entries {
            items.push(CompletionItem {
                label: format!("{module_name}/{fn_name}"),
                kind: Some(CompletionItemKind::FUNCTION),
                detail: Some(format!("{module_name} module")),
                ..CompletionItem::default()
            });
        }
    }
    items
}

/// Extract record field names from deftype declarations and return as keyword completions.
fn record_field_completions(nodes: &[Node]) -> Vec<CompletionItem> {
    use nexl_infer::DeftypeDecl;
    let mut items = Vec::new();
    let mut seen = HashSet::new();
    for node in nodes {
        if let Ok(decl) = nexl_infer::parse_deftype(node) {
            let fields: &[(String, Type)] = match &decl {
                DeftypeDecl::Record { fields, .. } => fields,
                _ => continue,
            };
            let type_name = match &decl {
                DeftypeDecl::Record { name, .. } => name.as_str(),
                _ => continue,
            };
            for (field_name, _ty) in fields {
                if seen.insert(field_name.clone()) {
                    items.push(CompletionItem {
                        label: format!(":{field_name}"),
                        kind: Some(CompletionItemKind::FIELD),
                        detail: Some(format!("{type_name} field")),
                        ..CompletionItem::default()
                    });
                }
            }
        }
    }
    items
}

/// Check whether the given byte offset falls within a `:imports` vector
/// inside a `(module ...)` form.
fn is_in_imports_context(nodes: &[Node], offset: usize) -> bool {
    let module_node = match nodes.first() {
        Some(node) if list_head_is(node, "module") => node,
        _ => return false,
    };
    let items = match &module_node.kind {
        NodeKind::List(items) => items,
        _ => return false,
    };
    // Look for :imports keyword followed by a vector
    for window in items.windows(2) {
        if let NodeKind::Atom(Atom::Keyword { name, .. }) = &window[0].kind {
            if name == "imports" {
                if span_contains(window[1].span, offset) {
                    return true;
                }
            }
        }
    }
    false
}

fn list_head_is(node: &Node, name: &str) -> bool {
    match &node.kind {
        NodeKind::List(items) => match items.first() {
            Some(first) => match &first.kind {
                NodeKind::Atom(Atom::Symbol {
                    ns: None,
                    name: head,
                }) => head == name,
                _ => false,
            },
            None => false,
        },
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Cross-module go-to-definition helpers
// ---------------------------------------------------------------------------

/// Walk up from `start` looking for a `project.nx` file.
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

/// Resolved project context needed for cross-module navigation.
struct ProjectContext {
    /// The package prefix from the manifest (e.g. `"my-app"`).
    prefix: String,
    /// The absolute source root: `project_root.join(source_dir)`.
    source_root: PathBuf,
    /// Dependency prefix → source root mappings from path dependencies.
    dep_roots: std::collections::HashMap<String, PathBuf>,
}

/// Read `project.nx` and extract prefix + source root + dependency roots.
fn resolve_project_context(file_path: &Path) -> Option<ProjectContext> {
    let project_root = find_project_root(file_path)?;
    let manifest_path = project_root.join("project.nx");
    let manifest_src = std::fs::read_to_string(&manifest_path).ok()?;
    let manifest = nexl_pkg::parse_manifest(&manifest_src).ok()?;
    let source_root = project_root.join(&manifest.package.source_dir);

    // Resolve path dependencies to their source roots.
    let mut dep_roots = std::collections::HashMap::new();
    for (_name, dep_spec) in manifest.dependencies.iter().chain(manifest.dev_dependencies.iter()) {
        // Extract path from either Version (no path) or Detailed (optional path).
        let dep_path = match dep_spec {
            nexl_pkg::DependencySpec::Detailed(detail) => detail.path.as_deref(),
            _ => None,
        };
        if let Some(path) = dep_path {
            let dep_dir = project_root.join(path);
            let dep_manifest_path = dep_dir.join("project.nx");
            if let Ok(dep_manifest_src) = std::fs::read_to_string(&dep_manifest_path) {
                if let Ok(dep_manifest) = nexl_pkg::parse_manifest(&dep_manifest_src) {
                    let dep_source_root = dep_dir.join(&dep_manifest.package.source_dir);
                    dep_roots.insert(dep_manifest.package.prefix.clone(), dep_source_root);
                }
            }
        }
    }

    Some(ProjectContext {
        prefix: manifest.package.prefix,
        source_root,
        dep_roots,
    })
}

/// Extract import declarations from the first top-level `(module ...)` form.
fn extract_module_imports(nodes: &[Node]) -> Option<Vec<ImportDecl>> {
    let first = nodes.first()?;
    if !list_head_is(first, "module") {
        return None;
    }
    let NodeKind::List(items) = &first.kind else {
        return None;
    };
    let decl = parse_module_decl(items).ok()?;
    Some(decl.imports)
}

/// Find the module path for a given alias in imports.
fn find_module_for_alias<'a>(imports: &'a [ImportDecl], alias: &str) -> Option<&'a str> {
    imports.iter().find_map(|imp| match &imp.kind {
        ImportKind::Alias(a) if a == alias => Some(imp.module_path.as_str()),
        _ => None,
    })
}

/// Find which import brings an unqualified name into scope.
/// Returns `(module_path, original_name_in_that_module)`.
fn find_import_for_unqualified_name<'a>(
    imports: &'a [ImportDecl],
    name: &str,
) -> Option<(&'a str, String)> {
    for imp in imports {
        match &imp.kind {
            ImportKind::Refer(names) if names.iter().any(|n| n == name) => {
                return Some((&imp.module_path, name.to_string()));
            }
            ImportKind::All => {
                return Some((&imp.module_path, name.to_string()));
            }
            ImportKind::Exclude(excluded) if !excluded.iter().any(|n| n == name) => {
                return Some((&imp.module_path, name.to_string()));
            }
            ImportKind::Rename(renames) => {
                for (old, new) in renames {
                    if new == name {
                        return Some((&imp.module_path, old.clone()));
                    }
                }
            }
            _ => {}
        }
    }
    None
}

/// Convert a dotted module path to a file path via `nexl_modules`.
///
/// Tries the current project's prefix first, then checks path dependencies.
fn resolve_module_to_file_path(module_path: &str, ctx: &ProjectContext) -> Option<PathBuf> {
    // 1. Try current project's prefix.
    match nexl_modules::module_name_to_path(module_path, &ctx.prefix) {
        Ok(rel) => {
            let abs = ctx.source_root.join(rel);
            if abs.is_file() {
                return Some(abs);
            }
        }
        Err(nexl_modules::ModulePathError::PrefixMismatch { .. }) => {
            // Prefix didn't match — fall through to dependency lookup.
        }
        Err(_) => return None,
    }

    // 2. Try each dependency's prefix.
    let first_dot = module_path.find('.');
    let candidate_prefix = if let Some(pos) = first_dot {
        &module_path[..pos]
    } else {
        module_path
    };

    if let Some(dep_source_root) = ctx.dep_roots.get(candidate_prefix) {
        if let Ok(rel) = nexl_modules::module_name_to_path(module_path, candidate_prefix) {
            let abs = dep_source_root.join(rel);
            if abs.is_file() {
                return Some(abs);
            }
        }
    }

    None
}

/// Collect top-level named symbols from a parsed file for `textDocument/documentSymbol`.
///
/// Recognises:
/// - `(defn name ...)` → FUNCTION
/// - `(def name ...)` → VARIABLE
/// - `(deftype name ...)` → CLASS
fn collect_document_symbols(nodes: &[Node], source: &str) -> Vec<DocumentSymbol> {
    let mut symbols = Vec::new();
    for node in nodes {
        let form_range = span_to_range(source, node.span);
        if let Some((name_node, _)) = defn_name_and_docstring(node) {
            if let Some(name) = symbol_name(name_node) {
                #[allow(deprecated)]
                symbols.push(DocumentSymbol {
                    name,
                    detail: None,
                    kind: SymbolKind::FUNCTION,
                    tags: None,
                    deprecated: None,
                    range: form_range,
                    selection_range: span_to_range(source, name_node.span),
                    children: None,
                });
            }
        } else if let Some(name_node) = def_name_node(node) {
            if let Some(name) = symbol_name(name_node) {
                #[allow(deprecated)]
                symbols.push(DocumentSymbol {
                    name,
                    detail: None,
                    kind: SymbolKind::VARIABLE,
                    tags: None,
                    deprecated: None,
                    range: form_range,
                    selection_range: span_to_range(source, name_node.span),
                    children: None,
                });
            }
        } else if list_head_is(node, "deftype") {
            if let NodeKind::List(items) = &node.kind {
                if let Some(name_node) = items.get(1) {
                    if let Some(name) = symbol_name(name_node) {
                        #[allow(deprecated)]
                        symbols.push(DocumentSymbol {
                            name,
                            detail: None,
                            kind: SymbolKind::CLASS,
                            tags: None,
                            deprecated: None,
                            range: form_range,
                            selection_range: span_to_range(source, name_node.span),
                            children: None,
                        });
                    }
                }
            }
        }
    }
    symbols
}

/// Read a file, parse it, and search for a definition by name.
fn find_definition_in_file(path: &Path, name: &str) -> Option<(Url, Range)> {
    let source = std::fs::read_to_string(path).ok()?;
    let nodes = nexl_reader::read(&source, FileId(0)).ok()?;
    let range = find_definition_range(&nodes, name, &source)?;
    let url = Url::from_file_path(path).ok()?;
    Some((url, range))
}

/// Collect all ranges in `nodes` where a symbol with the given full name appears.
///
/// `name` must be the fully-qualified string as returned by `symbol_name`:
/// `"foo"` for unqualified symbols or `"alias/foo"` for qualified ones.
fn collect_symbol_uses(nodes: &[Node], name: &str, source: &str) -> Vec<Range> {
    let mut out = Vec::new();
    for node in nodes {
        collect_symbol_uses_in_node(node, name, source, &mut out);
    }
    out
}

fn collect_symbol_uses_in_node(node: &Node, name: &str, source: &str, out: &mut Vec<Range>) {
    match &node.kind {
        NodeKind::Atom(Atom::Symbol { ns, name: n }) => {
            let full = match ns {
                Some(prefix) => format!("{prefix}/{n}"),
                None => n.clone(),
            };
            if full == name {
                out.push(span_to_range(source, node.span));
            }
        }
        NodeKind::List(items) | NodeKind::Vector(items) | NodeKind::Set(items) => {
            for item in items {
                collect_symbol_uses_in_node(item, name, source, out);
            }
        }
        NodeKind::Map(entries) => {
            for (k, v) in entries {
                collect_symbol_uses_in_node(k, name, source, out);
                collect_symbol_uses_in_node(v, name, source, out);
            }
        }
        NodeKind::Quote(inner)
        | NodeKind::Deref(inner)
        | NodeKind::Discard(inner)
        | NodeKind::Quasiquote(inner)
        | NodeKind::Unquote(inner)
        | NodeKind::UnquoteSplice(inner) => {
            collect_symbol_uses_in_node(inner, name, source, out);
        }
        NodeKind::Atom(_) => {}
    }
}

/// Read a file, parse it, and collect all use-site `Location`s for `name`.
fn find_references_in_file(path: &Path, name: &str) -> Vec<Location> {
    let source = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let nodes = match nexl_reader::read(&source, FileId(0)) {
        Ok(n) => n,
        Err(_) => return Vec::new(),
    };
    let url = match Url::from_file_path(path) {
        Ok(u) => u,
        Err(_) => return Vec::new(),
    };
    collect_symbol_uses(&nodes, name, &source)
        .into_iter()
        .map(|range| Location { uri: url.clone(), range })
        .collect()
}

/// Walk all `.nx` files under `dir`, collecting every reference to `name`,
/// and skipping `skip_path` (the already-searched current file).
fn collect_references_across_project(
    dir: &Path,
    name: &str,
    skip_path: &Path,
    out: &mut Vec<Location>,
) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_references_across_project(&path, name, skip_path, out);
        } else if path.extension().is_some_and(|ext| ext == "nx") && path != skip_path {
            out.extend(find_references_in_file(&path, name));
        }
    }
}

// ---------------------------------------------------------------------------
// Code actions
// ---------------------------------------------------------------------------

/// Collect all applicable code actions for the given cursor position.
fn collect_code_actions(
    nodes: &[Node],
    source: &str,
    offset: usize,
    _range: Range,
    uri: &Url,
    actions: &mut Vec<CodeActionOrCommand>,
) {
    // Collect all enclosing nodes from outermost to innermost so that
    // actions on any ancestor form are offered regardless of cursor depth.
    let mut ancestors = Vec::new();
    collect_ancestors(nodes, offset, &mut ancestors);

    for node in &ancestors {
        thread_actions(node, source, uri, actions);
        unwind_thread_action(node, source, uri, actions);
        def_defn_convert_actions(node, source, uri, actions);
        negate_condition_action(node, source, uri, actions);
        flip_binary_action(node, source, uri, actions);
        cycle_collection_action(node, source, uri, actions);
        if_cond_convert_actions(node, source, uri, actions);
        wrap_in_fn_action(node, source, uri, actions);
        demorgan_action(node, source, uri, actions);
        str_to_interpolation_action(node, source, uri, actions);
    }
    // Extract variable needs the ancestor chain to find the parent form.
    extract_variable_action(&ancestors, source, uri, actions);
}

/// Collect all nodes whose span contains `offset`, from outermost to innermost.
fn collect_ancestors<'a>(nodes: &'a [Node], offset: usize, out: &mut Vec<&'a Node>) {
    for node in nodes {
        if span_contains(node.span, offset) {
            out.push(node);
            collect_ancestors_in_node(node, offset, out);
            return;
        }
    }
}

/// Recurse into a node's children to collect all enclosing nodes.
fn collect_ancestors_in_node<'a>(node: &'a Node, offset: usize, out: &mut Vec<&'a Node>) {
    match &node.kind {
        NodeKind::List(children) | NodeKind::Vector(children) | NodeKind::Set(children) => {
            for child in children {
                if span_contains(child.span, offset) {
                    out.push(child);
                    collect_ancestors_in_node(child, offset, out);
                    return;
                }
            }
        }
        NodeKind::Map(pairs) => {
            for (k, v) in pairs {
                if span_contains(k.span, offset) {
                    out.push(k);
                    collect_ancestors_in_node(k, offset, out);
                    return;
                }
                if span_contains(v.span, offset) {
                    out.push(v);
                    collect_ancestors_in_node(v, offset, out);
                    return;
                }
            }
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Thread first / Thread last
// ---------------------------------------------------------------------------

/// Check if a node is a threadable nested call chain like `(f (g (h x)))`.
///
/// Returns the list of (function_name_source, extra_args_source) from outer→inner,
/// plus the innermost value's source text, if the chain is at least 2 calls deep.
///
/// For thread-first, the nested call is the *first* argument: `(f (g x) ...)`.
/// For thread-last, the nested call is the *last* argument: `(f ... (g x))`.
/// We check both positions.
fn extract_thread_chain(
    node: &Node,
    source: &str,
) -> Option<ThreadChain> {
    let NodeKind::List(children) = &node.kind else {
        return None;
    };
    if children.len() < 2 {
        return None;
    }
    // Don't thread special forms (defn, if, let, etc.)
    if is_special_form(children) {
        return None;
    }

    // Determine if this is a first-threadable or last-threadable chain.
    // Thread-first: nested call is always the *second* element (first arg).
    // Thread-last: nested call is always the *last* element.
    // For a simple 2-element list `(f (g x))`, both positions coincide.

    // We'll extract a chain for thread-first: the nested call is at position 1.
    let mut chain_first = Vec::new();
    let seed_first = extract_chain_first(node, source, &mut chain_first);
    let first_ok = seed_first.is_some() && chain_first.len() >= 2;

    // And for thread-last: the nested call is at the last position.
    let mut chain_last = Vec::new();
    let seed_last = extract_chain_last(node, source, &mut chain_last);
    let last_ok = seed_last.is_some() && chain_last.len() >= 2;

    // Heuristic: reject threading when the chain looks like structured/DSL
    // code rather than a pipeline. We check if any step has a complex extra
    // arg (map, vector, or set literal) — that signals component/template code
    // where threading would be nonsensical.
    let first_ok = first_ok && !chain_has_complex_extras(node, true);
    let last_ok = last_ok && !chain_has_complex_extras(node, false);

    if !first_ok && !last_ok {
        return None;
    }

    Some(ThreadChain {
        first: if first_ok {
            Some((chain_first, seed_first.unwrap()))
        } else {
            None
        },
        last: if last_ok {
            Some((chain_last, seed_last.unwrap()))
        } else {
            None
        },
    })
}

/// Check if a threadable chain contains complex extra args (maps, vectors, sets)
/// which signal DSL/component code rather than a pipeline.
fn chain_has_complex_extras(node: &Node, is_first: bool) -> bool {
    let NodeKind::List(children) = &node.kind else {
        return false;
    };
    if children.len() < 2 {
        return false;
    }

    // Check extra args at this level (everything except head and the nested call).
    let extras = if is_first {
        // Thread-first: nested call at position 1, extras are positions 2..
        &children[2..]
    } else {
        // Thread-last: nested call at last position, extras are positions 1..len-1
        &children[1..children.len() - 1]
    };

    for extra in extras {
        if matches!(
            extra.kind,
            NodeKind::Map(_) | NodeKind::Vector(_) | NodeKind::Set(_)
        ) {
            return true;
        }
    }

    // Recurse into the nested call.
    let nested = if is_first {
        children.get(1)
    } else {
        children.last()
    };
    if let Some(nested) = nested {
        if matches!(&nested.kind, NodeKind::List(inner) if !inner.is_empty() && !is_special_form(inner))
        {
            return chain_has_complex_extras(nested, is_first);
        }
    }

    false
}

struct ThreadChain {
    /// (steps, seed) for thread-first. Each step is (fn_text, extra_args_text).
    first: Option<(Vec<(String, String)>, String)>,
    /// (steps, seed) for thread-last.
    last: Option<(Vec<(String, String)>, String)>,
}

/// Extract a thread-first chain: at each level, the nested call is at position 1
/// (the first argument). Extra args follow.
fn extract_chain_first(node: &Node, source: &str, steps: &mut Vec<(String, String)>) -> Option<String> {
    let NodeKind::List(children) = &node.kind else {
        return Some(node_source(node, source).to_string());
    };
    if children.is_empty() {
        return None;
    }
    let fn_text = node_source(&children[0], source).to_string();
    let extra: Vec<&str> = children[2..].iter().map(|n| node_source(n, source)).collect();
    let extra_text = extra.join(" ");
    steps.push((fn_text, extra_text));

    if children.len() < 2 {
        return None;
    }

    // Recurse into the first argument (position 1).
    let first_arg = &children[1];
    match &first_arg.kind {
        NodeKind::List(inner) if !inner.is_empty() && !is_special_form(inner) => {
            extract_chain_first(first_arg, source, steps)
        }
        _ => Some(node_source(first_arg, source).to_string()),
    }
}

/// Extract a thread-last chain: at each level, the nested call is at the last position.
fn extract_chain_last(node: &Node, source: &str, steps: &mut Vec<(String, String)>) -> Option<String> {
    let NodeKind::List(children) = &node.kind else {
        return Some(node_source(node, source).to_string());
    };
    if children.is_empty() {
        return None;
    }
    let fn_text = node_source(&children[0], source).to_string();
    let extra: Vec<&str> = children[1..children.len() - 1]
        .iter()
        .map(|n| node_source(n, source))
        .collect();
    let extra_text = extra.join(" ");
    steps.push((fn_text, extra_text));

    if children.len() < 2 {
        return None;
    }

    // Recurse into the last argument.
    let last_arg = children.last().unwrap();
    match &last_arg.kind {
        NodeKind::List(inner) if !inner.is_empty() && !is_special_form(inner) => {
            extract_chain_last(last_arg, source, steps)
        }
        _ => Some(node_source(last_arg, source).to_string()),
    }
}

/// Check if a list starts with a special form head (def, defn, fn, let, if, match, etc.)
fn is_special_form(children: &[Node]) -> bool {
    if let Some(head) = children.first() {
        if let NodeKind::Atom(Atom::Symbol { ns: None, name }) = &head.kind {
            return matches!(
                name.as_str(),
                "def" | "defn" | "fn" | "let" | "if" | "cond" | "match" | "do" | "quote"
                    | "import" | "module" | "deftype" | "defeffect" | "defhandler"
                    | "handle" | "if-let" | "when" | "when-let" | "->" | "->>" | "and" | "or"
            );
        }
    }
    false
}

/// Get the source text for a node.
fn node_source<'a>(node: &Node, source: &'a str) -> &'a str {
    let start = node.span.start as usize;
    let end = node.span.end() as usize;
    &source[start..end]
}

/// Build the threaded text from a chain.
fn build_threaded_text(steps: &[(String, String)], seed: &str, arrow: &str) -> String {
    let mut parts = vec![arrow.to_string(), seed.to_string()];
    // Steps are outer→inner, but threading reads inner→outer.
    for (fn_text, extra) in steps.iter().rev() {
        if extra.is_empty() {
            parts.push(fn_text.clone());
        } else {
            // For thread-first: (-> x (f extra))
            // For thread-last: (->> x (f extra))
            parts.push(format!("({fn_text} {extra})"));
        }
    }
    format!("({})", parts.join(" "))
}

/// Create a WorkspaceEdit that replaces a node's span.
/// Re-parse and pretty-print code to get proper indentation.
fn format_code(text: &str) -> String {
    let Ok(nodes) = nexl_reader::read(text, FileId::SYNTHETIC) else {
        return text.to_string();
    };
    if nodes.len() == 1 {
        let printer = PrettyPrinter::default_config();
        // print_file adds trailing newline; we just want the form.
        let formatted = printer.print_file(&nodes);
        formatted.trim_end().to_string()
    } else {
        text.to_string()
    }
}

fn make_edit(uri: &Url, node: &Node, source: &str, new_text: String) -> WorkspaceEdit {
    let range = span_to_range(source, node.span);
    let formatted = format_code(&new_text);
    let edit = TextEdit {
        range,
        new_text: formatted,
    };
    let mut changes = HashMap::new();
    changes.insert(uri.clone(), vec![edit]);
    WorkspaceEdit {
        changes: Some(changes),
        ..Default::default()
    }
}

/// Offer "Thread first" / "Thread last" for nested call chains.
fn thread_actions(
    node: &Node,
    source: &str,
    uri: &Url,
    actions: &mut Vec<CodeActionOrCommand>,
) {
    // Walk up to find the outermost list at this position.
    // Since we get the innermost node, we need the outermost list.
    // Actually, we need to check the node itself — if cursor is on `(`,
    // find_node_at_offset returns the list node.
    let Some(chain) = extract_thread_chain(node, source) else {
        return;
    };

    let first_text = chain
        .first
        .as_ref()
        .map(|(steps, seed)| build_threaded_text(steps, seed, "->"));
    let last_text = chain
        .last
        .as_ref()
        .map(|(steps, seed)| build_threaded_text(steps, seed, "->>"));

    if let Some(text) = &first_text {
        let edit = make_edit(uri, node, source, text.clone());
        actions.push(CodeActionOrCommand::CodeAction(CodeAction {
            title: "Thread first (->)".to_string(),
            kind: Some(CodeActionKind::REFACTOR_REWRITE),
            edit: Some(edit),
            ..Default::default()
        }));
    }

    // Only show thread-last if it produces a different result than thread-first.
    if let Some(text) = &last_text {
        if first_text.as_ref() != Some(text) {
            let edit = make_edit(uri, node, source, text.clone());
            actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                title: "Thread last (->>)".to_string(),
                kind: Some(CodeActionKind::REFACTOR_REWRITE),
                edit: Some(edit),
                ..Default::default()
            }));
        }
    }
}

// ---------------------------------------------------------------------------
// Unwind threading
// ---------------------------------------------------------------------------

/// Detect `(-> seed f g h)` or `(->> seed f g h)` and offer to unwind.
fn unwind_thread_action(
    node: &Node,
    source: &str,
    uri: &Url,
    actions: &mut Vec<CodeActionOrCommand>,
) {
    let NodeKind::List(children) = &node.kind else {
        return;
    };
    if children.len() < 3 {
        return;
    }
    let arrow = match &children[0].kind {
        NodeKind::Atom(Atom::Symbol { ns: None, name }) if name == "->" || name == "->>" => {
            name.as_str()
        }
        _ => return,
    };
    let is_first = arrow == "->";

    // children[1] is the seed, children[2..] are the steps.
    let seed_text = node_source(&children[1], source);
    let steps = &children[2..];

    // Build nested calls.
    let mut result = seed_text.to_string();
    for step in steps {
        match &step.kind {
            // Bare symbol: (fn result) or (fn result)
            NodeKind::Atom(_) => {
                let fn_name = node_source(step, source);
                result = format!("({fn_name} {result})");
            }
            // Wrapped form like (f extra): thread-first inserts as first arg,
            // thread-last inserts as last arg.
            NodeKind::List(inner) if !inner.is_empty() => {
                let fn_name = node_source(&inner[0], source);
                let extra_args: Vec<&str> =
                    inner[1..].iter().map(|n| node_source(n, source)).collect();
                if is_first {
                    // (f result extra...)
                    let mut parts = vec![fn_name, &result];
                    parts.extend(extra_args.iter().copied());
                    result = format!("({})", parts.join(" "));
                } else {
                    // (f extra... result)
                    let mut parts = vec![fn_name];
                    parts.extend(extra_args.iter().copied());
                    parts.push(&result);
                    result = format!("({})", parts.join(" "));
                }
            }
            _ => return, // unexpected form, bail
        }
    }

    let edit = make_edit(uri, node, source, result.clone());
    actions.push(CodeActionOrCommand::CodeAction(CodeAction {
        title: "Unwind threading".to_string(),
        kind: Some(CodeActionKind::REFACTOR_REWRITE),
        edit: Some(edit),
        ..Default::default()
    }));
}

// ---------------------------------------------------------------------------
// Convert def ↔ defn
// ---------------------------------------------------------------------------

/// Offer "Convert to defn" on `(def name (fn [params] body))` and
/// "Convert to def" on `(defn name [params] body)`.
fn def_defn_convert_actions(
    node: &Node,
    source: &str,
    uri: &Url,
    actions: &mut Vec<CodeActionOrCommand>,
) {
    let NodeKind::List(children) = &node.kind else {
        return;
    };
    if children.len() < 3 {
        return;
    }

    let head_name = match &children[0].kind {
        NodeKind::Atom(Atom::Symbol { ns: None, name }) => name.as_str(),
        _ => return,
    };

    match head_name {
        "def" => {
            // (def name (fn [params] body...))
            // children: [def, name, (fn [params] body...)]
            if children.len() != 3 {
                return;
            }
            let name_text = node_source(&children[1], source);
            let fn_form = &children[2];
            let NodeKind::List(fn_children) = &fn_form.kind else {
                return;
            };
            // fn_children: [fn, [params], body...]
            if fn_children.len() < 3 {
                return;
            }
            if !matches!(
                &fn_children[0].kind,
                NodeKind::Atom(Atom::Symbol { ns: None, name }) if name == "fn"
            ) {
                return;
            }
            let params_text = node_source(&fn_children[1], source);
            let body_parts: Vec<&str> = fn_children[2..]
                .iter()
                .map(|n| node_source(n, source))
                .collect();
            let body_text = body_parts.join(" ");
            let new_text = format!("(defn {name_text} {params_text} {body_text})");
            let edit = make_edit(uri, node, source, new_text);
            actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                title: "Convert to defn".to_string(),
                kind: Some(CodeActionKind::REFACTOR_REWRITE),
                edit: Some(edit),
                ..Default::default()
            }));
        }
        "defn" => {
            // (defn name [params] body...)
            // children: [defn, name, [params], body...]
            if children.len() < 4 {
                return;
            }
            // Skip if there's a docstring (children[2] is a string, params at [3]).
            let (params_idx, _has_doc) = match &children[2].kind {
                NodeKind::Atom(Atom::Str(_)) => (3, true),
                NodeKind::Vector(_) => (2, false),
                _ => return,
            };
            if params_idx >= children.len() {
                return;
            }
            let name_text = node_source(&children[1], source);
            let params_text = node_source(&children[params_idx], source);
            let body_parts: Vec<&str> = children[params_idx + 1..]
                .iter()
                .map(|n| node_source(n, source))
                .collect();
            let body_text = body_parts.join(" ");
            let new_text = format!("(def {name_text} (fn {params_text} {body_text}))");
            let edit = make_edit(uri, node, source, new_text);
            actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                title: "Convert to def".to_string(),
                kind: Some(CodeActionKind::REFACTOR_REWRITE),
                edit: Some(edit),
                ..Default::default()
            }));
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Negate condition
// ---------------------------------------------------------------------------

/// Offer "Negate condition" on `(if cond then else)` — swaps branches and
/// negates the condition. If condition is already `(not x)`, simplifies to `x`.
fn negate_condition_action(
    node: &Node,
    source: &str,
    uri: &Url,
    actions: &mut Vec<CodeActionOrCommand>,
) {
    let NodeKind::List(children) = &node.kind else {
        return;
    };
    // (if cond then else) — must have exactly 4 elements
    if children.len() != 4 {
        return;
    }
    let is_if = matches!(
        &children[0].kind,
        NodeKind::Atom(Atom::Symbol { ns: None, name }) if name == "if"
    );
    if !is_if {
        return;
    }

    let cond = &children[1];
    let then_text = node_source(&children[2], source);
    let else_text = node_source(&children[3], source);

    // Check if condition is already `(not x)` — if so, simplify.
    let new_cond = if let NodeKind::List(cond_children) = &cond.kind {
        if cond_children.len() == 2 {
            if let NodeKind::Atom(Atom::Symbol { ns: None, name }) = &cond_children[0].kind {
                if name == "not" {
                    // Already negated: unwrap
                    Some(node_source(&cond_children[1], source).to_string())
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    };

    let new_cond = new_cond.unwrap_or_else(|| format!("(not {})", node_source(cond, source)));
    let new_text = format!("(if {new_cond} {else_text} {then_text})");
    let edit = make_edit(uri, node, source, new_text);
    actions.push(CodeActionOrCommand::CodeAction(CodeAction {
        title: "Negate condition".to_string(),
        kind: Some(CodeActionKind::REFACTOR_REWRITE),
        edit: Some(edit),
        ..Default::default()
    }));
}

// ---------------------------------------------------------------------------
// Flip binary expression
// ---------------------------------------------------------------------------

/// Offer "Flip operands" on binary operators like `(< a b)` → `(> b a)`.
fn flip_binary_action(
    node: &Node,
    source: &str,
    uri: &Url,
    actions: &mut Vec<CodeActionOrCommand>,
) {
    let NodeKind::List(children) = &node.kind else {
        return;
    };
    if children.len() != 3 {
        return;
    }
    let op = match &children[0].kind {
        NodeKind::Atom(Atom::Symbol { ns: None, name }) => name.as_str(),
        _ => return,
    };

    let flipped_op = match op {
        "<" => ">",
        ">" => "<",
        "<=" => ">=",
        ">=" => "<=",
        "=" => "=",
        "!=" => "!=",
        "and" => "and",
        "or" => "or",
        "+" => "+",
        "*" => "*",
        _ => return,
    };

    let left = node_source(&children[1], source);
    let right = node_source(&children[2], source);
    let new_text = format!("({flipped_op} {right} {left})");
    let edit = make_edit(uri, node, source, new_text);
    actions.push(CodeActionOrCommand::CodeAction(CodeAction {
        title: format!("Flip operands of `{op}`"),
        kind: Some(CodeActionKind::REFACTOR_REWRITE),
        edit: Some(edit),
        ..Default::default()
    }));
}

// ---------------------------------------------------------------------------
// Cycle collection
// ---------------------------------------------------------------------------

/// Offer to convert between vector `[...]` and set `#{...}` literals.
fn cycle_collection_action(
    node: &Node,
    source: &str,
    uri: &Url,
    actions: &mut Vec<CodeActionOrCommand>,
) {
    match &node.kind {
        NodeKind::Vector(children) => {
            let inner: Vec<&str> = children.iter().map(|n| node_source(n, source)).collect();
            let new_text = format!("#{{{}}}", inner.join(" "));
            let edit = make_edit(uri, node, source, new_text);
            actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                title: "Convert to set".to_string(),
                kind: Some(CodeActionKind::REFACTOR_REWRITE),
                edit: Some(edit),
                ..Default::default()
            }));
        }
        NodeKind::Set(children) => {
            let inner: Vec<&str> = children.iter().map(|n| node_source(n, source)).collect();
            let new_text = format!("[{}]", inner.join(" "));
            let edit = make_edit(uri, node, source, new_text);
            actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                title: "Convert to vector".to_string(),
                kind: Some(CodeActionKind::REFACTOR_REWRITE),
                edit: Some(edit),
                ..Default::default()
            }));
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Extract variable
// ---------------------------------------------------------------------------

/// Offer "Extract variable" on a non-trivial expression inside a call.
///
/// Finds the innermost compound expression (list/call) and wraps the parent
/// form in a `(let [x extracted] ...)` binding.
fn extract_variable_action(
    ancestors: &[&Node],
    source: &str,
    uri: &Url,
    actions: &mut Vec<CodeActionOrCommand>,
) {
    // Need at least 2 ancestors: the parent form and the expression to extract.
    if ancestors.len() < 2 {
        return;
    }

    // Walk from innermost to outermost to find the first compound expression
    // (a list that looks like a function call, not a special form).
    let mut target_idx = None;
    for i in (0..ancestors.len()).rev() {
        let node = ancestors[i];
        if let NodeKind::List(children) = &node.kind {
            if children.len() >= 2 && !is_special_form(children) {
                // This is a function call — it's a good extraction target.
                // But only if it has a parent to wrap.
                if i > 0 {
                    target_idx = Some(i);
                    break;
                }
            }
        }
    }

    let Some(target_idx) = target_idx else {
        return;
    };

    let target = ancestors[target_idx]; // expression to extract
    let target_src = node_source(target, source);

    // Check if any ancestor is a `let` form — if so, add the binding there
    // instead of creating a new nested let.
    if let Some(let_node) = find_enclosing_let(ancestors, target_idx) {
        if let NodeKind::List(let_children) = &let_node.kind {
            // let_children: [let, [bindings...], body...]
            if let_children.len() >= 3 {
                if let NodeKind::Vector(_) = &let_children[1].kind {
                    let bindings_node = &let_children[1];

                    // Build new bindings vector: append "x target_src"
                    let existing_bindings = node_source(bindings_node, source);
                    // Insert before the closing ']'
                    let inner = &existing_bindings[1..existing_bindings.len() - 1];
                    let new_bindings = if inner.trim().is_empty() {
                        format!("[x {target_src}]")
                    } else {
                        format!("[{} x {target_src}]", inner.trim_end())
                    };

                    // Build new let form with the target replaced by 'x' in the body
                    let let_src = node_source(let_node, source);
                    let old_bindings = node_source(bindings_node, source);
                    // First, replace the bindings vector
                    let with_new_bindings = let_src.replacen(old_bindings, &new_bindings, 1);
                    // Then replace the target expression with 'x' in the result
                    // (must not accidentally replace inside the new bindings)
                    let binding_end = with_new_bindings.find(&new_bindings)
                        .map(|pos| pos + new_bindings.len())
                        .unwrap_or(0);
                    let (prefix, suffix) = with_new_bindings.split_at(binding_end);
                    let new_suffix = suffix.replacen(target_src, "x", 1);
                    let new_text = format!("{prefix}{new_suffix}");

                    let edit = make_edit(uri, let_node, source, new_text);
                    actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                        title: "Extract variable".to_string(),
                        kind: Some(CodeActionKind::REFACTOR_EXTRACT),
                        edit: Some(edit),
                        ..Default::default()
                    }));
                    return;
                }
            }
        }
    }

    // No enclosing let — find the best scope to wrap.
    // Walk up to find the outermost non-special-form parent, or the enclosing
    // special form (defn body, if branch, etc.).
    let wrap_node = ancestors[target_idx - 1];

    let wrap_src = node_source(wrap_node, source);
    let new_wrap = wrap_src.replacen(target_src, "x", 1);
    let new_text = format!("(let [x {target_src}] {new_wrap})");
    let edit = make_edit(uri, wrap_node, source, new_text);
    actions.push(CodeActionOrCommand::CodeAction(CodeAction {
        title: "Extract variable".to_string(),
        kind: Some(CodeActionKind::REFACTOR_EXTRACT),
        edit: Some(edit),
        ..Default::default()
    }));
}

/// Find the nearest enclosing `(let [...] ...)` form in the ancestor chain.
fn find_enclosing_let<'a>(ancestors: &[&'a Node], below_idx: usize) -> Option<&'a Node> {
    for i in (0..below_idx).rev() {
        if let NodeKind::List(children) = &ancestors[i].kind {
            if children.len() >= 3 {
                if let NodeKind::Atom(Atom::Symbol { ns: None, name }) = &children[0].kind {
                    if name == "let" {
                        // Verify it has a vector bindings form
                        if matches!(&children[1].kind, NodeKind::Vector(_)) {
                            return Some(ancestors[i]);
                        }
                    }
                }
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Convert if ↔ cond
// ---------------------------------------------------------------------------

/// Offer "Convert to cond" on `(if a 1 (if b 2 3))` chains.
/// Offer "Convert to if" on `(cond a 1 b 2 :else 3)`.
fn if_cond_convert_actions(
    node: &Node,
    source: &str,
    uri: &Url,
    actions: &mut Vec<CodeActionOrCommand>,
) {
    let NodeKind::List(children) = &node.kind else {
        return;
    };
    if children.len() < 3 {
        return;
    }

    let head = match &children[0].kind {
        NodeKind::Atom(Atom::Symbol { ns: None, name }) => name.as_str(),
        _ => return,
    };

    match head {
        "if" => {
            // Check if this is an if-chain: (if cond then (if cond2 then2 else2))
            if children.len() != 4 {
                return;
            }
            // The else branch must be another if
            let else_branch = &children[3];
            if !list_head_is(else_branch, "if") {
                return;
            }

            // Collect the chain
            let mut clauses = Vec::new();
            collect_if_chain(node, source, &mut clauses);

            if clauses.len() < 2 {
                return;
            }

            // Build cond form: (cond c1 e1 c2 e2 :else default)
            let mut parts = vec!["cond".to_string()];
            for (cond, expr) in &clauses {
                parts.push(cond.clone());
                parts.push(expr.clone());
            }
            let new_text = format!("({})", parts.join(" "));
            let edit = make_edit(uri, node, source, new_text);
            actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                title: "Convert to cond".to_string(),
                kind: Some(CodeActionKind::REFACTOR_REWRITE),
                edit: Some(edit),
                ..Default::default()
            }));
        }
        "cond" => {
            // (cond c1 e1 c2 e2 :else default) → nested ifs
            // Must have odd number of args after "cond" (pairs + optional :else default)
            let args = &children[1..];
            if args.len() < 4 || args.len() % 2 != 0 {
                return;
            }

            let pairs: Vec<(&str, &str)> = args
                .chunks(2)
                .map(|pair| (node_source(&pair[0], source), node_source(&pair[1], source)))
                .collect();

            let new_text = build_nested_ifs(&pairs);
            let edit = make_edit(uri, node, source, new_text);
            actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                title: "Convert to if".to_string(),
                kind: Some(CodeActionKind::REFACTOR_REWRITE),
                edit: Some(edit),
                ..Default::default()
            }));
        }
        _ => {}
    }
}

/// Collect an if-chain: (if c1 e1 (if c2 e2 default)) → [(c1,e1), (c2,e2), (:else,default)]
fn collect_if_chain(node: &Node, source: &str, clauses: &mut Vec<(String, String)>) {
    let NodeKind::List(children) = &node.kind else {
        return;
    };
    if children.len() != 4 || !list_head_is(node, "if") {
        return;
    }

    let cond = node_source(&children[1], source).to_string();
    let then = node_source(&children[2], source).to_string();
    clauses.push((cond, then));

    let else_branch = &children[3];
    if list_head_is(else_branch, "if") {
        collect_if_chain(else_branch, source, clauses);
    } else {
        clauses.push((":else".to_string(), node_source(else_branch, source).to_string()));
    }
}

/// Build nested ifs from (cond, expr) pairs. Last pair's condition is treated as :else.
fn build_nested_ifs(pairs: &[(&str, &str)]) -> String {
    if pairs.len() == 1 {
        // Last pair — just the expression (it's the :else branch)
        return pairs[0].1.to_string();
    }
    if pairs.len() == 2 {
        let (c1, e1) = pairs[0];
        let (_c2, e2) = pairs[1]; // c2 is :else
        return format!("(if {c1} {e1} {e2})");
    }
    let (c, e) = pairs[0];
    let rest = build_nested_ifs(&pairs[1..]);
    format!("(if {c} {e} {rest})")
}

// ---------------------------------------------------------------------------
// Wrap in anonymous function
// ---------------------------------------------------------------------------

/// Offer "Wrap in fn" on a bare symbol that's used as a function argument.
/// `(map inc xs)` → cursor on `inc` → `(fn [x] (inc x))`
fn wrap_in_fn_action(
    node: &Node,
    source: &str,
    uri: &Url,
    actions: &mut Vec<CodeActionOrCommand>,
) {
    // Only on bare symbols (not qualified)
    let NodeKind::Atom(Atom::Symbol { ns: None, name }) = &node.kind else {
        return;
    };

    // Skip special keywords/forms
    if matches!(
        name.as_str(),
        "def" | "defn" | "fn" | "let" | "if" | "cond" | "match" | "do"
            | "true" | "false" | "unit"
    ) {
        return;
    }

    let fn_name = node_source(node, source);
    let new_text = format!("(fn [x] ({fn_name} x))");
    let edit = make_edit(uri, node, source, new_text);
    actions.push(CodeActionOrCommand::CodeAction(CodeAction {
        title: "Wrap in fn".to_string(),
        kind: Some(CodeActionKind::REFACTOR_REWRITE),
        edit: Some(edit),
        ..Default::default()
    }));
}

// ---------------------------------------------------------------------------
// De Morgan's law
// ---------------------------------------------------------------------------

/// Offer De Morgan transformation on `(not (and a b))` ↔ `(or (not a) (not b))`
/// and `(not (or a b))` ↔ `(and (not a) (not b))`.
fn demorgan_action(
    node: &Node,
    source: &str,
    uri: &Url,
    actions: &mut Vec<CodeActionOrCommand>,
) {
    let NodeKind::List(children) = &node.kind else {
        return;
    };

    // Pattern 1: (not (and/or a b ...)) → (or/and (not a) (not b) ...)
    if children.len() == 2 {
        if let NodeKind::Atom(Atom::Symbol { ns: None, name }) = &children[0].kind {
            if name == "not" {
                if let NodeKind::List(inner) = &children[1].kind {
                    if inner.len() >= 3 {
                        if let NodeKind::Atom(Atom::Symbol { ns: None, name: op }) =
                            &inner[0].kind
                        {
                            let (flipped, label) = match op.as_str() {
                                "and" => ("or", "Apply De Morgan's law"),
                                "or" => ("and", "Apply De Morgan's law"),
                                _ => return,
                            };
                            let negated: Vec<String> = inner[1..]
                                .iter()
                                .map(|n| format!("(not {})", node_source(n, source)))
                                .collect();
                            let new_text =
                                format!("({flipped} {})", negated.join(" "));
                            let edit = make_edit(uri, node, source, new_text);
                            actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                                title: label.to_string(),
                                kind: Some(CodeActionKind::REFACTOR_REWRITE),
                                edit: Some(edit),
                                ..Default::default()
                            }));
                        }
                    }
                }
            }
        }
    }

    // Pattern 2: (and/or (not a) (not b)) → (not (or/and a b))
    if children.len() >= 3 {
        if let NodeKind::Atom(Atom::Symbol { ns: None, name }) = &children[0].kind {
            let flipped = match name.as_str() {
                "and" => "or",
                "or" => "and",
                _ => return,
            };

            // Check all args are (not x)
            let mut unwrapped = Vec::new();
            for arg in &children[1..] {
                if let NodeKind::List(inner) = &arg.kind {
                    if inner.len() == 2 {
                        if let NodeKind::Atom(Atom::Symbol { ns: None, name: n }) = &inner[0].kind
                        {
                            if n == "not" {
                                unwrapped.push(node_source(&inner[1], source));
                                continue;
                            }
                        }
                    }
                }
                return; // Not all args are (not x)
            }

            let new_text = format!("(not ({flipped} {}))", unwrapped.join(" "));
            let edit = make_edit(uri, node, source, new_text);
            actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                title: "Apply De Morgan's law".to_string(),
                kind: Some(CodeActionKind::REFACTOR_REWRITE),
                edit: Some(edit),
                ..Default::default()
            }));
        }
    }
}

// ---------------------------------------------------------------------------
// Convert (str ...) to interpolated string
// ---------------------------------------------------------------------------

/// Offer "Convert to interpolated string" on `(str "Hello, " name "!")`
/// → `"Hello, {name}!"`.
fn str_to_interpolation_action(
    node: &Node,
    source: &str,
    uri: &Url,
    actions: &mut Vec<CodeActionOrCommand>,
) {
    let NodeKind::List(children) = &node.kind else {
        return;
    };
    if children.len() < 3 {
        return;
    }

    // Head must be `str`
    let is_str = matches!(
        &children[0].kind,
        NodeKind::Atom(Atom::Symbol { ns: None, name }) if name == "str"
    );
    if !is_str {
        return;
    }

    let args = &children[1..];

    // Must have at least one non-string arg (otherwise no point interpolating)
    let has_expr = args.iter().any(|a| !matches!(&a.kind, NodeKind::Atom(Atom::Str(_))));
    if !has_expr {
        return;
    }

    // Build the interpolated string
    let mut result = String::new();
    for arg in args {
        match &arg.kind {
            NodeKind::Atom(Atom::Str(s)) => {
                // Escape any literal { or } in the string content
                for ch in s.chars() {
                    match ch {
                        '{' => result.push_str("{{"),
                        '}' => result.push_str("}}"),
                        _ => result.push(ch),
                    }
                }
            }
            _ => {
                // Interpolate the expression
                let expr_src = node_source(arg, source);
                result.push('{');
                result.push_str(expr_src);
                result.push('}');
            }
        }
    }

    let new_text = format!("\"{result}\"");
    // Don't re-format interpolated strings — they're already in final form.
    let range = span_to_range(source, node.span);
    let edit = TextEdit {
        range,
        new_text: new_text.clone(),
    };
    let mut changes = HashMap::new();
    changes.insert(uri.clone(), vec![edit]);
    let workspace_edit = WorkspaceEdit {
        changes: Some(changes),
        ..Default::default()
    };

    actions.push(CodeActionOrCommand::CodeAction(CodeAction {
        title: "Convert to interpolated string".to_string(),
        kind: Some(CodeActionKind::REFACTOR_REWRITE),
        edit: Some(workspace_edit),
        ..Default::default()
    }));
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                definition_provider: Some(OneOf::Left(true)),
                references_provider: Some(OneOf::Left(true)),
                completion_provider: Some(CompletionOptions::default()),
                document_formatting_provider: Some(OneOf::Left(true)),
                document_symbol_provider: Some(OneOf::Left(true)),
                code_action_provider: Some(CodeActionProviderCapability::Options(
                    CodeActionOptions {
                        code_action_kinds: Some(vec![
                            CodeActionKind::QUICKFIX,
                            CodeActionKind::REFACTOR_REWRITE,
                            CodeActionKind::REFACTOR_EXTRACT,
                            CodeActionKind::REFACTOR_INLINE,
                            CodeActionKind::SOURCE,
                        ]),
                        ..Default::default()
                    },
                )),
                ..Default::default()
            },
            ..Default::default()
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "nexl language server initialized")
            .await;
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let doc = params.text_document;
        let uri = doc.uri.clone();
        let version = doc.version;
        let text = doc.text.clone();
        self.documents.insert(
            uri.clone(),
            TextDocumentItem {
                uri: uri.clone(),
                language_id: doc.language_id,
                version,
                text: text.clone(),
            },
        );
        self.publish_diagnostics(&uri, &text, Some(version)).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;
        let change = match params.content_changes.into_iter().last() {
            Some(change) => change,
            None => return,
        };
        let mut doc = match self.documents.get_mut(&uri) {
            Some(doc) => doc,
            None => return,
        };
        doc.text = change.text;
        doc.version = params.text_document.version;
        let text = doc.text.clone();
        let version = doc.version;
        drop(doc);
        self.publish_diagnostics(&uri, &text, Some(version)).await;
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri;
        self.documents.remove(&uri);
        self.client.publish_diagnostics(uri, Vec::new(), None).await;
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let text_document = params.text_document_position_params.text_document;
        let position = params.text_document_position_params.position;
        let source = match self.get_document_text(&text_document.uri) {
            Some(source) => source,
            None => return Ok(None),
        };
        let offset = position_to_offset(&source, position);
        let nodes = match nexl_reader::read(&source, FileId(0)) {
            Ok(nodes) => nodes,
            Err(_) => return Ok(None),
        };
        let file_path = text_document.uri.to_file_path().ok();
        Ok(hover_for_offset(&nodes, offset, &source, file_path.as_deref()))
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let text_document = params.text_document_position_params.text_document;
        let position = params.text_document_position_params.position;
        let source = match self.get_document_text(&text_document.uri) {
            Some(source) => source,
            None => return Ok(None),
        };
        let offset = position_to_offset(&source, position);
        let nodes = match nexl_reader::read(&source, FileId(0)) {
            Ok(nodes) => nodes,
            Err(_) => return Ok(None),
        };
        let symbol_node = match find_symbol_at_offset(&nodes, offset) {
            Some(node) => node,
            None => return Ok(None),
        };

        // Decompose the symbol into namespace (alias) and bare name.
        let (ns, bare_name) = match &symbol_node.kind {
            NodeKind::Atom(Atom::Symbol { ns, name }) => (ns.as_deref(), name.as_str()),
            _ => return Ok(None),
        };
        let full_name = match ns {
            Some(prefix) => format!("{prefix}/{bare_name}"),
            None => bare_name.to_string(),
        };

        // 1. Try same-file lookup first (existing behaviour).
        if let Some(range) = find_definition_range(&nodes, &full_name, &source) {
            let location = Location {
                uri: text_document.uri,
                range,
            };
            return Ok(Some(GotoDefinitionResponse::Scalar(location)));
        }

        // 2. Cross-module resolution — requires a project context.
        let file_path = text_document.uri.to_file_path().ok();
        let ctx = file_path.as_deref().and_then(resolve_project_context);
        let (ctx, imports) = match (ctx, extract_module_imports(&nodes)) {
            (Some(ctx), Some(imports)) => (ctx, imports),
            _ => return Ok(None),
        };

        let resolved = if let Some(alias) = ns {
            // Qualified: alias/name → find module for alias → look up bare_name.
            find_module_for_alias(&imports, alias)
                .and_then(|mp| resolve_module_to_file_path(mp, &ctx))
                .and_then(|fp| find_definition_in_file(&fp, bare_name))
        } else {
            // Unqualified: find which import brings this name into scope.
            find_import_for_unqualified_name(&imports, bare_name).and_then(|(mp, orig)| {
                let fp = resolve_module_to_file_path(mp, &ctx)?;
                find_definition_in_file(&fp, &orig)
            })
        };

        match resolved {
            Some((url, range)) => Ok(Some(GotoDefinitionResponse::Scalar(Location {
                uri: url,
                range,
            }))),
            None => Ok(None),
        }
    }

    async fn references(&self, params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        let uri = params.text_document_position.text_document.uri.clone();
        let position = params.text_document_position.position;
        let source = match self.get_document_text(&uri) {
            Some(s) => s,
            None => return Ok(None),
        };
        let offset = position_to_offset(&source, position);
        let nodes = match nexl_reader::read(&source, FileId(0)) {
            Ok(n) => n,
            Err(_) => return Ok(None),
        };
        let symbol_node = match find_symbol_at_offset(&nodes, offset) {
            Some(n) => n,
            None => return Ok(None),
        };
        let full_name = match symbol_name(symbol_node) {
            Some(n) => n,
            None => return Ok(None),
        };

        // Collect all occurrences in the current file.
        let current_uri = uri.clone();
        let mut locations: Vec<Location> = collect_symbol_uses(&nodes, &full_name, &source)
            .into_iter()
            .map(|range| Location { uri: current_uri.clone(), range })
            .collect();

        // If the caller does not want the declaration, filter it out.
        if !params.context.include_declaration {
            let def_range = find_definition_range(&nodes, &full_name, &source);
            locations.retain(|loc| Some(loc.range) != def_range);
        }

        // Cross-project search: walk every other .nx file in the source tree.
        let file_path = uri.to_file_path().ok();
        if let (Some(fp), Some(ctx)) = (&file_path, file_path.as_deref().and_then(resolve_project_context)) {
            collect_references_across_project(
                &ctx.source_root,
                &full_name,
                fp,
                &mut locations,
            );
        }

        if locations.is_empty() {
            Ok(None)
        } else {
            Ok(Some(locations))
        }
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let text_document = params.text_document_position.text_document;
        let source = match self.get_document_text(&text_document.uri) {
            Some(source) => source,
            None => return Ok(None),
        };
        let nodes = match nexl_reader::read(&source, FileId(0)) {
            Ok(nodes) => nodes,
            Err(_) => return Ok(None),
        };

        let position = params.text_document_position.position;
        let offset = position_to_offset(&source, position);

        // If cursor is inside :imports, complete module names.
        if is_in_imports_context(&nodes, offset) {
            let mut items = stdlib_module_completions();
            if let Ok(file_path) = text_document.uri.to_file_path() {
                items.extend(project_module_completions(&file_path));
            }
            return Ok(Some(CompletionResponse::Array(items)));
        }

        let mut items = completion_items(&nodes);
        // Include record field names from deftype declarations.
        items.extend(record_field_completions(&nodes));
        // Include qualified stdlib function names (json/encode, http/get, etc.).
        items.extend(stdlib_function_completions());
        Ok(Some(CompletionResponse::Array(items)))
    }

    async fn formatting(&self, params: DocumentFormattingParams) -> Result<Option<Vec<TextEdit>>> {
        let source = match self.get_document_text(&params.text_document.uri) {
            Some(source) => source,
            None => return Ok(None),
        };

        let nodes = match nexl_reader::read(&source, FileId(0)) {
            Ok(nodes) => nodes,
            Err(_) => return Ok(None), // Don't format broken files
        };

        let tab_size = params.options.tab_size as usize;
        let config = nexl_ast::printer::PrintConfig {
            indent_width: tab_size,
            ..nexl_ast::printer::PrintConfig::default()
        };
        let printer = nexl_ast::printer::PrettyPrinter::new(config);
        let formatted = printer.print_file(&nodes);

        if formatted == source {
            return Ok(Some(Vec::new())); // Already formatted
        }

        // Replace entire document
        let last_line = source.lines().count().saturating_sub(1) as u32;
        let last_char = source.lines().last().map_or(0, |l| l.len()) as u32;
        let edit = TextEdit {
            range: Range::new(Position::new(0, 0), Position::new(last_line, last_char)),
            new_text: formatted,
        };
        Ok(Some(vec![edit]))
    }

    async fn code_action(&self, params: CodeActionParams) -> Result<Option<CodeActionResponse>> {
        let uri = &params.text_document.uri;
        let source = match self.get_document_text(uri) {
            Some(source) => source,
            None => return Ok(None),
        };
        let nodes = match nexl_reader::read(&source, FileId(0)) {
            Ok(nodes) => nodes,
            Err(_) => return Ok(None),
        };

        let mut actions = Vec::new();
        let range = params.range;
        let offset = position_to_offset(&source, range.start);

        collect_code_actions(&nodes, &source, offset, range, uri, &mut actions);

        if actions.is_empty() {
            Ok(Some(Vec::new()))
        } else {
            Ok(Some(actions))
        }
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        let source = match self.get_document_text(&params.text_document.uri) {
            Some(source) => source,
            None => return Ok(None),
        };
        let nodes = match nexl_reader::read(&source, FileId(0)) {
            Ok(nodes) => nodes,
            Err(_) => return Ok(None),
        };
        let symbols = collect_document_symbols(&nodes, &source);
        Ok(Some(DocumentSymbolResponse::Nested(symbols)))
    }
}

/// Start the LSP server on stdin/stdout.
pub async fn run_server() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(Backend::new);
    Server::new(stdin, stdout, socket).serve(service).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;
    use serde_json::json;
    use std::time::Duration;
    use tower::Service;
    use tower::ServiceExt;
    use tower_lsp::ClientSocket;
    use tower_lsp::jsonrpc::Request;
    use tower_lsp::lsp_types::notification::Notification;
    use tower_lsp::lsp_types::notification::PublishDiagnostics;

    async fn initialize_service(service: &mut LspService<Backend>) {
        let request = Request::build("initialize")
            .params(json!({"capabilities": {}}))
            .id(1)
            .finish();
        let response = service
            .ready()
            .await
            .expect("service should be ready")
            .call(request)
            .await
            .expect("initialize request should succeed");
        assert!(response.is_some());
    }

    async fn next_publish_diagnostics(socket: &mut ClientSocket) -> PublishDiagnosticsParams {
        let request = tokio::time::timeout(Duration::from_secs(1), socket.next())
            .await
            .expect("publishDiagnostics timeout")
            .expect("publishDiagnostics message");
        let (method, _id, params) = request.into_parts();
        assert_eq!(method.as_ref(), PublishDiagnostics::METHOD);
        let params = params.expect("publishDiagnostics params");
        serde_json::from_value(params).expect("publishDiagnostics params decode")
    }

    #[tokio::test]
    async fn test_initialize_returns_capabilities() {
        let (service, _socket) = LspService::new(Backend::new);
        let backend = service.inner();
        let result = backend
            .initialize(InitializeParams::default())
            .await
            .expect("initialize should succeed");

        let caps = result.capabilities;

        // Text document sync should be Full
        assert_eq!(
            caps.text_document_sync,
            Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL))
        );

        // Hover should be enabled
        assert_eq!(
            caps.hover_provider,
            Some(HoverProviderCapability::Simple(true))
        );

        // Definition should be enabled
        assert_eq!(caps.definition_provider, Some(OneOf::Left(true)));

        // Completion should be enabled
        assert!(caps.completion_provider.is_some());

        // Formatting should be enabled
        assert_eq!(caps.document_formatting_provider, Some(OneOf::Left(true)));

        // Code actions should be enabled
        assert!(caps.code_action_provider.is_some());
    }

    /// Helper: get code actions for source at a byte offset.
    fn get_actions_at(source: &str, offset: usize) -> Vec<CodeActionOrCommand> {
        let nodes = nexl_reader::read(source, FileId(0)).unwrap();
        let pos = offset_to_position(source, offset);
        let range = Range::new(pos, pos);
        let uri = test_uri("test.nx");
        let mut actions = Vec::new();
        collect_code_actions(&nodes, source, offset, range, &uri, &mut actions);
        actions
    }

    /// Helper: find a code action by title substring.
    fn find_action<'a>(
        actions: &'a [CodeActionOrCommand],
        title_contains: &str,
    ) -> Option<&'a CodeAction> {
        actions.iter().find_map(|a| match a {
            CodeActionOrCommand::CodeAction(ca) if ca.title.contains(title_contains) => Some(ca),
            _ => None,
        })
    }

    /// Helper: extract the new text from a code action's single-edit.
    fn action_new_text(action: &CodeAction) -> &str {
        let edit = action.edit.as_ref().expect("action should have edit");
        let changes = edit.changes.as_ref().expect("edit should have changes");
        let edits = changes.values().next().expect("should have at least one file");
        &edits[0].new_text
    }

    fn code_action_params(uri: Url, range: Range) -> CodeActionParams {
        CodeActionParams {
            text_document: TextDocumentIdentifier { uri },
            range,
            context: CodeActionContext {
                diagnostics: Vec::new(),
                only: None,
                trigger_kind: None,
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        }
    }

    #[tokio::test]
    async fn code_action_empty_for_clean_file() {
        let (service, _socket) = LspService::new(Backend::new);
        let backend = service.inner();
        let uri = test_uri("clean.nx");
        backend
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "nexl".to_string(),
                    version: 1,
                    text: "(def x 42)".to_string(),
                },
            })
            .await;

        let result = backend
            .code_action(code_action_params(
                uri,
                Range::new(Position::new(0, 0), Position::new(0, 0)),
            ))
            .await
            .expect("code_action should succeed");

        // Clean file with cursor not on any actionable form → empty list
        assert_eq!(result, Some(Vec::new()));
    }

    #[tokio::test]
    async fn code_action_returns_none_for_unknown_doc() {
        let (service, _socket) = LspService::new(Backend::new);
        let backend = service.inner();
        let uri = test_uri("nonexistent.nx");

        let result = backend
            .code_action(code_action_params(
                uri,
                Range::new(Position::new(0, 0), Position::new(0, 0)),
            ))
            .await
            .expect("code_action should succeed");

        assert_eq!(result, None);
    }

    // -----------------------------------------------------------------------
    // B: Thread first / Thread last
    // -----------------------------------------------------------------------

    #[test]
    fn code_action_thread_first_nested_calls() {
        let source = "(f (g (h x)))";
        // cursor on the opening paren of outer call
        let actions = get_actions_at(source, 0);
        let action = find_action(&actions, "Thread first")
            .expect("should offer 'Thread first'");
        assert_eq!(action_new_text(action), "(-> x h g f)");
    }

    #[test]
    fn code_action_thread_last_nested_calls() {
        let source = "(f (g (h x)))";
        let actions = get_actions_at(source, 0);
        let action = find_action(&actions, "Thread last")
            .expect("should offer 'Thread last'");
        assert_eq!(action_new_text(action), "(->> x h g f)");
    }

    #[test]
    fn code_action_unwind_thread_first() {
        let source = "(-> x h g f)";
        let actions = get_actions_at(source, 0);
        let action = find_action(&actions, "Unwind threading")
            .expect("should offer 'Unwind threading'");
        assert_eq!(action_new_text(action), "(f (g (h x)))");
    }

    #[test]
    fn code_action_defn_convert_with_cursor_inside() {
        // Cursor on 'add' (offset 6), should still offer Convert to def
        // because the ancestor (defn ...) list is checked.
        let source = "(defn add [a b] (+ a b))";
        let actions = get_actions_at(source, 6); // on 'a' of 'add'
        let action = find_action(&actions, "Convert to def")
            .expect("should offer 'Convert to def' with cursor inside form");
        assert_eq!(
            action_new_text(action),
            "(def add\n  (fn [a b]\n    (+ a b)))"
        );
    }

    #[test]
    fn code_action_no_threading_on_simple_call() {
        // Single call with no nesting — nothing to thread
        let source = "(f x)";
        let actions = get_actions_at(source, 0);
        assert!(find_action(&actions, "Thread first").is_none());
        assert!(find_action(&actions, "Thread last").is_none());
    }

    #[test]
    fn code_action_no_threading_on_dsl_code() {
        // Every level has extra args — this is DSL/component code, not a pipeline.
        let source = r#"(attrs {:id "card"} (on :dragstart (set-signal :card-id id)))"#;
        let actions = get_actions_at(source, 0);
        assert!(
            find_action(&actions, "Thread").is_none(),
            "should not offer threading on DSL code where every call has extra args"
        );
    }

    #[test]
    fn code_action_threading_with_mixed_args() {
        // (map inc (filter even? xs)) — filter is bare-ish, map has extra arg
        // thread-last: (->> xs (filter even?) (map inc))
        let source = "(map inc (filter even? xs))";
        let actions = get_actions_at(source, 0);
        // This should offer thread-last because filter has a bare nested call
        assert!(find_action(&actions, "Thread last").is_some());
    }

    #[test]
    fn code_action_no_threading_on_special_forms() {
        // defn, if, let etc. should never be offered for threading
        let source = "(defn add [a b] (+ a b))";
        let actions = get_actions_at(source, 0);
        assert!(find_action(&actions, "Thread").is_none());

        let source = "(if cond (foo x) (bar y))";
        let actions = get_actions_at(source, 0);
        assert!(find_action(&actions, "Thread").is_none());
    }

    // -----------------------------------------------------------------------
    // C: Convert def ↔ defn
    // -----------------------------------------------------------------------

    #[test]
    fn code_action_convert_def_fn_to_defn() {
        let source = "(def add (fn [a b] (+ a b)))";
        let actions = get_actions_at(source, 0);
        let action = find_action(&actions, "Convert to defn")
            .expect("should offer 'Convert to defn'");
        assert_eq!(action_new_text(action), "(defn add [a b]\n  (+ a b))");
    }

    #[test]
    fn code_action_convert_defn_to_def_fn() {
        let source = "(defn add [a b] (+ a b))";
        let actions = get_actions_at(source, 0);
        let action = find_action(&actions, "Convert to def")
            .expect("should offer 'Convert to def'");
        assert_eq!(
            action_new_text(action),
            "(def add\n  (fn [a b]\n    (+ a b)))"
        );
    }

    #[test]
    fn code_action_no_convert_on_non_fn_def() {
        let source = "(def x 42)";
        let actions = get_actions_at(source, 0);
        assert!(find_action(&actions, "Convert to defn").is_none());
    }

    // -----------------------------------------------------------------------
    // D: Negate condition
    // -----------------------------------------------------------------------

    #[test]
    fn code_action_negate_if_condition() {
        let source = "(if cond a b)";
        let actions = get_actions_at(source, 0);
        let action = find_action(&actions, "Negate condition")
            .expect("should offer 'Negate condition'");
        assert_eq!(action_new_text(action), "(if (not cond) b a)");
    }

    #[test]
    fn code_action_negate_already_negated() {
        let source = "(if (not cond) a b)";
        let actions = get_actions_at(source, 0);
        let action = find_action(&actions, "Negate condition")
            .expect("should offer 'Negate condition'");
        assert_eq!(action_new_text(action), "(if cond b a)");
    }

    // -----------------------------------------------------------------------
    // E: Flip binary expression
    // -----------------------------------------------------------------------

    #[test]
    fn code_action_flip_comparison() {
        let source = "(< a b)";
        let actions = get_actions_at(source, 0);
        let action = find_action(&actions, "Flip")
            .expect("should offer flip action");
        assert_eq!(action_new_text(action), "(> b a)");
    }

    #[test]
    fn code_action_flip_equality() {
        let source = "(= a b)";
        let actions = get_actions_at(source, 0);
        let action = find_action(&actions, "Flip")
            .expect("should offer flip action");
        assert_eq!(action_new_text(action), "(= b a)");
    }

    // -----------------------------------------------------------------------
    // F: Cycle collection
    // -----------------------------------------------------------------------

    #[test]
    fn code_action_cycle_vector_to_set() {
        let source = "[1 2 3]";
        let actions = get_actions_at(source, 0);
        let action = find_action(&actions, "Convert to set")
            .expect("should offer 'Convert to set'");
        assert_eq!(action_new_text(action), "#{1 2 3}");
    }

    #[test]
    fn code_action_cycle_set_to_vector() {
        let source = "#{1 2 3}";
        let actions = get_actions_at(source, 0);
        let action = find_action(&actions, "Convert to vector")
            .expect("should offer 'Convert to vector'");
        assert_eq!(action_new_text(action), "[1 2 3]");
    }

    #[test]
    fn code_action_no_cycle_on_list() {
        // Lists are calls, not data — don't cycle them.
        let source = "(f x)";
        let actions = get_actions_at(source, 0);
        assert!(find_action(&actions, "Convert to set").is_none());
        assert!(find_action(&actions, "Convert to vector").is_none());
    }

    // -----------------------------------------------------------------------
    // G: Extract variable
    // -----------------------------------------------------------------------

    #[test]
    fn code_action_extract_variable() {
        // Cursor on (+ a b) inside a call — offer to extract it
        let source = "(f (+ a b) c)";
        // offset 3 = opening paren of (+ a b)
        let actions = get_actions_at(source, 3);
        let action = find_action(&actions, "Extract variable")
            .expect("should offer 'Extract variable'");
        assert_eq!(action_new_text(action), "(let [x (+ a b)] (f x c))");
    }

    #[test]
    fn code_action_extract_variable_into_existing_let() {
        let source = "(let [a 1] (f (+ a 2) b))";
        // offset 14 = opening paren of (+ a 2)
        let actions = get_actions_at(source, 14);
        let action = find_action(&actions, "Extract variable")
            .expect("should offer 'Extract variable'");
        assert_eq!(
            action_new_text(action),
            "(let [a 1\n      x (+ a 2)]\n  (f x b))"
        );
    }

    #[test]
    fn code_action_no_extract_on_atom() {
        // Extracting a single symbol is useless
        let source = "(f x)";
        let actions = get_actions_at(source, 3); // on 'x'
        assert!(find_action(&actions, "Extract variable").is_none());
    }

    // -----------------------------------------------------------------------
    // H: Convert if ↔ cond
    // -----------------------------------------------------------------------

    #[test]
    fn code_action_convert_if_chain_to_cond() {
        let source = "(if a 1 (if b 2 3))";
        let actions = get_actions_at(source, 0);
        let action = find_action(&actions, "Convert to cond")
            .expect("should offer 'Convert to cond'");
        assert_eq!(action_new_text(action), "(cond a 1 b 2 :else 3)");
    }

    #[test]
    fn code_action_convert_cond_to_if() {
        let source = "(cond a 1 b 2 :else 3)";
        let actions = get_actions_at(source, 0);
        let action = find_action(&actions, "Convert to if")
            .expect("should offer 'Convert to if'");
        assert_eq!(
            action_new_text(action),
            "(if a\n  1\n  (if b 2 3))"
        );
    }

    #[test]
    fn code_action_no_cond_on_simple_if() {
        // Single if with no else-if chain — no point converting to cond
        let source = "(if a 1 2)";
        let actions = get_actions_at(source, 0);
        assert!(find_action(&actions, "Convert to cond").is_none());
    }

    // -----------------------------------------------------------------------
    // I: Wrap/unwrap in anonymous function
    // -----------------------------------------------------------------------

    #[test]
    fn code_action_wrap_in_fn() {
        let source = "(map inc xs)";
        // cursor on `inc` (offset 5)
        let actions = get_actions_at(source, 5);
        let action = find_action(&actions, "Wrap in fn")
            .expect("should offer 'Wrap in fn'");
        assert_eq!(action_new_text(action), "(fn [x]\n  (inc x))");
    }

    // -----------------------------------------------------------------------
    // J: De Morgan's law
    // -----------------------------------------------------------------------

    #[test]
    fn code_action_demorgan_not_and() {
        let source = "(not (and a b))";
        let actions = get_actions_at(source, 0);
        let action = find_action(&actions, "De Morgan")
            .expect("should offer De Morgan's law");
        assert_eq!(action_new_text(action), "(or (not a) (not b))");
    }

    #[test]
    fn code_action_demorgan_not_or() {
        let source = "(not (or a b))";
        let actions = get_actions_at(source, 0);
        let action = find_action(&actions, "De Morgan")
            .expect("should offer De Morgan's law");
        assert_eq!(action_new_text(action), "(and (not a) (not b))");
    }

    #[test]
    fn code_action_demorgan_reverse() {
        // (and (not a) (not b)) → (not (or a b))
        let source = "(and (not a) (not b))";
        let actions = get_actions_at(source, 0);
        let action = find_action(&actions, "De Morgan")
            .expect("should offer De Morgan's law reverse");
        assert_eq!(action_new_text(action), "(not (or a b))");
    }

    // -----------------------------------------------------------------------
    // K: Convert str to interpolated string
    // -----------------------------------------------------------------------

    #[test]
    fn code_action_str_to_interpolation_basic() {
        let source = r#"(str "Hello, " name "!")"#;
        let actions = get_actions_at(source, 0);
        let action = find_action(&actions, "interpolated string")
            .expect("should offer interpolation");
        assert_eq!(action_new_text(action), r#""Hello, {name}!""#);
    }

    #[test]
    fn code_action_str_to_interpolation_multiple_exprs() {
        let source = r#"(str "Status: " code " (" msg ")")"#;
        let actions = get_actions_at(source, 0);
        let action = find_action(&actions, "interpolated string")
            .expect("should offer interpolation");
        assert_eq!(action_new_text(action), r#""Status: {code} ({msg})""#);
    }

    #[test]
    fn code_action_str_to_interpolation_expr_call() {
        let source = r#"(str "Count: " (count xs))"#;
        let actions = get_actions_at(source, 0);
        let action = find_action(&actions, "interpolated string")
            .expect("should offer interpolation");
        assert_eq!(action_new_text(action), r#""Count: {(count xs)}""#);
    }

    #[test]
    fn code_action_no_interpolation_on_all_strings() {
        // All args are strings — no point converting
        let source = r#"(str "Hello" " " "world")"#;
        let actions = get_actions_at(source, 0);
        assert!(find_action(&actions, "interpolated string").is_none());
    }

    fn test_uri(name: &str) -> Url {
        Url::parse(&format!("file:///tmp/{name}")).expect("valid url")
    }

    #[tokio::test]
    async fn test_did_open_stores_document() {
        let (service, _socket) = LspService::new(Backend::new);
        let backend = service.inner();
        let uri = test_uri("test.nexl");

        // Before open: no document
        assert!(backend.get_document_text(&uri).is_none());

        backend
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "nexl".to_string(),
                    version: 1,
                    text: "(def x 42)".to_string(),
                },
            })
            .await;

        // After open: document present
        assert_eq!(
            backend.get_document_text(&uri),
            Some("(def x 42)".to_string())
        );
    }

    #[tokio::test]
    async fn test_did_close_removes_document() {
        let (service, _socket) = LspService::new(Backend::new);
        let backend = service.inner();
        let uri = test_uri("close.nexl");

        // Open a document
        backend
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "nexl".to_string(),
                    version: 1,
                    text: "(+ 1 2)".to_string(),
                },
            })
            .await;
        assert!(backend.get_document_text(&uri).is_some());

        // Close it
        backend
            .did_close(DidCloseTextDocumentParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
            })
            .await;

        // Should be gone
        assert!(backend.get_document_text(&uri).is_none());
    }

    #[tokio::test]
    async fn test_did_change_updates_document() {
        let (service, _socket) = LspService::new(Backend::new);
        let backend = service.inner();
        let uri = test_uri("change.nexl");

        // Open with initial text
        backend
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "nexl".to_string(),
                    version: 1,
                    text: "(def x 1)".to_string(),
                },
            })
            .await;

        // Change to new text (full sync)
        backend
            .did_change(DidChangeTextDocumentParams {
                text_document: VersionedTextDocumentIdentifier {
                    uri: uri.clone(),
                    version: 2,
                },
                content_changes: vec![TextDocumentContentChangeEvent {
                    range: None,
                    range_length: None,
                    text: "(def x 42)".to_string(),
                }],
            })
            .await;

        assert_eq!(
            backend.get_document_text(&uri),
            Some("(def x 42)".to_string())
        );
    }

    #[tokio::test]
    async fn test_publish_diagnostics_parse_error() {
        let (mut service, mut socket) = LspService::new(Backend::new);
        initialize_service(&mut service).await;
        let backend = service.inner();
        let uri = test_uri("parse-error.nexl");

        backend
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "nexl".to_string(),
                    version: 1,
                    text: "(".to_string(),
                },
            })
            .await;

        let params = next_publish_diagnostics(&mut socket).await;
        assert_eq!(params.uri, uri);
        assert_eq!(params.diagnostics.len(), 1);
        let diag = &params.diagnostics[0];
        assert_eq!(diag.severity, Some(DiagnosticSeverity::ERROR));
        assert!(diag.message.contains("unclosed"));
        assert_eq!(diag.range.start.line, 0);
        assert_eq!(diag.range.start.character, 0);
        assert_eq!(diag.range.end.line, 0);
        assert_eq!(diag.range.end.character, 1);
    }

    #[tokio::test]
    async fn test_publish_diagnostics_type_error() {
        let (mut service, mut socket) = LspService::new(Backend::new);
        initialize_service(&mut service).await;
        let backend = service.inner();
        let uri = test_uri("type-error.nexl");

        // (def) is a malformed form — too few elements.
        backend
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "nexl".to_string(),
                    version: 1,
                    text: "(def)".to_string(),
                },
            })
            .await;

        let params = next_publish_diagnostics(&mut socket).await;
        assert_eq!(params.uri, uri);
        assert_eq!(params.diagnostics.len(), 1);
        let diag = &params.diagnostics[0];
        assert_eq!(diag.severity, Some(DiagnosticSeverity::ERROR));
        assert!(diag.message.contains("def expects"));
    }

    #[tokio::test]
    async fn test_unbound_variable_suppressed() {
        let (mut service, mut socket) = LspService::new(Backend::new);
        initialize_service(&mut service).await;
        let backend = service.inner();
        let uri = test_uri("unbound.nexl");

        // A bare unknown symbol would normally produce UnboundVariable,
        // but the LSP suppresses those since stdlib types aren't loaded.
        backend
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "nexl".to_string(),
                    version: 1,
                    text: "unknown".to_string(),
                },
            })
            .await;

        let params = next_publish_diagnostics(&mut socket).await;
        assert_eq!(params.uri, uri);
        assert_eq!(params.diagnostics.len(), 0);
    }

    #[tokio::test]
    async fn test_did_change_clears_diagnostics() {
        let (mut service, mut socket) = LspService::new(Backend::new);
        initialize_service(&mut service).await;
        let backend = service.inner();
        let uri = test_uri("change-clear.nexl");

        backend
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "nexl".to_string(),
                    version: 1,
                    text: "unknown".to_string(),
                },
            })
            .await;
        let _ = next_publish_diagnostics(&mut socket).await;

        backend
            .did_change(DidChangeTextDocumentParams {
                text_document: VersionedTextDocumentIdentifier {
                    uri: uri.clone(),
                    version: 2,
                },
                content_changes: vec![TextDocumentContentChangeEvent {
                    range: None,
                    range_length: None,
                    text: "42".to_string(),
                }],
            })
            .await;

        let params = next_publish_diagnostics(&mut socket).await;
        assert_eq!(params.uri, uri);
        assert!(params.diagnostics.is_empty());
    }

    fn hover_value(hover: &Hover) -> &str {
        match &hover.contents {
            HoverContents::Markup(content) => content.value.as_str(),
            other => panic!("expected markup hover, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_hover_defn_includes_type_and_docstring() {
        let (service, _socket) = LspService::new(Backend::new);
        let backend = service.inner();
        let uri = test_uri("hover-defn.nexl");
        let source = "(defn one \"One.\" [] 1)";

        backend
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "nexl".to_string(),
                    version: 1,
                    text: source.to_string(),
                },
            })
            .await;

        let offset = source.find("one").expect("one in source");
        let position = offset_to_position(source, offset);
        let hover = backend
            .hover(HoverParams {
                text_document_position_params: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: uri.clone() },
                    position,
                },
                work_done_progress_params: Default::default(),
            })
            .await
            .expect("hover request")
            .expect("hover result");

        let value = hover_value(&hover);
        assert!(value.contains("one : (Fn [] -> Int)"));
        assert!(value.contains("One."));
        let end = offset_to_position(source, offset + "one".len());
        assert_eq!(hover.range, Some(Range::new(position, end)));
    }

    #[tokio::test]
    async fn test_hover_def_includes_type() {
        let (service, _socket) = LspService::new(Backend::new);
        let backend = service.inner();
        let uri = test_uri("hover-def.nexl");
        let source = "(def answer 42)";

        backend
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "nexl".to_string(),
                    version: 1,
                    text: source.to_string(),
                },
            })
            .await;

        let offset = source.find("answer").expect("answer in source");
        let position = offset_to_position(source, offset);
        let hover = backend
            .hover(HoverParams {
                text_document_position_params: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: uri.clone() },
                    position,
                },
                work_done_progress_params: Default::default(),
            })
            .await
            .expect("hover request")
            .expect("hover result");

        let value = hover_value(&hover);
        assert!(value.contains("answer : Int"));
    }

    #[tokio::test]
    async fn test_definition_returns_defn_location() {
        let (service, _socket) = LspService::new(Backend::new);
        let backend = service.inner();
        let uri = test_uri("definition-defn.nexl");
        let source = "(defn one [] 1)\n(one)";

        backend
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "nexl".to_string(),
                    version: 1,
                    text: source.to_string(),
                },
            })
            .await;

        let usage_offset = source.rfind("one").expect("one usage");
        let usage_position = offset_to_position(source, usage_offset);
        let response = backend
            .goto_definition(GotoDefinitionParams {
                text_document_position_params: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: uri.clone() },
                    position: usage_position,
                },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            })
            .await
            .expect("definition request")
            .expect("definition result");

        let expected_start = offset_to_position(source, source.find("one").expect("one def"));
        let expected_end =
            offset_to_position(source, source.find("one").expect("one def") + "one".len());
        let expected = Location {
            uri: uri.clone(),
            range: Range::new(expected_start, expected_end),
        };

        match response {
            GotoDefinitionResponse::Scalar(location) => assert_eq!(location, expected),
            GotoDefinitionResponse::Array(locations) => {
                assert_eq!(locations.len(), 1);
                assert_eq!(locations[0], expected);
            }
            GotoDefinitionResponse::Link(_) => panic!("unexpected link response"),
        }
    }

    #[tokio::test]
    async fn test_definition_none_for_unknown_symbol() {
        let (service, _socket) = LspService::new(Backend::new);
        let backend = service.inner();
        let uri = test_uri("definition-none.nexl");
        let source = "unknown";

        backend
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "nexl".to_string(),
                    version: 1,
                    text: source.to_string(),
                },
            })
            .await;

        let position = offset_to_position(source, 0);
        let response = backend
            .goto_definition(GotoDefinitionParams {
                text_document_position_params: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: uri.clone() },
                    position,
                },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            })
            .await
            .expect("definition request");

        assert!(response.is_none());
    }

    #[tokio::test]
    async fn test_completion_includes_defs() {
        let (service, _socket) = LspService::new(Backend::new);
        let backend = service.inner();
        let uri = test_uri("completion.nexl");
        let source = "(defn one [] 1)\n(def answer 42)\n";

        backend
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "nexl".to_string(),
                    version: 1,
                    text: source.to_string(),
                },
            })
            .await;

        let position = offset_to_position(source, source.len());
        let response = backend
            .completion(CompletionParams {
                text_document_position: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: uri.clone() },
                    position,
                },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
                context: None,
            })
            .await
            .expect("completion request")
            .expect("completion result");

        let items = match response {
            CompletionResponse::Array(items) => items,
            CompletionResponse::List(list) => list.items,
        };

        let one = items.iter().find(|item| item.label == "one");
        assert!(matches!(
            one.and_then(|item| item.kind),
            Some(CompletionItemKind::FUNCTION)
        ));

        let answer = items.iter().find(|item| item.label == "answer");
        assert!(matches!(
            answer.and_then(|item| item.kind),
            Some(CompletionItemKind::VARIABLE)
        ));
    }

    fn formatting_params(uri: Url) -> DocumentFormattingParams {
        DocumentFormattingParams {
            text_document: TextDocumentIdentifier { uri },
            options: FormattingOptions {
                tab_size: 2,
                insert_spaces: true,
                ..FormattingOptions::default()
            },
            work_done_progress_params: Default::default(),
        }
    }

    #[tokio::test]
    async fn test_formatting_returns_edits() {
        let (service, _socket) = LspService::new(Backend::new);
        let backend = service.inner();
        let uri = test_uri("format.nexl");
        // Multi-form file without trailing newlines
        let source = "(def x 1)\n(def y 2)";

        backend
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "nexl".to_string(),
                    version: 1,
                    text: source.to_string(),
                },
            })
            .await;

        let result = backend
            .formatting(formatting_params(uri))
            .await
            .expect("formatting request")
            .expect("formatting result");

        assert!(
            !result.is_empty(),
            "should return edits for unformatted file"
        );
        let edit = &result[0];
        assert!(edit.new_text.ends_with('\n'));
    }

    #[tokio::test]
    async fn test_formatting_none_on_parse_error() {
        let (service, _socket) = LspService::new(Backend::new);
        let backend = service.inner();
        let uri = test_uri("format-error.nexl");

        backend
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "nexl".to_string(),
                    version: 1,
                    text: "(".to_string(),
                },
            })
            .await;

        let result = backend
            .formatting(formatting_params(uri))
            .await
            .expect("formatting request");

        assert!(result.is_none(), "should return None for parse error");
    }

    #[tokio::test]
    async fn test_formatting_noop_already_formatted() {
        let (service, _socket) = LspService::new(Backend::new);
        let backend = service.inner();
        let uri = test_uri("format-noop.nexl");
        let source = "(def x 1)\n";

        backend
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "nexl".to_string(),
                    version: 1,
                    text: source.to_string(),
                },
            })
            .await;

        let result = backend
            .formatting(formatting_params(uri))
            .await
            .expect("formatting request")
            .expect("formatting result");

        assert!(
            result.is_empty(),
            "should return empty edits for already-formatted file"
        );
    }

    // ── Hover on usage sites ────────────────────────────────────────────

    #[tokio::test]
    async fn test_hover_usage_site_shows_type() {
        let (service, _socket) = LspService::new(Backend::new);
        let backend = service.inner();
        let uri = test_uri("hover-usage.nexl");
        let source = "(defn add1 [x] (+ x 1))\n(add1 5)";

        backend
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "nexl".to_string(),
                    version: 1,
                    text: source.to_string(),
                },
            })
            .await;

        // Hover on the usage of `add1` in `(add1 5)`, not the definition
        let usage_offset = source.rfind("add1").expect("add1 usage");
        let position = offset_to_position(source, usage_offset);
        let hover = backend
            .hover(HoverParams {
                text_document_position_params: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: uri.clone() },
                    position,
                },
                work_done_progress_params: Default::default(),
            })
            .await
            .expect("hover request")
            .expect("hover result");

        let value = hover_value(&hover);
        assert!(value.contains("add1"), "should contain the name");
    }

    #[tokio::test]
    async fn test_hover_usage_site_shows_docstring() {
        let (service, _socket) = LspService::new(Backend::new);
        let backend = service.inner();
        let uri = test_uri("hover-usage-doc.nexl");
        let source = "(defn greet \"Says hello.\" [name] name)\n(greet \"world\")";

        backend
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "nexl".to_string(),
                    version: 1,
                    text: source.to_string(),
                },
            })
            .await;

        let usage_offset = source.rfind("greet").expect("greet usage");
        let position = offset_to_position(source, usage_offset);
        let hover = backend
            .hover(HoverParams {
                text_document_position_params: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: uri.clone() },
                    position,
                },
                work_done_progress_params: Default::default(),
            })
            .await
            .expect("hover request")
            .expect("hover result");

        let value = hover_value(&hover);
        assert!(
            value.contains("Says hello."),
            "should contain the docstring"
        );
    }

    // ── Hover on stdlib functions ───────────────────────────────────────

    #[tokio::test]
    async fn test_hover_stdlib_function() {
        let (service, _socket) = LspService::new(Backend::new);
        let backend = service.inner();
        let uri = test_uri("hover-stdlib.nexl");
        let source = "(map inc [1 2 3])";

        backend
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "nexl".to_string(),
                    version: 1,
                    text: source.to_string(),
                },
            })
            .await;

        let offset = source.find("map").expect("map in source");
        let position = offset_to_position(source, offset);
        let hover = backend
            .hover(HoverParams {
                text_document_position_params: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: uri.clone() },
                    position,
                },
                work_done_progress_params: Default::default(),
            })
            .await
            .expect("hover request")
            .expect("hover result");

        let value = hover_value(&hover);
        assert!(value.contains("map"), "should contain the name");
        assert!(
            value.contains("returning a new vector"),
            "should contain stdlib doc"
        );
    }

    #[tokio::test]
    async fn test_hover_qualified_stdlib() {
        let (service, _socket) = LspService::new(Backend::new);
        let backend = service.inner();
        let uri = test_uri("hover-qualified.nexl");
        let source = "(str/split \"a,b\" \",\")";

        backend
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "nexl".to_string(),
                    version: 1,
                    text: source.to_string(),
                },
            })
            .await;

        let offset = source.find("str/split").expect("str/split in source");
        let position = offset_to_position(source, offset);
        let hover = backend
            .hover(HoverParams {
                text_document_position_params: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: uri.clone() },
                    position,
                },
                work_done_progress_params: Default::default(),
            })
            .await
            .expect("hover request")
            .expect("hover result");

        let value = hover_value(&hover);
        assert!(value.contains("str/split"), "should contain the name");
        assert!(
            value.contains("Split a string into a vector"),
            "should contain stdlib doc"
        );
    }

    // ── Hover on special forms ──────────────────────────────────────────

    #[tokio::test]
    async fn test_hover_special_form_let() {
        let (service, _socket) = LspService::new(Backend::new);
        let backend = service.inner();
        let uri = test_uri("hover-let.nexl");
        let source = "(let [x 1] x)";

        backend
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "nexl".to_string(),
                    version: 1,
                    text: source.to_string(),
                },
            })
            .await;

        let offset = source.find("let").expect("let in source");
        let position = offset_to_position(source, offset);
        let hover = backend
            .hover(HoverParams {
                text_document_position_params: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: uri.clone() },
                    position,
                },
                work_done_progress_params: Default::default(),
            })
            .await
            .expect("hover request")
            .expect("hover result");

        let value = hover_value(&hover);
        assert!(value.contains("let"), "should contain the name");
        assert!(
            value.contains("Bind local variables"),
            "should contain special form doc"
        );
    }

    #[tokio::test]
    async fn test_hover_special_form_if() {
        let (service, _socket) = LspService::new(Backend::new);
        let backend = service.inner();
        let uri = test_uri("hover-if.nexl");
        let source = "(if true 1 2)";

        backend
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "nexl".to_string(),
                    version: 1,
                    text: source.to_string(),
                },
            })
            .await;

        let offset = source.find("if").expect("if in source");
        let position = offset_to_position(source, offset);
        let hover = backend
            .hover(HoverParams {
                text_document_position_params: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: uri.clone() },
                    position,
                },
                work_done_progress_params: Default::default(),
            })
            .await
            .expect("hover request")
            .expect("hover result");

        let value = hover_value(&hover);
        assert!(value.contains("if"), "should contain the name");
        assert!(
            value.contains("Conditional branch"),
            "should contain special form doc"
        );
    }

    // ── Unit test for doc lookup functions ───────────────────────────────

    #[test]
    fn test_stdlib_doc_coverage() {
        // Spot-check a few entries exist
        assert!(stdlib_doc("+").is_some());
        assert!(stdlib_doc("str/split").is_some());
        assert!(stdlib_doc("math/pi").is_some());
        assert!(stdlib_doc("io/println").is_some());
        assert!(stdlib_doc("core/identity").is_some());
        assert!(stdlib_doc("nonexistent").is_none());
    }

    #[test]
    fn test_special_form_doc_coverage() {
        assert!(special_form_doc("defn").is_some());
        assert!(special_form_doc("let").is_some());
        assert!(special_form_doc("if").is_some());
        assert!(special_form_doc("match").is_some());
        assert!(special_form_doc("nonexistent").is_none());
    }

    #[test]
    fn test_builtin_type_doc_coverage() {
        // Core primitives
        assert!(builtin_type_doc("Int").is_some());
        assert!(builtin_type_doc("Float").is_some());
        assert!(builtin_type_doc("Str").is_some());
        assert!(builtin_type_doc("Bool").is_some());
        assert!(builtin_type_doc("Unit").is_some());
        // Collections
        assert!(builtin_type_doc("Vec").is_some());
        assert!(builtin_type_doc("Map").is_some());
        assert!(builtin_type_doc("Set").is_some());
        // Result types
        assert!(builtin_type_doc("Option").is_some());
        assert!(builtin_type_doc("Result").is_some());
        // Others
        assert!(builtin_type_doc("Fn").is_some());
        assert!(builtin_type_doc("Never").is_some());
        assert!(builtin_type_doc("Any").is_some());
        // Fixed-width
        assert!(builtin_type_doc("U8").is_some());
        assert!(builtin_type_doc("F32").is_some());
        assert!(builtin_type_doc("Int32").is_some());
        // Unknown
        assert!(builtin_type_doc("NotAType").is_none());
    }

    #[tokio::test]
    async fn test_hover_builtin_type_int() {
        let (service, _socket) = LspService::new(Backend::new);
        let backend = service.inner();
        let uri = test_uri("hover-type.nexl");
        let source = "(defn id [x :- Int] x)";

        backend
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "nexl".to_string(),
                    version: 1,
                    text: source.to_string(),
                },
            })
            .await;

        let offset = source.find("Int").expect("Int in source");
        let position = offset_to_position(source, offset);
        let hover = backend
            .hover(HoverParams {
                text_document_position_params: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: uri.clone() },
                    position,
                },
                work_done_progress_params: Default::default(),
            })
            .await
            .expect("hover request")
            .expect("hover result");

        let value = hover_value(&hover);
        assert!(value.contains("Int"), "should contain type name");
        assert!(value.contains("64-bit"), "should contain Int doc");
    }

    #[tokio::test]
    async fn test_hover_builtin_type_option() {
        let (service, _socket) = LspService::new(Backend::new);
        let backend = service.inner();
        let uri = test_uri("hover-option.nexl");
        let source = "(defn maybe [] (Some 42))";

        backend
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "nexl".to_string(),
                    version: 1,
                    text: source.to_string(),
                },
            })
            .await;

        // Hover over "Option" as a bare symbol
        let source2 = "Option";
        let uri2 = test_uri("hover-option2.nexl");
        backend
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri2.clone(),
                    language_id: "nexl".to_string(),
                    version: 1,
                    text: source2.to_string(),
                },
            })
            .await;
        let position = offset_to_position(source2, 0);
        let hover = backend
            .hover(HoverParams {
                text_document_position_params: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: uri2 },
                    position,
                },
                work_done_progress_params: Default::default(),
            })
            .await
            .expect("hover request")
            .expect("hover result");

        let value = hover_value(&hover);
        assert!(value.contains("Option"), "should contain Option");
        assert!(value.contains("Some"), "should mention Some constructor");
    }

    // ── Cross-module go-to-definition tests ──────────────────────────────

    fn parse_nodes(src: &str) -> Vec<Node> {
        nexl_reader::read(src, FileId(0)).expect("parse failed")
    }

    // Test 1: extract_module_imports from a module declaration
    #[test]
    fn test_extract_module_imports_basic() {
        let nodes = parse_nodes(
            "(module todo.app :imports [[todo.model :as model] [todo.util :refer [helper]]])",
        );
        let imports = extract_module_imports(&nodes).expect("should extract imports");
        assert_eq!(imports.len(), 2);
        assert_eq!(imports[0].module_path, "todo.model");
        assert_eq!(imports[0].kind, ImportKind::Alias("model".to_string()));
        assert_eq!(imports[1].module_path, "todo.util");
        assert_eq!(
            imports[1].kind,
            ImportKind::Refer(vec!["helper".to_string()])
        );
    }

    // Test 2: extract_module_imports returns None for non-module files
    #[test]
    fn test_extract_module_imports_no_module_decl() {
        let nodes = parse_nodes("(defn foo [] 42)");
        assert!(extract_module_imports(&nodes).is_none());
    }

    // Test 3: find_module_for_alias hit
    #[test]
    fn test_find_module_for_alias_found() {
        let imports = vec![
            ImportDecl {
                module_path: "todo.model".to_string(),
                kind: ImportKind::Alias("model".to_string()),
            },
            ImportDecl {
                module_path: "todo.util".to_string(),
                kind: ImportKind::Alias("util".to_string()),
            },
        ];
        assert_eq!(find_module_for_alias(&imports, "model"), Some("todo.model"));
        assert_eq!(find_module_for_alias(&imports, "util"), Some("todo.util"));
    }

    // Test 4: find_module_for_alias miss
    #[test]
    fn test_find_module_for_alias_not_found() {
        let imports = vec![ImportDecl {
            module_path: "todo.model".to_string(),
            kind: ImportKind::Alias("model".to_string()),
        }];
        assert_eq!(find_module_for_alias(&imports, "unknown"), None);
    }

    // Test 5: find_import_for_unqualified_name — :refer
    #[test]
    fn test_find_import_for_unqualified_refer() {
        let imports = vec![ImportDecl {
            module_path: "todo.model".to_string(),
            kind: ImportKind::Refer(vec!["Task".to_string(), "Priority".to_string()]),
        }];
        let result = find_import_for_unqualified_name(&imports, "Task");
        assert_eq!(result, Some(("todo.model", "Task".to_string())));
        assert!(find_import_for_unqualified_name(&imports, "Unknown").is_none());
    }

    // Test 6: find_import_for_unqualified_name — :all
    #[test]
    fn test_find_import_for_unqualified_all() {
        let imports = vec![ImportDecl {
            module_path: "todo.model".to_string(),
            kind: ImportKind::All,
        }];
        let result = find_import_for_unqualified_name(&imports, "anything");
        assert_eq!(result, Some(("todo.model", "anything".to_string())));
    }

    // Test 7: find_import_for_unqualified_name — :exclude
    #[test]
    fn test_find_import_for_unqualified_exclude() {
        let imports = vec![ImportDecl {
            module_path: "todo.model".to_string(),
            kind: ImportKind::Exclude(vec!["internal".to_string()]),
        }];
        // "public-fn" is not excluded, should match
        let result = find_import_for_unqualified_name(&imports, "public-fn");
        assert_eq!(result, Some(("todo.model", "public-fn".to_string())));
        // "internal" is excluded, should not match
        assert!(find_import_for_unqualified_name(&imports, "internal").is_none());
    }

    // Test 8: find_import_for_unqualified_name — :rename
    #[test]
    fn test_find_import_for_unqualified_rename() {
        let imports = vec![ImportDecl {
            module_path: "todo.model".to_string(),
            kind: ImportKind::Rename(vec![("create-task".to_string(), "new-task".to_string())]),
        }];
        let result = find_import_for_unqualified_name(&imports, "new-task");
        assert_eq!(result, Some(("todo.model", "create-task".to_string())));
        // The original name should not be in scope
        assert!(find_import_for_unqualified_name(&imports, "create-task").is_none());
    }

    // Test 9: find_import_for_unqualified_name — no match
    #[test]
    fn test_find_import_for_unqualified_not_found() {
        let imports = vec![ImportDecl {
            module_path: "todo.model".to_string(),
            kind: ImportKind::Refer(vec!["Task".to_string()]),
        }];
        assert!(find_import_for_unqualified_name(&imports, "NotImported").is_none());
    }

    // Test 10: find_definition_in_file on a real temp file
    #[test]
    fn test_find_definition_in_file_basic() {
        let dir = std::env::temp_dir().join("nexl-lsp-test-def-in-file");
        let _ = std::fs::create_dir_all(&dir);
        let file = dir.join("model.nx");
        std::fs::write(&file, "(defn create-task [name] name)\n(def MAX 100)\n")
            .expect("write temp file");

        let result = find_definition_in_file(&file, "create-task");
        assert!(result.is_some());
        let (url, range) = result.unwrap();
        assert!(url.path().ends_with("model.nx"));
        // "create-task" starts at column 6 on line 0
        assert_eq!(range.start.line, 0);
        assert_eq!(range.start.character, 6);

        let result2 = find_definition_in_file(&file, "MAX");
        assert!(result2.is_some());

        // Clean up
        let _ = std::fs::remove_dir_all(&dir);
    }

    // Test 11: find_definition_in_file gracefully returns None for missing file
    #[test]
    fn test_find_definition_in_file_missing_file() {
        let result = find_definition_in_file(Path::new("/nonexistent/path/foo.nx"), "anything");
        assert!(result.is_none());
    }

    // Test 12: same-file go-to-def still works (regression test)
    #[tokio::test]
    async fn test_goto_definition_same_file_regression() {
        let (service, _socket) = LspService::new(Backend::new);
        let backend = service.inner();
        let uri = test_uri("same-file-regression.nexl");
        let source = "(defn greet [name] name)\n(greet \"world\")";

        backend
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "nexl".to_string(),
                    version: 1,
                    text: source.to_string(),
                },
            })
            .await;

        let usage_offset = source.rfind("greet").expect("greet usage");
        let position = offset_to_position(source, usage_offset);
        let response = backend
            .goto_definition(GotoDefinitionParams {
                text_document_position_params: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: uri.clone() },
                    position,
                },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            })
            .await
            .expect("definition request")
            .expect("definition result");

        let def_offset = source.find("greet").expect("greet def");
        let expected_start = offset_to_position(source, def_offset);
        let expected_end = offset_to_position(source, def_offset + "greet".len());
        match response {
            GotoDefinitionResponse::Scalar(loc) => {
                assert_eq!(loc.uri, uri);
                assert_eq!(loc.range, Range::new(expected_start, expected_end));
            }
            _ => panic!("expected scalar response"),
        }
    }

    // Test 13: qualified symbol without project context returns None
    #[tokio::test]
    async fn test_goto_definition_qualified_no_project() {
        let (service, _socket) = LspService::new(Backend::new);
        let backend = service.inner();
        // Use file:///tmp/ URI — no project.nx will exist there.
        let uri = test_uri("no-project.nexl");
        let source = "(module test.app :imports [[test.model :as model]])\n(model/create)";

        backend
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "nexl".to_string(),
                    version: 1,
                    text: source.to_string(),
                },
            })
            .await;

        let offset = source.find("model/create").expect("model/create");
        let position = offset_to_position(source, offset);
        let response = backend
            .goto_definition(GotoDefinitionParams {
                text_document_position_params: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: uri.clone() },
                    position,
                },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            })
            .await
            .expect("definition request");

        assert!(response.is_none(), "should return None without project.nx");
    }

    // ── prettify_type tests ──────────────────────────────────────────────

    #[test]
    fn prettify_concrete_type_unchanged() {
        let ty = Type::Fn {
            params: vec![Type::Int, Type::Str],
            ret: Box::new(Type::Bool),
            effects: nexl_types::EffectRow::empty(),
        };
        assert_eq!(prettify_type(&ty), "(Fn [Int Str] -> Bool)");
    }

    #[test]
    fn prettify_vars_get_clean_names() {
        let ty = Type::Fn {
            params: vec![Type::Var(TypeVar(45)), Type::Var(TypeVar(46))],
            ret: Box::new(Type::Var(TypeVar(58))),
            effects: nexl_types::EffectRow::empty(),
        };
        assert_eq!(prettify_type(&ty), "(Fn [a b] -> c)");
    }

    #[test]
    fn prettify_repeated_var_same_name() {
        // identity: (Fn [t5] -> t5) should become (Fn [a] -> a)
        let ty = Type::Fn {
            params: vec![Type::Var(TypeVar(5))],
            ret: Box::new(Type::Var(TypeVar(5))),
            effects: nexl_types::EffectRow::empty(),
        };
        assert_eq!(prettify_type(&ty), "(Fn [a] -> a)");
    }

    #[test]
    fn prettify_no_vars_passthrough() {
        assert_eq!(prettify_type(&Type::Int), "Int");
        assert_eq!(prettify_type(&Type::Str), "Str");
    }

    // ---- M25: Module completion tests ----

    #[test]
    fn stdlib_module_completions_includes_json() {
        let items = stdlib_module_completions();
        let names: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(names.contains(&"json"), "should include json: {names:?}");
        assert!(names.contains(&"http"), "should include http: {names:?}");
        assert!(names.contains(&"db"), "should include db: {names:?}");
        assert!(names.contains(&"io"), "should include io: {names:?}");
        // All should be MODULE kind
        for item in &items {
            assert_eq!(item.kind, Some(CompletionItemKind::MODULE));
        }
    }

    #[test]
    fn is_in_imports_context_inside() {
        // Cursor inside the :imports vector should return true.
        let src = "(module my.app :imports [[json] [http]])";
        let nodes = nexl_reader::read(src, FileId(0)).expect("parse");
        // The :imports vector spans bytes roughly 25..39.
        // Position inside "[json]" area.
        let inside = src.find("[json").expect("find [json") + 1;
        assert!(is_in_imports_context(&nodes, inside), "should be in imports context at offset {inside}");
    }

    #[test]
    fn is_in_imports_context_outside() {
        // Cursor outside :imports should return false.
        let src = "(module my.app :imports [[json]])";
        let nodes = nexl_reader::read(src, FileId(0)).expect("parse");
        // Position at beginning of the file (before module form).
        assert!(!is_in_imports_context(&nodes, 0), "should not be in imports at start");
    }

    #[test]
    fn file_to_module_path_works() {
        let root = Path::new("/project/src");
        let file = Path::new("/project/src/app/main.nx");
        let result = file_to_module_path(file, root, "my-app");
        assert_eq!(result, Some("my-app.app.main".to_string()));
    }

    #[test]
    fn record_field_completions_from_deftype() {
        let src = "(deftype Point {:x Int :y Int})";
        let nodes = nexl_reader::read(src, FileId(0)).expect("parse");
        let items = record_field_completions(&nodes);
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(labels.contains(&":x"), "should include :x field: {labels:?}");
        assert!(labels.contains(&":y"), "should include :y field: {labels:?}");
        for item in &items {
            assert_eq!(item.kind, Some(CompletionItemKind::FIELD));
        }
    }

    #[test]
    fn record_field_completions_empty_for_sum_type() {
        let src = "(deftype Color (Red) (Green) (Blue))";
        let nodes = nexl_reader::read(src, FileId(0)).expect("parse");
        let items = record_field_completions(&nodes);
        assert!(items.is_empty(), "sum types should produce no field completions");
    }

    #[tokio::test]
    async fn document_symbol_capability_advertised() {
        let (service, _socket) = LspService::new(Backend::new);
        let backend = service.inner();
        let result = backend
            .initialize(InitializeParams::default())
            .await
            .expect("initialize should succeed");
        assert_eq!(
            result.capabilities.document_symbol_provider,
            Some(OneOf::Left(true)),
            "documentSymbol capability should be advertised"
        );
    }

    #[test]
    fn document_symbol_mixed_file() {
        let src = "(deftype Role | Admin | User)\n(def max-retries 3)\n(defn login [email] true)";
        let nodes = nexl_reader::read(src, FileId(0)).expect("parse");
        let symbols = collect_document_symbols(&nodes, src);
        assert_eq!(symbols.len(), 3, "expected three symbols: {symbols:?}");
        assert_eq!(symbols[0].name, "Role");
        assert_eq!(symbols[0].kind, SymbolKind::CLASS);
        assert_eq!(symbols[1].name, "max-retries");
        assert_eq!(symbols[1].kind, SymbolKind::VARIABLE);
        assert_eq!(symbols[2].name, "login");
        assert_eq!(symbols[2].kind, SymbolKind::FUNCTION);
    }

    #[test]
    fn document_symbol_finds_deftype() {
        let src = "(deftype Color | Red | Green | Blue)";
        let nodes = nexl_reader::read(src, FileId(0)).expect("parse");
        let symbols = collect_document_symbols(&nodes, src);
        assert_eq!(symbols.len(), 1, "expected one symbol");
        let sym = &symbols[0];
        assert_eq!(sym.name, "Color");
        assert_eq!(sym.kind, SymbolKind::CLASS);
    }

    #[test]
    fn document_symbol_finds_def() {
        let src = "(def counter 0)";
        let nodes = nexl_reader::read(src, FileId(0)).expect("parse");
        let symbols = collect_document_symbols(&nodes, src);
        assert_eq!(symbols.len(), 1, "expected one symbol");
        let sym = &symbols[0];
        assert_eq!(sym.name, "counter");
        assert_eq!(sym.kind, SymbolKind::VARIABLE);
    }

    #[test]
    fn document_symbol_finds_defn() {
        let src = "(defn greet [name] \"hi\")";
        let nodes = nexl_reader::read(src, FileId(0)).expect("parse");
        let symbols = collect_document_symbols(&nodes, src);
        assert_eq!(symbols.len(), 1, "expected one symbol");
        let sym = &symbols[0];
        assert_eq!(sym.name, "greet");
        assert_eq!(sym.kind, SymbolKind::FUNCTION);
    }

    #[test]
    fn stdlib_function_completions_includes_json_encode() {
        let items = stdlib_function_completions();
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(labels.contains(&"json/encode"), "should include json/encode: first few = {:?}", &labels[..5.min(labels.len())]);
        assert!(labels.contains(&"http/get"), "should include http/get");
        assert!(labels.contains(&"db/open"), "should include db/open");
        assert!(labels.contains(&"io/println"), "should include io/println");
        for item in &items {
            assert_eq!(item.kind, Some(CompletionItemKind::FUNCTION));
        }
    }

    // ── textDocument/references ───────────────────────────────────────────

    async fn open_doc(backend: &Backend, uri: Url, text: &str) {
        backend
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri,
                    language_id: "nexl".to_string(),
                    version: 1,
                    text: text.to_string(),
                },
            })
            .await;
    }

    fn ref_params(uri: Url, position: Position, include_declaration: bool) -> ReferenceParams {
        ReferenceParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position,
            },
            context: ReferenceContext { include_declaration },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        }
    }

    #[tokio::test]
    async fn test_references_finds_all_uses_same_file() {
        // Three occurrences of `foo`: definition + 2 calls.
        let (service, _socket) = LspService::new(Backend::new);
        let backend = service.inner();
        let uri = test_uri("refs-all.nx");
        let source = "(defn foo [] 1)\n(foo)\n(foo)";
        open_doc(backend, uri.clone(), source).await;

        let def_offset = source.find("foo").unwrap();
        let position = offset_to_position(source, def_offset);
        let locs = backend
            .references(ref_params(uri.clone(), position, true))
            .await
            .expect("ok")
            .expect("some locations");

        assert_eq!(locs.len(), 3, "expected 3 (def + 2 calls), got: {locs:?}");
        assert!(locs.iter().all(|l| l.uri == uri));
    }

    #[tokio::test]
    async fn test_references_exclude_declaration() {
        // include_declaration=false → only the 2 call sites.
        let (service, _socket) = LspService::new(Backend::new);
        let backend = service.inner();
        let uri = test_uri("refs-excl.nx");
        let source = "(defn foo [] 1)\n(foo)\n(foo)";
        open_doc(backend, uri.clone(), source).await;

        let def_offset = source.find("foo").unwrap();
        let position = offset_to_position(source, def_offset);
        let locs = backend
            .references(ref_params(uri.clone(), position, false))
            .await
            .expect("ok")
            .expect("some locations");

        assert_eq!(locs.len(), 2, "expected 2 call sites, got: {locs:?}");
        let def_range = Range::new(
            offset_to_position(source, def_offset),
            offset_to_position(source, def_offset + "foo".len()),
        );
        assert!(
            !locs.iter().any(|l| l.range == def_range),
            "def site must be excluded"
        );
    }

    #[tokio::test]
    async fn test_references_from_call_site() {
        // Cursor on a call site — result set is the same as from the def site.
        let (service, _socket) = LspService::new(Backend::new);
        let backend = service.inner();
        let uri = test_uri("refs-callsite.nx");
        let source = "(defn foo [] 1)\n(foo)\n(foo)";
        open_doc(backend, uri.clone(), source).await;

        // Offset of `foo` inside the last `(foo)`.
        let call_offset = source.rfind("foo").unwrap();
        let position = offset_to_position(source, call_offset);
        let locs = backend
            .references(ref_params(uri.clone(), position, false))
            .await
            .expect("ok")
            .expect("some locations");

        assert_eq!(locs.len(), 2, "expected 2 call sites, got: {locs:?}");
    }

    #[tokio::test]
    async fn test_references_none_when_no_symbol_at_cursor() {
        let (service, _socket) = LspService::new(Backend::new);
        let backend = service.inner();
        let uri = test_uri("refs-none.nx");
        let source = "(defn foo [] 1)\n";
        open_doc(backend, uri.clone(), source).await;

        // Cursor on the trailing newline — no symbol there.
        let position = offset_to_position(source, source.len() - 1);
        let result = backend
            .references(ref_params(uri, position, true))
            .await
            .expect("ok");

        assert!(
            result.is_none(),
            "should be None when cursor is not on a symbol"
        );
    }

    // -- defhandler LSP tests --

    #[tokio::test]
    async fn test_hover_defhandler_name_site() {
        // Hovering on the handler name in a defhandler definition should return hover info.
        let (service, _socket) = LspService::new(Backend::new);
        let backend = service.inner();
        let uri = test_uri("defhandler-hover.nexl");
        let source = "(defhandler Logger Log (info [msg] msg))";
        open_doc(backend, uri.clone(), source).await;

        let name_offset = source.find("Logger").expect("Logger");
        let position = offset_to_position(source, name_offset);
        let hover = backend
            .hover(HoverParams {
                text_document_position_params: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: uri.clone() },
                    position,
                },
                work_done_progress_params: Default::default(),
            })
            .await
            .expect("hover request")
            .expect("hover result");

        let value = hover_value(&hover);
        assert!(value.contains("Logger"), "hover should contain handler name");
    }

    #[tokio::test]
    async fn test_goto_definition_defhandler() {
        // Go-to-definition on a reference to a defhandler should jump to the definition site.
        let (service, _socket) = LspService::new(Backend::new);
        let backend = service.inner();
        let uri = test_uri("defhandler-goto.nexl");
        let source =
            "(defhandler Logger Log (info [msg] msg))\n(handle [Logger] (info \"hello\"))";
        open_doc(backend, uri.clone(), source).await;

        // Cursor on the second occurrence of "Logger" (usage in handle form)
        let usage_offset = source.rfind("Logger").expect("Logger usage");
        let position = offset_to_position(source, usage_offset);
        let response = backend
            .goto_definition(GotoDefinitionParams {
                text_document_position_params: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: uri.clone() },
                    position,
                },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            })
            .await
            .expect("definition request")
            .expect("definition result");

        let def_offset = source.find("Logger").expect("Logger def");
        let expected_start = offset_to_position(source, def_offset);
        let expected_end = offset_to_position(source, def_offset + "Logger".len());
        match response {
            GotoDefinitionResponse::Scalar(loc) => {
                assert_eq!(loc.uri, uri);
                assert_eq!(loc.range, Range::new(expected_start, expected_end));
            }
            _ => panic!("expected scalar response"),
        }
    }

    #[tokio::test]
    async fn test_completion_includes_defhandler() {
        // Completions should include handler names defined via defhandler.
        let (service, _socket) = LspService::new(Backend::new);
        let backend = service.inner();
        let uri = test_uri("defhandler-completion.nexl");
        let source = "(defhandler Logger Log (info [msg] msg))\n";
        open_doc(backend, uri.clone(), source).await;

        let position = offset_to_position(source, source.len().saturating_sub(1));
        let response = backend
            .completion(CompletionParams {
                text_document_position: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: uri.clone() },
                    position,
                },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
                context: None,
            })
            .await
            .expect("completion request")
            .expect("completion result");

        let items = match response {
            CompletionResponse::Array(items) => items,
            CompletionResponse::List(list) => list.items,
        };
        let has_logger = items.iter().any(|item| item.label == "Logger");
        assert!(has_logger, "completions should include 'Logger' from defhandler");
    }

    /// Regression test: importing a deftype from another module must make the
    /// constructor visible to the type checker so that `(match v (Ctor x) ...)`
    /// patterns don't produce "unknown constructor" false positives.
    #[test]
    fn test_imported_deftype_constructor_no_false_positive() {
        let tmp = std::env::temp_dir().join("nexl_lsp_test_imported_deftype");
        std::fs::create_dir_all(tmp.join("src/mylib")).unwrap();
        std::fs::create_dir_all(tmp.join("tests")).unwrap();

        std::fs::write(
            tmp.join("project.nx"),
            r#"{:package {:name "mylib" :version "0.1.0" :prefix "mylib" :source-dir "src"}}"#,
        )
        .unwrap();

        // Source module: defines a single-variant ADT.
        std::fs::write(
            tmp.join("src/mylib/types.nx"),
            "(module mylib.types)\n(deftype Wrapper | (Wrapper Str))\n",
        )
        .unwrap();

        // Test file: imports Wrapper and uses it in a match pattern.
        let test_source = r#"(module tests.wrapper
  :imports [[mylib.types :refer [Wrapper]]])
(defn unwrap [v]
  (match v
    (Wrapper s) s
    _           ""))
"#;
        let test_path = tmp.join("tests/wrapper_test.nx");
        let nodes = nexl_reader::read(test_source, FileId::SYNTHETIC).unwrap();
        let diagnostics = type_check_diagnostics(&nodes, test_source, Some(&test_path));

        let unknown_ctor: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.message.contains("unknown constructor"))
            .collect();
        assert!(
            unknown_ctor.is_empty(),
            "expected no 'unknown constructor' diagnostics for imported ADT, got: {unknown_ctor:?}"
        );
    }
}
