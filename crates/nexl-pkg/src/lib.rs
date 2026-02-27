//! `nexl-pkg` — package manifest schema for `project.nexl` (EDN format).

use meta::{Atom, Node, NodeKind};
use rusqlite::{Connection, params};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt;
use std::path::Path;
use thiserror::Error;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A parsed `project.nexl` manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageManifest {
    /// Package metadata.
    pub package: PackageSection,
    /// Production dependencies.
    pub dependencies: BTreeMap<String, DependencySpec>,
    /// Development-only dependencies.
    pub dev_dependencies: BTreeMap<String, DependencySpec>,
    /// Optional registry definitions.
    pub registries: BTreeMap<String, RegistrySpec>,
    /// Optional sandbox configuration.
    pub sandbox: Option<SandboxConfig>,
    /// Optional per-profile overrides.
    pub profiles: BTreeMap<String, ProfileConfig>,
}

/// The `:package` section in `project.nexl`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageSection {
    /// Package name.
    pub name: String,
    /// Semver version string.
    pub version: String,
    /// Optional description for humans.
    pub description: Option<String>,
    /// Package prefix for module names.
    pub prefix: String,
    /// Source directory relative to project root (default `"."`).
    pub source_dir: String,
}

/// A dependency entry in `:dependencies` / `:dev-dependencies`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DependencySpec {
    /// A plain semver range string, e.g. `"^1.2.3"`.
    Version(String),
    /// A structured dependency with a registry override.
    Detailed(DependencyDetail),
}

/// Structured dependency specification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DependencyDetail {
    /// Semver range string.
    pub version: String,
    /// Optional registry alias to resolve against.
    pub registry: Option<String>,
}

/// Registry configuration in `:registries`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistrySpec {
    /// Base URL for the registry.
    pub url: String,
    /// Environment variable holding an auth token.
    pub token_env: Option<String>,
}

/// A sandbox capability that can be granted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Capability {
    /// Network access.
    Net,
    /// File-system access.
    Fs,
    /// Console I/O.
    Console,
    /// Wall-clock time.
    Time,
    /// Random number generation.
    Random,
    /// Concurrency primitives.
    Concurrent,
    /// Unsafe FFI operations.
    Unsafe,
}

impl Capability {
    /// Keyword name for this capability (without the leading `:`).
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Net => "net",
            Self::Fs => "fs",
            Self::Console => "console",
            Self::Time => "time",
            Self::Random => "random",
            Self::Concurrent => "concurrent",
            Self::Unsafe => "unsafe",
        }
    }

    /// Parse a keyword name into a capability.
    fn from_str(s: &str) -> Result<Self, ManifestError> {
        match s {
            "net" => Ok(Self::Net),
            "fs" => Ok(Self::Fs),
            "console" => Ok(Self::Console),
            "time" => Ok(Self::Time),
            "random" => Ok(Self::Random),
            "concurrent" => Ok(Self::Concurrent),
            "unsafe" => Ok(Self::Unsafe),
            other => Err(ManifestError::Parse(format!("unknown capability :{other}"))),
        }
    }
}

impl fmt::Display for Capability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Sandbox configuration in `:sandbox`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxConfig {
    /// Capabilities granted to this package.
    pub allow: BTreeSet<Capability>,
    /// Allowed network hosts.
    pub net: BTreeSet<String>,
    /// Allowed file-system paths.
    pub fs: BTreeSet<String>,
}

/// Per-profile configuration override.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileConfig {
    /// Sandbox override for this profile.
    pub sandbox: Option<SandboxConfig>,
}

/// The dependency bucket a manifest entry came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DependencyKind {
    /// Runtime dependency from `:dependencies`.
    Runtime,
    /// Development-only dependency from `:dev-dependencies`.
    Dev,
}

impl DependencyKind {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Runtime => "runtime",
            Self::Dev => "dev",
        }
    }

    fn from_str(s: &str) -> Result<Self, ManifestError> {
        match s {
            "runtime" => Ok(Self::Runtime),
            "dev" => Ok(Self::Dev),
            other => Err(ManifestError::Parse(format!(
                "unknown dependency kind :{other}"
            ))),
        }
    }
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lockfile {
    /// Locked dependencies keyed by name.
    pub dependencies: BTreeMap<String, LockedDependency>,
}

/// A single locked dependency entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockedDependency {
    /// Semver range string.
    pub version: String,
    /// Optional registry alias.
    pub registry: Option<String>,
    /// Dependency kind.
    pub kind: DependencyKind,
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors returned when parsing a `project.nexl` manifest.
#[derive(Debug, Error)]
pub enum ManifestError {
    /// The manifest source could not be parsed as EDN.
    #[error("parse error: {0}")]
    Parse(String),
    /// A required field is missing.
    #[error("missing field: {0}")]
    MissingField(String),
    /// A field has the wrong type.
    #[error("field `{field}`: expected {expected}")]
    TypeError {
        /// The field that has the wrong type.
        field: String,
        /// Description of the expected type.
        expected: String,
    },
}

// ---------------------------------------------------------------------------
// EDN Parser
// ---------------------------------------------------------------------------

/// Parse a `project.nexl` manifest (EDN format) into its schema representation.
pub fn parse_manifest(source: &str) -> Result<PackageManifest, ManifestError> {
    let nodes = nexl_reader::read(source, meta::FileId::SYNTHETIC)
        .map_err(|e| ManifestError::Parse(e.to_string()))?;

    if nodes.len() != 1 {
        return Err(ManifestError::Parse(
            "expected a single top-level map".to_string(),
        ));
    }

    let pairs = match &nodes[0].kind {
        NodeKind::Map(pairs) => pairs,
        _ => {
            return Err(ManifestError::Parse(
                "expected a map as the top-level form".to_string(),
            ));
        }
    };

    let mut package = None;
    let mut dependencies = BTreeMap::new();
    let mut dev_dependencies = BTreeMap::new();
    let mut registries = BTreeMap::new();
    let mut sandbox = None;
    let mut profiles = BTreeMap::new();

    for (key, value) in pairs {
        match keyword_name(key) {
            Some("package") => package = Some(parse_package_section(value)?),
            Some("dependencies") => dependencies = parse_dependencies(value)?,
            Some("dev-dependencies") => dev_dependencies = parse_dependencies(value)?,
            Some("registries") => registries = parse_registries(value)?,
            Some("sandbox") => sandbox = Some(parse_sandbox_section(value)?),
            Some("profiles") => profiles = parse_profiles(value)?,
            Some(other) => {
                return Err(ManifestError::Parse(format!(
                    "unknown top-level key :{other}"
                )));
            }
            None => {
                return Err(ManifestError::Parse(
                    "top-level keys must be keywords".to_string(),
                ));
            }
        }
    }

    let package = package.ok_or_else(|| ManifestError::MissingField("package".to_string()))?;

    Ok(PackageManifest {
        package,
        dependencies,
        dev_dependencies,
        registries,
        sandbox,
        profiles,
    })
}

fn keyword_name(node: &Node) -> Option<&str> {
    match &node.kind {
        NodeKind::Atom(Atom::Keyword { ns: None, name }) => Some(name.as_str()),
        _ => None,
    }
}

fn string_value(node: &Node) -> Option<&str> {
    match &node.kind {
        NodeKind::Atom(Atom::Str(s)) => Some(s.as_str()),
        _ => None,
    }
}

fn expect_string(node: &Node, field: &str) -> Result<String, ManifestError> {
    string_value(node)
        .map(|s| s.to_string())
        .ok_or_else(|| ManifestError::TypeError {
            field: field.to_string(),
            expected: "string".to_string(),
        })
}

fn expect_map<'a>(node: &'a Node, field: &str) -> Result<&'a [(Node, Node)], ManifestError> {
    match &node.kind {
        NodeKind::Map(pairs) => Ok(pairs),
        _ => Err(ManifestError::TypeError {
            field: field.to_string(),
            expected: "map".to_string(),
        }),
    }
}

fn parse_package_section(node: &Node) -> Result<PackageSection, ManifestError> {
    let pairs = expect_map(node, "package")?;
    let mut name = None;
    let mut version = None;
    let mut description = None;
    let mut prefix = None;
    let mut source_dir = None;

    for (key, value) in pairs {
        match keyword_name(key) {
            Some("name") => name = Some(expect_string(value, "package.name")?),
            Some("version") => version = Some(expect_string(value, "package.version")?),
            Some("description") => description = Some(expect_string(value, "package.description")?),
            Some("prefix") => prefix = Some(expect_string(value, "package.prefix")?),
            Some("source-dir") => source_dir = Some(expect_string(value, "package.source-dir")?),
            Some(other) => {
                return Err(ManifestError::Parse(format!(
                    "unknown package field :{other}"
                )));
            }
            None => {
                return Err(ManifestError::Parse(
                    "package keys must be keywords".to_string(),
                ));
            }
        }
    }

    Ok(PackageSection {
        name: name.ok_or_else(|| ManifestError::MissingField("package.name".to_string()))?,
        version: version
            .ok_or_else(|| ManifestError::MissingField("package.version".to_string()))?,
        description,
        prefix: prefix.ok_or_else(|| ManifestError::MissingField("package.prefix".to_string()))?,
        source_dir: source_dir.unwrap_or_else(|| ".".to_string()),
    })
}

fn parse_dependencies(node: &Node) -> Result<BTreeMap<String, DependencySpec>, ManifestError> {
    let pairs = expect_map(node, "dependencies")?;
    let mut deps = BTreeMap::new();

    for (key, value) in pairs {
        let name = expect_string(key, "dependency name")?;
        let spec = match &value.kind {
            NodeKind::Atom(Atom::Str(v)) => DependencySpec::Version(v.clone()),
            NodeKind::Map(inner) => {
                let detail = parse_dependency_detail(inner)?;
                DependencySpec::Detailed(detail)
            }
            _ => {
                return Err(ManifestError::TypeError {
                    field: format!("dependency `{name}`"),
                    expected: "string or map".to_string(),
                });
            }
        };
        deps.insert(name, spec);
    }

    Ok(deps)
}

fn parse_dependency_detail(pairs: &[(Node, Node)]) -> Result<DependencyDetail, ManifestError> {
    let mut version = None;
    let mut registry = None;

    for (key, value) in pairs {
        match keyword_name(key) {
            Some("version") => version = Some(expect_string(value, "dependency.version")?),
            Some("registry") => registry = Some(expect_string(value, "dependency.registry")?),
            Some(other) => {
                return Err(ManifestError::Parse(format!(
                    "unknown dependency field :{other}"
                )));
            }
            None => {
                return Err(ManifestError::Parse(
                    "dependency detail keys must be keywords".to_string(),
                ));
            }
        }
    }

    Ok(DependencyDetail {
        version: version
            .ok_or_else(|| ManifestError::MissingField("dependency.version".to_string()))?,
        registry,
    })
}

fn parse_registries(node: &Node) -> Result<BTreeMap<String, RegistrySpec>, ManifestError> {
    let pairs = expect_map(node, "registries")?;
    let mut regs = BTreeMap::new();

    for (key, value) in pairs {
        let name = expect_string(key, "registry name")?;
        let inner = expect_map(value, &format!("registry `{name}`"))?;
        let mut url = None;
        let mut token_env = None;

        for (k, v) in inner {
            match keyword_name(k) {
                Some("url") => url = Some(expect_string(v, "registry.url")?),
                Some("token-env") => token_env = Some(expect_string(v, "registry.token-env")?),
                Some(other) => {
                    return Err(ManifestError::Parse(format!(
                        "unknown registry field :{other}"
                    )));
                }
                None => {
                    return Err(ManifestError::Parse(
                        "registry keys must be keywords".to_string(),
                    ));
                }
            }
        }

        regs.insert(
            name,
            RegistrySpec {
                url: url.ok_or_else(|| ManifestError::MissingField("registry.url".to_string()))?,
                token_env,
            },
        );
    }

    Ok(regs)
}

fn parse_sandbox_section(node: &Node) -> Result<SandboxConfig, ManifestError> {
    let pairs = expect_map(node, "sandbox")?;
    parse_sandbox_config(pairs)
}

fn parse_sandbox_config(pairs: &[(Node, Node)]) -> Result<SandboxConfig, ManifestError> {
    let mut allow = BTreeSet::new();
    let mut net = BTreeSet::new();
    let mut fs = BTreeSet::new();

    for (key, value) in pairs {
        match keyword_name(key) {
            Some("allow") => allow = parse_capability_set(value)?,
            Some("net") => net = parse_string_set(value, "sandbox.net")?,
            Some("fs") => fs = parse_string_set(value, "sandbox.fs")?,
            Some(other) => {
                return Err(ManifestError::Parse(format!(
                    "unknown sandbox field :{other}"
                )));
            }
            None => {
                return Err(ManifestError::Parse(
                    "sandbox keys must be keywords".to_string(),
                ));
            }
        }
    }

    Ok(SandboxConfig { allow, net, fs })
}

fn parse_capability_set(node: &Node) -> Result<BTreeSet<Capability>, ManifestError> {
    let items = match &node.kind {
        NodeKind::Set(items) => items,
        _ => {
            return Err(ManifestError::TypeError {
                field: "sandbox.allow".to_string(),
                expected: "set".to_string(),
            });
        }
    };

    let mut caps = BTreeSet::new();
    for item in items {
        let name = keyword_name(item).ok_or_else(|| ManifestError::TypeError {
            field: "sandbox.allow".to_string(),
            expected: "keyword".to_string(),
        })?;
        caps.insert(Capability::from_str(name)?);
    }
    Ok(caps)
}

fn parse_string_set(node: &Node, field: &str) -> Result<BTreeSet<String>, ManifestError> {
    let items = match &node.kind {
        NodeKind::Set(items) => items,
        _ => {
            return Err(ManifestError::TypeError {
                field: field.to_string(),
                expected: "set".to_string(),
            });
        }
    };

    let mut set = BTreeSet::new();
    for item in items {
        set.insert(expect_string(item, field)?);
    }
    Ok(set)
}

fn parse_profiles(node: &Node) -> Result<BTreeMap<String, ProfileConfig>, ManifestError> {
    let pairs = expect_map(node, "profiles")?;
    let mut profiles = BTreeMap::new();

    for (key, value) in pairs {
        let name = keyword_name(key)
            .ok_or_else(|| ManifestError::TypeError {
                field: "profiles".to_string(),
                expected: "keyword key".to_string(),
            })?
            .to_string();

        let inner = expect_map(value, &format!("profile `{name}`"))?;
        let mut sandbox = None;

        for (k, v) in inner {
            match keyword_name(k) {
                Some("sandbox") => {
                    let sb_pairs = expect_map(v, &format!("profile `{name}` sandbox"))?;
                    sandbox = Some(parse_sandbox_config(sb_pairs)?);
                }
                Some(other) => {
                    return Err(ManifestError::Parse(format!(
                        "unknown profile field :{other}"
                    )));
                }
                None => {
                    return Err(ManifestError::Parse(
                        "profile keys must be keywords".to_string(),
                    ));
                }
            }
        }

        profiles.insert(name, ProfileConfig { sandbox });
    }

    Ok(profiles)
}

// ---------------------------------------------------------------------------
// EDN Serializer
// ---------------------------------------------------------------------------

/// Serialize a manifest to EDN format.
pub fn serialize_manifest(manifest: &PackageManifest) -> String {
    let mut sections: Vec<(&str, Vec<String>)> = Vec::new();

    // Package section
    sections.push(("package", serialize_package_lines(&manifest.package)));

    // Dependencies
    if !manifest.dependencies.is_empty() {
        sections.push(("dependencies", serialize_deps_lines(&manifest.dependencies)));
    }

    // Dev-dependencies
    if !manifest.dev_dependencies.is_empty() {
        sections.push((
            "dev-dependencies",
            serialize_deps_lines(&manifest.dev_dependencies),
        ));
    }

    // Registries
    if !manifest.registries.is_empty() {
        sections.push((
            "registries",
            serialize_registries_lines(&manifest.registries),
        ));
    }

    // Sandbox
    if let Some(sb) = &manifest.sandbox {
        sections.push(("sandbox", serialize_sandbox_lines(sb)));
    }

    // Profiles
    if !manifest.profiles.is_empty() {
        sections.push(("profiles", serialize_profiles_lines(&manifest.profiles)));
    }

    format_top_level_map(&sections)
}

/// Serialize a lockfile to EDN format.
pub fn serialize_lockfile(lockfile: &Lockfile) -> String {
    let mut lines = Vec::new();
    for (name, dep) in &lockfile.dependencies {
        let mut inner = format!("{{:version {:?} :kind :{}", dep.version, dep.kind.as_str());
        if let Some(reg) = &dep.registry {
            inner.push_str(&format!(" :registry {:?}", reg));
        }
        inner.push('}');
        lines.push(format!("{:?} {}", name, inner));
    }

    let sections: Vec<(&str, Vec<String>)> = vec![("dependencies", lines)];
    format_top_level_map(&sections)
}

/// Parse a lockfile from EDN format.
pub fn parse_lockfile(source: &str) -> Result<Lockfile, ManifestError> {
    let nodes = nexl_reader::read(source, meta::FileId::SYNTHETIC)
        .map_err(|e| ManifestError::Parse(e.to_string()))?;

    if nodes.len() != 1 {
        return Err(ManifestError::Parse(
            "expected a single top-level map".to_string(),
        ));
    }

    let pairs = expect_map(&nodes[0], "lockfile")?;
    let mut dependencies = BTreeMap::new();

    for (key, value) in pairs {
        if let Some("dependencies") = keyword_name(key) {
            let dep_pairs = expect_map(value, "lockfile.dependencies")?;
            for (dk, dv) in dep_pairs {
                let name = expect_string(dk, "locked dependency name")?;
                let inner = expect_map(dv, &format!("locked dependency `{name}`"))?;
                let mut version = None;
                let mut kind = None;
                let mut registry = None;

                for (ik, iv) in inner {
                    match keyword_name(ik) {
                        Some("version") => version = Some(expect_string(iv, "locked.version")?),
                        Some("kind") => {
                            let k = keyword_name(iv).ok_or_else(|| ManifestError::TypeError {
                                field: "locked.kind".to_string(),
                                expected: "keyword".to_string(),
                            })?;
                            kind = Some(DependencyKind::from_str(k)?);
                        }
                        Some("registry") => registry = Some(expect_string(iv, "locked.registry")?),
                        _ => {}
                    }
                }

                dependencies.insert(
                    name,
                    LockedDependency {
                        version: version.ok_or_else(|| {
                            ManifestError::MissingField("locked.version".to_string())
                        })?,
                        kind: kind.ok_or_else(|| {
                            ManifestError::MissingField("locked.kind".to_string())
                        })?,
                        registry,
                    },
                );
            }
        }
    }

    Ok(Lockfile { dependencies })
}

fn format_top_level_map(sections: &[(&str, Vec<String>)]) -> String {
    let mut buf = String::from("{");

    for (i, (key, value_lines)) in sections.iter().enumerate() {
        if i > 0 {
            buf.push_str("\n\n ");
        }
        let kw = format!(":{key}");
        // Padding = position of content inside the value map's `{}`
        // For first section: `{:key {` → pad = 1 + kw.len() + 2
        // For subsequent:    ` :key {` → same formula (` ` from "\n\n ")
        let pad_width = 1 + kw.len() + 2;
        let pad = " ".repeat(pad_width);

        buf.push_str(&kw);
        buf.push_str(" {");
        for (j, line) in value_lines.iter().enumerate() {
            if j > 0 {
                buf.push('\n');
                buf.push_str(&pad);
            }
            buf.push_str(line);
        }
        buf.push('}');
    }

    buf.push_str("}\n");
    buf
}

fn serialize_package_lines(pkg: &PackageSection) -> Vec<String> {
    let mut lines = Vec::new();
    lines.push(format!(":name {:?}", pkg.name));
    lines.push(format!(":version {:?}", pkg.version));
    if let Some(desc) = &pkg.description {
        lines.push(format!(":description {:?}", desc));
    }
    lines.push(format!(":prefix {:?}", pkg.prefix));
    lines
}

fn serialize_deps_lines(deps: &BTreeMap<String, DependencySpec>) -> Vec<String> {
    let mut lines = Vec::new();
    for (name, spec) in deps {
        match spec {
            DependencySpec::Version(v) => {
                lines.push(format!("{:?} {:?}", name, v));
            }
            DependencySpec::Detailed(d) => {
                let mut inner = format!("{{:version {:?}", d.version);
                if let Some(reg) = &d.registry {
                    inner.push_str(&format!(" :registry {:?}", reg));
                }
                inner.push('}');
                lines.push(format!("{:?} {}", name, inner));
            }
        }
    }
    lines
}

fn serialize_registries_lines(regs: &BTreeMap<String, RegistrySpec>) -> Vec<String> {
    let mut lines = Vec::new();
    for (name, spec) in regs {
        let mut inner = format!("{{:url {:?}", spec.url);
        if let Some(tok) = &spec.token_env {
            inner.push_str(&format!(" :token-env {:?}", tok));
        }
        inner.push('}');
        lines.push(format!("{:?} {}", name, inner));
    }
    lines
}

fn serialize_sandbox_lines(sb: &SandboxConfig) -> Vec<String> {
    let mut lines = Vec::new();
    if !sb.allow.is_empty() {
        let caps: Vec<String> = sb
            .allow
            .iter()
            .map(|c| format!(":{}", c.as_str()))
            .collect();
        lines.push(format!(":allow #{{{}}}", caps.join(" ")));
    }
    if !sb.net.is_empty() {
        let nets: Vec<String> = sb.net.iter().map(|s| format!("{:?}", s)).collect();
        lines.push(format!(":net #{{{}}}", nets.join(" ")));
    }
    if !sb.fs.is_empty() {
        let paths: Vec<String> = sb.fs.iter().map(|s| format!("{:?}", s)).collect();
        lines.push(format!(":fs #{{{}}}", paths.join(" ")));
    }
    lines
}

fn serialize_profiles_lines(profiles: &BTreeMap<String, ProfileConfig>) -> Vec<String> {
    let mut lines = Vec::new();
    for (name, config) in profiles {
        match &config.sandbox {
            Some(sb) => {
                let sb_inner = serialize_sandbox_inline(sb);
                lines.push(format!(":{name} {{:sandbox {sb_inner}}}"));
            }
            None => {
                lines.push(format!(":{name} {{}}"));
            }
        }
    }
    lines
}

fn serialize_sandbox_inline(sb: &SandboxConfig) -> String {
    let mut parts = Vec::new();
    if !sb.allow.is_empty() {
        let caps: Vec<String> = sb
            .allow
            .iter()
            .map(|c| format!(":{}", c.as_str()))
            .collect();
        parts.push(format!(":allow #{{{}}}", caps.join(" ")));
    }
    if !sb.net.is_empty() {
        let nets: Vec<String> = sb.net.iter().map(|s| format!("{:?}", s)).collect();
        parts.push(format!(":net #{{{}}}", nets.join(" ")));
    }
    if !sb.fs.is_empty() {
        let paths: Vec<String> = sb.fs.iter().map(|s| format!("{:?}", s)).collect();
        parts.push(format!(":fs #{{{}}}", paths.join(" ")));
    }
    format!("{{{}}}", parts.join(" "))
}

// ---------------------------------------------------------------------------
// Resolution
// ---------------------------------------------------------------------------

/// Errors produced during dependency resolution.
#[derive(Debug, Error)]
pub enum ResolveError {
    /// Two entries for the same dependency disagree.
    #[error("dependency `{name}` has conflicting requirements: {a} vs {b}")]
    Conflict {
        /// Dependency name.
        name: String,
        /// First specification.
        a: String,
        /// Second specification.
        b: String,
    },
    /// A dependency references a registry that is not declared.
    #[error("dependency `{name}` references unknown registry `{registry}`")]
    UnknownRegistry {
        /// Dependency name.
        name: String,
        /// Registry alias.
        registry: String,
    },
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

// ---------------------------------------------------------------------------
// Content Hashing
// ---------------------------------------------------------------------------

/// Compute a SHA-256 content hash for a top-level definition.
///
/// The hash input is: `canonical_source || "\0" || type_signature || "\0" || effect_row`.
/// This ensures the hash captures both the surface form and the inferred types,
/// so that any change to the definition or its inferred type triggers recompilation.
pub fn hash_definition(canonical_source: &str, type_sig: &str, effect_row: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(canonical_source.as_bytes());
    hasher.update(b"\0");
    hasher.update(type_sig.as_bytes());
    hasher.update(b"\0");
    hasher.update(effect_row.as_bytes());
    format!("{:x}", hasher.finalize())
}

// ---------------------------------------------------------------------------
// Dependency Collection
// ---------------------------------------------------------------------------

/// Collect all free symbol names referenced by an AST node.
///
/// Walks the node tree and returns every `Symbol` name that is not locally
/// bound by a `let`, `fn`, `defn`, or `loop` form. These are the external
/// dependencies of the definition.
///
/// `local_names` is the initial set of locally-bound names (e.g. the function's
/// own parameters for a `defn` form).
pub fn collect_deps(node: &Node, local_names: &HashSet<String>) -> HashSet<String> {
    let mut deps = HashSet::new();
    let mut locals = local_names.clone();
    collect_deps_node(node, &mut locals, &mut deps);
    deps
}

fn collect_deps_node(node: &Node, locals: &mut HashSet<String>, deps: &mut HashSet<String>) {
    match &node.kind {
        NodeKind::Atom(Atom::Symbol { ns: None, name }) => {
            if !locals.contains(name) && !is_special_form(name) {
                deps.insert(name.clone());
            }
        }
        NodeKind::Atom(Atom::Symbol { ns: Some(ns), name }) => {
            // Qualified symbol: the module prefix is a dependency.
            deps.insert(format!("{ns}/{name}"));
        }
        NodeKind::List(items) if !items.is_empty() => {
            collect_deps_list(items, locals, deps);
        }
        NodeKind::Vector(items) => {
            for item in items {
                collect_deps_node(item, locals, deps);
            }
        }
        NodeKind::Map(pairs) => {
            for (k, v) in pairs {
                collect_deps_node(k, locals, deps);
                collect_deps_node(v, locals, deps);
            }
        }
        NodeKind::Set(items) => {
            for item in items {
                collect_deps_node(item, locals, deps);
            }
        }
        NodeKind::Quote(_) | NodeKind::Discard(_) => {
            // Quoted forms and discards don't reference runtime values.
        }
        NodeKind::Deref(inner) => collect_deps_node(inner, locals, deps),
        NodeKind::Quasiquote(inner) => collect_deps_node(inner, locals, deps),
        NodeKind::Unquote(inner) => collect_deps_node(inner, locals, deps),
        NodeKind::UnquoteSplice(inner) => collect_deps_node(inner, locals, deps),
        _ => {}
    }
}

fn collect_deps_list(items: &[Node], locals: &mut HashSet<String>, deps: &mut HashSet<String>) {
    if let NodeKind::Atom(Atom::Symbol { ns: None, name }) = &items[0].kind {
        match name.as_str() {
            "fn" => {
                // (fn [params...] body...)
                let mut fn_locals = locals.clone();
                if items.len() >= 2
                    && let NodeKind::Vector(params) = &items[1].kind
                {
                    for p in params {
                        if let NodeKind::Atom(Atom::Symbol { ns: None, name }) = &p.kind {
                            fn_locals.insert(name.clone());
                        }
                    }
                }
                for item in &items[2..] {
                    collect_deps_node(item, &mut fn_locals, deps);
                }
                return;
            }
            "let" => {
                // (let [name val name val ...] body...)
                let mut let_locals = locals.clone();
                if items.len() >= 2
                    && let NodeKind::Vector(bindings) = &items[1].kind
                {
                    for pair in bindings.chunks(2) {
                        if pair.len() == 2 {
                            // Eval the value in current scope.
                            collect_deps_node(&pair[1], &mut let_locals, deps);
                            // Add the name to scope.
                            if let NodeKind::Atom(Atom::Symbol { ns: None, name }) = &pair[0].kind {
                                let_locals.insert(name.clone());
                            }
                        }
                    }
                }
                for item in &items[2..] {
                    collect_deps_node(item, &mut let_locals, deps);
                }
                return;
            }
            "defn" => {
                // (defn name [params...] body...)
                let mut defn_locals = locals.clone();
                if items.len() >= 2
                    && let NodeKind::Atom(Atom::Symbol { ns: None, name }) = &items[1].kind
                {
                    defn_locals.insert(name.clone());
                }
                if items.len() >= 3
                    && let NodeKind::Vector(params) = &items[2].kind
                {
                    for p in params {
                        if let NodeKind::Atom(Atom::Symbol { ns: None, name }) = &p.kind {
                            defn_locals.insert(name.clone());
                        }
                    }
                }
                for item in &items[3..] {
                    collect_deps_node(item, &mut defn_locals, deps);
                }
                return;
            }
            "loop" => {
                // (loop [var init var init ...] body...)
                let mut loop_locals = locals.clone();
                if items.len() >= 2
                    && let NodeKind::Vector(bindings) = &items[1].kind
                {
                    for pair in bindings.chunks(2) {
                        if pair.len() == 2 {
                            collect_deps_node(&pair[1], &mut loop_locals, deps);
                            if let NodeKind::Atom(Atom::Symbol { ns: None, name }) = &pair[0].kind {
                                loop_locals.insert(name.clone());
                            }
                        }
                    }
                }
                for item in &items[2..] {
                    collect_deps_node(item, &mut loop_locals, deps);
                }
                return;
            }
            "match" => {
                // (match expr pat body pat body ...)
                if items.len() >= 2 {
                    collect_deps_node(&items[1], locals, deps);
                }
                for pair in items[2..].chunks(2) {
                    if pair.len() == 2 {
                        // Pattern introduces bindings for its body.
                        let mut arm_locals = locals.clone();
                        collect_pattern_names(&pair[0], &mut arm_locals);
                        collect_deps_node(&pair[1], &mut arm_locals, deps);
                    }
                }
                return;
            }
            _ => {}
        }
    }
    // Default: traverse all items.
    for item in items {
        collect_deps_node(item, locals, deps);
    }
}

/// Extract names bound by a match pattern.
fn collect_pattern_names(node: &Node, locals: &mut HashSet<String>) {
    match &node.kind {
        NodeKind::Atom(Atom::Symbol { ns: None, name }) => {
            if name != "_" && !name.chars().next().is_some_and(|c| c.is_uppercase()) {
                locals.insert(name.clone());
            }
        }
        NodeKind::List(items) => {
            // Constructor pattern: (Ctor args...) — skip the ctor name, bind args.
            for item in items.iter().skip(1) {
                collect_pattern_names(item, locals);
            }
        }
        _ => {}
    }
}

fn is_special_form(name: &str) -> bool {
    matches!(
        name,
        "fn" | "let"
            | "if"
            | "do"
            | "defn"
            | "def"
            | "deftype"
            | "defeffect"
            | "defmacro"
            | "defn-macro"
            | "defmacro-syntax"
            | "loop"
            | "recur"
            | "match"
            | "try"
            | "catch"
            | "quote"
            | "quasiquote"
            | "unquote"
            | "unquote-splice"
            | "module"
            | "import"
            | "export"
            | "handle"
            | "perform"
            | "resume"
            | "assert!"
            | "assert-unreachable!"
            | "panic!"
    )
}

// ---------------------------------------------------------------------------
// Definition Store
// ---------------------------------------------------------------------------

/// A stored definition entry with all metadata needed for incremental compilation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefinitionEntry {
    /// The content hash of this definition.
    pub hash: String,
    /// The definition name (e.g. function name, type name).
    pub def_name: String,
    /// The inferred type signature as a string.
    pub type_sig: String,
    /// The inferred effect row as a string.
    pub effect_row: String,
    /// Content hashes of definitions this one depends on.
    pub dep_hashes: Vec<String>,
    /// The compiled artifact bytes (WASM, native, etc.).
    pub artifact: Vec<u8>,
}

/// Result of planning an incremental build.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncrementalPlan {
    /// Definitions that need to be (re)compiled.
    pub to_compile: Vec<String>,
    /// Definitions whose cached artifacts can be reused.
    pub cached: Vec<String>,
}

/// SQLite-backed content-addressed definition store.
///
/// Maps definition content hashes to compiled artifacts plus type/effect/dependency
/// metadata. Used for incremental compilation: if a definition's hash hasn't changed
/// and none of its dependencies have changed, the cached artifact is reused.
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

    /// JSON serialization error.
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
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

    /// Store an artifact by its content hash (simple API, backward-compatible).
    pub fn put(&self, hash: &str, artifact: &[u8]) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT OR REPLACE INTO definitions (hash, def_name, type_sig, effect_row, dep_hashes, artifact) VALUES (?1, '', '', '', '[]', ?2)",
            params![hash, artifact],
        )?;
        Ok(())
    }

    /// Fetch an artifact by its content hash (simple API, backward-compatible).
    pub fn get(&self, hash: &str) -> Result<Option<Vec<u8>>, StoreError> {
        let mut stmt = self
            .conn
            .prepare("SELECT artifact FROM definitions WHERE hash = ?1")?;
        let mut rows = stmt.query(params![hash])?;
        match rows.next()? {
            Some(row) => {
                let data: Vec<u8> = row.get(0)?;
                Ok(Some(data))
            }
            None => Ok(None),
        }
    }

    /// Store a full definition entry with type/effect/dependency metadata.
    pub fn put_definition(&self, entry: &DefinitionEntry) -> Result<(), StoreError> {
        let dep_hashes_json = serde_json::to_string(&entry.dep_hashes)?;
        self.conn.execute(
            "INSERT OR REPLACE INTO definitions (hash, def_name, type_sig, effect_row, dep_hashes, artifact) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                entry.hash,
                entry.def_name,
                entry.type_sig,
                entry.effect_row,
                dep_hashes_json,
                entry.artifact,
            ],
        )?;
        Ok(())
    }

    /// Fetch a full definition entry by its content hash.
    pub fn get_definition(&self, hash: &str) -> Result<Option<DefinitionEntry>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT hash, def_name, type_sig, effect_row, dep_hashes, artifact FROM definitions WHERE hash = ?1",
        )?;
        let mut rows = stmt.query(params![hash])?;
        match rows.next()? {
            Some(row) => {
                let dep_hashes_json: String = row.get(4)?;
                let dep_hashes: Vec<String> = serde_json::from_str(&dep_hashes_json)?;
                Ok(Some(DefinitionEntry {
                    hash: row.get(0)?,
                    def_name: row.get(1)?,
                    type_sig: row.get(2)?,
                    effect_row: row.get(3)?,
                    dep_hashes,
                    artifact: row.get(5)?,
                }))
            }
            None => Ok(None),
        }
    }

    /// Look up a definition by name (returns the most recent entry).
    pub fn get_by_name(&self, def_name: &str) -> Result<Option<DefinitionEntry>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT hash, def_name, type_sig, effect_row, dep_hashes, artifact FROM definitions WHERE def_name = ?1 LIMIT 1",
        )?;
        let mut rows = stmt.query(params![def_name])?;
        match rows.next()? {
            Some(row) => {
                let dep_hashes_json: String = row.get(4)?;
                let dep_hashes: Vec<String> = serde_json::from_str(&dep_hashes_json)?;
                Ok(Some(DefinitionEntry {
                    hash: row.get(0)?,
                    def_name: row.get(1)?,
                    type_sig: row.get(2)?,
                    effect_row: row.get(3)?,
                    dep_hashes,
                    artifact: row.get(5)?,
                }))
            }
            None => Ok(None),
        }
    }

    /// Check if a definition with the given hash exists in the store.
    pub fn contains(&self, hash: &str) -> Result<bool, StoreError> {
        let mut stmt = self
            .conn
            .prepare("SELECT 1 FROM definitions WHERE hash = ?1")?;
        let mut rows = stmt.query(params![hash])?;
        Ok(rows.next()?.is_some())
    }

    /// Check if a cached definition is still valid by verifying its dependency hashes
    /// all still exist and match.
    pub fn is_valid(&self, hash: &str) -> Result<bool, StoreError> {
        let entry = self.get_definition(hash)?;
        match entry {
            None => Ok(false),
            Some(entry) => {
                for dep_hash in &entry.dep_hashes {
                    if !self.contains(dep_hash)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
        }
    }

    /// Record that a definition was expanded using a macro.
    ///
    /// When the macro's body changes, all definitions expanded from it
    /// must be invalidated and re-expanded.
    pub fn record_macro_expansion(
        &self,
        def_hash: &str,
        macro_name: &str,
    ) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT OR IGNORE INTO macro_expansions (def_hash, macro_name) VALUES (?1, ?2)",
            params![def_hash, macro_name],
        )?;
        Ok(())
    }

    /// Find all definition hashes that were expanded using the given macro.
    pub fn definitions_using_macro(&self, macro_name: &str) -> Result<Vec<String>, StoreError> {
        let mut stmt = self
            .conn
            .prepare("SELECT def_hash FROM macro_expansions WHERE macro_name = ?1")?;
        let rows = stmt.query_map(params![macro_name], |row| row.get(0))?;
        let mut result = Vec::new();
        for row in rows {
            result.push(row?);
        }
        Ok(result)
    }

    /// Invalidate all definitions that were expanded using the given macro.
    ///
    /// Removes both the expansion records and the definitions themselves from
    /// the store.
    pub fn invalidate_macro(&self, macro_name: &str) -> Result<usize, StoreError> {
        let hashes = self.definitions_using_macro(macro_name)?;
        let count = hashes.len();
        for hash in &hashes {
            self.conn
                .execute("DELETE FROM definitions WHERE hash = ?1", params![hash])?;
        }
        self.conn.execute(
            "DELETE FROM macro_expansions WHERE macro_name = ?1",
            params![macro_name],
        )?;
        Ok(count)
    }

    /// Check if a definition needs to be recompiled.
    ///
    /// Returns `true` if the definition is not in the store or if any of its
    /// dependency hashes are missing (indicating a transitive change).
    pub fn needs_recompile(&self, hash: &str) -> Result<bool, StoreError> {
        Ok(!self.is_valid(hash)?)
    }

    /// Plan an incremental build: given a list of `(hash, def_name)` pairs,
    /// return the names of definitions that need recompilation and those
    /// that can be skipped (cached).
    pub fn plan_incremental(
        &self,
        definitions: &[(String, String)],
    ) -> Result<IncrementalPlan, StoreError> {
        let mut to_compile = Vec::new();
        let mut cached = Vec::new();
        for (hash, def_name) in definitions {
            if self.needs_recompile(hash)? {
                to_compile.push(def_name.clone());
            } else {
                cached.push(def_name.clone());
            }
        }
        Ok(IncrementalPlan { to_compile, cached })
    }

    fn init(&self) -> Result<(), StoreError> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS definitions (
                hash TEXT PRIMARY KEY,
                def_name TEXT NOT NULL,
                type_sig TEXT NOT NULL,
                effect_row TEXT NOT NULL,
                dep_hashes TEXT NOT NULL DEFAULT '[]',
                artifact BLOB NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_def_name ON definitions (def_name);
            CREATE TABLE IF NOT EXISTS macro_expansions (
                def_hash TEXT NOT NULL,
                macro_name TEXT NOT NULL,
                PRIMARY KEY (def_hash, macro_name)
            );
            CREATE INDEX IF NOT EXISTS idx_macro_name ON macro_expansions (macro_name);",
        )?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

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
    fn manifest_parses_basic_example() {
        let input = r#"
{:package {:name "my-app"
           :version "1.0.0"
           :description "My application"
           :prefix "my-app"}

 :dependencies {"http-server" "^2.1.0"
                "json" "~1.0.0"}

 :dev-dependencies {"test-utils" "^1.0.0"
                    "bench-tools" "^0.5.0"}}
"#;

        let manifest = parse_manifest(input).expect("manifest parse");
        assert_eq!(manifest.package.name, "my-app");
        assert_eq!(manifest.package.version, "1.0.0");
        assert_eq!(
            manifest.package.description.as_deref(),
            Some("My application")
        );
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
    fn manifest_parses_registry_dependency() {
        let input = r#"
{:package {:name "demo"
           :version "0.1.0"
           :prefix "demo"}

 :registries {"internal" {:url "https://registry.corp.example.com"
                          :token-env "NEXL_CORP_TOKEN"}}

 :dependencies {"internal-lib" {:version "^1.0.0" :registry "internal"}}}
"#;

        let manifest = parse_manifest(input).expect("manifest parse");
        let registry = manifest.registries.get("internal").expect("registry entry");
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
{:package {:name "solo"
           :version "0.1.0"
           :prefix "solo"}}
"#;

        let manifest = parse_manifest(input).expect("manifest parse");
        assert!(manifest.dependencies.is_empty());
        assert!(manifest.dev_dependencies.is_empty());
        assert!(manifest.registries.is_empty());
        assert!(manifest.sandbox.is_none());
        assert!(manifest.profiles.is_empty());
    }

    #[test]
    fn parse_manifest_missing_package_is_error() {
        let input = r#"
{:dependencies {"json" "~1.0.0"}}
"#;

        let err = parse_manifest(input).expect_err("missing package should error");
        let message = err.to_string();
        assert!(message.contains("package"), "unexpected error: {message}");
    }

    #[test]
    fn manifest_parses_sandbox_config() {
        let input = r#"
{:package {:name "app"
           :version "0.1.0"
           :prefix "app"}

 :sandbox {:allow #{:net :console :time}
           :net #{"api.example.com"}
           :fs #{"/tmp" "/data"}}}
"#;

        let manifest = parse_manifest(input).expect("manifest parse");
        let sb = manifest.sandbox.as_ref().expect("sandbox");
        assert!(sb.allow.contains(&Capability::Net));
        assert!(sb.allow.contains(&Capability::Console));
        assert!(sb.allow.contains(&Capability::Time));
        assert_eq!(sb.allow.len(), 3);
        assert!(sb.net.contains("api.example.com"));
        assert!(sb.fs.contains("/tmp"));
        assert!(sb.fs.contains("/data"));
    }

    #[test]
    fn manifest_parses_profiles() {
        let input = r#"
{:package {:name "app"
           :version "0.1.0"
           :prefix "app"}

 :profiles {:dev  {:sandbox {:allow #{:net :fs :console}}}
            :test {:sandbox {:allow #{:console :time}}}}}
"#;

        let manifest = parse_manifest(input).expect("manifest parse");
        let dev = manifest.profiles.get("dev").expect("dev profile");
        let dev_sb = dev.sandbox.as_ref().expect("dev sandbox");
        assert!(dev_sb.allow.contains(&Capability::Net));
        assert!(dev_sb.allow.contains(&Capability::Fs));
        assert!(dev_sb.allow.contains(&Capability::Console));

        let test = manifest.profiles.get("test").expect("test profile");
        let test_sb = test.sandbox.as_ref().expect("test sandbox");
        assert!(test_sb.allow.contains(&Capability::Console));
        assert!(test_sb.allow.contains(&Capability::Time));
        assert_eq!(test_sb.allow.len(), 2);
    }

    #[test]
    fn manifest_roundtrips_through_serializer() {
        let input = r#"
{:package {:name "my-app"
           :version "1.0.0"
           :description "My application"
           :prefix "my-app"}

 :dependencies {"http-server" "^2.1.0"
                "json" "~1.0.0"
                "internal-lib" {:version "^1.0.0" :registry "internal"}}

 :dev-dependencies {"test-utils" "^1.0.0"}

 :registries {"internal" {:url "https://registry.corp.example.com"
                          :token-env "NEXL_CORP_TOKEN"}}

 :sandbox {:allow #{:net :console :time}
           :net #{"api.example.com"}
           :fs #{"/tmp" "/data"}}

 :profiles {:dev  {:sandbox {:allow #{:console :fs :net}}}
            :test {:sandbox {:allow #{:console :time}}}}}
"#;

        let manifest = parse_manifest(input).expect("parse");
        let serialized = serialize_manifest(&manifest);
        let reparsed = parse_manifest(&serialized).expect("reparse");
        assert_eq!(manifest, reparsed);
    }

    #[test]
    fn lockfile_roundtrips_through_serializer() {
        let lockfile = Lockfile {
            dependencies: BTreeMap::from([
                (
                    "core".to_string(),
                    LockedDependency {
                        version: "^1.0.0".to_string(),
                        registry: None,
                        kind: DependencyKind::Runtime,
                    },
                ),
                (
                    "test-utils".to_string(),
                    LockedDependency {
                        version: "^0.1.0".to_string(),
                        registry: Some("internal".to_string()),
                        kind: DependencyKind::Dev,
                    },
                ),
            ]),
        };

        let serialized = serialize_lockfile(&lockfile);
        let reparsed = parse_lockfile(&serialized).expect("reparse lockfile");
        assert_eq!(lockfile, reparsed);
    }

    fn base_manifest() -> PackageManifest {
        PackageManifest {
            package: PackageSection {
                name: "demo".to_string(),
                version: "0.1.0".to_string(),
                description: None,
                prefix: "demo".to_string(),
                source_dir: ".".to_string(),
            },
            dependencies: BTreeMap::new(),
            dev_dependencies: BTreeMap::new(),
            registries: BTreeMap::new(),
            sandbox: None,
            profiles: BTreeMap::new(),
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
            dep.name == "test-utils" && dep.kind == DependencyKind::Dev && dep.version == "^0.1.0"
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
        let core = lockfile.dependencies.get("core").expect("core entry");
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
        store.put("hash-1", b"artifact").expect("store write");
        let fetched = store.get("hash-1").expect("store read");
        assert_eq!(fetched, Some(b"artifact".to_vec()));
    }

    #[test]
    fn definition_store_missing_returns_none() {
        let store = DefinitionStore::open_in_memory().expect("store open");
        let fetched = store.get("missing").expect("store read");
        assert_eq!(fetched, None);
    }

    // ─── Content hashing ────────────────────────────────────────────────────

    #[test]
    fn hash_definition_deterministic() {
        let h1 = hash_definition("(defn f [x] x)", "Int -> Int", "pure");
        let h2 = hash_definition("(defn f [x] x)", "Int -> Int", "pure");
        assert_eq!(h1, h2);
    }

    #[test]
    fn hash_definition_changes_on_source_change() {
        let h1 = hash_definition("(defn f [x] x)", "Int -> Int", "pure");
        let h2 = hash_definition("(defn f [x] (+ x 1))", "Int -> Int", "pure");
        assert_ne!(h1, h2);
    }

    #[test]
    fn hash_definition_changes_on_type_change() {
        let h1 = hash_definition("(defn f [x] x)", "Int -> Int", "pure");
        let h2 = hash_definition("(defn f [x] x)", "Float -> Float", "pure");
        assert_ne!(h1, h2);
    }

    #[test]
    fn hash_definition_changes_on_effect_change() {
        let h1 = hash_definition("(defn f [x] x)", "Int -> Int", "pure");
        let h2 = hash_definition("(defn f [x] x)", "Int -> Int", "IO");
        assert_ne!(h1, h2);
    }

    #[test]
    fn hash_definition_is_hex_sha256() {
        let h = hash_definition("test", "Int", "pure");
        assert_eq!(h.len(), 64); // SHA-256 hex = 64 chars
        assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
    }

    // ─── Extended definition store ──────────────────────────────────────────

    #[test]
    fn put_and_get_definition_entry() {
        let store = DefinitionStore::open_in_memory().expect("store");
        let entry = DefinitionEntry {
            hash: "abc123".to_string(),
            def_name: "my-fn".to_string(),
            type_sig: "Int -> Int".to_string(),
            effect_row: "pure".to_string(),
            dep_hashes: vec!["dep1".to_string(), "dep2".to_string()],
            artifact: b"wasm bytes".to_vec(),
        };
        store.put_definition(&entry).expect("put");
        let fetched = store.get_definition("abc123").expect("get").expect("some");
        assert_eq!(fetched.def_name, "my-fn");
        assert_eq!(fetched.type_sig, "Int -> Int");
        assert_eq!(fetched.effect_row, "pure");
        assert_eq!(fetched.dep_hashes, vec!["dep1", "dep2"]);
        assert_eq!(fetched.artifact, b"wasm bytes");
    }

    #[test]
    fn get_by_name_finds_entry() {
        let store = DefinitionStore::open_in_memory().expect("store");
        let entry = DefinitionEntry {
            hash: "h1".to_string(),
            def_name: "add".to_string(),
            type_sig: "Int -> Int -> Int".to_string(),
            effect_row: "pure".to_string(),
            dep_hashes: vec![],
            artifact: b"code".to_vec(),
        };
        store.put_definition(&entry).expect("put");
        let found = store.get_by_name("add").expect("get").expect("some");
        assert_eq!(found.hash, "h1");
    }

    #[test]
    fn contains_returns_true_for_existing() {
        let store = DefinitionStore::open_in_memory().expect("store");
        store.put("exists", b"data").expect("put");
        assert!(store.contains("exists").expect("check"));
        assert!(!store.contains("missing").expect("check"));
    }

    #[test]
    fn is_valid_with_no_deps() {
        let store = DefinitionStore::open_in_memory().expect("store");
        let entry = DefinitionEntry {
            hash: "h1".to_string(),
            def_name: "f".to_string(),
            type_sig: "Int".to_string(),
            effect_row: "pure".to_string(),
            dep_hashes: vec![],
            artifact: b"code".to_vec(),
        };
        store.put_definition(&entry).expect("put");
        assert!(store.is_valid("h1").expect("check"));
    }

    #[test]
    fn is_valid_fails_when_dep_missing() {
        let store = DefinitionStore::open_in_memory().expect("store");
        let entry = DefinitionEntry {
            hash: "h1".to_string(),
            def_name: "f".to_string(),
            type_sig: "Int".to_string(),
            effect_row: "pure".to_string(),
            dep_hashes: vec!["missing-dep".to_string()],
            artifact: b"code".to_vec(),
        };
        store.put_definition(&entry).expect("put");
        assert!(!store.is_valid("h1").expect("check"));
    }

    // ─── Dependency collection ────────────────────────────────────────────

    fn parse_one(src: &str) -> Node {
        let nodes = nexl_reader::read(src, meta::FileId::SYNTHETIC).expect("parse");
        nodes.into_iter().next().expect("at least one node")
    }

    #[test]
    fn deps_finds_free_symbols() {
        // (+ x y) — +, x, y are all free
        let node = parse_one("(+ x y)");
        let deps = collect_deps(&node, &HashSet::new());
        assert!(deps.contains("+"));
        assert!(deps.contains("x"));
        assert!(deps.contains("y"));
    }

    #[test]
    fn deps_excludes_locals_in_fn() {
        // (fn [x] (+ x 1)) — x is local, + is free
        let node = parse_one("(fn [x] (+ x 1))");
        let deps = collect_deps(&node, &HashSet::new());
        assert!(deps.contains("+"));
        assert!(!deps.contains("x"));
    }

    #[test]
    fn deps_excludes_let_bindings() {
        // (let [x 1] (+ x y)) — x is local, y and + are free
        let node = parse_one("(let [x 1] (+ x y))");
        let deps = collect_deps(&node, &HashSet::new());
        assert!(deps.contains("+"));
        assert!(deps.contains("y"));
        assert!(!deps.contains("x"));
    }

    #[test]
    fn deps_excludes_initial_locals() {
        // (+ a b) with a as initial local
        let node = parse_one("(+ a b)");
        let mut initial = HashSet::new();
        initial.insert("a".to_string());
        let deps = collect_deps(&node, &initial);
        assert!(deps.contains("+"));
        assert!(deps.contains("b"));
        assert!(!deps.contains("a"));
    }

    #[test]
    fn deps_handles_defn_params() {
        // (defn f [x y] (+ x y)) — f, x, y are local; + is free
        let node = parse_one("(defn f [x y] (+ x y))");
        let deps = collect_deps(&node, &HashSet::new());
        assert!(deps.contains("+"));
        assert!(!deps.contains("f"));
        assert!(!deps.contains("x"));
        assert!(!deps.contains("y"));
    }

    #[test]
    fn deps_skips_special_forms() {
        // (if cond a b) — if is special, cond/a/b are free
        let node = parse_one("(if cond a b)");
        let deps = collect_deps(&node, &HashSet::new());
        assert!(!deps.contains("if"));
        assert!(deps.contains("cond"));
        assert!(deps.contains("a"));
        assert!(deps.contains("b"));
    }

    #[test]
    fn deps_handles_qualified_symbols() {
        let node = parse_one("(math/sqrt x)");
        let deps = collect_deps(&node, &HashSet::new());
        assert!(deps.contains("math/sqrt"));
        assert!(deps.contains("x"));
    }

    // ─── Macro invalidation ────────────────────────────────────────────────

    #[test]
    fn record_and_find_macro_expansions() {
        let store = DefinitionStore::open_in_memory().expect("store");
        let entry = DefinitionEntry {
            hash: "h1".to_string(),
            def_name: "my-fn".to_string(),
            type_sig: "Int".to_string(),
            effect_row: "pure".to_string(),
            dep_hashes: vec![],
            artifact: b"code".to_vec(),
        };
        store.put_definition(&entry).expect("put");
        store
            .record_macro_expansion("h1", "my-macro")
            .expect("record");
        let defs = store.definitions_using_macro("my-macro").expect("query");
        assert_eq!(defs, vec!["h1"]);
    }

    #[test]
    fn invalidate_macro_removes_definitions() {
        let store = DefinitionStore::open_in_memory().expect("store");
        let entry = DefinitionEntry {
            hash: "h1".to_string(),
            def_name: "expanded-fn".to_string(),
            type_sig: "Int".to_string(),
            effect_row: "pure".to_string(),
            dep_hashes: vec![],
            artifact: b"code".to_vec(),
        };
        store.put_definition(&entry).expect("put");
        store
            .record_macro_expansion("h1", "my-macro")
            .expect("record");

        let count = store.invalidate_macro("my-macro").expect("invalidate");
        assert_eq!(count, 1);

        // Definition should be gone.
        assert!(!store.contains("h1").expect("check"));
        // No more expansion records.
        assert!(
            store
                .definitions_using_macro("my-macro")
                .expect("query")
                .is_empty()
        );
    }

    #[test]
    fn invalidate_macro_with_no_expansions() {
        let store = DefinitionStore::open_in_memory().expect("store");
        let count = store.invalidate_macro("nonexistent").expect("invalidate");
        assert_eq!(count, 0);
    }

    #[test]
    fn is_valid_succeeds_when_deps_present() {
        let store = DefinitionStore::open_in_memory().expect("store");
        let dep = DefinitionEntry {
            hash: "dep-hash".to_string(),
            def_name: "helper".to_string(),
            type_sig: "Int".to_string(),
            effect_row: "pure".to_string(),
            dep_hashes: vec![],
            artifact: b"dep-code".to_vec(),
        };
        store.put_definition(&dep).expect("put dep");

        let entry = DefinitionEntry {
            hash: "h1".to_string(),
            def_name: "f".to_string(),
            type_sig: "Int".to_string(),
            effect_row: "pure".to_string(),
            dep_hashes: vec!["dep-hash".to_string()],
            artifact: b"code".to_vec(),
        };
        store.put_definition(&entry).expect("put");
        assert!(store.is_valid("h1").expect("check"));
    }

    // --- Incremental recompilation tests ---

    #[test]
    fn needs_recompile_missing_hash() {
        let store = DefinitionStore::open_in_memory().expect("open");
        assert!(store.needs_recompile("nonexistent").expect("check"));
    }

    #[test]
    fn needs_recompile_cached_valid() {
        let store = DefinitionStore::open_in_memory().expect("open");
        let entry = DefinitionEntry {
            hash: "h1".to_string(),
            def_name: "f".to_string(),
            type_sig: "Int -> Int".to_string(),
            effect_row: "pure".to_string(),
            dep_hashes: vec![],
            artifact: b"cached-wasm".to_vec(),
        };
        store.put_definition(&entry).expect("put");
        assert!(!store.needs_recompile("h1").expect("check"));
    }

    #[test]
    fn needs_recompile_dep_missing() {
        let store = DefinitionStore::open_in_memory().expect("open");
        let entry = DefinitionEntry {
            hash: "h1".to_string(),
            def_name: "f".to_string(),
            type_sig: "Int -> Int".to_string(),
            effect_row: "pure".to_string(),
            dep_hashes: vec!["missing-dep".to_string()],
            artifact: b"cached-wasm".to_vec(),
        };
        store.put_definition(&entry).expect("put");
        // dep is missing from the store, so recompilation is needed
        assert!(store.needs_recompile("h1").expect("check"));
    }

    #[test]
    fn plan_incremental_skips_unchanged() {
        let store = DefinitionStore::open_in_memory().expect("open");
        // Store two definitions
        for (hash, name) in [("h1", "add"), ("h2", "mul")] {
            store
                .put_definition(&DefinitionEntry {
                    hash: hash.to_string(),
                    def_name: name.to_string(),
                    type_sig: "Int -> Int -> Int".to_string(),
                    effect_row: "pure".to_string(),
                    dep_hashes: vec![],
                    artifact: b"wasm-code".to_vec(),
                })
                .expect("put");
        }

        let defs = vec![
            ("h1".to_string(), "add".to_string()),
            ("h2".to_string(), "mul".to_string()),
        ];
        let plan = store.plan_incremental(&defs).expect("plan");
        assert!(plan.to_compile.is_empty());
        assert_eq!(plan.cached, vec!["add", "mul"]);
    }

    #[test]
    fn cold_build_equals_warm_build_reproducibility() {
        // Verify that hashing is deterministic: same inputs always produce
        // the same hash, so cold build = warm build from the store's perspective.
        let source = "(defn add [x y] (+ x y))";
        let type_sig = "Int -> Int -> Int";
        let effect_row = "pure";

        let hash1 = hash_definition(source, type_sig, effect_row);
        let hash2 = hash_definition(source, type_sig, effect_row);
        assert_eq!(hash1, hash2, "hash must be deterministic");

        // Simulate: cold build stores artifact, warm build finds it cached.
        let store = DefinitionStore::open_in_memory().expect("open");
        let artifact = b"compiled-wasm-bytes".to_vec();
        store
            .put_definition(&DefinitionEntry {
                hash: hash1.clone(),
                def_name: "add".to_string(),
                type_sig: type_sig.to_string(),
                effect_row: effect_row.to_string(),
                dep_hashes: vec![],
                artifact: artifact.clone(),
            })
            .expect("put");

        // Warm build: same source hashes to same key, finds cached artifact.
        let cached = store
            .get_definition(&hash2)
            .expect("get")
            .expect("should exist");
        assert_eq!(
            cached.artifact, artifact,
            "warm build reuses cold build artifact"
        );
        assert!(!store.needs_recompile(&hash2).expect("check"));
    }

    #[test]
    fn plan_incremental_recompiles_changed() {
        let store = DefinitionStore::open_in_memory().expect("open");
        // Store one definition
        store
            .put_definition(&DefinitionEntry {
                hash: "h1".to_string(),
                def_name: "add".to_string(),
                type_sig: "Int -> Int -> Int".to_string(),
                effect_row: "pure".to_string(),
                dep_hashes: vec![],
                artifact: b"wasm-code".to_vec(),
            })
            .expect("put");

        // Now build with a changed hash for "add" and a new def "sub"
        let defs = vec![
            ("h1-changed".to_string(), "add".to_string()),
            ("h3".to_string(), "sub".to_string()),
        ];
        let plan = store.plan_incremental(&defs).expect("plan");
        assert_eq!(plan.to_compile, vec!["add", "sub"]);
        assert!(plan.cached.is_empty());
    }
}
