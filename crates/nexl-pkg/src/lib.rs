//! `nexl-pkg` — package manifest schema for `nexl.toml`.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

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
    pub registry: Option<String>,
}

/// Registry configuration in `[registries]`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct RegistrySpec {
    /// Base URL for the registry.
    pub url: String,
    /// Environment variable holding an auth token.
    #[serde(rename = "token-env")]
    pub token_env: Option<String>,
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

        let manifest: PackageManifest = toml::from_str(input).expect("manifest parse");
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

        let manifest: PackageManifest = toml::from_str(input).expect("manifest parse");
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

        let manifest: PackageManifest = toml::from_str(input).expect("manifest parse");
        assert!(manifest.dependencies.is_empty());
        assert!(manifest.dev_dependencies.is_empty());
        assert!(manifest.registries.is_empty());
    }
}
