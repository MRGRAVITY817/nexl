//! Structured REPL protocol (§14.3).
//!
//! JSON-based machine-readable protocol for AI agent and IDE integration.
//! Input: one JSON object per line on stdin.
//! Output: one JSON object per line on stdout.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{self, BufRead, Write};

// ---------------------------------------------------------------------------
// Request / Response types
// ---------------------------------------------------------------------------

/// A protocol request from the client.
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct Request {
    /// The operation to perform.
    pub op: String,
    /// Source code (for eval, define, type-of, expand, etc.).
    #[serde(default)]
    pub code: String,
    /// Session identifier.
    #[serde(default)]
    pub session: String,
}

/// A protocol response to the client.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Response {
    /// Status: "ok", "error", or "output" (for streaming).
    pub status: String,
    /// The result value as a string (for eval/define).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    /// The inferred type as a string.
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub typ: Option<String>,
    /// Effects list.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effects: Option<Vec<String>>,
    /// Diagnostic messages.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diagnostics: Option<Vec<Diagnostic>>,
    /// Captured console output.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    /// Dependencies (for the "deps" op).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deps: Option<Vec<String>>,
    /// Expansion result (for the "expand" op).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expansion: Option<String>,
    /// Session ID (for session-create).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
}

/// A diagnostic message.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Diagnostic {
    /// Severity: "error", "warning", "info".
    pub severity: String,
    /// Human-readable message.
    pub message: String,
    /// Line number (1-based).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    /// Column number (1-based).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column: Option<u32>,
    /// Optional fix suggestion.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggestion: Option<String>,
}

impl Response {
    /// Create a successful response with a value.
    pub fn ok(value: &str, typ: Option<&str>) -> Self {
        Self {
            status: "ok".to_string(),
            value: Some(value.to_string()),
            typ: typ.map(|s| s.to_string()),
            effects: Some(vec![]),
            diagnostics: Some(vec![]),
            output: Some(String::new()),
            deps: None,
            expansion: None,
            session: None,
        }
    }

    /// Create an error response.
    pub fn error(diagnostics: Vec<Diagnostic>) -> Self {
        Self {
            status: "error".to_string(),
            value: None,
            typ: None,
            effects: None,
            diagnostics: Some(diagnostics),
            output: None,
            deps: None,
            expansion: None,
            session: None,
        }
    }

    /// Create a simple error response with a single message.
    pub fn simple_error(message: &str) -> Self {
        Self::error(vec![Diagnostic {
            severity: "error".to_string(),
            message: message.to_string(),
            line: None,
            column: None,
            suggestion: None,
        }])
    }
}

// ---------------------------------------------------------------------------
// Session
// ---------------------------------------------------------------------------

/// A REPL session holding evaluation and type-checking state.
pub struct Session {
    /// Evaluation environment.
    pub eval_env: std::rc::Rc<nexl_eval::Env>,
    /// Type inference environment.
    pub type_env: nexl_infer::Env,
    /// Type inference state.
    pub type_state: nexl_infer::InferState,
}

impl Session {
    /// Create a new session with a standard environment.
    pub fn new() -> Self {
        Self {
            eval_env: nexl_eval::stdlib::standard_env(),
            type_env: nexl_infer::Env::new(),
            type_state: nexl_infer::InferState::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Protocol handler
// ---------------------------------------------------------------------------

/// Manages sessions and dispatches protocol requests.
pub struct ProtocolHandler {
    sessions: HashMap<String, Session>,
    next_session_id: u64,
}

impl ProtocolHandler {
    /// Create a new protocol handler.
    pub fn new() -> Self {
        Self {
            sessions: HashMap::new(),
            next_session_id: 1,
        }
    }

    /// Handle a single request and return a response.
    pub fn handle(&mut self, req: &Request) -> Response {
        match req.op.as_str() {
            "eval" => self.handle_eval(req),
            "define" => self.handle_eval(req), // define is eval with side effects
            "type-of" => self.handle_type_of(req),
            "effects-of" => self.handle_effects_of(req),
            "deps" => self.handle_deps(req),
            "expand" => self.handle_expand(req),
            "test" => self.handle_test(req),
            "complete" => self.handle_complete(req),
            "session-create" => self.handle_session_create(),
            "session-destroy" => self.handle_session_destroy(req),
            other => Response::simple_error(&format!("unknown op: {other}")),
        }
    }

    fn get_or_create_session(&mut self, session_id: &str) -> &mut Session {
        if !self.sessions.contains_key(session_id) {
            let id = if session_id.is_empty() {
                "default".to_string()
            } else {
                session_id.to_string()
            };
            self.sessions.insert(id.clone(), Session::new());
        }
        let key = if session_id.is_empty() {
            "default"
        } else {
            session_id
        };
        self.sessions.get_mut(key).expect("session just created")
    }

    fn handle_eval(&mut self, req: &Request) -> Response {
        let session = self.get_or_create_session(&req.session);
        let nodes = match nexl_reader::read(&req.code, meta::FileId::SYNTHETIC) {
            Ok(nodes) => nodes,
            Err(diag) => {
                return Response::simple_error(&format!("{diag}"));
            }
        };

        // Type check
        let type_errors =
            crate::update_repl_type_env(&nodes, &mut session.type_env, &mut session.type_state);
        let mut diagnostics: Vec<Diagnostic> = type_errors
            .iter()
            .map(|e| Diagnostic {
                severity: "warning".to_string(),
                message: e.clone(),
                line: None,
                column: None,
                suggestion: None,
            })
            .collect();

        // Evaluate
        let mut last_value = String::from("()");
        for node in &nodes {
            match nexl_eval::eval::eval(node, &session.eval_env) {
                Ok(value) => last_value = format!("{value}"),
                Err(err) => {
                    diagnostics.push(Diagnostic {
                        severity: "error".to_string(),
                        message: format!("{err}"),
                        line: None,
                        column: None,
                        suggestion: None,
                    });
                    return Response {
                        status: "error".to_string(),
                        value: None,
                        typ: None,
                        effects: None,
                        diagnostics: Some(diagnostics),
                        output: None,
                        deps: None,
                        expansion: None,
                        session: None,
                    };
                }
            }
        }

        Response {
            status: "ok".to_string(),
            value: Some(last_value),
            typ: None,
            effects: Some(vec![]),
            diagnostics: if diagnostics.is_empty() {
                Some(vec![])
            } else {
                Some(diagnostics)
            },
            output: Some(String::new()),
            deps: None,
            expansion: None,
            session: None,
        }
    }

    fn handle_type_of(&mut self, req: &Request) -> Response {
        let session = self.get_or_create_session(&req.session);
        match crate::infer_repl_type(&req.code, &session.type_env) {
            Ok(ty) => Response::ok("", Some(&ty)),
            Err(message) => Response::simple_error(&message),
        }
    }

    fn handle_effects_of(&mut self, req: &Request) -> Response {
        // Effects tracking is done through the type/infer system.
        // For now, return empty effects list as a placeholder.
        let session = self.get_or_create_session(&req.session);
        match crate::infer_repl_type(&req.code, &session.type_env) {
            Ok(ty) => Response {
                status: "ok".to_string(),
                value: None,
                typ: Some(ty),
                effects: Some(vec![]),
                diagnostics: Some(vec![]),
                output: None,
                deps: None,
                expansion: None,
                session: None,
            },
            Err(message) => Response::simple_error(&message),
        }
    }

    fn handle_deps(&mut self, req: &Request) -> Response {
        let nodes = match nexl_reader::read(&req.code, meta::FileId::SYNTHETIC) {
            Ok(nodes) => nodes,
            Err(diag) => return Response::simple_error(&format!("{diag}")),
        };

        let local_names = std::collections::HashSet::new();
        let mut all_deps = std::collections::HashSet::new();
        for node in &nodes {
            let deps = nexl_pkg::collect_deps(node, &local_names);
            all_deps.extend(deps);
        }
        let mut deps: Vec<String> = all_deps.into_iter().collect();
        deps.sort();

        Response {
            status: "ok".to_string(),
            value: None,
            typ: None,
            effects: None,
            diagnostics: Some(vec![]),
            output: None,
            deps: Some(deps),
            expansion: None,
            session: None,
        }
    }

    fn handle_expand(&mut self, req: &Request) -> Response {
        // Macro expansion — for now, return the source as-is since
        // macro expansion isn't wired up yet in the reader.
        let nodes = match nexl_reader::read(&req.code, meta::FileId::SYNTHETIC) {
            Ok(nodes) => nodes,
            Err(diag) => return Response::simple_error(&format!("{diag}")),
        };

        let expansion = nodes
            .iter()
            .map(|n| format!("{n}"))
            .collect::<Vec<_>>()
            .join("\n");

        Response {
            status: "ok".to_string(),
            value: None,
            typ: None,
            effects: None,
            diagnostics: Some(vec![]),
            output: None,
            deps: None,
            expansion: Some(expansion),
            session: None,
        }
    }

    fn handle_test(&mut self, _req: &Request) -> Response {
        // Test running is a placeholder — would need to look up the function's
        // :examples and run them.
        Response::ok("no tests found", None)
    }

    fn handle_complete(&mut self, _req: &Request) -> Response {
        // Completion is a placeholder — would integrate with LSP completions.
        Response::ok("[]", None)
    }

    fn handle_session_create(&mut self) -> Response {
        let id = format!("s{}", self.next_session_id);
        self.next_session_id += 1;
        self.sessions.insert(id.clone(), Session::new());
        Response {
            status: "ok".to_string(),
            value: None,
            typ: None,
            effects: None,
            diagnostics: None,
            output: None,
            deps: None,
            expansion: None,
            session: Some(id),
        }
    }

    fn handle_session_destroy(&mut self, req: &Request) -> Response {
        if self.sessions.remove(&req.session).is_some() {
            Response::ok("session destroyed", None)
        } else {
            Response::simple_error(&format!("unknown session: {}", req.session))
        }
    }
}

// ---------------------------------------------------------------------------
// Protocol loop
// ---------------------------------------------------------------------------

/// Run the structured REPL protocol loop, reading JSON lines from input
/// and writing JSON lines to output.
pub fn protocol_loop<R: BufRead, W: Write>(input: R, mut output: W) -> io::Result<()> {
    let mut handler = ProtocolHandler::new();

    for line in input.lines() {
        let line = line?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let request: Request = match serde_json::from_str(trimmed) {
            Ok(req) => req,
            Err(e) => {
                let resp = Response::simple_error(&format!("invalid JSON: {e}"));
                serde_json::to_writer(&mut output, &resp).map_err(io::Error::other)?;
                writeln!(output)?;
                output.flush()?;
                continue;
            }
        };

        let response = handler.handle(&request);
        serde_json::to_writer(&mut output, &response).map_err(io::Error::other)?;
        writeln!(output)?;
        output.flush()?;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn handle_json(handler: &mut ProtocolHandler, json: &str) -> Response {
        let req: Request = serde_json::from_str(json).expect("valid JSON");
        handler.handle(&req)
    }

    #[test]
    fn eval_simple_expression() {
        let mut handler = ProtocolHandler::new();
        let resp = handle_json(&mut handler, r#"{"op": "eval", "code": "(+ 1 2)"}"#);
        assert_eq!(resp.status, "ok");
        assert_eq!(resp.value.as_deref(), Some("3"));
    }

    #[test]
    fn eval_parse_error() {
        let mut handler = ProtocolHandler::new();
        let resp = handle_json(&mut handler, r#"{"op": "eval", "code": "(+ 1"}"#);
        assert_eq!(resp.status, "error");
        assert!(resp.diagnostics.is_some());
        let diags = resp.diagnostics.unwrap();
        assert!(!diags.is_empty());
    }

    #[test]
    fn unknown_op_returns_error() {
        let mut handler = ProtocolHandler::new();
        let resp = handle_json(&mut handler, r#"{"op": "bogus", "code": ""}"#);
        assert_eq!(resp.status, "error");
        let diags = resp.diagnostics.unwrap();
        assert!(diags[0].message.contains("unknown op"));
    }

    #[test]
    fn type_of_expression() {
        let mut handler = ProtocolHandler::new();
        let resp = handle_json(&mut handler, r#"{"op": "type-of", "code": "42"}"#);
        assert_eq!(resp.status, "ok");
        assert!(resp.typ.is_some());
    }

    #[test]
    fn deps_returns_sorted_symbols() {
        let mut handler = ProtocolHandler::new();
        let resp = handle_json(&mut handler, r#"{"op": "deps", "code": "(+ x (* y z))"}"#);
        assert_eq!(resp.status, "ok");
        let deps = resp.deps.unwrap();
        assert!(deps.contains(&"x".to_string()));
        assert!(deps.contains(&"y".to_string()));
        assert!(deps.contains(&"z".to_string()));
        assert!(deps.contains(&"+".to_string()));
        assert!(deps.contains(&"*".to_string()));
    }

    #[test]
    fn expand_returns_parsed_form() {
        let mut handler = ProtocolHandler::new();
        let resp = handle_json(&mut handler, r#"{"op": "expand", "code": "(+ 1 2)"}"#);
        assert_eq!(resp.status, "ok");
        assert!(resp.expansion.is_some());
    }

    #[test]
    fn session_create_and_destroy() {
        let mut handler = ProtocolHandler::new();
        let resp = handle_json(&mut handler, r#"{"op": "session-create", "code": ""}"#);
        assert_eq!(resp.status, "ok");
        let session_id = resp.session.unwrap();
        assert!(session_id.starts_with('s'));

        // Destroy the session
        let resp = handle_json(
            &mut handler,
            &format!(r#"{{"op": "session-destroy", "code": "", "session": "{session_id}"}}"#),
        );
        assert_eq!(resp.status, "ok");

        // Destroying again should fail
        let resp = handle_json(
            &mut handler,
            &format!(r#"{{"op": "session-destroy", "code": "", "session": "{session_id}"}}"#),
        );
        assert_eq!(resp.status, "error");
    }

    #[test]
    fn protocol_loop_processes_lines() {
        let input = r#"{"op": "eval", "code": "(+ 1 2)"}
{"op": "eval", "code": "(+ 3 4)"}
"#;
        let mut output = Vec::new();
        protocol_loop(io::Cursor::new(input), &mut output).expect("loop");

        let output_str = String::from_utf8(output).expect("utf8");
        let lines: Vec<&str> = output_str.trim().split('\n').collect();
        assert_eq!(lines.len(), 2);

        let resp1: Response = serde_json::from_str(lines[0]).expect("parse resp1");
        assert_eq!(resp1.value.as_deref(), Some("3"));

        let resp2: Response = serde_json::from_str(lines[1]).expect("parse resp2");
        assert_eq!(resp2.value.as_deref(), Some("7"));
    }

    #[test]
    fn invalid_json_returns_error() {
        let input = "not valid json\n";
        let mut output = Vec::new();
        protocol_loop(io::Cursor::new(input), &mut output).expect("loop");

        let output_str = String::from_utf8(output).expect("utf8");
        let resp: Response = serde_json::from_str(output_str.trim()).expect("parse");
        assert_eq!(resp.status, "error");
        let diag = &resp.diagnostics.unwrap()[0];
        assert!(diag.message.contains("invalid JSON"));
    }
}
