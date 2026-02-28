//! Effect ↔ WASI capability mapping (M23 task 7).
//!
//! Maps Nexl effect names (`:performs [Net]`, `:performs [FileSystem]`, …) to their
//! corresponding WASI interface imports.  Provides enforcement: a module that does
//! not declare an effect cannot receive the corresponding WASI capability.
//!
//! # Effect → WASI mapping
//!
//! | Nexl effect      | WASI interface         |
//! |------------------|------------------------|
//! | `Net`            | `wasi:http/outgoing`   |
//! | `FileSystem`     | `wasi:filesystem`      |
//! | `Console`        | `wasi:cli/io`          |
//! | `Clock` / `Time` | `wasi:clocks`          |
//! | `Random`         | `wasi:random`          |
//! | `Sockets`        | `wasi:sockets`         |

use nexl_types::Type;

use crate::wit_export::{WitImportIface, WitModuleExport};

// ─── Static capability map ────────────────────────────────────────────────────

/// A single entry mapping a Nexl effect name to a WASI interface descriptor.
#[derive(Debug, Clone, PartialEq)]
pub struct EffectWasiEntry {
    /// Nexl effect name (e.g. `"Net"`, `"FileSystem"`).
    pub effect_name: &'static str,
    /// WASI package + interface path (e.g. `"wasi:http/outgoing-handler"`).
    pub wasi_interface: &'static str,
    /// Human-readable description.
    pub description: &'static str,
}

/// The static effect → WASI capability table.
pub static EFFECT_WASI_MAP: &[EffectWasiEntry] = &[
    EffectWasiEntry {
        effect_name: "Net",
        wasi_interface: "wasi:http/outgoing-handler",
        description: "HTTP client requests",
    },
    EffectWasiEntry {
        effect_name: "FileSystem",
        wasi_interface: "wasi:filesystem/types",
        description: "Filesystem read/write access",
    },
    EffectWasiEntry {
        effect_name: "Console",
        wasi_interface: "wasi:cli/stdout",
        description: "Standard I/O (stdin/stdout/stderr)",
    },
    EffectWasiEntry {
        effect_name: "Clock",
        wasi_interface: "wasi:clocks/wall-clock",
        description: "Wall-clock time",
    },
    EffectWasiEntry {
        effect_name: "Time",
        wasi_interface: "wasi:clocks/wall-clock",
        description: "Wall-clock time (alias for Clock)",
    },
    EffectWasiEntry {
        effect_name: "Random",
        wasi_interface: "wasi:random/random",
        description: "Cryptographically secure random bytes",
    },
    EffectWasiEntry {
        effect_name: "Sockets",
        wasi_interface: "wasi:sockets/tcp",
        description: "TCP client and server sockets",
    },
];

// ─── Lookup ───────────────────────────────────────────────────────────────────

/// Look up the WASI interface path for a Nexl effect name.
///
/// Returns `None` if no mapping exists (the effect is user-defined and has no
/// corresponding WASI capability).
pub fn effect_to_wasi_interface(effect_name: &str) -> Option<&'static str> {
    EFFECT_WASI_MAP
        .iter()
        .find(|e| e.effect_name == effect_name)
        .map(|e| e.wasi_interface)
}

/// Return all effects that map to a given WASI interface path.
///
/// A single WASI interface may be the target of multiple effect names
/// (e.g. `Clock` and `Time` both map to `wasi:clocks/wall-clock`).
pub fn wasi_interface_effects(wasi_interface: &str) -> Vec<&'static str> {
    EFFECT_WASI_MAP
        .iter()
        .filter(|e| e.wasi_interface == wasi_interface)
        .map(|e| e.effect_name)
        .collect()
}

// ─── Validation ───────────────────────────────────────────────────────────────

/// An error produced when a module's WASI imports exceed its declared effects.
#[derive(Debug, Clone, PartialEq)]
pub struct CapabilityViolation {
    /// The WASI interface that was requested without the corresponding effect.
    pub wasi_interface: String,
    /// The effect name(s) that would permit this interface.
    pub required_effects: Vec<String>,
}

impl std::fmt::Display for CapabilityViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let effects = self.required_effects.join(" or ");
        write!(
            f,
            "WASI import `{}` requires :performs [{}]",
            self.wasi_interface, effects
        )
    }
}

impl std::error::Error for CapabilityViolation {}

/// Validate that all WASI imports in a [`WitModuleExport`] are permitted by the
/// given set of declared Nexl effects.
///
/// Returns `Ok(())` if every import is covered, or a vec of [`CapabilityViolation`]s
/// for each import that lacks a corresponding declared effect.
///
/// # Example
/// ```
/// # use nexl_wasm::effect_wasi::validate_wasi_capabilities;
/// # use nexl_wasm::wit_export::{WitModuleExport, WitImportIface, WitExportFn};
/// # use nexl_types::Type;
/// let module = WitModuleExport {
///     name: "my-service".to_string(),
///     exports: vec![],
///     imports: vec![WitImportIface {
///         name: "wasi:http/outgoing-handler".to_string(),
///         functions: vec![],
///     }],
/// };
/// let effects = &["Net".to_string()];
/// assert!(validate_wasi_capabilities(&module, effects).is_ok());
/// ```
pub fn validate_wasi_capabilities(
    module: &WitModuleExport,
    declared_effects: &[String],
) -> Result<(), Vec<CapabilityViolation>> {
    let mut violations = Vec::new();

    for import in &module.imports {
        let wasi_iface = &import.name;

        // Check whether any declared effect covers this WASI interface.
        let permitted = declared_effects.iter().any(|eff| {
            effect_to_wasi_interface(eff)
                .map(|wasi| wasi == wasi_iface)
                .unwrap_or(false)
        });

        if !permitted {
            let required = wasi_interface_effects(wasi_iface)
                .into_iter()
                .map(|s| s.to_string())
                .collect();
            violations.push(CapabilityViolation {
                wasi_interface: wasi_iface.clone(),
                required_effects: required,
            });
        }
    }

    if violations.is_empty() {
        Ok(())
    } else {
        Err(violations)
    }
}

// ─── World generation from effects ───────────────────────────────────────────

/// `(fn_name, params, ret)` triple for a WASI stub function.
type WasiFnStub = (String, Vec<Type>, Type);

/// Return canonical WASI function signatures for each well-known interface.
///
/// Each entry is `(wasi_interface, vec[(fn_name, params, ret)])`.
///
/// Signatures are simplified stubs; the real Component Model uses resource handles.
fn wasi_interface_stubs() -> Vec<(&'static str, Vec<WasiFnStub>)> {
    vec![
        // wasi:http/outgoing-handler — HTTP client
        (
            "wasi:http/outgoing-handler",
            vec![
                ("get".to_string(), vec![Type::Str], Type::Str),
                ("post".to_string(), vec![Type::Str, Type::Str], Type::Str),
            ],
        ),
        // wasi:filesystem/types — filesystem access
        (
            "wasi:filesystem/types",
            vec![
                (
                    "read-file".to_string(),
                    vec![Type::Str],
                    Type::Vec(Box::new(Type::U8)),
                ),
                (
                    "write-file".to_string(),
                    vec![Type::Str, Type::Vec(Box::new(Type::U8))],
                    Type::Unit,
                ),
            ],
        ),
        // wasi:cli/stdout — console I/O
        (
            "wasi:cli/stdout",
            vec![("write".to_string(), vec![Type::Str], Type::Unit)],
        ),
        // wasi:clocks/wall-clock — time
        (
            "wasi:clocks/wall-clock",
            vec![("now".to_string(), vec![], Type::Int)],
        ),
        // wasi:random/random — random bytes
        (
            "wasi:random/random",
            vec![(
                "get-random-bytes".to_string(),
                vec![Type::U64],
                Type::Vec(Box::new(Type::U8)),
            )],
        ),
        // wasi:sockets/tcp
        (
            "wasi:sockets/tcp",
            vec![(
                "connect".to_string(),
                vec![Type::Str, Type::Int32],
                Type::Int32,
            )],
        ),
    ]
}

/// Build the [`WitImportIface`] list for a set of declared Nexl effects.
///
/// Each effect that has a WASI mapping produces one import interface with
/// canonical stub function signatures. Effects without a WASI mapping are
/// silently skipped.
pub fn effects_to_wasm_imports(declared_effects: &[String]) -> Vec<WitImportIface> {
    // Deduplicate by WASI interface path (Clock and Time both → wasi:clocks).
    let mut seen = std::collections::HashSet::new();
    let mut imports = Vec::new();

    let stubs = wasi_interface_stubs();

    for effect in declared_effects {
        if let Some(wasi_iface) = effect_to_wasi_interface(effect).filter(|w| seen.insert(*w)) {
            {
                // Find the stub signatures for this interface.
                let functions = stubs
                    .iter()
                    .find(|(iface, _)| *iface == wasi_iface)
                    .map(|(_, fns)| fns.clone())
                    .unwrap_or_default();

                imports.push(WitImportIface {
                    name: wasi_iface.to_string(),
                    functions,
                });
            }
        }
    }

    imports
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wit_export::WitExportFn;

    // ── Test 1 ──────────────────────────────────────────────────────────────

    #[test]
    fn test_net_effect_maps_to_http() {
        let iface = effect_to_wasi_interface("Net").unwrap();
        assert_eq!(iface, "wasi:http/outgoing-handler");
    }

    // ── Test 2 ──────────────────────────────────────────────────────────────

    #[test]
    fn test_filesystem_effect_maps_to_filesystem() {
        let iface = effect_to_wasi_interface("FileSystem").unwrap();
        assert_eq!(iface, "wasi:filesystem/types");
    }

    // ── Test 3 ──────────────────────────────────────────────────────────────

    #[test]
    fn test_clock_and_time_both_map_to_clocks() {
        assert_eq!(
            effect_to_wasi_interface("Clock"),
            Some("wasi:clocks/wall-clock")
        );
        assert_eq!(
            effect_to_wasi_interface("Time"),
            Some("wasi:clocks/wall-clock")
        );
    }

    // ── Test 4 ──────────────────────────────────────────────────────────────

    #[test]
    fn test_random_effect_maps_to_random() {
        let iface = effect_to_wasi_interface("Random").unwrap();
        assert_eq!(iface, "wasi:random/random");
    }

    // ── Test 5 ──────────────────────────────────────────────────────────────

    #[test]
    fn test_unknown_effect_returns_none() {
        assert_eq!(effect_to_wasi_interface("UserDefinedEffect"), None);
        assert_eq!(effect_to_wasi_interface("Logging"), None);
    }

    // ── Test 6 ──────────────────────────────────────────────────────────────

    #[test]
    fn test_effects_to_imports() {
        let effects = vec!["Net".to_string(), "FileSystem".to_string()];
        let imports = effects_to_wasm_imports(&effects);
        assert_eq!(imports.len(), 2);
        assert_eq!(imports[0].name, "wasi:http/outgoing-handler");
        assert_eq!(imports[1].name, "wasi:filesystem/types");
    }

    // ── Test 7 ──────────────────────────────────────────────────────────────

    #[test]
    fn test_effects_deduplicated() {
        // Clock and Time both → wasi:clocks/wall-clock; should produce only one import.
        let effects = vec!["Clock".to_string(), "Time".to_string()];
        let imports = effects_to_wasm_imports(&effects);
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].name, "wasi:clocks/wall-clock");
    }

    // ── Test 8 ──────────────────────────────────────────────────────────────

    #[test]
    fn test_validate_permitted_effects() {
        // Module with Net effect → wasi:http import is permitted.
        let module = WitModuleExport {
            name: "service".to_string(),
            exports: vec![],
            imports: vec![WitImportIface {
                name: "wasi:http/outgoing-handler".to_string(),
                functions: vec![],
            }],
        };
        let effects = vec!["Net".to_string()];
        assert!(validate_wasi_capabilities(&module, &effects).is_ok());
    }

    // ── Test 9 ──────────────────────────────────────────────────────────────

    #[test]
    fn test_validate_missing_effect() {
        // Module WITHOUT Net effect → wasi:http import is a violation.
        let module = WitModuleExport {
            name: "service".to_string(),
            exports: vec![],
            imports: vec![WitImportIface {
                name: "wasi:http/outgoing-handler".to_string(),
                functions: vec![],
            }],
        };
        let effects: Vec<String> = vec![]; // no effects declared
        let result = validate_wasi_capabilities(&module, &effects);
        assert!(result.is_err());
        let violations = result.unwrap_err();
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].wasi_interface, "wasi:http/outgoing-handler");
        assert!(violations[0].required_effects.contains(&"Net".to_string()));
    }

    // ── Test 10 ─────────────────────────────────────────────────────────────

    #[test]
    fn test_validate_multiple_violations() {
        // Module imports http AND filesystem but declares no effects.
        let module = WitModuleExport {
            name: "service".to_string(),
            exports: vec![],
            imports: vec![
                WitImportIface {
                    name: "wasi:http/outgoing-handler".to_string(),
                    functions: vec![],
                },
                WitImportIface {
                    name: "wasi:filesystem/types".to_string(),
                    functions: vec![],
                },
            ],
        };
        let effects: Vec<String> = vec![];
        let result = validate_wasi_capabilities(&module, &effects);
        let violations = result.unwrap_err();
        assert_eq!(violations.len(), 2);
    }

    // ── Test 11 ─────────────────────────────────────────────────────────────

    #[test]
    fn test_generate_world_from_effects() {
        // Build a world with Net + FileSystem effects auto-populating imports.
        let declared_effects = vec!["Net".to_string(), "FileSystem".to_string()];
        let imports = effects_to_wasm_imports(&declared_effects);
        let module = WitModuleExport {
            name: "my-service".to_string(),
            exports: vec![WitExportFn {
                name: "run".to_string(),
                params: vec![],
                ret: Type::Unit,
            }],
            imports,
        };

        let wit = crate::wit_export::generate_wit_world(&module).unwrap();
        assert!(
            wit.contains("import wasi:http/outgoing-handler: interface {"),
            "missing http import: {wit}"
        );
        assert!(
            wit.contains("import wasi:filesystem/types: interface {"),
            "missing fs import: {wit}"
        );
        assert!(wit.contains("export run: func();"), "missing run export: {wit}");
    }
}
