use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use crate::{Env, EvalError, ModuleExports, eval::eval, stdlib};
use meta::{
    Atom, ImportDecl, ImportKind, ModuleDecl, Node, NodeKind, parse_import_decl, parse_module_decl,
};
use nexl_modules::{ModuleInfo, build_module_graph};

/// Parsed module source: declaration, imports, and remaining forms.
#[derive(Debug, Clone)]
pub struct ModuleSource {
    /// The module declaration parsed from `(module ...)`.
    pub decl: ModuleDecl,
    /// Import declarations in this module.
    pub imports: Vec<ImportDecl>,
    /// Remaining top-level forms to evaluate.
    pub forms: Vec<Node>,
}

/// Parse a module file's nodes into a [`ModuleSource`].
pub fn parse_module_source(nodes: &[Node]) -> Result<ModuleSource, EvalError> {
    let mut iter = nodes.iter();
    let module_node = iter.next().ok_or(EvalError::MissingModuleDecl)?;
    let module_items = match &module_node.kind {
        NodeKind::List(items) if list_head_is(items, "module") => items,
        _ => return Err(EvalError::MissingModuleDecl),
    };
    let decl =
        parse_module_decl(module_items).map_err(|e| EvalError::ModuleParse(e.description))?;

    // Start with imports declared via :imports in the module form
    let mut imports: Vec<ImportDecl> = decl.imports.clone();
    let mut forms = Vec::new();
    for node in iter {
        match &node.kind {
            NodeKind::List(items) if list_head_is(items, "import") => {
                let import =
                    parse_import_decl(items).map_err(|e| EvalError::ModuleParse(e.description))?;
                imports.push(import);
            }
            _ => forms.push(node.clone()),
        }
    }

    Ok(ModuleSource {
        decl,
        imports,
        forms,
    })
}

/// Evaluate a set of modules, returning their environments keyed by module name.
pub fn eval_modules(modules: Vec<ModuleSource>) -> Result<HashMap<String, Rc<Env>>, EvalError> {
    let mut by_name: HashMap<String, ModuleSource> = HashMap::new();
    for module in modules {
        let name = module.decl.name.clone();
        if by_name.contains_key(&name) {
            return Err(EvalError::ModuleGraph(format!(
                "duplicate module declaration: {name}"
            )));
        }
        by_name.insert(name, module);
    }

    let infos: Vec<ModuleInfo> = by_name
        .values()
        .map(|m| ModuleInfo {
            name: m.decl.name.clone(),
            imports: m.imports.clone(),
        })
        .collect();
    let graph = build_module_graph(&infos).map_err(|e| EvalError::ModuleGraph(e.to_string()))?;
    let order = graph
        .topo_sort()
        .map_err(|e| EvalError::ModuleGraph(e.to_string()))?;

    let base_env = stdlib::standard_env();
    let mut runtimes: HashMap<String, ModuleRuntime> = HashMap::new();

    for name in order {
        let module = by_name
            .get(&name)
            .ok_or_else(|| EvalError::UnknownModule(name.clone()))?;
        let env = Rc::new(Env::child(Rc::clone(&base_env)));

        for import in &module.imports {
            let imported = runtimes
                .get(&import.module_path)
                .ok_or_else(|| EvalError::UnknownModule(import.module_path.clone()))?;
            apply_import(import, &imported.exports, &env)?;
        }

        let defined = eval_module_forms(&module.forms, &env)?;
        let exports = build_exports(&module.decl, &defined, &env)?;

        runtimes.insert(
            module.decl.name.clone(),
            ModuleRuntime {
                env: Rc::clone(&env),
                exports,
            },
        );
    }

    Ok(runtimes
        .into_iter()
        .map(|(name, runtime)| (name, runtime.env))
        .collect())
}

struct ModuleRuntime {
    env: Rc<Env>,
    exports: ModuleExports,
}

fn eval_module_forms(forms: &[Node], env: &Rc<Env>) -> Result<Vec<String>, EvalError> {
    let mut defined = Vec::new();
    for form in forms {
        if let Some(name) = top_level_def_name(form) {
            defined.push(name);
        }
        eval(form, env)?;
    }
    Ok(defined)
}

fn build_exports(
    decl: &ModuleDecl,
    defined: &[String],
    env: &Rc<Env>,
) -> Result<ModuleExports, EvalError> {
    let export_names: Vec<String> = match &decl.exports {
        Some(exports) => exports.clone(),
        None => defined.to_vec(),
    };

    let mut exports = HashMap::new();
    for name in export_names {
        let value = env.get(&name).ok_or_else(|| EvalError::MissingExport {
            module: decl.name.clone(),
            name: name.clone(),
        })?;
        exports.insert(Rc::from(name.as_str()), value);
    }
    Ok(Rc::new(exports))
}

fn apply_import(
    import: &ImportDecl,
    exports: &ModuleExports,
    env: &Rc<Env>,
) -> Result<(), EvalError> {
    match &import.kind {
        ImportKind::Alias(alias) => {
            env.define_module_alias(Rc::from(alias.as_str()), Rc::clone(exports));
        }
        ImportKind::All => {
            for (name, value) in exports.iter() {
                env.define(name.clone(), value.clone());
            }
        }
        ImportKind::Refer(names) => {
            for name in names {
                let value =
                    exports
                        .get(name.as_str())
                        .ok_or_else(|| EvalError::ImportNotExported {
                            module: import.module_path.clone(),
                            name: name.clone(),
                        })?;
                env.define(Rc::from(name.as_str()), value.clone());
            }
        }
        ImportKind::Exclude(excluded) => {
            let excluded_set: HashSet<&str> = excluded.iter().map(|s| s.as_str()).collect();
            for name in excluded {
                if !exports.contains_key(name.as_str()) {
                    return Err(EvalError::ImportNotExported {
                        module: import.module_path.clone(),
                        name: name.clone(),
                    });
                }
            }
            for (name, value) in exports.iter() {
                if !excluded_set.contains(name.as_ref()) {
                    env.define(name.clone(), value.clone());
                }
            }
        }
        ImportKind::Rename(renames) => {
            for (old, new) in renames {
                let value =
                    exports
                        .get(old.as_str())
                        .ok_or_else(|| EvalError::ImportNotExported {
                            module: import.module_path.clone(),
                            name: old.clone(),
                        })?;
                env.define(Rc::from(new.as_str()), value.clone());
            }
        }
    }
    Ok(())
}

fn list_head_is(items: &[Node], head: &str) -> bool {
    matches!(
        items.first(),
        Some(Node {
            kind: NodeKind::Atom(Atom::Symbol { ns: None, name }),
            ..
        }) if name == head
    )
}

fn top_level_def_name(node: &Node) -> Option<String> {
    let items = match &node.kind {
        NodeKind::List(items) => items.as_slice(),
        _ => return None,
    };
    match items {
        [
            Node {
                kind: NodeKind::Atom(Atom::Symbol { ns: None, name }),
                ..
            },
            Node {
                kind:
                    NodeKind::Atom(Atom::Symbol {
                        ns: None,
                        name: def_name,
                    }),
                ..
            },
            ..,
        ] if name == "def" || name == "defn" => Some(def_name.clone()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use meta::FileId;
    use nexl_runtime::Value;

    fn module_from_src(src: &str) -> ModuleSource {
        let nodes = nexl_reader::read(src, FileId::SYNTHETIC).expect("parse failed");
        parse_module_source(&nodes).expect("module parse failed")
    }

    fn get_int(env: &Rc<Env>, name: &str) -> i64 {
        match env.get(name) {
            Some(Value::Int(v)) => v,
            other => panic!("expected Int for {name}, got {other:?}"),
        }
    }

    // -- Test 1 --
    #[test]
    fn eval_modules_respects_init_order() {
        let mod_a = module_from_src(
            "(module app.a :exports [x])
             (def x 41)",
        );
        let mod_b = module_from_src(
            "(module app.b :exports [y])
             (import app.a :refer [x])
             (def y (+ x 1))",
        );

        let modules = eval_modules(vec![mod_b, mod_a]).expect("eval failed");
        let env_b = modules.get("app.b").expect("missing app.b env");
        assert_eq!(get_int(env_b, "y"), 42);
    }

    // -- Test 2 --
    #[test]
    fn eval_modules_is_module_scoped() {
        let mod_a = module_from_src(
            "(module app.a :exports [x])
             (def x 1)",
        );
        let mod_b = module_from_src(
            "(module app.b :exports [y])
             (def y (+ x 1))",
        );

        let err = eval_modules(vec![mod_a, mod_b]).unwrap_err();
        assert!(
            matches!(err, EvalError::UnboundSymbol(ref name) if name == "x"),
            "expected unbound symbol x, got {err:?}"
        );
    }

    // -- Test 3 --
    #[test]
    fn eval_modules_supports_qualified_access() {
        let mod_a = module_from_src(
            "(module app.a :exports [inc])
             (defn inc [n] (+ n 1))",
        );
        let mod_b = module_from_src(
            "(module app.b :exports [y])
             (import app.a :as a)
             (def y (a/inc 1))",
        );

        let modules = eval_modules(vec![mod_a, mod_b]).expect("eval failed");
        let env_b = modules.get("app.b").expect("missing app.b env");
        assert_eq!(get_int(env_b, "y"), 2);
    }

    // -- Test 4 --
    #[test]
    fn parse_module_source_with_imports() {
        let m = module_from_src(
            "(module app.main
               :imports [[app.util :as u]
                         [app.data :refer [load!]]])
             (def x 1)",
        );
        assert_eq!(m.imports.len(), 2);
        assert_eq!(m.imports[0].module_path, "app.util");
        assert_eq!(m.imports[0].kind, ImportKind::Alias("u".to_string()));
        assert_eq!(m.imports[1].module_path, "app.data");
        assert_eq!(
            m.imports[1].kind,
            ImportKind::Refer(vec!["load!".to_string()])
        );
        assert_eq!(m.forms.len(), 1);
    }

    // -- Test 5 --
    #[test]
    fn parse_module_source_imports_and_standalone_merge() {
        let m = module_from_src(
            "(module app.main
               :imports [[app.util :as u]])
             (import app.data :refer [load!])
             (def x 1)",
        );
        // :imports entries come first, standalone import appended
        assert_eq!(m.imports.len(), 2);
        assert_eq!(m.imports[0].module_path, "app.util");
        assert_eq!(m.imports[1].module_path, "app.data");
        assert_eq!(m.forms.len(), 1);
    }
}
