/// Name resolution: map qualified (`alias/name`) and unqualified (`name`) symbol
/// references to their canonical `(module_path, name)` origin.
///
/// Build a [`NameResolver`] via [`build_name_resolver`], then call
/// [`NameResolver::resolve_qualified`] or [`NameResolver::resolve_unqualified`].
use std::collections::HashMap;

use nexl_ast::{ImportDecl, ImportKind};

// ---------------------------------------------------------------------------
// Output types
// ---------------------------------------------------------------------------

/// The resolved canonical origin of a symbol reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedName {
    /// Fully-qualified module path that owns this name.
    pub module: String,
    /// The name as it appears in the owning module.
    pub name: String,
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors produced during name resolution.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ResolveError {
    /// A qualified reference `alias/name` used an alias that was never imported.
    #[error("unknown import alias `{alias}`")]
    UnknownAlias { alias: String },

    /// A name is not in the module's exported set.
    #[error("`{name}` is not exported by module `{module}`")]
    NameNotExported { module: String, name: String },

    /// A bare name matches exports from more than one imported module.
    #[error("`{name}` is ambiguous: found in modules {modules:?}")]
    AmbiguousName { name: String, modules: Vec<String> },

    /// A bare name is not in scope at all.
    #[error("`{name}` is not in scope")]
    UnknownName { name: String },
}

// ---------------------------------------------------------------------------
// Internal storage
// ---------------------------------------------------------------------------

/// Internal entry for the unqualified name table.
#[derive(Debug)]
enum UnqualifiedEntry {
    Unique(ResolvedName),
    Ambiguous(Vec<String>), // sorted list of module paths that all export this name
}

// ---------------------------------------------------------------------------
// Resolver
// ---------------------------------------------------------------------------

/// Resolved name scope built from a module's import declarations.
#[derive(Debug)]
pub struct NameResolver {
    /// alias → module_path, for qualified resolution.
    alias_to_module: HashMap<String, String>,
    /// alias → exported name list, for qualified validation.
    alias_exports: HashMap<String, Vec<String>>,
    /// bare name → unique origin or ambiguity info.
    unqualified: HashMap<String, UnqualifiedEntry>,
}

impl NameResolver {
    /// Resolve a qualified reference `alias/name` to its canonical origin.
    pub fn resolve_qualified(&self, alias: &str, name: &str) -> Result<ResolvedName, ResolveError> {
        let module = self
            .alias_to_module
            .get(alias)
            .ok_or_else(|| ResolveError::UnknownAlias {
                alias: alias.to_string(),
            })?;

        let exports = self.alias_exports.get(alias).expect("alias_exports in sync");
        if !exports.contains(&name.to_string()) {
            return Err(ResolveError::NameNotExported {
                module: module.clone(),
                name: name.to_string(),
            });
        }

        Ok(ResolvedName {
            module: module.clone(),
            name: name.to_string(),
        })
    }

    /// Resolve a bare (unqualified) name to its canonical origin.
    pub fn resolve_unqualified(&self, name: &str) -> Result<ResolvedName, ResolveError> {
        match self.unqualified.get(name) {
            None => Err(ResolveError::UnknownName {
                name: name.to_string(),
            }),
            Some(UnqualifiedEntry::Unique(resolved)) => Ok(resolved.clone()),
            Some(UnqualifiedEntry::Ambiguous(modules)) => Err(ResolveError::AmbiguousName {
                name: name.to_string(),
                modules: modules.clone(),
            }),
        }
    }
}

// ---------------------------------------------------------------------------
// Builder
// ---------------------------------------------------------------------------

/// Build a [`NameResolver`] from a module's imports and each imported module's
/// exported name list.
///
/// `imports` is a slice of `(ImportDecl, exports)` where `exports` is the list
/// of names the imported module makes public.  If a module's `ModuleDecl` has
/// `exports: None` (everything exported), the caller should pass the full set
/// of top-level definition names.
///
/// Returns an error if any import references a name that the target module
/// does not export.
pub fn build_name_resolver(
    imports: &[(ImportDecl, Vec<String>)],
) -> Result<NameResolver, ResolveError> {
    let mut alias_to_module: HashMap<String, String> = HashMap::new();
    let mut alias_exports: HashMap<String, Vec<String>> = HashMap::new();
    // name → all (module, original_name) pairs that bring it into unqualified scope
    let mut candidates: HashMap<String, Vec<ResolvedName>> = HashMap::new();

    for (import, exports) in imports {
        match &import.kind {
            ImportKind::Alias(alias) => {
                alias_to_module.insert(alias.clone(), import.module_path.clone());
                alias_exports.insert(alias.clone(), exports.clone());
            }

            ImportKind::Refer(names) => {
                for name in names {
                    if !exports.contains(name) {
                        return Err(ResolveError::NameNotExported {
                            module: import.module_path.clone(),
                            name: name.clone(),
                        });
                    }
                    candidates.entry(name.clone()).or_default().push(ResolvedName {
                        module: import.module_path.clone(),
                        name: name.clone(),
                    });
                }
            }

            ImportKind::All => {
                for name in exports {
                    candidates.entry(name.clone()).or_default().push(ResolvedName {
                        module: import.module_path.clone(),
                        name: name.clone(),
                    });
                }
            }

            ImportKind::Exclude(excluded) => {
                for excl in excluded {
                    if !exports.contains(excl) {
                        return Err(ResolveError::NameNotExported {
                            module: import.module_path.clone(),
                            name: excl.clone(),
                        });
                    }
                }
                for name in exports {
                    if !excluded.contains(name) {
                        candidates.entry(name.clone()).or_default().push(ResolvedName {
                            module: import.module_path.clone(),
                            name: name.clone(),
                        });
                    }
                }
            }

            ImportKind::Rename(renames) => {
                for (old, new) in renames {
                    if !exports.contains(old) {
                        return Err(ResolveError::NameNotExported {
                            module: import.module_path.clone(),
                            name: old.clone(),
                        });
                    }
                    candidates.entry(new.clone()).or_default().push(ResolvedName {
                        module: import.module_path.clone(),
                        name: old.clone(),
                    });
                }
            }
        }
    }

    // Collapse candidates into unique entries or ambiguity markers.
    let mut unqualified: HashMap<String, UnqualifiedEntry> = HashMap::new();
    for (name, mut resolved_list) in candidates {
        if resolved_list.len() == 1 {
            unqualified.insert(name, UnqualifiedEntry::Unique(resolved_list.remove(0)));
        } else {
            let mut modules: Vec<String> =
                resolved_list.iter().map(|r| r.module.clone()).collect();
            modules.sort();
            modules.dedup();
            unqualified.insert(name, UnqualifiedEntry::Ambiguous(modules));
        }
    }

    Ok(NameResolver {
        alias_to_module,
        alias_exports,
        unqualified,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use nexl_ast::ImportKind;

    fn mk_import(module_path: &str, kind: ImportKind) -> ImportDecl {
        ImportDecl {
            module_path: module_path.to_string(),
            kind,
        }
    }

    // ── Test 1 ──────────────────────────────────────────────────────────────

    #[test]
    fn resolve_qualified_alias() {
        // (import lib.http :as http), exports [get post]
        // http/get → { module: "lib.http", name: "get" }
        let imports = vec![(
            mk_import("lib.http", ImportKind::Alias("http".to_string())),
            vec!["get".to_string(), "post".to_string()],
        )];
        let resolver = build_name_resolver(&imports).expect("build failed");
        let resolved = resolver
            .resolve_qualified("http", "get")
            .expect("resolve failed");
        assert_eq!(resolved.module, "lib.http");
        assert_eq!(resolved.name, "get");
    }

    // ── Test 2 ──────────────────────────────────────────────────────────────

    #[test]
    fn resolve_qualified_unknown_alias() {
        // No alias `foo` imported — foo/bar should give UnknownAlias.
        let imports: Vec<(ImportDecl, Vec<String>)> = vec![];
        let resolver = build_name_resolver(&imports).expect("build failed");
        let err = resolver.resolve_qualified("foo", "bar").unwrap_err();
        assert_eq!(err, ResolveError::UnknownAlias { alias: "foo".to_string() });
    }

    // ── Test 3 ──────────────────────────────────────────────────────────────

    #[test]
    fn resolve_qualified_name_not_exported() {
        // (import lib.http :as http), exports [get]
        // http/post → NameNotExported
        let imports = vec![(
            mk_import("lib.http", ImportKind::Alias("http".to_string())),
            vec!["get".to_string()],
        )];
        let resolver = build_name_resolver(&imports).expect("build failed");
        let err = resolver.resolve_qualified("http", "post").unwrap_err();
        assert_eq!(
            err,
            ResolveError::NameNotExported {
                module: "lib.http".to_string(),
                name: "post".to_string(),
            }
        );
    }

    // ── Test 4 ──────────────────────────────────────────────────────────────

    #[test]
    fn resolve_unqualified_refer_found() {
        // (import lib.coll :refer [map filter]) — resolve `map`
        let imports = vec![(
            mk_import(
                "lib.coll",
                ImportKind::Refer(vec!["map".to_string(), "filter".to_string()]),
            ),
            vec!["map".to_string(), "filter".to_string(), "reduce".to_string()],
        )];
        let resolver = build_name_resolver(&imports).expect("build failed");
        let resolved = resolver.resolve_unqualified("map").expect("resolve failed");
        assert_eq!(resolved.module, "lib.coll");
        assert_eq!(resolved.name, "map");
    }

    // ── Test 5 ──────────────────────────────────────────────────────────────

    #[test]
    fn resolve_unqualified_refer_not_in_list() {
        // (import lib.coll :refer [map]) — resolve `filter` → UnknownName
        let imports = vec![(
            mk_import("lib.coll", ImportKind::Refer(vec!["map".to_string()])),
            vec!["map".to_string(), "filter".to_string()],
        )];
        let resolver = build_name_resolver(&imports).expect("build failed");
        let err = resolver.resolve_unqualified("filter").unwrap_err();
        assert_eq!(err, ResolveError::UnknownName { name: "filter".to_string() });
    }

    // ── Test 6 ──────────────────────────────────────────────────────────────

    #[test]
    fn resolve_unqualified_all() {
        // (import lib.math) — bare import, exports [add sub mul]
        let imports = vec![(
            mk_import("lib.math", ImportKind::All),
            vec!["add".to_string(), "sub".to_string(), "mul".to_string()],
        )];
        let resolver = build_name_resolver(&imports).expect("build failed");
        let resolved = resolver.resolve_unqualified("add").expect("resolve failed");
        assert_eq!(resolved.module, "lib.math");
        assert_eq!(resolved.name, "add");
    }

    // ── Test 7 ──────────────────────────────────────────────────────────────

    #[test]
    fn resolve_unqualified_exclude() {
        // (import lib.math :exclude [div]), exports [add sub div]
        let imports = vec![(
            mk_import("lib.math", ImportKind::Exclude(vec!["div".to_string()])),
            vec!["add".to_string(), "sub".to_string(), "div".to_string()],
        )];
        let resolver = build_name_resolver(&imports).expect("build failed");
        // add is in scope
        let resolved = resolver.resolve_unqualified("add").expect("resolve failed");
        assert_eq!(resolved.module, "lib.math");
        assert_eq!(resolved.name, "add");
        // div is excluded
        let err = resolver.resolve_unqualified("div").unwrap_err();
        assert_eq!(err, ResolveError::UnknownName { name: "div".to_string() });
    }

    // ── Test 8 ──────────────────────────────────────────────────────────────

    #[test]
    fn resolve_unqualified_rename() {
        // (import lib.str :rename {split split-str}), exports [split join]
        let imports = vec![(
            mk_import(
                "lib.str",
                ImportKind::Rename(vec![("split".to_string(), "split-str".to_string())]),
            ),
            vec!["split".to_string(), "join".to_string()],
        )];
        let resolver = build_name_resolver(&imports).expect("build failed");
        // New name resolves to original
        let resolved = resolver
            .resolve_unqualified("split-str")
            .expect("resolve failed");
        assert_eq!(resolved.module, "lib.str");
        assert_eq!(resolved.name, "split");
        // Original name is not in scope
        let err = resolver.resolve_unqualified("split").unwrap_err();
        assert_eq!(err, ResolveError::UnknownName { name: "split".to_string() });
    }

    // ── Test 9 ──────────────────────────────────────────────────────────────

    #[test]
    fn resolve_unqualified_ambiguous() {
        // (import mod-a) exports [foo] + (import mod-b) exports [foo]
        // resolve `foo` → AmbiguousName
        let imports = vec![
            (
                mk_import("mod-a", ImportKind::All),
                vec!["foo".to_string()],
            ),
            (
                mk_import("mod-b", ImportKind::All),
                vec!["foo".to_string()],
            ),
        ];
        let resolver = build_name_resolver(&imports).expect("build failed");
        let err = resolver.resolve_unqualified("foo").unwrap_err();
        match err {
            ResolveError::AmbiguousName { name, mut modules } => {
                assert_eq!(name, "foo");
                modules.sort();
                assert_eq!(modules, vec!["mod-a".to_string(), "mod-b".to_string()]);
            }
            other => panic!("expected AmbiguousName, got {other:?}"),
        }
    }

    // ── Test 10 ─────────────────────────────────────────────────────────────

    #[test]
    fn resolve_unqualified_unknown() {
        // `baz` is never brought into scope
        let imports: Vec<(ImportDecl, Vec<String>)> = vec![];
        let resolver = build_name_resolver(&imports).expect("build failed");
        let err = resolver.resolve_unqualified("baz").unwrap_err();
        assert_eq!(err, ResolveError::UnknownName { name: "baz".to_string() });
    }

    // ── Test 11 ─────────────────────────────────────────────────────────────

    #[test]
    fn resolve_refer_nonexistent_name() {
        // (import lib.coll :refer [nonexistent]), exports [map filter]
        // → NameNotExported at build time
        let imports = vec![(
            mk_import(
                "lib.coll",
                ImportKind::Refer(vec!["nonexistent".to_string()]),
            ),
            vec!["map".to_string(), "filter".to_string()],
        )];
        let err = build_name_resolver(&imports).unwrap_err();
        assert_eq!(
            err,
            ResolveError::NameNotExported {
                module: "lib.coll".to_string(),
                name: "nonexistent".to_string(),
            }
        );
    }

    // ── Test 12 ─────────────────────────────────────────────────────────────

    #[test]
    fn resolve_exclude_nonexistent_name() {
        // (import lib.coll :exclude [nonexistent]), exports [map filter]
        // → NameNotExported at build time
        let imports = vec![(
            mk_import(
                "lib.coll",
                ImportKind::Exclude(vec!["nonexistent".to_string()]),
            ),
            vec!["map".to_string(), "filter".to_string()],
        )];
        let err = build_name_resolver(&imports).unwrap_err();
        assert_eq!(
            err,
            ResolveError::NameNotExported {
                module: "lib.coll".to_string(),
                name: "nonexistent".to_string(),
            }
        );
    }
}
