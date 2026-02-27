//! WIT (WebAssembly Interface Types) generation from Nexl types (spec §15.1–§15.2).
//!
//! Converts Nexl [`Type`] representations into WIT text format, enabling
//! WASM Component Model interop.

use nexl_types::Type;
use std::fmt::Write;

/// Errors that can occur during WIT generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WitError {
    /// A Nexl type has no WIT representation.
    UnsupportedType(String),
}

impl std::fmt::Display for WitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WitError::UnsupportedType(t) => write!(f, "unsupported WIT type: {t}"),
        }
    }
}

impl std::error::Error for WitError {}

/// Convert a Nexl [`Type`] to its WIT type name (spec §15.2).
pub fn type_to_wit(ty: &Type) -> Result<String, WitError> {
    match ty {
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
        Type::Record { name, fields } => {
            let mut out = String::new();
            writeln!(out, "record {} {{", wit_name(name)).expect("write to string");
            for (fname, ftype) in fields {
                let wt = type_to_wit(ftype)?;
                writeln!(out, "    {}: {},", wit_name(fname), wt).expect("write to string");
            }
            out.push('}');
            Ok(out)
        }
        _ => Err(WitError::UnsupportedType(format!("{ty:?}"))),
    }
}

/// Convert a Nexl function type to a WIT function signature.
///
/// Returns `"func(p0: type0, p1: type1) -> ret_type"` or
/// `"func(p0: type0)"` when the return type is `Unit`.
pub fn fn_type_to_wit(params: &[Type], ret: &Type) -> Result<String, WitError> {
    let mut out = String::from("func(");
    for (i, p) in params.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        let wt = type_to_wit(p)?;
        write!(out, "p{i}: {wt}").expect("write to string");
    }
    out.push(')');

    if *ret != Type::Unit {
        let wt = type_to_wit(ret)?;
        write!(out, " -> {wt}").expect("write to string");
    }

    Ok(out)
}

/// Generate a full WIT interface from a component name and a list of
/// `(export_name, params, ret)` function signatures.
///
/// Produces output like:
/// ```wit
/// package nexl:string-utils;
///
/// interface string-utils {
///     reverse-words: func(p0: string) -> string;
///     word-count: func(p0: string) -> s64;
/// }
/// ```
pub fn generate_wit_interface(
    component_name: &str,
    functions: &[(String, Vec<Type>, Type)],
) -> Result<String, WitError> {
    let mut out = String::new();
    writeln!(out, "package nexl:{component_name};\n").expect("write to string");
    writeln!(out, "interface {component_name} {{").expect("write to string");

    for (name, params, ret) in functions {
        let sig = fn_type_to_wit(params, ret)?;
        writeln!(out, "    {}: {sig};", wit_name(name)).expect("write to string");
    }

    out.push('}');
    Ok(out)
}

/// A WIT resource definition with its methods.
#[derive(Debug, Clone, PartialEq)]
pub struct WitResource {
    /// Resource name (e.g. `"connection"`).
    pub name: String,
    /// Methods as `(name, params, ret)`.
    pub methods: Vec<(String, Vec<Type>, Type)>,
}

/// Generate WIT text for a resource type.
///
/// Produces output like:
/// ```wit
/// resource connection {
///     constructor(p0: string);
///     query: func(p0: string) -> string;
///     close: func();
/// }
/// ```
pub fn generate_wit_resource(resource: &WitResource) -> Result<String, WitError> {
    let mut out = String::new();
    writeln!(out, "resource {} {{", resource.name).expect("write to string");

    for (name, params, ret) in &resource.methods {
        let sig = fn_type_to_wit(params, ret)?;
        writeln!(out, "    {}: {sig};", wit_name(name)).expect("write to string");
    }

    out.push('}');
    Ok(out)
}

/// Generate a full WIT interface including both functions and resources.
pub fn generate_wit_interface_full(
    component_name: &str,
    functions: &[(String, Vec<Type>, Type)],
    resources: &[WitResource],
) -> Result<String, WitError> {
    let mut out = String::new();
    writeln!(out, "package nexl:{component_name};\n").expect("write to string");
    writeln!(out, "interface {component_name} {{").expect("write to string");

    for resource in resources {
        writeln!(out, "    resource {} {{", resource.name).expect("write to string");
        for (name, params, ret) in &resource.methods {
            let sig = fn_type_to_wit(params, ret)?;
            writeln!(out, "        {}: {sig};", wit_name(name)).expect("write to string");
        }
        writeln!(out, "    }}").expect("write to string");
    }

    for (name, params, ret) in functions {
        let sig = fn_type_to_wit(params, ret)?;
        writeln!(out, "    {}: {sig};", wit_name(name)).expect("write to string");
    }

    out.push('}');
    Ok(out)
}

/// Generate a WIT interface from a Nexl effect definition (spec §15.1).
///
/// Maps each effect operation to a WIT function. For example:
/// ```nexl
/// (defeffect Log
///   (info  : (Fn [Str] -> Unit))
///   (error : (Fn [Str] -> Unit)))
/// ```
/// generates:
/// ```wit
/// interface log {
///     info: func(p0: string);
///     error: func(p0: string);
/// }
/// ```
pub fn effect_to_wit_interface(effect: &nexl_types::EffectDef) -> Result<String, WitError> {
    let mut out = String::new();
    writeln!(out, "interface {} {{", wit_name(&effect.name)).expect("write to string");

    for op in &effect.operations {
        match &op.signature {
            Type::Fn { params, ret, .. } => {
                let sig = fn_type_to_wit(params, ret)?;
                writeln!(out, "    {}: {sig};", wit_name(&op.name)).expect("write to string");
            }
            _ => {
                return Err(WitError::UnsupportedType(format!(
                    "effect operation `{}` has non-function type: {:?}",
                    op.name, op.signature
                )));
            }
        }
    }

    out.push('}');
    Ok(out)
}

/// Convert a WIT interface into a Nexl effect definition (spec §15.1).
///
/// Takes a WIT interface name and a list of operations (each with params and
/// return type) and produces an `EffectDef` suitable for use in Nexl's
/// effect system.
pub fn wit_interface_to_effect(
    interface_name: &str,
    operations: &[(String, Vec<Type>, Type)],
) -> nexl_types::EffectDef {
    use nexl_types::{EffectOpDef, EffectRow};

    let effect_name = nexl_name(interface_name);
    let ops = operations
        .iter()
        .map(|(name, params, ret)| EffectOpDef {
            name: name.clone(),
            signature: Type::Fn {
                params: params.clone(),
                ret: Box::new(ret.clone()),
                effects: EffectRow::empty(),
            },
        })
        .collect();

    nexl_types::EffectDef {
        name: effect_name,
        params: vec![],
        operations: ops,
    }
}

/// Convert a WIT-style name (kebab-case) to a Nexl PascalCase name.
///
/// E.g. `"file-system"` → `"FileSystem"`, `"log"` → `"Log"`.
fn nexl_name(wit_name: &str) -> String {
    let mut result = String::new();
    let mut capitalize_next = true;
    for ch in wit_name.chars() {
        if ch == '-' {
            capitalize_next = true;
        } else if capitalize_next {
            result.push(ch.to_ascii_uppercase());
            capitalize_next = false;
        } else {
            result.push(ch);
        }
    }
    result
}

/// Convert a Nexl identifier to a WIT-compatible name.
///
/// WIT uses lowercase kebab-case. Nexl already uses kebab-case for most names.
/// This function lowercases PascalCase names and strips trailing `!` or `?`.
fn wit_name(name: &str) -> String {
    let stripped = name.trim_end_matches('!').trim_end_matches('?');
    // Convert PascalCase to kebab-case
    let mut result = String::new();
    for (i, ch) in stripped.chars().enumerate() {
        if ch.is_uppercase() && i > 0 {
            result.push('-');
        }
        result.push(ch.to_ascii_lowercase());
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexl_types::{EffectDef, EffectOpDef, EffectRow};

    // ── Test 1 ──

    #[test]
    fn test_wit_type_int() {
        assert_eq!(type_to_wit(&Type::Int).unwrap(), "s64");
        assert_eq!(type_to_wit(&Type::Int64).unwrap(), "s64");
    }

    // ── Test 2 ──

    #[test]
    fn test_wit_type_float() {
        assert_eq!(type_to_wit(&Type::Float).unwrap(), "float64");
        assert_eq!(type_to_wit(&Type::F64).unwrap(), "float64");
    }

    // ── Test 3 ──

    #[test]
    fn test_wit_type_bool() {
        assert_eq!(type_to_wit(&Type::Bool).unwrap(), "bool");
    }

    // ── Test 4 ──

    #[test]
    fn test_wit_type_str() {
        assert_eq!(type_to_wit(&Type::Str).unwrap(), "string");
    }

    // ── Test 5 ──

    #[test]
    fn test_wit_type_unit() {
        assert_eq!(type_to_wit(&Type::Unit).unwrap(), "unit");
    }

    // ── Test 6 ──

    #[test]
    fn test_wit_type_fixed_width() {
        assert_eq!(type_to_wit(&Type::Int8).unwrap(), "s8");
        assert_eq!(type_to_wit(&Type::Int16).unwrap(), "s16");
        assert_eq!(type_to_wit(&Type::Int32).unwrap(), "s32");
        assert_eq!(type_to_wit(&Type::U8).unwrap(), "u8");
        assert_eq!(type_to_wit(&Type::U16).unwrap(), "u16");
        assert_eq!(type_to_wit(&Type::U32).unwrap(), "u32");
        assert_eq!(type_to_wit(&Type::U64).unwrap(), "u64");
        assert_eq!(type_to_wit(&Type::F32).unwrap(), "float32");
    }

    // ── Test 7 ──

    #[test]
    fn test_wit_fn_type() {
        let result = fn_type_to_wit(&[Type::Str], &Type::Str).unwrap();
        assert_eq!(result, "func(p0: string) -> string");
    }

    // ── Test 8 ──

    #[test]
    fn test_wit_fn_multi_params() {
        let result = fn_type_to_wit(&[Type::Str, Type::Int, Type::Int], &Type::Str).unwrap();
        assert_eq!(result, "func(p0: string, p1: s64, p2: s64) -> string");
    }

    // ── Test 9 ──

    #[test]
    fn test_wit_fn_unit_return() {
        let result = fn_type_to_wit(&[Type::Str], &Type::Unit).unwrap();
        assert_eq!(result, "func(p0: string)");
    }

    // ── Test 10 ──

    #[test]
    fn test_wit_record_type() {
        let ty = Type::Record {
            name: "User".to_string(),
            fields: vec![
                ("name".to_string(), Type::Str),
                ("age".to_string(), Type::Int),
            ],
        };
        let result = type_to_wit(&ty).unwrap();
        assert!(result.contains("record user {"));
        assert!(result.contains("name: string,"));
        assert!(result.contains("age: s64,"));
    }

    // ── Test 11 ──

    #[test]
    fn test_wit_generate_interface() {
        let functions = vec![
            ("reverse-words".to_string(), vec![Type::Str], Type::Str),
            ("word-count".to_string(), vec![Type::Str], Type::Int),
        ];
        let result = generate_wit_interface("string-utils", &functions).unwrap();
        assert!(result.contains("package nexl:string-utils;"));
        assert!(result.contains("interface string-utils {"));
        assert!(result.contains("reverse-words: func(p0: string) -> string;"));
        assert!(result.contains("word-count: func(p0: string) -> s64;"));
    }

    // ── Test 12 ──

    #[test]
    fn test_wit_resource_type() {
        let resource = WitResource {
            name: "connection".to_string(),
            methods: vec![
                ("open".to_string(), vec![Type::Str], Type::Str),
                ("query".to_string(), vec![Type::Str], Type::Str),
                ("close".to_string(), vec![], Type::Unit),
            ],
        };
        let result = generate_wit_resource(&resource).unwrap();
        assert!(result.contains("resource connection {"));
        assert!(result.contains("open: func(p0: string) -> string;"));
        assert!(result.contains("query: func(p0: string) -> string;"));
        assert!(result.contains("close: func();"));
    }

    // ── Test 13 ──

    #[test]
    fn test_wit_resource_in_interface() {
        let resources = vec![WitResource {
            name: "connection".to_string(),
            methods: vec![
                ("open".to_string(), vec![Type::Str], Type::Str),
                ("close".to_string(), vec![], Type::Unit),
            ],
        }];
        let functions = vec![("version".to_string(), vec![], Type::Str)];
        let result = generate_wit_interface_full("database", &functions, &resources).unwrap();
        assert!(result.contains("package nexl:database;"));
        assert!(result.contains("interface database {"));
        assert!(result.contains("resource connection {"));
        assert!(result.contains("version: func() -> string;"));
    }

    // ── Test 14 ──

    #[test]
    fn test_effect_to_wit_interface() {
        // (defeffect Log (info : (Fn [Str] -> Unit)) (error : (Fn [Str] -> Unit)))
        let effect = EffectDef {
            name: "Log".to_string(),
            params: vec![],
            operations: vec![
                EffectOpDef {
                    name: "info".to_string(),
                    signature: Type::Fn {
                        params: vec![Type::Str],
                        ret: Box::new(Type::Unit),
                        effects: EffectRow::empty(),
                    },
                },
                EffectOpDef {
                    name: "error".to_string(),
                    signature: Type::Fn {
                        params: vec![Type::Str],
                        ret: Box::new(Type::Unit),
                        effects: EffectRow::empty(),
                    },
                },
            ],
        };
        let result = effect_to_wit_interface(&effect).unwrap();
        assert!(result.contains("interface log {"));
        assert!(result.contains("info: func(p0: string);"));
        assert!(result.contains("error: func(p0: string);"));
    }

    // ── Test 15 ──

    #[test]
    fn test_wit_interface_to_effect() {
        // WIT interface "log" with info/error ops → EffectDef "Log"
        let ops = vec![
            ("info".to_string(), vec![Type::Str], Type::Unit),
            ("error".to_string(), vec![Type::Str], Type::Unit),
        ];
        let effect = wit_interface_to_effect("log", &ops);
        assert_eq!(effect.name, "Log");
        assert_eq!(effect.operations.len(), 2);
        assert_eq!(effect.operations[0].name, "info");
        assert_eq!(effect.operations[1].name, "error");
    }

    // ── Test 16 ──

    #[test]
    fn test_wit_interface_to_effect_kebab_case() {
        // "file-system" → "FileSystem"
        let ops = vec![("read".to_string(), vec![Type::Str], Type::Str)];
        let effect = wit_interface_to_effect("file-system", &ops);
        assert_eq!(effect.name, "FileSystem");
    }

    // ── Test 17 ──

    #[test]
    fn test_effect_to_wit_interface_with_return() {
        // (defeffect State (get : (Fn [] -> Int)) (put : (Fn [Int] -> Unit)))
        let effect = EffectDef {
            name: "State".to_string(),
            params: vec![],
            operations: vec![
                EffectOpDef {
                    name: "get".to_string(),
                    signature: Type::Fn {
                        params: vec![],
                        ret: Box::new(Type::Int),
                        effects: EffectRow::empty(),
                    },
                },
                EffectOpDef {
                    name: "put".to_string(),
                    signature: Type::Fn {
                        params: vec![Type::Int],
                        ret: Box::new(Type::Unit),
                        effects: EffectRow::empty(),
                    },
                },
            ],
        };
        let result = effect_to_wit_interface(&effect).unwrap();
        assert!(result.contains("interface state {"));
        assert!(result.contains("get: func() -> s64;"));
        assert!(result.contains("put: func(p0: s64);"));
    }
}
