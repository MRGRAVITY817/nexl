//! `nexl-lsp` — Language Server Protocol implementation for Nexl.
//!
//! Provides a `tower-lsp`-based LSP server with diagnostics, hover,
//! go-to-definition, and completion support for Nexl source files.

use dashmap::DashMap;
use nexl_ast::{Atom, FileId, Node, NodeKind, Span};
use nexl_errors::{Diagnostic as NexlDiagnostic, Severity as NexlSeverity};
use nexl_infer::{Env, InferState};
use nexl_types::{Type, TypeError, TypeErrorKind};
use std::borrow::Cow;
use std::collections::HashSet;
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
    match nexl_reader::read(source, FileId(0)) {
        Ok(nodes) => type_check_diagnostics(&nodes, source),
        Err(diag) => vec![reader_diagnostic_to_lsp(&diag, uri, source)],
    }
}

fn type_check_diagnostics(nodes: &[Node], source: &str) -> Vec<Diagnostic> {
    let mut env = Env::new();
    let mut state = InferState::new();
    for node in nodes {
        // Skip module infrastructure and type definition forms — they are
        // structural declarations, not expressions the type checker handles.
        if list_head_is(node, "module")
            || list_head_is(node, "import")
            || list_head_is(node, "deftype")
        {
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

/// Return documentation for a stdlib function, if known.
fn stdlib_doc(name: &str) -> Option<&'static str> {
    Some(match name {
        // ── arithmetic ──────────────────────────────────────────────
        "+" => "`(+ args...)` — Variadic addition (Int or Float). Identity = 0.",
        "-" => "`(- x)` or `(- x y ...)` — Unary negation or variadic subtraction.",
        "*" => "`(* args...)` — Variadic multiplication (Int or Float). Identity = 1.",
        "/" => "`(/ x y)` — Integer or float division. Division by zero is a runtime error.",
        "mod" => "`(mod x y)` — Integer remainder.",
        // ── comparison ──────────────────────────────────────────────
        "=" => "`(= a b)` — Structural equality.",
        "<" => "`(< a b)` — Less-than on Int or Float.",
        ">" => "`(> a b)` — Greater-than on Int or Float.",
        "<=" => "`(<= a b)` — Less-than-or-equal on Int or Float.",
        ">=" => "`(>= a b)` — Greater-than-or-equal on Int or Float.",
        // ── logic ───────────────────────────────────────────────────
        "not" => "`(not x)` — Boolean negation.",
        "and" => "`(and a b ...)` — Short-circuit boolean AND. Stops at first falsy.",
        "or" => "`(or a b ...)` — Short-circuit boolean OR. Stops at first truthy.",
        // ── string / collection ─────────────────────────────────────
        "str" => "`(str args...)` — Convert each argument to its display string and concatenate.",
        "count" => "`(count coll)` — Number of elements in a collection or string.",
        "get" => "`(get coll key)` — Return `(Some value)` if key is in bounds, else `None`.",
        "put" => "`(put coll key val)` — Update the value at key/index.",
        "append" => "`(append vec val)` — Append to the end of a vector.",
        "first" => "`(first vec)` — Return `(Some x)` for the first element, or `None`.",
        "rest" => "`(rest vec)` — Return the tail of the vector (empty if length <= 1).",
        "last" => "`(last vec)` — Return `(Some x)` for the last element, or `None`.",
        "slice" => "`(slice vec start end)` — Return elements in [start, end).",
        "remove" => "`(remove map key)` — Remove key from map if present.",
        "keys" => "`(keys map)` — Return map keys in insertion order.",
        "vals" => "`(vals map)` — Return map values in insertion order.",
        "entries" => "`(entries map)` — Return map entries as a Vec of 2-tuples.",
        "contains?" => "`(contains? coll key)` — Check for key membership.",
        "add" => "`(add set val)` — Add element to a set if missing.",
        "union" => "`(union a b)` — Set union.",
        "intersection" => "`(intersection a b)` — Set intersection.",
        "difference" => "`(difference a b)` — Set difference (elements in a not in b).",
        // ── higher-order ────────────────────────────────────────────
        "map" => "`(map f coll)` — Apply f to each element, return new collection.",
        "filter" => "`(filter pred coll)` — Keep elements where pred returns true.",
        "reduce" => "`(reduce f init coll)` — Reduce collection with accumulator.",
        "sort" => "`(sort vec)` — Stable sort using default comparison (Int, Float, Str).",
        "sort-by" => "`(sort-by f vec)` — Stable sort by key function.",
        "reverse" => "`(reverse vec)` — Reverse a vector.",
        "range" => "`(range n)` or `(range start end)` or `(range start end step)` — Generate integer range.",
        "flat-map" => "`(flat-map f vec)` — Map then flatten one level.",
        "group-by" => "`(group-by f vec)` — Group elements by key function, returns Map.",
        "zip" => "`(zip a b)` — Zip two Vecs into a Vec of 2-element Vecs.",
        "take" => "`(take n vec)` — Take first n elements.",
        "drop" => "`(drop n vec)` — Drop first n elements.",
        "take-while" => "`(take-while pred vec)` — Take while predicate is true.",
        "drop-while" => "`(drop-while pred vec)` — Drop while predicate is true.",
        // ── bitwise ─────────────────────────────────────────────────
        "bit-and" => "`(bit-and a b)` — Bitwise AND of two integers.",
        "bit-or" => "`(bit-or a b)` — Bitwise OR of two integers.",
        "bit-xor" => "`(bit-xor a b)` — Bitwise XOR of two integers.",
        "bit-not" => "`(bit-not x)` — Bitwise NOT of an integer.",
        "bit-shift-left" => "`(bit-shift-left x n)` — Shift left by n bits.",
        "bit-shift-right" => "`(bit-shift-right x n)` — Arithmetic shift right by n bits.",
        // ── option/result constructors ──────────────────────────────
        "Some" => "`(Some val)` — Wrap a value in an Option.",
        "None" => "`None` — The absent Option value.",
        "Ok" => "`(Ok val)` — Wrap a success value in a Result.",
        "Err" => "`(Err val)` — Wrap an error value in a Result.",
        // ── str module ──────────────────────────────────────────────
        "str/split" => "`(str/split s sep)` — Split string by separator, return Vec of Str.",
        "str/join" => "`(str/join vec sep)` — Join a Vec of Str with separator.",
        "str/trim" => "`(str/trim s)` — Remove leading and trailing whitespace.",
        "str/upper" => "`(str/upper s)` — Convert to uppercase.",
        "str/lower" => "`(str/lower s)` — Convert to lowercase.",
        "str/starts-with?" => "`(str/starts-with? s prefix)` — Check if string starts with prefix.",
        "str/ends-with?" => "`(str/ends-with? s suffix)` — Check if string ends with suffix.",
        "str/contains?" => "`(str/contains? s sub)` — Check if string contains substring.",
        "str/replace" => "`(str/replace s from to)` — Replace all occurrences of `from` with `to`.",
        "str/index-of" => "`(str/index-of s sub)` — Return `(Some Int)` of first occurrence, or `None`.",
        "str/blank?" => "`(str/blank? s)` — True if empty or only whitespace.",
        "str/chars" => "`(str/chars s)` — Return Vec of Char (Unicode scalar values).",
        "str/graphemes" => "`(str/graphemes s)` — Return Vec of Str (grapheme clusters).",
        "str/trim-start" => "`(str/trim-start s)` — Remove leading whitespace.",
        "str/trim-end" => "`(str/trim-end s)` — Remove trailing whitespace.",
        "str/format" => "`(str/format template args...)` — Positional `{}` placeholder formatting.",
        // ── math module ─────────────────────────────────────────────
        "math/abs" => "`(math/abs x)` — Absolute value (works for Int and Float).",
        "math/floor" => "`(math/floor x)` — Floor (returns Float).",
        "math/ceil" => "`(math/ceil x)` — Ceiling (returns Float).",
        "math/round" => "`(math/round x)` — Round to nearest integer (returns Float).",
        "math/pow" => "`(math/pow base exp)` — Exponentiation (returns Float).",
        "math/sqrt" => "`(math/sqrt x)` — Square root (returns Float).",
        "math/log" => "`(math/log x)` — Natural logarithm (returns Float).",
        "math/exp" => "`(math/exp x)` — e^x (returns Float).",
        "math/sin" => "`(math/sin x)` — Sine (radians, returns Float).",
        "math/cos" => "`(math/cos x)` — Cosine (radians, returns Float).",
        "math/tan" => "`(math/tan x)` — Tangent (radians, returns Float).",
        "math/asin" => "`(math/asin x)` — Arc sine (returns Float in radians).",
        "math/acos" => "`(math/acos x)` — Arc cosine (returns Float in radians).",
        "math/atan" => "`(math/atan x)` — Arc tangent (returns Float in radians).",
        "math/atan2" => "`(math/atan2 y x)` — Two-argument arc tangent (returns Float in radians).",
        "math/min" => "`(math/min a b)` — Minimum of two numbers.",
        "math/max" => "`(math/max a b)` — Maximum of two numbers.",
        "math/clamp" => "`(math/clamp x lo hi)` — Clamp x to [lo, hi].",
        "math/pi" => "`math/pi` — The constant π.",
        "math/e" => "`math/e` — The constant e.",
        // ── io module ───────────────────────────────────────────────
        "io/println" => "`(io/println s)` — Print string with newline.",
        "io/print" => "`(io/print s)` — Print string without newline.",
        "io/read-file" => "`(io/read-file path)` — Read file contents as Str. Returns `(Result Str Str)`.",
        "io/write-file" => "`(io/write-file path content)` — Write string to file. Returns `(Result Unit Str)`.",
        "io/path-join" => "`(io/path-join parts...)` — Join path components.",
        // ── core module ─────────────────────────────────────────────
        "core/identity" => "`(core/identity x)` — Returns its argument unchanged.",
        "core/comp" => "`(core/comp f g)` — Returns a function that applies g then f.",
        "core/partial" => "`(core/partial f args...)` — Returns a function with args pre-applied.",
        "core/constantly" => "`(core/constantly x)` — Returns a function that always returns x.",
        "core/juxt" => "`(core/juxt f g ...)` — Returns a function that applies each fn and collects results.",
        "core/apply" => "`(core/apply f args)` — Call f with args. Last argument must be a Vec.",
        _ => return None,
    })
}

/// Return documentation for a special form, if known.
fn special_form_doc(name: &str) -> Option<&'static str> {
    Some(match name {
        "defn" => "`(defn name \"doc?\" [params] body)` — Define a named function.",
        "def" => "`(def name expr)` — Bind a value to a name.",
        "fn" => "`(fn [params] body)` — Create an anonymous function.",
        "let" => "`(let [name val ...] body)` — Bind local variables.",
        "if" => "`(if test then else)` — Conditional branch.",
        "do" => "`(do exprs...)` — Evaluate forms sequentially, return last.",
        "when" => "`(when test body...)` — Evaluate body when test is true.",
        "unless" => "`(unless test body...)` — Evaluate body when test is false.",
        "cond" => "`(cond test1 result1 ...)` — Multi-way conditional.",
        "match" => "`(match expr pattern1 body1 ...)` — Pattern matching.",
        "loop" => "`(loop [name val ...] body)` — Loop with rebindable locals.",
        "recur" => "`(recur args...)` — Tail-recursive jump back to enclosing loop/fn.",
        "deftype" => "`(deftype Name variants...)` — Define an algebraic data type.",
        "defeffect" => "`(defeffect Name operations...)` — Define an effect type.",
        "defprotocol" => "`(defprotocol Name methods...)` — Define a protocol (interface).",
        "handle" => "`(handle expr handlers...)` — Handle effects from an expression.",
        "module" => "`(module name :imports [[mod :as alias] ...] :exports [...])` — Declare module with imports.",
        "import" => "`(import module-path :as alias)` — Import a module (standalone form).",
        "try" => "`(try expr (catch pattern body))` — Error handling.",
        "for" => "`(for [binding clause ...] body)` — List comprehension.",
        "each" => "`(each [binding iterable] body)` — Iterate for side effects.",
        _ => return None,
    })
}

fn hover_for_offset(nodes: &[Node], offset: usize, source: &str) -> Option<Hover> {
    let mut env = Env::new();
    let mut state = InferState::new();
    for node in nodes {
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
                    if is_target
                        && let Some(name) = symbol_name(name_node)
                    {
                        return Some(build_simple_hover(
                            &name,
                            name_node.span,
                            source,
                        ));
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

    // 1. Check inference env for user-defined bindings
    if let Some(scheme) = env.lookup(&name) {
        let ty = scheme.instantiate(&mut state.supply);
        // Try to find a docstring from the defn that defines this name
        let docstring = find_defn_docstring(nodes, &name);
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

fn build_hover(name: &str, ty: &Type, docstring: Option<&str>, span: Span, source: &str) -> Hover {
    let mut value = format!("```nexl\n{name} : {ty}\n```");
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
    if !list_head_is(node, "defn") {
        return None;
    }
    let NodeKind::List(items) = &node.kind else {
        return None;
    };
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

fn symbol_name(node: &Node) -> Option<String> {
    match &node.kind {
        NodeKind::Atom(Atom::Symbol { ns: None, name }) => Some(name.clone()),
        NodeKind::Atom(Atom::Symbol {
            ns: Some(ns),
            name,
        }) => Some(format!("{ns}/{name}")),
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
    }
    items
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
                completion_provider: Some(CompletionOptions::default()),
                document_formatting_provider: Some(OneOf::Left(true)),
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
        Ok(hover_for_offset(&nodes, offset, &source))
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
        let name = match symbol_name(symbol_node) {
            Some(name) => name,
            None => return Ok(None),
        };
        let range = match find_definition_range(&nodes, &name, &source) {
            Some(range) => range,
            None => return Ok(None),
        };
        let location = Location {
            uri: text_document.uri,
            range,
        };
        Ok(Some(GotoDefinitionResponse::Scalar(location)))
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
        let items = completion_items(&nodes);
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
        assert!(value.contains("Says hello."), "should contain the docstring");
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
            value.contains("Apply f to each element"),
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
            value.contains("Split string by separator"),
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
}
