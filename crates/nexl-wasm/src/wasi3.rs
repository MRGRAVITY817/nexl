//! WASI 0.3 async readiness design (M23 task 10, design only).
//!
//! Maps Nexl's `Concurrent` effect to the WASI Preview 3 async I/O model.
//! **No runtime changes are made here** — this module defines the design
//! types and mapping tables. Actual async lift/lower support is gated behind
//! `--experimental-wasi3` and deferred until the WASI 0.3 spec is stable.
//!
//! # WASI 0.3 async model
//!
//! WASI Preview 3 introduces first-class async support at Component Model
//! boundaries via `future<T>` and `stream<T>` value types plus the
//! `async` lifting/lowering convention:
//!
//! ```wit
//! interface outgoing-handler {
//!     async get: func(url: string) -> string;          // returns future<string>
//!     async read-file: func(path: string) -> list<u8>; // returns future<list<u8>>
//! }
//! ```
//!
//! Nexl's `Concurrent` effect tracks which functions may return futures
//! rather than blocking values:
//!
//! ```nexl
//! (defn fetch [url : Str] : (Future Str)
//!   :performs [Net Concurrent]
//!   (http/get-async url))
//! ```
//!
//! # Mapping table
//!
//! | Nexl effect         | WASI 0.3 async primitive           |
//! |---------------------|-------------------------------------|
//! | `Concurrent`        | `future<T>` / `stream<T>`          |
//! | `Net` + Concurrent  | `wasi:http/outgoing-handler` async  |
//! | `FileSystem` + Concurrent | `wasi:filesystem` async reads  |
//! | `Sockets` + Concurrent | `wasi:sockets` async accept/read |
//!
//! # Implementation status
//!
//! - ✅ Design types defined (this module)
//! - ✅ `--experimental-wasi3` CLI flag scaffolding
//! - ❌ Runtime async lift/lower (deferred — spec not final)
//! - ❌ Code generation for `future<T>` at component boundaries
//! - ❌ Nexl evaluator integration (requires async task runtime)

// ─── Design types ─────────────────────────────────────────────────────────────

/// A WASI 0.3 async operation type that a Nexl function may perform.
///
/// These are design-time descriptors — no runtime representation yet.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum AsyncOperation {
    /// Non-blocking HTTP GET (`wasi:http/outgoing-handler` async).
    HttpGet,
    /// Non-blocking HTTP POST.
    HttpPost,
    /// Non-blocking file read (`wasi:filesystem` async).
    FileRead,
    /// Non-blocking file write.
    FileWrite,
    /// Non-blocking TCP accept (`wasi:sockets/tcp` async).
    TcpAccept,
    /// Non-blocking TCP receive.
    TcpRecv,
    /// A user-defined concurrent operation.
    Custom(String),
}

impl AsyncOperation {
    /// Return the WASI 0.3 interface path for this operation.
    pub fn wasi_interface(&self) -> &str {
        match self {
            AsyncOperation::HttpGet | AsyncOperation::HttpPost => {
                "wasi:http/outgoing-handler"
            }
            AsyncOperation::FileRead | AsyncOperation::FileWrite => {
                "wasi:filesystem/types"
            }
            AsyncOperation::TcpAccept | AsyncOperation::TcpRecv => {
                "wasi:sockets/tcp"
            }
            AsyncOperation::Custom(_) => "user-defined",
        }
    }

    /// Return the WIT function name for this operation (async variant).
    pub fn wit_function_name(&self) -> &str {
        match self {
            AsyncOperation::HttpGet => "get",
            AsyncOperation::HttpPost => "post",
            AsyncOperation::FileRead => "read-file",
            AsyncOperation::FileWrite => "write-file",
            AsyncOperation::TcpAccept => "accept",
            AsyncOperation::TcpRecv => "recv",
            AsyncOperation::Custom(name) => name,
        }
    }
}

/// Mapping of Nexl effect combinations to WASI 0.3 async operations.
///
/// A function performing both `Net` and `Concurrent` effects should use the
/// async HTTP interfaces at component boundaries; one with only `Net` uses
/// the synchronous variant.
#[derive(Debug, Clone, PartialEq)]
pub struct ConcurrentEffectMapping {
    /// The base WASI effect (e.g. `"Net"`, `"FileSystem"`).
    pub base_effect: String,
    /// The async operations enabled when combined with `Concurrent`.
    pub async_operations: Vec<AsyncOperation>,
}

/// Return the canonical effect → async operation mappings for WASI 0.3.
pub fn concurrent_mappings() -> Vec<ConcurrentEffectMapping> {
    vec![
        ConcurrentEffectMapping {
            base_effect: "Net".to_string(),
            async_operations: vec![AsyncOperation::HttpGet, AsyncOperation::HttpPost],
        },
        ConcurrentEffectMapping {
            base_effect: "FileSystem".to_string(),
            async_operations: vec![AsyncOperation::FileRead, AsyncOperation::FileWrite],
        },
        ConcurrentEffectMapping {
            base_effect: "Sockets".to_string(),
            async_operations: vec![AsyncOperation::TcpAccept, AsyncOperation::TcpRecv],
        },
    ]
}

/// Configuration for WASI 0.3 experimental features.
///
/// Controlled by the `--experimental-wasi3` CLI flag.  All fields default to
/// `false` (disabled) until the WASI 0.3 spec is finalized.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Wasi3Config {
    /// Enable async lifting/lowering at component boundaries.
    pub async_lift_lower: bool,
    /// Enable `future<T>` in generated WIT interfaces.
    pub future_types: bool,
    /// Enable `stream<T>` in generated WIT interfaces.
    pub stream_types: bool,
}

impl Wasi3Config {
    /// Return the configuration for `--experimental-wasi3` mode.
    ///
    /// Enables the design-level flags but guards against accidental use by
    /// emitting a runtime error if any flag is actually exercised.
    pub fn experimental() -> Self {
        Wasi3Config {
            async_lift_lower: true,
            future_types: true,
            stream_types: true,
        }
    }

    /// Return the notice string to display when `--experimental-wasi3` is active.
    pub fn experimental_notice() -> &'static str {
        "⚠ --experimental-wasi3: WASI Preview 3 async support is experimental.\n\
         The spec is not finalized; async lift/lower and future<T> types are\n\
         not yet implemented. Programs will run synchronously."
    }
}

/// Check whether a set of declared effects requires WASI 0.3 async support.
///
/// Returns `true` when `"Concurrent"` appears alongside any WASI-mapped effect.
pub fn requires_wasi3(declared_effects: &[String]) -> bool {
    let has_concurrent = declared_effects.iter().any(|e| e == "Concurrent");
    if !has_concurrent {
        return false;
    }
    let wasi_effects = ["Net", "FileSystem", "Sockets", "Clock", "Random"];
    declared_effects
        .iter()
        .any(|e| wasi_effects.contains(&e.as_str()))
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Test 1 ──────────────────────────────────────────────────────────────

    #[test]
    fn test_async_operation_wasi_interface() {
        assert_eq!(
            AsyncOperation::HttpGet.wasi_interface(),
            "wasi:http/outgoing-handler"
        );
        assert_eq!(
            AsyncOperation::FileRead.wasi_interface(),
            "wasi:filesystem/types"
        );
        assert_eq!(
            AsyncOperation::TcpAccept.wasi_interface(),
            "wasi:sockets/tcp"
        );
    }

    // ── Test 2 ──────────────────────────────────────────────────────────────

    #[test]
    fn test_async_operation_wit_function_names() {
        assert_eq!(AsyncOperation::HttpGet.wit_function_name(), "get");
        assert_eq!(AsyncOperation::HttpPost.wit_function_name(), "post");
        assert_eq!(AsyncOperation::FileRead.wit_function_name(), "read-file");
        assert_eq!(AsyncOperation::FileWrite.wit_function_name(), "write-file");
        assert_eq!(AsyncOperation::TcpAccept.wit_function_name(), "accept");
        assert_eq!(AsyncOperation::TcpRecv.wit_function_name(), "recv");
    }

    // ── Test 3 ──────────────────────────────────────────────────────────────

    #[test]
    fn test_concurrent_mappings_net() {
        let mappings = concurrent_mappings();
        let net = mappings.iter().find(|m| m.base_effect == "Net").unwrap();
        assert!(net.async_operations.contains(&AsyncOperation::HttpGet));
        assert!(net.async_operations.contains(&AsyncOperation::HttpPost));
    }

    // ── Test 4 ──────────────────────────────────────────────────────────────

    #[test]
    fn test_concurrent_mappings_filesystem() {
        let mappings = concurrent_mappings();
        let fs = mappings
            .iter()
            .find(|m| m.base_effect == "FileSystem")
            .unwrap();
        assert!(fs.async_operations.contains(&AsyncOperation::FileRead));
        assert!(fs.async_operations.contains(&AsyncOperation::FileWrite));
    }

    // ── Test 5 ──────────────────────────────────────────────────────────────

    #[test]
    fn test_wasi3_config_default_disabled() {
        let config = Wasi3Config::default();
        assert!(!config.async_lift_lower);
        assert!(!config.future_types);
        assert!(!config.stream_types);
    }

    // ── Test 6 ──────────────────────────────────────────────────────────────

    #[test]
    fn test_wasi3_config_experimental_enabled() {
        let config = Wasi3Config::experimental();
        assert!(config.async_lift_lower);
        assert!(config.future_types);
        assert!(config.stream_types);
    }

    // ── Test 7 ──────────────────────────────────────────────────────────────

    #[test]
    fn test_requires_wasi3_concurrent_plus_net() {
        let effects = vec!["Net".to_string(), "Concurrent".to_string()];
        assert!(requires_wasi3(&effects));
    }

    // ── Test 8 ──────────────────────────────────────────────────────────────

    #[test]
    fn test_requires_wasi3_no_concurrent() {
        let effects = vec!["Net".to_string(), "FileSystem".to_string()];
        assert!(!requires_wasi3(&effects), "no Concurrent → no WASI 0.3 needed");
    }

    // ── Test 9 ──────────────────────────────────────────────────────────────

    #[test]
    fn test_requires_wasi3_concurrent_only() {
        // Concurrent alone (no WASI-mapped effect) → not WASI 0.3
        let effects = vec!["Concurrent".to_string()];
        assert!(!requires_wasi3(&effects));
    }

    // ── Test 10 ─────────────────────────────────────────────────────────────

    #[test]
    fn test_experimental_notice_mentions_wasi3() {
        let notice = Wasi3Config::experimental_notice();
        assert!(notice.contains("WASI Preview 3") || notice.contains("wasi3"), "{notice}");
    }

    // ── Test 11 ─────────────────────────────────────────────────────────────

    #[test]
    fn test_custom_async_operation() {
        let op = AsyncOperation::Custom("my-op".to_string());
        assert_eq!(op.wasi_interface(), "user-defined");
        assert_eq!(op.wit_function_name(), "my-op");
    }
}
