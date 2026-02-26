//! `nexl-lsp` — Language Server Protocol implementation for Nexl.
//!
//! Provides a `tower-lsp`-based LSP server with diagnostics, hover,
//! go-to-definition, and completion support for Nexl source files.

use dashmap::DashMap;
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
        self.documents.insert(
            doc.uri.clone(),
            TextDocumentItem {
                uri: doc.uri,
                language_id: doc.language_id,
                version: doc.version,
                text: doc.text,
            },
        );
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;
        if let Some(change) = params.content_changes.into_iter().last()
            && let Some(mut doc) = self.documents.get_mut(&uri)
        {
            doc.text = change.text;
            doc.version = params.text_document.version;
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        self.documents.remove(&params.text_document.uri);
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
}
