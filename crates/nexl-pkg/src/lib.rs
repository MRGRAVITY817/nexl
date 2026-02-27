//! `nexl-pkg` — package manifest schema for `nexl.toml`.

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use thiserror::Error;

/// A parsed `nexl.toml` manifest.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct PackageManifest {
    /// Package metadata.
    pub package: PackageSection,
    /// Production dependencies.
    #[serde(default)]
    pub dependencies: BTreeMap<String, DependencySpec>,
    /// Development-only dependencies.
    #[serde(rename = "dev-dependencies", default)]
    pub dev_dependencies: BTreeMap<String, DependencySpec>,
    /// Optional registry definitions.
    #[serde(default)]
    pub registries: BTreeMap<String, RegistrySpec>,
}

/// The `[package]` table in `nexl.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct PackageSection {
    /// Package name.
    pub name: String,
    /// Semver version string.
    pub version: String,
    /// Optional description for humans.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Package prefix for module names.
    pub prefix: String,
}

/// A dependency entry in `[dependencies]` / `[dev-dependencies]`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum DependencySpec {
    /// A plain semver range string, e.g. "^1.2.3".
    Version(String),
    /// A structured dependency with a registry override.
    Detailed(DependencyDetail),
}

/// Structured dependency specification.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct DependencyDetail {
    /// Semver range string.
    pub version: String,
    /// Optional registry alias to resolve against.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub registry: Option<String>,
}

/// Registry configuration in `[registries]`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct RegistrySpec {
    /// Base URL for the registry.
    pub url: String,
    /// Environment variable holding an auth token.
    #[serde(rename = "token-env")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_env: Option<String>,
}

/// Errors returned when parsing a `nexl.toml` manifest.
#[derive(Debug, Error)]
pub enum ManifestError {
    /// The manifest is not valid TOML or does not match the schema.
    #[error("invalid manifest: {0}")]
    Toml(#[from] toml::de::Error),
}

/// Parse a `nexl.toml` manifest into its schema representation.
pub fn parse_manifest(source: &str) -> Result<PackageManifest, ManifestError> {
    Ok(toml::from_str(source)?)
}

/// The dependency bucket a manifest entry came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum DependencyKind {
    /// Runtime dependency from `[dependencies]`.
    Runtime,
    /// Development-only dependency from `[dev-dependencies]`.
    Dev,
}

/// A resolved dependency entry in a flat graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedDependency {
    /// Dependency name.
    pub name: String,
    /// Semver range string.
    pub version: String,
    /// Optional registry alias.
    pub registry: Option<String>,
    /// Dependency kind.
    pub kind: DependencyKind,
}

/// A lockfile generated from a resolved manifest.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct Lockfile {
    /// Locked dependencies keyed by name.
    pub dependencies: BTreeMap<String, LockedDependency>,
}

/// A single locked dependency entry.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct LockedDependency {
    /// Semver range string.
    pub version: String,
    /// Optional registry alias.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub registry: Option<String>,
    /// Dependency kind.
    pub kind: DependencyKind,
}

/// Errors produced during dependency resolution.
#[derive(Debug, Error)]
pub enum ResolveError {
    /// Two entries for the same dependency disagree.
    #[error("dependency `{name}` has conflicting requirements: {a} vs {b}")]
    Conflict { name: String, a: String, b: String },
    /// A dependency references a registry that is not declared.
    #[error("dependency `{name}` references unknown registry `{registry}`")]
    UnknownRegistry { name: String, registry: String },
}

/// Resolve dependencies into a flat list with conflict checking.
pub fn resolve_dependencies(
    manifest: &PackageManifest,
) -> Result<Vec<ResolvedDependency>, ResolveError> {
    let mut resolved = Vec::new();
    let mut seen: HashMap<String, (String, Option<String>, DependencyKind)> = HashMap::new();

    for (name, spec) in &manifest.dependencies {
        add_dependency(
            name,
            spec,
            DependencyKind::Runtime,
            manifest,
            &mut resolved,
            &mut seen,
        )?;
    }

    for (name, spec) in &manifest.dev_dependencies {
        add_dependency(
            name,
            spec,
            DependencyKind::Dev,
            manifest,
            &mut resolved,
            &mut seen,
        )?;
    }

    Ok(resolved)
}

/// Build a lockfile from the manifest's resolved dependencies.
pub fn build_lockfile(manifest: &PackageManifest) -> Result<Lockfile, ResolveError> {
    let resolved = resolve_dependencies(manifest)?;
    let mut dependencies = BTreeMap::new();
    for dep in resolved {
        dependencies.insert(
            dep.name,
            LockedDependency {
                version: dep.version,
                registry: dep.registry,
                kind: dep.kind,
            },
        );
    }
    Ok(Lockfile { dependencies })
}

/// SQLite-backed content-addressed definition store.
#[derive(Debug)]
pub struct DefinitionStore {
    conn: Connection,
}

/// Errors returned by the definition store.
#[derive(Debug, Error)]
pub enum StoreError {
    /// Database error returned by SQLite.
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
}

impl DefinitionStore {
    /// Open or create a definition store at the given path.
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        let conn = Connection::open(path)?;
        let store = Self { conn };
        store.init()?;
        Ok(store)
    }

    /// Open an in-memory definition store (useful for tests).
    pub fn open_in_memory() -> Result<Self, StoreError> {
        let conn = Connection::open_in_memory()?;
        let store = Self { conn };
        store.init()?;
        Ok(store)
    }

    /// Store an artifact by its content hash.
    pub fn put(&self, hash: &str, artifact: &[u8]) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT OR REPLACE INTO artifacts (hash, artifact) VALUES (?1, ?2)",
            params![hash, artifact],
        )?;
        Ok(())
    }

    /// Fetch an artifact by its content hash.
    pub fn get(&self, hash: &str) -> Result<Option<Vec<u8>>, StoreError> {
        let mut stmt = self
            .conn
            .prepare("SELECT artifact FROM artifacts WHERE hash = ?1")?;
        let mut rows = stmt.query(params![hash])?;
        match rows.next()? {
            Some(row) => {
                let data: Vec<u8> = row.get(0)?;
                Ok(Some(data))
            }
            None => Ok(None),
        }
    }

    fn init(&self) -> Result<(), StoreError> {
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS artifacts (hash TEXT PRIMARY KEY, artifact BLOB NOT NULL)",
            [],
        )?;
        Ok(())
    }
}

fn add_dependency(
    name: &str,
    spec: &DependencySpec,
    kind: DependencyKind,
    manifest: &PackageManifest,
    resolved: &mut Vec<ResolvedDependency>,
    seen: &mut HashMap<String, (String, Option<String>, DependencyKind)>,
) -> Result<(), ResolveError> {
    let (version, registry) = normalize_spec(spec);
    match registry.as_ref() {
        Some(registry_name) if !manifest.registries.contains_key(registry_name) => {
            return Err(ResolveError::UnknownRegistry {
                name: name.to_string(),
                registry: registry_name.clone(),
            });
        }
        _ => {}
    }

    match seen.get_mut(name) {
        Some((seen_version, seen_registry, seen_kind)) => {
            if seen_version != &version || seen_registry != &registry {
                return Err(ResolveError::Conflict {
                    name: name.to_string(),
                    a: format_spec(seen_version, seen_registry),
                    b: format_spec(&version, &registry),
                });
            }
            if *seen_kind == DependencyKind::Dev && kind == DependencyKind::Runtime {
                *seen_kind = DependencyKind::Runtime;
                if let Some(entry) = resolved.iter_mut().find(|entry| entry.name == name) {
                    entry.kind = DependencyKind::Runtime;
                }
            }
        }
        None => {
            seen.insert(name.to_string(), (version.clone(), registry.clone(), kind));
            resolved.push(ResolvedDependency {
                name: name.to_string(),
                version,
                registry,
                kind,
            });
        }
    }

    Ok(())
}

fn normalize_spec(spec: &DependencySpec) -> (String, Option<String>) {
    match spec {
        DependencySpec::Version(version) => (version.clone(), None),
        DependencySpec::Detailed(detail) => (detail.version.clone(), detail.registry.clone()),
    }
}

fn format_spec(version: &str, registry: &Option<String>) -> String {
    match registry {
        Some(registry) => format!("{version} (registry {registry})"),
        None => version.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_deserializes_basic_example() {
        let input = r#"
[package]
name = "my-app"
version = "1.0.0"
description = "My application"
prefix = "my-app"

[dependencies]
http-server = "^2.1.0"
json = "~1.0.0"

[dev-dependencies]
test-utils = "^1.0.0"
bench-tools = "^0.5.0"
"#;

        let manifest = parse_manifest(input).expect("manifest parse");
        assert_eq!(manifest.package.name, "my-app");
        assert_eq!(manifest.package.version, "1.0.0");
        assert_eq!(manifest.package.prefix, "my-app");
        assert_eq!(
            manifest.dependencies.get("http-server"),
            Some(&DependencySpec::Version("^2.1.0".to_string()))
        );
        assert_eq!(
            manifest.dev_dependencies.get("bench-tools"),
            Some(&DependencySpec::Version("^0.5.0".to_string()))
        );
    }

    #[test]
    fn manifest_deserializes_registry_dependency() {
        let input = r#"
[package]
name = "demo"
version = "0.1.0"
prefix = "demo"

[registries]
internal = { url = "https://registry.corp.example.com", token-env = "NEXL_CORP_TOKEN" }

[dependencies]
internal-lib = { version = "^1.0.0", registry = "internal" }
"#;

        let manifest = parse_manifest(input).expect("manifest parse");
        let registry = manifest
            .registries
            .get("internal")
            .expect("registry entry");
        assert_eq!(registry.url, "https://registry.corp.example.com");
        assert_eq!(registry.token_env.as_deref(), Some("NEXL_CORP_TOKEN"));
        assert_eq!(
            manifest.dependencies.get("internal-lib"),
            Some(&DependencySpec::Detailed(DependencyDetail {
                version: "^1.0.0".to_string(),
                registry: Some("internal".to_string())
            }))
        );
    }

    #[test]
    fn manifest_defaults_missing_sections() {
        let input = r#"
[package]
name = "solo"
version = "0.1.0"
prefix = "solo"
"#;

        let manifest = parse_manifest(input).expect("manifest parse");
        assert!(manifest.dependencies.is_empty());
        assert!(manifest.dev_dependencies.is_empty());
        assert!(manifest.registries.is_empty());
    }

    #[test]
    fn parse_manifest_missing_package_is_error() {
        let input = r#"
[dependencies]
json = "~1.0.0"
"#;

        let err = parse_manifest(input).expect_err("missing package should error");
        let message = err.to_string();
        assert!(message.contains("package"), "unexpected error: {message}");
    }

    fn base_manifest() -> PackageManifest {
        PackageManifest {
            package: PackageSection {
                name: "demo".to_string(),
                version: "0.1.0".to_string(),
                description: None,
                prefix: "demo".to_string(),
            },
            dependencies: BTreeMap::new(),
            dev_dependencies: BTreeMap::new(),
            registries: BTreeMap::new(),
        }
    }

    #[test]
    fn resolve_dependencies_combines_runtime_and_dev() {
        let mut manifest = base_manifest();
        manifest.dependencies.insert(
            "core".to_string(),
            DependencySpec::Version("^1.0.0".to_string()),
        );
        manifest.dev_dependencies.insert(
            "test-utils".to_string(),
            DependencySpec::Version("^0.1.0".to_string()),
        );

        let resolved = resolve_dependencies(&manifest).expect("resolve deps");
        assert_eq!(resolved.len(), 2);
        assert!(resolved.iter().any(|dep| {
            dep.name == "core" && dep.kind == DependencyKind::Runtime && dep.version == "^1.0.0"
        }));
        assert!(resolved.iter().any(|dep| {
            dep.name == "test-utils"
                && dep.kind == DependencyKind::Dev
                && dep.version == "^0.1.0"
        }));
    }

    #[test]
    fn resolve_dependencies_dedups_same_spec() {
        let mut manifest = base_manifest();
        manifest.dependencies.insert(
            "core".to_string(),
            DependencySpec::Version("^1.0.0".to_string()),
        );
        manifest.dev_dependencies.insert(
            "core".to_string(),
            DependencySpec::Version("^1.0.0".to_string()),
        );

        let resolved = resolve_dependencies(&manifest).expect("resolve deps");
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].name, "core");
        assert_eq!(resolved[0].kind, DependencyKind::Runtime);
    }

    #[test]
    fn resolve_dependencies_conflicts_on_version() {
        let mut manifest = base_manifest();
        manifest.dependencies.insert(
            "core".to_string(),
            DependencySpec::Version("^1.0.0".to_string()),
        );
        manifest.dev_dependencies.insert(
            "core".to_string(),
            DependencySpec::Version("^2.0.0".to_string()),
        );

        let err = resolve_dependencies(&manifest).expect_err("conflict should error");
        match err {
            ResolveError::Conflict { name, .. } => assert_eq!(name, "core"),
            other => panic!("expected conflict error, got {other:?}"),
        }
    }

    #[test]
    fn resolve_dependencies_unknown_registry_is_error() {
        let mut manifest = base_manifest();
        manifest.dependencies.insert(
            "internal-lib".to_string(),
            DependencySpec::Detailed(DependencyDetail {
                version: "^1.0.0".to_string(),
                registry: Some("internal".to_string()),
            }),
        );

        let err = resolve_dependencies(&manifest).expect_err("unknown registry should error");
        match err {
            ResolveError::UnknownRegistry { registry, .. } => assert_eq!(registry, "internal"),
            other => panic!("expected unknown registry error, got {other:?}"),
        }
    }

    #[test]
    fn build_lockfile_captures_dependency_kinds() {
        let mut manifest = base_manifest();
        manifest.dependencies.insert(
            "core".to_string(),
            DependencySpec::Version("^1.0.0".to_string()),
        );
        manifest.dev_dependencies.insert(
            "test-utils".to_string(),
            DependencySpec::Version("^0.1.0".to_string()),
        );

        let lockfile = build_lockfile(&manifest).expect("lockfile build");
        let core = lockfile
            .dependencies
            .get("core")
            .expect("core entry");
        assert_eq!(core.kind, DependencyKind::Runtime);
        let test_utils = lockfile
            .dependencies
            .get("test-utils")
            .expect("test utils entry");
        assert_eq!(test_utils.kind, DependencyKind::Dev);
    }

    #[test]
    fn definition_store_roundtrips_artifacts() {
        let store = DefinitionStore::open_in_memory().expect("store open");
        store
            .put("hash-1", b"artifact")
            .expect("store write");
        let fetched = store.get("hash-1").expect("store read");
        assert_eq!(fetched, Some(b"artifact".to_vec()));
    }

    #[test]
    fn definition_store_missing_returns_none() {
        let store = DefinitionStore::open_in_memory().expect("store open");
        let fetched = store.get("missing").expect("store read");
        assert_eq!(fetched, None);
    }
}
