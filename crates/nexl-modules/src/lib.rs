use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use nexl_ast::ImportDecl;

/// Errors produced while mapping module names to file paths.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ModulePathError {
    /// The module name does not start with the package prefix.
    #[error("module `{module}` does not start with prefix `{prefix}`")]
    PrefixMismatch { module: String, prefix: String },
    /// The module name is not a valid dotted name.
    #[error("module `{module}` is not a valid dotted name")]
    InvalidModuleName { module: String },
    /// The module path does not end with `.nxl`.
    #[error("module path `{path}` must end with .nxl")]
    MissingExtension { path: String },
    /// The module path does not start with the package prefix directory.
    #[error("module path `{path}` does not start with prefix `{prefix}`")]
    PathPrefixMismatch { path: String, prefix: String },
    /// The module path contains non-utf8 components.
    #[error("module path `{path}` contains non-utf8 characters")]
    NonUtf8Path { path: String },
}

/// A module with its declared imports, ready for resolution.
#[derive(Debug, Clone, PartialEq)]
pub struct ModuleInfo {
    /// Fully-qualified module name, e.g. `my-app.server`.
    pub name: String,
    /// Import declarations in this module.
    pub imports: Vec<ImportDecl>,
}

/// A directed graph of module dependencies.
#[derive(Debug, Clone, PartialEq)]
pub struct ModuleGraph {
    edges: HashMap<String, Vec<String>>,
}

impl ModuleGraph {
    /// Return the dependencies of a module, if present.
    pub fn dependencies(&self, module: &str) -> Option<&[String]> {
        self.edges.get(module).map(|v| v.as_slice())
    }

    /// Return a topological ordering of the modules, or an error on cycles.
    pub fn topo_sort(&self) -> Result<Vec<String>, ModuleGraphError> {
        let mut indegree: HashMap<String, usize> = HashMap::new();
        let mut dependents: HashMap<String, Vec<String>> = HashMap::new();

        for (module, deps) in &self.edges {
            indegree.insert(module.clone(), deps.len());
            for dep in deps {
                dependents
                    .entry(dep.clone())
                    .or_default()
                    .push(module.clone());
            }
        }

        let mut ready = BTreeSet::new();
        for (node, count) in &indegree {
            if *count == 0 {
                ready.insert(node.clone());
            }
        }

        let mut order = Vec::with_capacity(indegree.len());

        while let Some(node) = ready.iter().next().cloned() {
            ready.remove(&node);
            order.push(node.clone());
            if let Some(users) = dependents.get(&node) {
                for dependent in users {
                    if let Some(count) = indegree.get_mut(dependent) {
                        *count -= 1;
                        if *count == 0 {
                            ready.insert(dependent.clone());
                        }
                    }
                }
            }
        }

        if order.len() != indegree.len() {
            let mut cycle_nodes: Vec<String> = indegree
                .into_iter()
                .filter_map(|(node, count)| if count > 0 { Some(node) } else { None })
                .collect();
            cycle_nodes.sort();
            return Err(ModuleGraphError::CycleDetected { nodes: cycle_nodes });
        }

        Ok(order)
    }

    /// Detect cycles in the graph, returning the involved nodes if present.
    pub fn detect_cycles(&self) -> Result<Option<Vec<String>>, ModuleGraphError> {
        match self.topo_sort() {
            Ok(_) => Ok(None),
            Err(ModuleGraphError::CycleDetected { nodes }) => Ok(Some(nodes)),
            Err(err) => Err(err),
        }
    }
}

/// Errors produced while constructing a module dependency graph.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ModuleGraphError {
    /// Two modules declared the same name.
    #[error("duplicate module declaration: `{name}`")]
    DuplicateModule { name: String },
    /// A cycle exists in the module dependency graph.
    #[error("circular dependency detected: {nodes:?}")]
    CycleDetected { nodes: Vec<String> },
}

/// Build a dependency graph from module import declarations.
pub fn build_module_graph(modules: &[ModuleInfo]) -> Result<ModuleGraph, ModuleGraphError> {
    let mut edges = HashMap::new();
    for module in modules {
        if edges.contains_key(&module.name) {
            return Err(ModuleGraphError::DuplicateModule {
                name: module.name.clone(),
            });
        }
        let deps = module
            .imports
            .iter()
            .map(|import| import.module_path.clone())
            .collect();
        edges.insert(module.name.clone(), deps);
    }
    Ok(ModuleGraph { edges })
}

/// Convert a module name (e.g. `my-app.server`) to a `.nxl` file path.
pub fn module_name_to_path(module: &str, prefix: &str) -> Result<PathBuf, ModulePathError> {
    let parts = split_module_name(module)?;
    if !has_prefix(module, prefix) {
        return Err(ModulePathError::PrefixMismatch {
            module: module.to_string(),
            prefix: prefix.to_string(),
        });
    }
    let mut path = PathBuf::new();
    for part in parts {
        path.push(part);
    }
    path.set_extension("nxl");
    Ok(path)
}

/// Convert a `.nxl` file path back to a module name.
pub fn path_to_module_name(path: &Path, prefix: &str) -> Result<String, ModulePathError> {
    let path_str = path.to_string_lossy().into_owned();
    let ext = path.extension().and_then(|s| s.to_str());
    if ext != Some("nxl") {
        return Err(ModulePathError::MissingExtension { path: path_str });
    }

    let mut parts = Vec::new();
    let without_ext = path.with_extension("");
    for component in without_ext.components() {
        let std::path::Component::Normal(os) = component else {
            return Err(ModulePathError::InvalidModuleName {
                module: without_ext.to_string_lossy().into_owned(),
            });
        };
        let part = os.to_str().ok_or_else(|| ModulePathError::NonUtf8Path {
            path: without_ext.to_string_lossy().into_owned(),
        })?;
        if part.is_empty() {
            return Err(ModulePathError::InvalidModuleName {
                module: without_ext.to_string_lossy().into_owned(),
            });
        }
        parts.push(part);
    }

    if parts.is_empty() {
        return Err(ModulePathError::InvalidModuleName {
            module: without_ext.to_string_lossy().into_owned(),
        });
    }

    let module = parts.join(".");
    if !has_prefix(&module, prefix) {
        return Err(ModulePathError::PathPrefixMismatch {
            path: without_ext.to_string_lossy().into_owned(),
            prefix: prefix.to_string(),
        });
    }
    Ok(module)
}

fn split_module_name(module: &str) -> Result<Vec<&str>, ModulePathError> {
    if module.is_empty() {
        return Err(ModulePathError::InvalidModuleName {
            module: module.to_string(),
        });
    }
    let parts: Vec<&str> = module.split('.').collect();
    if parts.iter().any(|part| part.is_empty()) {
        return Err(ModulePathError::InvalidModuleName {
            module: module.to_string(),
        });
    }
    Ok(parts)
}

fn has_prefix(module: &str, prefix: &str) -> bool {
    if module == prefix {
        return true;
    }
    match module.strip_prefix(prefix) {
        Some(rest) => rest.starts_with('.'),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use nexl_ast::ImportKind;

    #[test]
    fn module_name_to_path_basic() {
        let path = module_name_to_path("my-app.server", "my-app").expect("map failed");
        assert_eq!(path, PathBuf::from("my-app/server.nxl"));
    }

    #[test]
    fn path_to_module_name_basic() {
        let name = path_to_module_name(Path::new("my-app/server.nxl"), "my-app")
            .expect("map failed");
        assert_eq!(name, "my-app.server");
    }

    #[test]
    fn build_graph_collects_imports() {
        let modules = vec![
            ModuleInfo {
                name: "app.core".to_string(),
                imports: vec![
                    ImportDecl {
                        module_path: "lib.math".to_string(),
                        kind: ImportKind::All,
                    },
                    ImportDecl {
                        module_path: "lib.str".to_string(),
                        kind: ImportKind::All,
                    },
                ],
            },
            ModuleInfo {
                name: "lib.math".to_string(),
                imports: Vec::new(),
            },
            ModuleInfo {
                name: "lib.str".to_string(),
                imports: Vec::new(),
            },
        ];

        let graph = build_module_graph(&modules).expect("graph failed");
        assert_eq!(
            graph.dependencies("app.core"),
            Some(&vec!["lib.math".to_string(), "lib.str".to_string()][..])
        );
    }

    #[test]
    fn topo_sort_respects_dependencies() {
        let modules = vec![
            ModuleInfo {
                name: "app.core".to_string(),
                imports: vec![
                    ImportDecl {
                        module_path: "lib.math".to_string(),
                        kind: ImportKind::All,
                    },
                    ImportDecl {
                        module_path: "lib.str".to_string(),
                        kind: ImportKind::All,
                    },
                ],
            },
            ModuleInfo {
                name: "lib.math".to_string(),
                imports: Vec::new(),
            },
            ModuleInfo {
                name: "lib.str".to_string(),
                imports: Vec::new(),
            },
        ];

        let graph = build_module_graph(&modules).expect("graph failed");
        let order = graph.topo_sort().expect("sort failed");
        let idx = |name: &str| order.iter().position(|n| n == name).unwrap();
        assert!(idx("lib.math") < idx("app.core"));
        assert!(idx("lib.str") < idx("app.core"));
    }

    #[test]
    fn detect_cycles_reports_nodes() {
        let modules = vec![
            ModuleInfo {
                name: "a.core".to_string(),
                imports: vec![ImportDecl {
                    module_path: "b.core".to_string(),
                    kind: ImportKind::All,
                }],
            },
            ModuleInfo {
                name: "b.core".to_string(),
                imports: vec![ImportDecl {
                    module_path: "a.core".to_string(),
                    kind: ImportKind::All,
                }],
            },
        ];

        let graph = build_module_graph(&modules).expect("graph failed");
        let mut cycle = graph
            .detect_cycles()
            .expect("detect failed")
            .expect("expected cycle");
        cycle.sort();
        assert_eq!(cycle, vec!["a.core".to_string(), "b.core".to_string()]);
    }
}
