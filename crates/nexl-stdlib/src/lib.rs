//! Nexl standard library — per-module native function implementations.
//!
//! Each submodule corresponds to a §11.1 core module and exports a list of
//! `(name, implementation)` pairs that get registered into the evaluator
//! environment as qualified module functions.

pub mod async_mod;
pub mod char;
pub mod conv;
pub mod core;
pub mod crypto;
pub mod db;
pub mod env;
pub mod gen_mod;
pub mod http;
pub mod io;
pub mod json;
pub mod log;
pub mod map;
pub mod math;
pub mod net;
pub mod random;
pub mod set;
pub mod str;
pub mod sys;
pub mod test;
pub mod time;
pub mod vec;

/// A single stdlib function entry: `(name, implementation)`.
pub type StdlibEntry = (
    &'static str,
    fn(&[nexl_runtime::Value]) -> Result<nexl_runtime::Value, String>,
);

/// Return embedded Nexl declaration source files for each stdlib module.
///
/// Each entry is `(module_name, nexl_source_code)`. The `"builtins"` entry
/// covers built-in operators and collection functions (unqualified names).
/// All other entries map to `"module_name/fn_name"` qualified keys.
///
/// These files are the single source of truth for stdlib documentation.
/// The LSP parses them with `nexl_reader` to build its hover-doc map.
pub fn nexl_declaration_sources() -> &'static [(&'static str, &'static str)] {
    &[
        ("builtins", include_str!("../nexl/builtins.nx")),
        ("core",     include_str!("../nexl/core.nx")),
        ("str",      include_str!("../nexl/str.nx")),
        ("math",     include_str!("../nexl/math.nx")),
        ("conv",     include_str!("../nexl/conv.nx")),
        ("io",       include_str!("../nexl/io.nx")),
        ("json",     include_str!("../nexl/json.nx")),
        ("http",     include_str!("../nexl/http.nx")),
        ("db",       include_str!("../nexl/db.nx")),
        ("env",      include_str!("../nexl/env.nx")),
        ("time",     include_str!("../nexl/time.nx")),
        ("random",   include_str!("../nexl/random.nx")),
        ("crypto",   include_str!("../nexl/crypto.nx")),
        ("log",      include_str!("../nexl/log.nx")),
        ("test",     include_str!("../nexl/test.nx")),
        ("net",      include_str!("../nexl/net.nx")),
        ("async",    include_str!("../nexl/async.nx")),
        ("sys",      include_str!("../nexl/sys.nx")),
        ("option",   include_str!("../nexl/option_impl.nx")),
        ("result",   include_str!("../nexl/result_impl.nx")),
        ("vec",      include_str!("../nexl/vec.nx")),
        ("char",     include_str!("../nexl/char.nx")),
    ]
}

/// Return Nexl-written stdlib sources to be evaluated at startup.
///
/// These `.nx` files contain real Nexl code (not documentation stubs) that
/// define combinator functions using `defn module/name` qualified names.
/// They are evaluated after Rust native modules are registered, so they can
/// reference builtins and other stdlib modules freely.
///
/// Each entry is `(module_name, nexl_source_code)`.
pub fn nexl_stdlib_sources() -> &'static [(&'static str, &'static str)] {
    &[
        ("option", include_str!("../nexl/option_impl.nx")),
        ("result", include_str!("../nexl/result_impl.nx")),
        ("vec",    include_str!("../nexl/vec_impl.nx")),
        ("map",    include_str!("../nexl/map_impl.nx")),
        ("set",    include_str!("../nexl/set_impl.nx")),
    ]
}

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
        ("gen", gen_mod::entries()),
        ("vec", vec::entries()),
        ("map", map::entries()),
        ("set", set::entries()),
        ("char", char::entries()),
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
    fn test_nexl_stdlib_sources_has_option() {
        let sources = nexl_stdlib_sources();
        let names: Vec<&str> = sources.iter().map(|(name, _)| *name).collect();
        assert!(names.contains(&"option"), "should have option module");
        let (_, src) = sources.iter().find(|(n, _)| *n == "option").unwrap();
        assert!(src.contains("option/some?"), "option source should define some?");
    }

    #[test]
    fn test_nexl_stdlib_sources_has_result() {
        let sources = nexl_stdlib_sources();
        let names: Vec<&str> = sources.iter().map(|(name, _)| *name).collect();
        assert!(names.contains(&"result"), "should have result module");
        let (_, src) = sources.iter().find(|(n, _)| *n == "result").unwrap();
        assert!(src.contains("result/ok?"), "result source should define ok?");
    }

    #[test]
    fn test_all_modules_registered() {
        let modules = all_modules();
        let names: Vec<&str> = modules.iter().map(|(name, _)| *name).collect();
        assert_eq!(
            names,
            vec![
                "core", "str", "math", "conv", "io", "json", "http", "db", "env", "time",
                "random", "crypto", "log", "test", "net", "async", "sys", "gen", "vec", "map",
                "set", "char"
            ]
        );
    }
}
