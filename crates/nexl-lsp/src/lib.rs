//! `nexl-lsp` — Language Server Protocol implementation for Nexl.
//!
//! Provides a `tower-lsp`-based LSP server with diagnostics, hover,
//! go-to-definition, and completion support for Nexl source files.

use dashmap::DashMap;
use std::borrow::Cow;
use nexl_ast::{Atom, FileId, Node, NodeKind, Span};
use nexl_errors::{Diagnostic as NexlDiagnostic, Severity as NexlSeverity};
use nexl_infer::{Env, InferState};
use nexl_types::{Type, TypeError, TypeErrorKind};
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
    match nexl_reader::read(source, FileId(0)) {
        Ok(nodes) => type_check_diagnostics(&nodes, source),
        Err(diag) => vec![reader_diagnostic_to_lsp(&diag, uri, source)],
    }
}

fn type_check_diagnostics(nodes: &[Node], source: &str) -> Vec<Diagnostic> {
    let mut env = Env::new();
    let mut state = InferState::new();
    for node in nodes {
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

    let mut diagnostics = Vec::new();
    for err in &state.errors {
        diagnostics.push(type_error_to_lsp(err, DiagnosticSeverity::ERROR, source));
    }
    for warning in &state.warnings {
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
        TypeErrorKind::ArityMismatch { expected, found } => format!(
            "function arity mismatch: expected {expected} parameter(s), found {found}"
        ),
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
                }
            }
            continue;
        }

        let _ = nexl_infer::synth(node, &env, &mut state);
    }
    None
}

fn build_hover(
    name: &str,
    ty: &Type,
    docstring: Option<&str>,
    span: Span,
    source: &str,
) -> Hover {
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

fn list_head_is(node: &Node, name: &str) -> bool {
    match &node.kind {
        NodeKind::List(items) => match items.first() {
            Some(first) => match &first.kind {
                NodeKind::Atom(Atom::Symbol { ns: None, name: head }) => head == name,
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
        self.publish_diagnostics(&uri, &text, Some(version))
            .await;
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
        self.publish_diagnostics(&uri, &text, Some(version))
            .await;
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
    use tower_lsp::jsonrpc::Request;
    use tower_lsp::lsp_types::notification::Notification;
    use tower_lsp::lsp_types::notification::PublishDiagnostics;
    use tower_lsp::ClientSocket;

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
        assert_eq!(params.diagnostics.len(), 1);
        let diag = &params.diagnostics[0];
        assert_eq!(diag.severity, Some(DiagnosticSeverity::ERROR));
        assert!(diag.message.contains("unbound variable"));
        assert_eq!(diag.range.start.line, 0);
        assert_eq!(diag.range.start.character, 0);
        assert_eq!(diag.range.end.line, 0);
        assert_eq!(diag.range.end.character, 7);
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
        assert_eq!(
            hover.range,
            Some(Range::new(position, end))
        );
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
}
