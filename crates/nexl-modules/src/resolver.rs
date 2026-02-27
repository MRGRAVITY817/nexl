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

/// Exported name sets for a module, split by visibility.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleExports {
    /// Publicly exported names (`:exports`).
    pub public: Vec<String>,
    /// Package-private names (`^:package`).
    pub package: Vec<String>,
}

impl ModuleExports {
    /// Convenience: build exports with only public names.
    pub fn public(names: Vec<String>) -> Self {
        Self {
            public: names,
            package: Vec::new(),
        }
    }
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

    /// A package-private name is accessed from outside its package.
    #[error(
        "package-private name `{name}` from module `{module}` is not visible outside its package"
    )]
    PackagePrivateNotVisible { module: String, name: String },

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
    /// alias → public exports, for qualified validation.
    alias_public_exports: HashMap<String, Vec<String>>,
    /// alias → package-private exports, for visibility errors.
    alias_package_exports: HashMap<String, Vec<String>>,
    /// alias → whether the import is from the same package.
    alias_same_package: HashMap<String, bool>,
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

        let public_exports = self
            .alias_public_exports
            .get(alias)
            .expect("alias_public_exports in sync");
        let package_exports = self
            .alias_package_exports
            .get(alias)
            .expect("alias_package_exports in sync");
        let same_package = *self
            .alias_same_package
            .get(alias)
            .expect("alias_same_package in sync");

        let name_string = name.to_string();
        let is_public = public_exports.contains(&name_string);
        let is_package = package_exports.contains(&name_string);

        if is_public || (same_package && is_package) {
            return Ok(ResolvedName {
                module: module.clone(),
                name: name_string,
            });
        }

        if is_package && !same_package {
            return Err(ResolveError::PackagePrivateNotVisible {
                module: module.clone(),
                name: name_string,
            });
        }

        Err(ResolveError::NameNotExported {
            module: module.clone(),
            name: name_string,
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
/// exported name lists.
///
/// `imports` is a slice of `(ImportDecl, ModuleExports)`. The resolver enforces
/// package-private visibility by comparing each import's module path to the
/// caller's `importer_package_prefix`.
///
/// Returns an error if any import references a name that the target module
/// does not export.
pub fn build_name_resolver(
    importer_package_prefix: &str,
    imports: &[(ImportDecl, ModuleExports)],
) -> Result<NameResolver, ResolveError> {
    let mut alias_to_module: HashMap<String, String> = HashMap::new();
    let mut alias_public_exports: HashMap<String, Vec<String>> = HashMap::new();
    let mut alias_package_exports: HashMap<String, Vec<String>> = HashMap::new();
    let mut alias_same_package: HashMap<String, bool> = HashMap::new();
    // name → all (module, original_name) pairs that bring it into unqualified scope
    let mut candidates: HashMap<String, Vec<ResolvedName>> = HashMap::new();

    for (import, exports) in imports {
        let same_package = crate::has_prefix(&import.module_path, importer_package_prefix);
        match &import.kind {
            ImportKind::Alias(alias) => {
                alias_to_module.insert(alias.clone(), import.module_path.clone());
                alias_public_exports.insert(alias.clone(), exports.public.clone());
                alias_package_exports.insert(alias.clone(), exports.package.clone());
                alias_same_package.insert(alias.clone(), same_package);
            }

            ImportKind::Refer(names) => {
                for name in names {
                    validate_visibility(&import.module_path, name, exports, same_package)?;
                    candidates
                        .entry(name.clone())
                        .or_default()
                        .push(ResolvedName {
                            module: import.module_path.clone(),
                            name: name.clone(),
                        });
                }
            }

            ImportKind::All => {
                for name in visible_exports(exports, same_package) {
                    candidates
                        .entry(name.clone())
                        .or_default()
                        .push(ResolvedName {
                            module: import.module_path.clone(),
                            name,
                        });
                }
            }

            ImportKind::Exclude(excluded) => {
                for excl in excluded {
                    validate_visibility(&import.module_path, excl, exports, same_package)?;
                }
                for name in visible_exports(exports, same_package) {
                    if !excluded.contains(&name) {
                        candidates
                            .entry(name.clone())
                            .or_default()
                            .push(ResolvedName {
                                module: import.module_path.clone(),
                                name,
                            });
                    }
                }
            }

            ImportKind::Rename(renames) => {
                for (old, new) in renames {
                    validate_visibility(&import.module_path, old, exports, same_package)?;
                    candidates
                        .entry(new.clone())
                        .or_default()
                        .push(ResolvedName {
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
            let mut modules: Vec<String> = resolved_list.iter().map(|r| r.module.clone()).collect();
            modules.sort();
            modules.dedup();
            unqualified.insert(name, UnqualifiedEntry::Ambiguous(modules));
        }
    }

    Ok(NameResolver {
        alias_to_module,
        alias_public_exports,
        alias_package_exports,
        alias_same_package,
        unqualified,
    })
}

fn visible_exports(exports: &ModuleExports, same_package: bool) -> Vec<String> {
    let mut names = exports.public.clone();
    if same_package {
        names.extend(exports.package.iter().cloned());
    }
    names.sort();
    names.dedup();
    names
}

fn validate_visibility(
    module: &str,
    name: &str,
    exports: &ModuleExports,
    same_package: bool,
) -> Result<(), ResolveError> {
    if exports.public.contains(&name.to_string()) {
        return Ok(());
    }
    if exports.package.contains(&name.to_string()) {
        if same_package {
            return Ok(());
        }
        return Err(ResolveError::PackagePrivateNotVisible {
            module: module.to_string(),
            name: name.to_string(),
        });
    }
    Err(ResolveError::NameNotExported {
        module: module.to_string(),
        name: name.to_string(),
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

    fn exports(public: &[&str]) -> ModuleExports {
        ModuleExports::public(public.iter().map(|name| (*name).to_string()).collect())
    }

    fn exports_with_package(public: &[&str], package: &[&str]) -> ModuleExports {
        ModuleExports {
            public: public.iter().map(|name| (*name).to_string()).collect(),
            package: package.iter().map(|name| (*name).to_string()).collect(),
        }
    }

    // ── Test 1 ──────────────────────────────────────────────────────────────

    #[test]
    fn resolve_qualified_alias() {
        // (import lib.http :as http), exports [get post]
        // http/get → { module: "lib.http", name: "get" }
        let imports = vec![(
            mk_import("lib.http", ImportKind::Alias("http".to_string())),
            exports(&["get", "post"]),
        )];
        let resolver = build_name_resolver("app", &imports).expect("build failed");
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
        let imports: Vec<(ImportDecl, ModuleExports)> = vec![];
        let resolver = build_name_resolver("app", &imports).expect("build failed");
        let err = resolver.resolve_qualified("foo", "bar").unwrap_err();
        assert_eq!(
            err,
            ResolveError::UnknownAlias {
                alias: "foo".to_string()
            }
        );
    }

    // ── Test 3 ──────────────────────────────────────────────────────────────

    #[test]
    fn resolve_qualified_name_not_exported() {
        // (import lib.http :as http), exports [get]
        // http/post → NameNotExported
        let imports = vec![(
            mk_import("lib.http", ImportKind::Alias("http".to_string())),
            exports(&["get"]),
        )];
        let resolver = build_name_resolver("app", &imports).expect("build failed");
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
            exports(&["map", "filter", "reduce"]),
        )];
        let resolver = build_name_resolver("app", &imports).expect("build failed");
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
            exports(&["map", "filter"]),
        )];
        let resolver = build_name_resolver("app", &imports).expect("build failed");
        let err = resolver.resolve_unqualified("filter").unwrap_err();
        assert_eq!(
            err,
            ResolveError::UnknownName {
                name: "filter".to_string()
            }
        );
    }

    // ── Test 6 ──────────────────────────────────────────────────────────────

    #[test]
    fn resolve_unqualified_all() {
        // (import lib.math) — bare import, exports [add sub mul]
        let imports = vec![(
            mk_import("lib.math", ImportKind::All),
            exports(&["add", "sub", "mul"]),
        )];
        let resolver = build_name_resolver("app", &imports).expect("build failed");
        let resolved = resolver.resolve_unqualified("add").expect("resolve failed");
        assert_eq!(resolved.module, "lib.math");
        assert_eq!(resolved.name, "add");
    }

    // ── Test 7 ──────────────────────────────────────────────────────────────

    #[test]
    fn resolve_unqualified_all_unexported_name() {
        // (import lib.math) — bare import, exports [add]
        // resolve `sub` → UnknownName
        let imports = vec![(mk_import("lib.math", ImportKind::All), exports(&["add"]))];
        let resolver = build_name_resolver("app", &imports).expect("build failed");
        let err = resolver.resolve_unqualified("sub").unwrap_err();
        assert_eq!(
            err,
            ResolveError::UnknownName {
                name: "sub".to_string()
            }
        );
    }

    // ── Test 8 ──────────────────────────────────────────────────────────────

    #[test]
    fn resolve_unqualified_exclude() {
        // (import lib.math :exclude [div]), exports [add sub div]
        let imports = vec![(
            mk_import("lib.math", ImportKind::Exclude(vec!["div".to_string()])),
            exports(&["add", "sub", "div"]),
        )];
        let resolver = build_name_resolver("app", &imports).expect("build failed");
        // add is in scope
        let resolved = resolver.resolve_unqualified("add").expect("resolve failed");
        assert_eq!(resolved.module, "lib.math");
        assert_eq!(resolved.name, "add");
        // div is excluded
        let err = resolver.resolve_unqualified("div").unwrap_err();
        assert_eq!(
            err,
            ResolveError::UnknownName {
                name: "div".to_string()
            }
        );
    }

    // ── Test 10 ─────────────────────────────────────────────────────────────

    #[test]
    fn resolve_unqualified_rename() {
        // (import lib.str :rename {split split-str}), exports [split join]
        let imports = vec![(
            mk_import(
                "lib.str",
                ImportKind::Rename(vec![("split".to_string(), "split-str".to_string())]),
            ),
            exports(&["split", "join"]),
        )];
        let resolver = build_name_resolver("app", &imports).expect("build failed");
        // New name resolves to original
        let resolved = resolver
            .resolve_unqualified("split-str")
            .expect("resolve failed");
        assert_eq!(resolved.module, "lib.str");
        assert_eq!(resolved.name, "split");
        // Original name is not in scope
        let err = resolver.resolve_unqualified("split").unwrap_err();
        assert_eq!(
            err,
            ResolveError::UnknownName {
                name: "split".to_string()
            }
        );
    }

    // ── Test 9 ──────────────────────────────────────────────────────────────

    #[test]
    fn resolve_unqualified_ambiguous() {
        // (import mod-a) exports [foo] + (import mod-b) exports [foo]
        // resolve `foo` → AmbiguousName
        let imports = vec![
            (mk_import("mod-a", ImportKind::All), exports(&["foo"])),
            (mk_import("mod-b", ImportKind::All), exports(&["foo"])),
        ];
        let resolver = build_name_resolver("app", &imports).expect("build failed");
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

    // ── Test 11 ─────────────────────────────────────────────────────────────

    #[test]
    fn resolve_unqualified_unknown() {
        // `baz` is never brought into scope
        let imports: Vec<(ImportDecl, ModuleExports)> = vec![];
        let resolver = build_name_resolver("app", &imports).expect("build failed");
        let err = resolver.resolve_unqualified("baz").unwrap_err();
        assert_eq!(
            err,
            ResolveError::UnknownName {
                name: "baz".to_string()
            }
        );
    }

    // ── Test 12 ─────────────────────────────────────────────────────────────

    #[test]
    fn resolve_refer_nonexistent_name() {
        // (import lib.coll :refer [nonexistent]), exports [map filter]
        // → NameNotExported at build time
        let imports = vec![(
            mk_import(
                "lib.coll",
                ImportKind::Refer(vec!["nonexistent".to_string()]),
            ),
            exports(&["map", "filter"]),
        )];
        let err = build_name_resolver("app", &imports).unwrap_err();
        assert_eq!(
            err,
            ResolveError::NameNotExported {
                module: "lib.coll".to_string(),
                name: "nonexistent".to_string(),
            }
        );
    }

    // ── Test 13 ─────────────────────────────────────────────────────────────

    #[test]
    fn resolve_exclude_nonexistent_name() {
        // (import lib.coll :exclude [nonexistent]), exports [map filter]
        // → NameNotExported at build time
        let imports = vec![(
            mk_import(
                "lib.coll",
                ImportKind::Exclude(vec!["nonexistent".to_string()]),
            ),
            exports(&["map", "filter"]),
        )];
        let err = build_name_resolver("app", &imports).unwrap_err();
        assert_eq!(
            err,
            ResolveError::NameNotExported {
                module: "lib.coll".to_string(),
                name: "nonexistent".to_string(),
            }
        );
    }

    // ── Test 14 ─────────────────────────────────────────────────────────────

    #[test]
    fn resolve_refer_package_private_external() {
        // (import lib.util :refer [internal]), exports [pub] + ^:package [internal]
        let imports = vec![(
            mk_import("lib.util", ImportKind::Refer(vec!["internal".to_string()])),
            exports_with_package(&["pub"], &["internal"]),
        )];
        let err = build_name_resolver("my-app", &imports).unwrap_err();
        assert_eq!(
            err,
            ResolveError::PackagePrivateNotVisible {
                module: "lib.util".to_string(),
                name: "internal".to_string(),
            }
        );
    }

    // ── Test 15 ─────────────────────────────────────────────────────────────

    #[test]
    fn resolve_refer_package_private_same_package() {
        // (import my-app.util :refer [internal]), exports [pub] + ^:package [internal]
        let imports = vec![(
            mk_import(
                "my-app.util",
                ImportKind::Refer(vec!["internal".to_string()]),
            ),
            exports_with_package(&["pub"], &["internal"]),
        )];
        let resolver = build_name_resolver("my-app", &imports).expect("build failed");
        let resolved = resolver
            .resolve_unqualified("internal")
            .expect("resolve failed");
        assert_eq!(resolved.module, "my-app.util");
        assert_eq!(resolved.name, "internal");
    }

    // ── Test 16 ─────────────────────────────────────────────────────────────

    #[test]
    fn resolve_qualified_package_private_external() {
        // (import lib.util :as util), exports [pub] + ^:package [internal]
        let imports = vec![(
            mk_import("lib.util", ImportKind::Alias("util".to_string())),
            exports_with_package(&["pub"], &["internal"]),
        )];
        let resolver = build_name_resolver("my-app", &imports).expect("build failed");
        let err = resolver.resolve_qualified("util", "internal").unwrap_err();
        assert_eq!(
            err,
            ResolveError::PackagePrivateNotVisible {
                module: "lib.util".to_string(),
                name: "internal".to_string(),
            }
        );
    }
}
