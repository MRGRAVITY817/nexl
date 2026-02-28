//! Nexl standard library — per-module native function implementations.
//!
//! Each submodule corresponds to a §11.1 core module and exports a list of
//! `(name, implementation)` pairs that get registered into the evaluator
//! environment as qualified module functions.

pub mod async_mod;
pub mod conv;
pub mod core;
pub mod crypto;
pub mod db;
pub mod env;
pub mod http;
pub mod io;
pub mod json;
pub mod log;
pub mod math;
pub mod net;
pub mod random;
pub mod str;
pub mod sys;
pub mod test;
pub mod time;

/// A single stdlib function entry: `(name, implementation)`.
pub type StdlibEntry = (
    &'static str,
    fn(&[nexl_runtime::Value]) -> Result<nexl_runtime::Value, String>,
);

/// Return all stdlib module registrations as `(module_name, entries)` pairs.
///
/// The module names here correspond to the Nexl qualified names used in
/// `import` declarations (e.g., `str`, `math`, `core`).
pub fn all_modules() -> Vec<(&'static str, Vec<StdlibEntry>)> {
    vec![
        ("core", core::entries()),
        ("str", str::entries()),
        ("math", math::entries()),
        ("conv", conv::entries()),
        ("io", io::entries()),
        ("json", json::entries()),
        ("http", http::entries()),
        ("db", db::entries()),
        ("env", env::entries()),
        ("time", time::entries()),
        ("random", random::entries()),
        ("crypto", crypto::entries()),
        ("log", log::entries()),
        ("test", test::entries()),
        ("net", net::entries()),
        ("async", async_mod::entries()),
        ("sys", sys::entries()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_core_module_exists() {
        let entries = core::entries();
        assert!(!entries.is_empty(), "core should have at least identity");
        assert_eq!(entries[0].0, "identity");
    }

    #[test]
    fn test_str_module_exists() {
        let entries = str::entries();
        assert!(!entries.is_empty(), "str should have entries");
        assert_eq!(entries[0].0, "split");
    }

    #[test]
    fn test_math_module_exists() {
        let entries = math::entries();
        assert!(!entries.is_empty(), "math should have entries");
        assert_eq!(entries[0].0, "abs");
    }

    #[test]
    fn test_all_modules_registered() {
        let modules = all_modules();
        let names: Vec<&str> = modules.iter().map(|(name, _)| *name).collect();
        assert_eq!(
            names,
            vec![
                "core", "str", "math", "conv", "io", "json", "http", "db", "env", "time",
                "random", "crypto", "log", "test", "net", "async", "sys"
            ]
        );
    }
}
