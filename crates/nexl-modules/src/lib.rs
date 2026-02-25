use std::path::{Path, PathBuf};

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
}
