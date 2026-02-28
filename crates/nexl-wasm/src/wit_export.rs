//! WIT export — expose Nexl modules as WIT interfaces and worlds.
//!
//! Implements the `wit-export` feature (M23 task 6):
//! - Convert Nexl [`Type`]s to WIT type strings (extending `wit.rs` with Vec, Adt, Tuple).
//! - Define [`WitModuleExport`] to describe a Nexl module's public surface.
//! - Generate a complete WIT world definition from a module's exports + imported effects.
//!
//! The output is a WIT text file that other languages and runtimes can consume to
//! interoperate with a compiled Nexl component.

use nexl_types::Type;

use crate::wit::WitError;

// ─── Extended type conversion ─────────────────────────────────────────────────

/// Convert a Nexl [`Type`] to its WIT type string, including composite types.
///
/// Extends [`crate::wit::type_to_wit`] with:
/// - `Type::Vec(T)` → `list<T>`
/// - `Type::Tuple([T...])` → `tuple<T, ...>`
/// - `Type::Adt { "Option", [T] }` → `option<T>`
/// - `Type::Adt { "Result", [T, E] }` → `result<T, E>`
/// - `Type::Adt { name, [] }` → `name` (named reference, kebab-cased)
/// - `Type::Record { name, fields }` → `name` (named reference; use [`record_type_def`] for the definition)
///
/// # Errors
/// Returns [`WitError::UnsupportedType`] for types that have no WIT representation
/// (e.g., `Type::Ratio`, `Type::Symbol`, higher-kinded or polymorphic types).
pub fn nexl_type_to_wit(ty: &Type) -> Result<String, WitError> {
    match ty {
        // ── Primitives — delegate to existing conversion ──
        Type::Int | Type::Int64 => Ok("s64".to_string()),
        Type::Int32 => Ok("s32".to_string()),
        Type::Int16 => Ok("s16".to_string()),
        Type::Int8 => Ok("s8".to_string()),
        Type::U64 => Ok("u64".to_string()),
        Type::U32 => Ok("u32".to_string()),
        Type::U16 => Ok("u16".to_string()),
        Type::U8 => Ok("u8".to_string()),
        Type::Float | Type::F64 => Ok("float64".to_string()),
        Type::F32 => Ok("float32".to_string()),
        Type::Bool => Ok("bool".to_string()),
        Type::Str => Ok("string".to_string()),
        Type::Unit => Ok("unit".to_string()),

        // ── Composite ──

        // list<T>
        Type::Vec(elem) => {
            let wt = nexl_type_to_wit(elem)?;
            Ok(format!("list<{wt}>"))
        }

        // tuple<T0, T1, …>
        Type::Tuple(elems) => {
            if elems.is_empty() {
                return Ok("unit".to_string());
            }
            let parts: Result<Vec<String>, _> = elems.iter().map(nexl_type_to_wit).collect();
            Ok(format!("tuple<{}>", parts?.join(", ")))
        }

        // ADT — special-case Option, Result; everything else is a named reference.
        Type::Adt { name, args } => match (name.as_str(), args.as_slice()) {
            // (Option T) → option<T>
            ("Option", [inner]) => {
                let wt = nexl_type_to_wit(inner)?;
                Ok(format!("option<{wt}>"))
            }
            // (Result T E) → result<T, E>
            ("Result", [ok_ty, err_ty]) => {
                let wok = nexl_type_to_wit(ok_ty)?;
                let werr = nexl_type_to_wit(err_ty)?;
                Ok(format!("result<{wok}, {werr}>"))
            }
            // Other ADTs with args → unsupported (no WIT generic syntax for user types)
            (_, args) if !args.is_empty() => Err(WitError::UnsupportedType(format!(
                "generic ADT with args: {name}"
            ))),
            // Nullary ADT → kebab-case named reference
            (_, _) => Ok(wit_ident(name)),
        },

        // Record → named reference (definition is separate).
        Type::Record { name, .. } => Ok(wit_ident(name)),

        _ => Err(WitError::UnsupportedType(format!("{ty:?}"))),
    }
}

/// Generate a WIT `record` type definition from a Nexl [`Type::Record`].
///
/// ```text
/// record person {
///     name: string,
///     age: s64,
/// }
/// ```
///
/// # Errors
/// Returns [`WitError::UnsupportedType`] if any field type cannot be represented in WIT.
pub fn record_type_def(ty: &Type) -> Result<String, WitError> {
    match ty {
        Type::Record { name, fields } => {
            let mut out = format!("record {} {{\n", wit_ident(name));
            for (fname, fty) in fields {
                let wt = nexl_type_to_wit(fty)?;
                out.push_str(&format!("    {}: {},\n", wit_ident(fname), wt));
            }
            out.push('}');
            Ok(out)
        }
        other => Err(WitError::UnsupportedType(format!(
            "expected Record, got {other:?}"
        ))),
    }
}

/// Generate a WIT `variant` type definition from a list of `(variant_name, Option<payload_type>)`
/// constructor descriptions.
///
/// ```text
/// variant shape {
///     circle(float64),
///     rectangle(tuple<float64, float64>),
///     point,
/// }
/// ```
///
/// # Errors
/// Returns [`WitError::UnsupportedType`] if any payload type cannot be represented.
pub fn variant_type_def(
    name: &str,
    cases: &[(String, Option<Type>)],
) -> Result<String, WitError> {
    let mut out = format!("variant {} {{\n", wit_ident(name));
    for (case_name, payload) in cases {
        match payload {
            Some(ty) => {
                let wt = nexl_type_to_wit(ty)?;
                out.push_str(&format!("    {}({}),\n", wit_ident(case_name), wt));
            }
            None => {
                out.push_str(&format!("    {},\n", wit_ident(case_name)));
            }
        }
    }
    out.push('}');
    Ok(out)
}

// ─── World generation ─────────────────────────────────────────────────────────

/// A single exported function in a Nexl WIT world.
#[derive(Debug, Clone)]
pub struct WitExportFn {
    /// Export name in kebab-case (e.g. `"add"`, `"read-file"`).
    pub name: String,
    /// Parameter types.
    pub params: Vec<Type>,
    /// Return type.
    pub ret: Type,
}

/// An imported interface in a Nexl WIT world (from an effect declaration).
///
/// Each entry is `(interface_name, functions)` where functions are
/// `(fn_name, param_types, return_type)`.
#[derive(Debug, Clone)]
pub struct WitImportIface {
    /// Interface name (e.g. `"log"`, `"filesystem"`).
    pub name: String,
    /// Functions in this interface.
    pub functions: Vec<(String, Vec<Type>, Type)>,
}

/// A complete description of a Nexl module's WIT world surface.
#[derive(Debug, Clone)]
pub struct WitModuleExport {
    /// Module/component name.
    pub name: String,
    /// Exported functions.
    pub exports: Vec<WitExportFn>,
    /// Imported interfaces (from effects the module performs).
    pub imports: Vec<WitImportIface>,
}

/// Generate a complete WIT world text for a Nexl module.
///
/// Produces output like:
/// ```wit
/// package nexl:my-service;
///
/// world my-service {
///     import log: interface {
///         info: func(p0: string);
///     }
///
///     export add: func(p0: s64, p1: s64) -> s64;
///     export greet: func(p0: string) -> string;
/// }
/// ```
///
/// # Errors
/// Returns [`WitError::UnsupportedType`] if any type cannot be represented in WIT.
pub fn generate_wit_world(module: &WitModuleExport) -> Result<String, WitError> {
    let mut out = String::new();
    let world_name = wit_ident(&module.name);

    out.push_str(&format!("package nexl:{world_name};\n\n"));
    out.push_str(&format!("world {world_name} {{\n"));

    // Imports (from effects).
    for iface in &module.imports {
        let iface_name = wit_ident(&iface.name);
        out.push_str(&format!("    import {iface_name}: interface {{\n"));
        for (fn_name, params, ret) in &iface.functions {
            let sig = fn_sig(params, ret)?;
            out.push_str(&format!("        {}: {sig};\n", wit_ident(fn_name)));
        }
        out.push_str("    }\n\n");
    }

    // Exports.
    for exp in &module.exports {
        let sig = fn_sig(&exp.params, &exp.ret)?;
        out.push_str(&format!("    export {}: {sig};\n", wit_ident(&exp.name)));
    }

    out.push('}');
    Ok(out)
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Build a `func(p0: T0, p1: T1, …) -> R` signature string.
fn fn_sig(params: &[Type], ret: &Type) -> Result<String, WitError> {
    let mut out = String::from("func(");
    for (i, p) in params.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        let wt = nexl_type_to_wit(p)?;
        out.push_str(&format!("p{i}: {wt}"));
    }
    out.push(')');
    if *ret != Type::Unit {
        let wt = nexl_type_to_wit(ret)?;
        out.push_str(&format!(" -> {wt}"));
    }
    Ok(out)
}

/// Convert a Nexl identifier to a WIT-compatible kebab-case name.
///
/// Lowercases PascalCase names and strips trailing `!` / `?`.
fn wit_ident(name: &str) -> String {
    let stripped = name.trim_end_matches('!').trim_end_matches('?');
    let mut result = String::new();
    for (i, ch) in stripped.chars().enumerate() {
        if ch.is_uppercase() && i > 0 {
            result.push('-');
        }
        result.push(ch.to_ascii_lowercase());
    }
    result
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Test 1 ──────────────────────────────────────────────────────────────

    #[test]
    fn test_nexl_vec_to_wit_list() {
        assert_eq!(
            nexl_type_to_wit(&Type::Vec(Box::new(Type::Str))).unwrap(),
            "list<string>"
        );
        assert_eq!(
            nexl_type_to_wit(&Type::Vec(Box::new(Type::Int))).unwrap(),
            "list<s64>"
        );
        // Nested list
        assert_eq!(
            nexl_type_to_wit(&Type::Vec(Box::new(Type::Vec(Box::new(Type::U8))))).unwrap(),
            "list<list<u8>>"
        );
    }

    // ── Test 2 ──────────────────────────────────────────────────────────────

    #[test]
    fn test_nexl_record_to_wit() {
        let ty = Type::Record {
            name: "Person".to_string(),
            fields: vec![
                ("name".to_string(), Type::Str),
                ("age".to_string(), Type::Int32),
            ],
        };
        // Type reference
        assert_eq!(nexl_type_to_wit(&ty).unwrap(), "person");
        // Full definition
        let def = record_type_def(&ty).unwrap();
        assert!(def.starts_with("record person {"), "got: {def}");
        assert!(def.contains("    name: string,"), "got: {def}");
        assert!(def.contains("    age: s32,"), "got: {def}");
    }

    // ── Test 3 ──────────────────────────────────────────────────────────────

    #[test]
    fn test_nexl_adt_option_to_wit() {
        let ty = Type::Adt {
            name: "Option".to_string(),
            args: vec![Type::Int],
        };
        assert_eq!(nexl_type_to_wit(&ty).unwrap(), "option<s64>");
    }

    // ── Test 4 ──────────────────────────────────────────────────────────────

    #[test]
    fn test_nexl_adt_result_to_wit() {
        let ty = Type::Adt {
            name: "Result".to_string(),
            args: vec![Type::Int, Type::Str],
        };
        assert_eq!(nexl_type_to_wit(&ty).unwrap(), "result<s64, string>");
    }

    // ── Test 5 ──────────────────────────────────────────────────────────────

    #[test]
    fn test_nexl_adt_opaque_to_wit() {
        // Nullary ADT → kebab-case name reference
        let ty = Type::Adt {
            name: "MyResource".to_string(),
            args: vec![],
        };
        assert_eq!(nexl_type_to_wit(&ty).unwrap(), "my-resource");
    }

    // ── Test 6 ──────────────────────────────────────────────────────────────

    #[test]
    fn test_generate_wit_world_simple() {
        let module = WitModuleExport {
            name: "calculator".to_string(),
            exports: vec![
                WitExportFn {
                    name: "add".to_string(),
                    params: vec![Type::Int, Type::Int],
                    ret: Type::Int,
                },
                WitExportFn {
                    name: "greet".to_string(),
                    params: vec![Type::Str],
                    ret: Type::Str,
                },
            ],
            imports: vec![],
        };
        let wit = generate_wit_world(&module).unwrap();
        assert!(wit.contains("package nexl:calculator;"), "missing package: {wit}");
        assert!(wit.contains("world calculator {"), "missing world: {wit}");
        assert!(
            wit.contains("export add: func(p0: s64, p1: s64) -> s64;"),
            "missing add: {wit}"
        );
        assert!(
            wit.contains("export greet: func(p0: string) -> string;"),
            "missing greet: {wit}"
        );
    }

    // ── Test 7 ──────────────────────────────────────────────────────────────

    #[test]
    fn test_generate_wit_world_with_effects() {
        let module = WitModuleExport {
            name: "logger-service".to_string(),
            exports: vec![WitExportFn {
                name: "run".to_string(),
                params: vec![],
                ret: Type::Unit,
            }],
            imports: vec![WitImportIface {
                name: "log".to_string(),
                functions: vec![
                    (
                        "info".to_string(),
                        vec![Type::Str],
                        Type::Unit,
                    ),
                    (
                        "error".to_string(),
                        vec![Type::Str],
                        Type::Unit,
                    ),
                ],
            }],
        };
        let wit = generate_wit_world(&module).unwrap();
        assert!(wit.contains("import log: interface {"), "missing import: {wit}");
        assert!(wit.contains("info: func(p0: string);"), "missing info: {wit}");
        assert!(wit.contains("error: func(p0: string);"), "missing error: {wit}");
        assert!(wit.contains("export run: func();"), "missing run: {wit}");
    }

    // ── Test 8 ──────────────────────────────────────────────────────────────

    #[test]
    fn test_nexl_tuple_to_wit() {
        let ty = Type::Tuple(vec![Type::Int, Type::Str]);
        assert_eq!(nexl_type_to_wit(&ty).unwrap(), "tuple<s64, string>");
        // Single element (degenerate)
        let single = Type::Tuple(vec![Type::Bool]);
        assert_eq!(nexl_type_to_wit(&single).unwrap(), "tuple<bool>");
        // Empty tuple → unit
        let empty = Type::Tuple(vec![]);
        assert_eq!(nexl_type_to_wit(&empty).unwrap(), "unit");
    }

    // ── Test 9 ──────────────────────────────────────────────────────────────

    #[test]
    fn test_export_fn_unit_return() {
        let module = WitModuleExport {
            name: "service".to_string(),
            exports: vec![WitExportFn {
                name: "init".to_string(),
                params: vec![Type::Str],
                ret: Type::Unit,
            }],
            imports: vec![],
        };
        let wit = generate_wit_world(&module).unwrap();
        // Unit return → no `-> unit` in the signature
        assert!(
            wit.contains("export init: func(p0: string);"),
            "unit return should omit `-> unit`: {wit}"
        );
    }

    // ── Test 10 ─────────────────────────────────────────────────────────────

    #[test]
    fn test_variant_type_def() {
        let cases = vec![
            ("circle".to_string(), Some(Type::Float)),
            ("rectangle".to_string(), Some(Type::Tuple(vec![Type::Float, Type::Float]))),
            ("point".to_string(), None),
        ];
        let def = variant_type_def("Shape", &cases).unwrap();
        assert!(def.starts_with("variant shape {"), "got: {def}");
        assert!(def.contains("    circle(float64),"), "got: {def}");
        assert!(def.contains("    rectangle(tuple<float64, float64>),"), "got: {def}");
        assert!(def.contains("    point,"), "got: {def}");
    }
}
