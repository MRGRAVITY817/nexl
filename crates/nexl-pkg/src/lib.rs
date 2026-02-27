//! `nexl-pkg` — package manifest schema for `project.nexl` (EDN format).

use meta::{Atom, Node, NodeKind};
use rusqlite::{params, Connection};
use std::collections::{BTreeMap, BTreeSet, HashMap};
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
            other => Err(ManifestError::Parse(format!(
                "unknown capability :{other}"
            ))),
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
            ))
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
                )))
            }
            None => {
                return Err(ManifestError::Parse(
                    "top-level keys must be keywords".to_string(),
                ))
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

    for (key, value) in pairs {
        match keyword_name(key) {
            Some("name") => name = Some(expect_string(value, "package.name")?),
            Some("version") => version = Some(expect_string(value, "package.version")?),
            Some("description") => {
                description = Some(expect_string(value, "package.description")?)
            }
            Some("prefix") => prefix = Some(expect_string(value, "package.prefix")?),
            Some(other) => {
                return Err(ManifestError::Parse(format!(
                    "unknown package field :{other}"
                )))
            }
            None => {
                return Err(ManifestError::Parse(
                    "package keys must be keywords".to_string(),
                ))
            }
        }
    }

    Ok(PackageSection {
        name: name.ok_or_else(|| ManifestError::MissingField("package.name".to_string()))?,
        version: version
            .ok_or_else(|| ManifestError::MissingField("package.version".to_string()))?,
        description,
        prefix: prefix
            .ok_or_else(|| ManifestError::MissingField("package.prefix".to_string()))?,
    })
}

fn parse_dependencies(
    node: &Node,
) -> Result<BTreeMap<String, DependencySpec>, ManifestError> {
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
                })
            }
        };
        deps.insert(name, spec);
    }

    Ok(deps)
}

fn parse_dependency_detail(
    pairs: &[(Node, Node)],
) -> Result<DependencyDetail, ManifestError> {
    let mut version = None;
    let mut registry = None;

    for (key, value) in pairs {
        match keyword_name(key) {
            Some("version") => version = Some(expect_string(value, "dependency.version")?),
            Some("registry") => registry = Some(expect_string(value, "dependency.registry")?),
            Some(other) => {
                return Err(ManifestError::Parse(format!(
                    "unknown dependency field :{other}"
                )))
            }
            None => {
                return Err(ManifestError::Parse(
                    "dependency detail keys must be keywords".to_string(),
                ))
            }
        }
    }

    Ok(DependencyDetail {
        version: version
            .ok_or_else(|| ManifestError::MissingField("dependency.version".to_string()))?,
        registry,
    })
}

fn parse_registries(
    node: &Node,
) -> Result<BTreeMap<String, RegistrySpec>, ManifestError> {
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
                    )))
                }
                None => {
                    return Err(ManifestError::Parse(
                        "registry keys must be keywords".to_string(),
                    ))
                }
            }
        }

        regs.insert(
            name,
            RegistrySpec {
                url: url
                    .ok_or_else(|| ManifestError::MissingField("registry.url".to_string()))?,
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
                )))
            }
            None => {
                return Err(ManifestError::Parse(
                    "sandbox keys must be keywords".to_string(),
                ))
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
            })
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

fn parse_string_set(
    node: &Node,
    field: &str,
) -> Result<BTreeSet<String>, ManifestError> {
    let items = match &node.kind {
        NodeKind::Set(items) => items,
        _ => {
            return Err(ManifestError::TypeError {
                field: field.to_string(),
                expected: "set".to_string(),
            })
        }
    };

    let mut set = BTreeSet::new();
    for item in items {
        set.insert(expect_string(item, field)?);
    }
    Ok(set)
}

fn parse_profiles(
    node: &Node,
) -> Result<BTreeMap<String, ProfileConfig>, ManifestError> {
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
                    )))
                }
                None => {
                    return Err(ManifestError::Parse(
                        "profile keys must be keywords".to_string(),
                    ))
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
        sections.push(("registries", serialize_registries_lines(&manifest.registries)));
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
                        Some("version") => {
                            version = Some(expect_string(iv, "locked.version")?)
                        }
                        Some("kind") => {
                            let k = keyword_name(iv).ok_or_else(|| {
                                ManifestError::TypeError {
                                    field: "locked.kind".to_string(),
                                    expected: "keyword".to_string(),
                                }
                            })?;
                            kind = Some(DependencyKind::from_str(k)?);
                        }
                        Some("registry") => {
                            registry = Some(expect_string(iv, "locked.registry")?)
                        }
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
        let caps: Vec<String> = sb.allow.iter().map(|c| format!(":{}", c.as_str())).collect();
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
        let caps: Vec<String> = sb.allow.iter().map(|c| format!(":{}", c.as_str())).collect();
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
// Definition Store
// ---------------------------------------------------------------------------

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
        assert_eq!(manifest.package.description.as_deref(), Some("My application"));
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
