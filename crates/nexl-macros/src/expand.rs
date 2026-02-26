//! Macro expansion engine (Phase 1).

use std::collections::HashMap;

use thiserror::Error;

use crate::syntax::SyntaxObj;
use crate::{Scope, ScopeSet};
use nexl_ast::{Atom, Node, NodeKind, Span};

/// Errors produced during macro parsing or expansion.
#[derive(Debug, Error)]
pub enum MacroError {
    /// A macro definition or invocation is malformed.
    #[error("macro error: {0}")]
    Message(String),
}

/// Expand macros in a sequence of top-level forms.
pub fn expand_forms(nodes: &[Node]) -> Result<Vec<Node>, MacroError> {
    let mut expander = Expander::new();
    expander.expand_forms(nodes)
}

#[derive(Debug, Clone)]
struct MacroDef {
    params: Vec<String>,
    rest: Option<String>,
    body: Node,
}

struct Expander {
    macros: HashMap<String, MacroDef>,
}

impl Expander {
    fn new() -> Self {
        Self {
            macros: HashMap::new(),
        }
    }

    fn expand_forms(&mut self, nodes: &[Node]) -> Result<Vec<Node>, MacroError> {
        let mut expanded = Vec::new();
        for node in nodes {
            if let Some(out) = self.expand_node(node)? {
                expanded.push(out);
            }
        }
        Ok(expanded)
    }

    fn expand_node(&mut self, node: &Node) -> Result<Option<Node>, MacroError> {
        if let Some(def) = parse_defmacro(node)? {
            self.macros.insert(def.0, def.1);
            return Ok(None);
        }

        let expanded = match &node.kind {
            NodeKind::List(items) => {
                if items.first().and_then(symbol_name) == Some("quote") {
                    return Ok(Some(node.clone()));
                }
                if let Some(head) = items.first()
                    && let Some(name) = symbol_name(head)
                    && let Some(def) = self.macros.get(name).cloned()
                {
                    let call = expand_macro_call(&def, node)?;
                    if let Some(out) = self.expand_node(&call)? {
                        return Ok(Some(out));
                    }
                    return Ok(None);
                }
                let mut next_items = Vec::with_capacity(items.len());
                for item in items {
                    if let Some(out) = self.expand_node(item)? {
                        next_items.push(out);
                    }
                }
                Node::new(NodeKind::List(next_items), node.span)
            }
            NodeKind::Vector(items) => {
                let mut next_items = Vec::with_capacity(items.len());
                for item in items {
                    if let Some(out) = self.expand_node(item)? {
                        next_items.push(out);
                    }
                }
                Node::new(NodeKind::Vector(next_items), node.span)
            }
            NodeKind::Map(pairs) => {
                let mut next_pairs = Vec::with_capacity(pairs.len());
                for (k, v) in pairs {
                    let Some(next_k) = self.expand_node(k)? else {
                        return Err(MacroError::Message(
                            "cannot drop map key during macro expansion".to_string(),
                        ));
                    };
                    let Some(next_v) = self.expand_node(v)? else {
                        return Err(MacroError::Message(
                            "cannot drop map value during macro expansion".to_string(),
                        ));
                    };
                    next_pairs.push((next_k, next_v));
                }
                Node::new(NodeKind::Map(next_pairs), node.span)
            }
            NodeKind::Set(items) => {
                let mut next_items = Vec::with_capacity(items.len());
                for item in items {
                    if let Some(out) = self.expand_node(item)? {
                        next_items.push(out);
                    }
                }
                Node::new(NodeKind::Set(next_items), node.span)
            }
            _ => node.clone(),
        };

        Ok(Some(expanded))
    }
}

fn parse_defmacro(node: &Node) -> Result<Option<(String, MacroDef)>, MacroError> {
    let NodeKind::List(items) = &node.kind else {
        return Ok(None);
    };
    if items.is_empty() {
        return Ok(None);
    }
    let head = &items[0];
    let Some(head_name) = symbol_name(head) else {
        return Ok(None);
    };
    if head_name != "defmacro" {
        return Ok(None);
    }
    if items.len() < 4 {
        return Err(MacroError::Message("defmacro requires name, params, body".to_string()));
    }
    let name = symbol_name(&items[1])
        .ok_or_else(|| MacroError::Message("defmacro name must be a symbol".to_string()))?
        .to_string();
    let params = match &items[2].kind {
        NodeKind::Vector(items) => items,
        _ => {
            return Err(MacroError::Message(
                "defmacro params must be a vector".to_string(),
            ))
        }
    };
    let (args, rest) = parse_params(params)?;
    let body = if items.len() == 4 {
        items[3].clone()
    } else if items.len() == 5
        && symbol_name(&items[3]) == Some("&")
        && symbol_name(&items[4]) == Some("form")
    {
        Node::atom(
            Atom::Symbol {
                ns: None,
                name: "&form".to_string(),
            },
            items[3].span,
        )
    } else {
        make_do(&items[3..])
    };
    Ok(Some((
        name.clone(),
        MacroDef {
            params: args,
            rest,
            body,
        },
    )))
}

fn parse_params(params: &[Node]) -> Result<(Vec<String>, Option<String>), MacroError> {
    let mut args = Vec::new();
    let mut rest = None;
    let mut iter = params.iter().peekable();
    while let Some(param) = iter.next() {
        match &param.kind {
            NodeKind::Atom(Atom::Symbol { ns: None, name }) if name == "&" => {
                let rest_node = iter
                    .next()
                    .ok_or_else(|| MacroError::Message("expected rest param after &".to_string()))?;
                let rest_name = symbol_name(rest_node)
                    .ok_or_else(|| MacroError::Message("rest param must be a symbol".to_string()))?
                    .to_string();
                if iter.peek().is_some() {
                    return Err(MacroError::Message(
                        "rest param must be last in parameter list".to_string(),
                    ));
                }
                rest = Some(rest_name);
            }
            NodeKind::Atom(Atom::Symbol { ns: None, name }) => args.push(name.clone()),
            _ => {
                return Err(MacroError::Message(
                    "macro params must be symbols".to_string(),
                ))
            }
        }
    }
    Ok((args, rest))
}

fn expand_macro_call(def: &MacroDef, call: &Node) -> Result<Node, MacroError> {
    let NodeKind::List(items) = &call.kind else {
        return Err(MacroError::Message("macro call must be a list".to_string()));
    };
    let args = &items[1..];
    if args.len() < def.params.len() {
        return Err(MacroError::Message("macro call has too few arguments".to_string()));
    }
    if def.rest.is_none() && args.len() > def.params.len() {
        return Err(MacroError::Message("macro call has too many arguments".to_string()));
    }

    let intro_scope = Scope::fresh();
    let mut bindings: HashMap<String, MacroBinding> = HashMap::new();
    for (param, arg) in def.params.iter().zip(args.iter()) {
        let stx = SyntaxObj::new(arg.clone(), ScopeSet::new()).add_scope_deep(intro_scope);
        bindings.insert(param.clone(), MacroBinding::One(stx));
    }
    if let Some(rest_name) = &def.rest {
        let rest_args = args[def.params.len()..]
            .iter()
            .cloned()
            .map(|arg| SyntaxObj::new(arg, ScopeSet::new()).add_scope_deep(intro_scope))
            .collect::<Vec<_>>();
        bindings.insert(rest_name.clone(), MacroBinding::Many(rest_args));
    }
    bindings.insert(
        "&form".to_string(),
        MacroBinding::One(SyntaxObj::new(call.clone(), ScopeSet::new()).add_scope_deep(intro_scope)),
    );

    let mut ctx = ExpansionCtx::new(bindings);
    let result = eval_macro_body(&def.body, &mut ctx)?;
    let flipped = result.flip_scope_deep(intro_scope);
    Ok(flipped.node)
}

#[derive(Debug, Clone)]
enum MacroBinding {
    One(SyntaxObj),
    Many(Vec<SyntaxObj>),
}

struct ExpansionCtx {
    bindings: HashMap<String, MacroBinding>,
    gensym_map: HashMap<String, String>,
    gensym_counter: u64,
}

impl ExpansionCtx {
    fn new(bindings: HashMap<String, MacroBinding>) -> Self {
        Self {
            bindings,
            gensym_map: HashMap::new(),
            gensym_counter: 0,
        }
    }

    fn gensym(&mut self, base: &str) -> String {
        if let Some(existing) = self.gensym_map.get(base) {
            return existing.clone();
        }
        self.gensym_counter += 1;
        let name = format!("{base}__{}__auto__", self.gensym_counter);
        self.gensym_map.insert(base.to_string(), name.clone());
        name
    }
}

fn eval_macro_body(node: &Node, ctx: &mut ExpansionCtx) -> Result<SyntaxObj, MacroError> {
    match &node.kind {
        NodeKind::Quasiquote(inner) => {
            let value = expand_quasiquote(inner, 1, ctx)?;
            let node = match value {
                NodeOrSplice::Node(node) => node,
                NodeOrSplice::Splice(_) => {
                    return Err(MacroError::Message(
                        "top-level unquote-splice is not allowed".to_string(),
                    ))
                }
            };
            Ok(SyntaxObj::new(node, ScopeSet::new()))
        }
        NodeKind::Atom(Atom::Symbol { ns: None, name }) => match ctx.bindings.get(name) {
            Some(MacroBinding::One(stx)) => Ok(stx.clone()),
            Some(MacroBinding::Many(_)) => Err(MacroError::Message(format!(
                "macro binding `{name}` is variadic"
            ))),
            None => Ok(SyntaxObj::new(node.clone(), ScopeSet::new())),
        },
        _ => Ok(SyntaxObj::new(node.clone(), ScopeSet::new())),
    }
}

#[derive(Debug)]
enum NodeOrSplice {
    Node(Node),
    Splice(Vec<Node>),
}

fn expand_quasiquote(
    node: &Node,
    depth: usize,
    ctx: &mut ExpansionCtx,
) -> Result<NodeOrSplice, MacroError> {
    match &node.kind {
        NodeKind::Quasiquote(inner) => {
            let inner = match expand_quasiquote(inner, depth + 1, ctx)? {
                NodeOrSplice::Node(node) => node,
                NodeOrSplice::Splice(_) => {
                    return Err(MacroError::Message(
                        "unquote-splice not allowed inside quasiquote".to_string(),
                    ))
                }
            };
            Ok(NodeOrSplice::Node(Node::new(
                NodeKind::Quasiquote(Box::new(inner)),
                node.span,
            )))
        }
        NodeKind::Unquote(inner) => {
            if depth == 1 {
                let value = eval_unquote(inner, ctx)?;
                Ok(NodeOrSplice::Node(value.node))
            } else {
                let inner = match expand_quasiquote(inner, depth - 1, ctx)? {
                    NodeOrSplice::Node(node) => node,
                    NodeOrSplice::Splice(_) => {
                        return Err(MacroError::Message(
                            "unquote-splice not allowed inside nested unquote".to_string(),
                        ))
                    }
                };
                Ok(NodeOrSplice::Node(Node::new(
                    NodeKind::Unquote(Box::new(inner)),
                    node.span,
                )))
            }
        }
        NodeKind::UnquoteSplice(inner) => {
            if depth == 1 {
                let items = eval_unquote_splice(inner, ctx)?;
                Ok(NodeOrSplice::Splice(items))
            } else {
                let inner = match expand_quasiquote(inner, depth - 1, ctx)? {
                    NodeOrSplice::Node(node) => node,
                    NodeOrSplice::Splice(_) => {
                        return Err(MacroError::Message(
                            "unquote-splice not allowed inside nested unquote-splice".to_string(),
                        ))
                    }
                };
                Ok(NodeOrSplice::Node(Node::new(
                    NodeKind::UnquoteSplice(Box::new(inner)),
                    node.span,
                )))
            }
        }
        NodeKind::List(items) => {
            let mut out = Vec::new();
            for item in items {
                match expand_quasiquote(item, depth, ctx)? {
                    NodeOrSplice::Node(node) => out.push(node),
                    NodeOrSplice::Splice(nodes) => out.extend(nodes),
                }
            }
            Ok(NodeOrSplice::Node(Node::new(NodeKind::List(out), node.span)))
        }
        NodeKind::Vector(items) => {
            let mut out = Vec::new();
            for item in items {
                match expand_quasiquote(item, depth, ctx)? {
                    NodeOrSplice::Node(node) => out.push(node),
                    NodeOrSplice::Splice(nodes) => out.extend(nodes),
                }
            }
            Ok(NodeOrSplice::Node(Node::new(NodeKind::Vector(out), node.span)))
        }
        NodeKind::Map(pairs) => {
            let mut out = Vec::with_capacity(pairs.len());
            for (k, v) in pairs {
                let key = match expand_quasiquote(k, depth, ctx)? {
                    NodeOrSplice::Node(node) => node,
                    NodeOrSplice::Splice(_) => {
                        return Err(MacroError::Message(
                            "unquote-splice not allowed in map keys".to_string(),
                        ))
                    }
                };
                let value = match expand_quasiquote(v, depth, ctx)? {
                    NodeOrSplice::Node(node) => node,
                    NodeOrSplice::Splice(_) => {
                        return Err(MacroError::Message(
                            "unquote-splice not allowed in map values".to_string(),
                        ))
                    }
                };
                out.push((key, value));
            }
            Ok(NodeOrSplice::Node(Node::new(NodeKind::Map(out), node.span)))
        }
        NodeKind::Set(items) => {
            let mut out = Vec::new();
            for item in items {
                match expand_quasiquote(item, depth, ctx)? {
                    NodeOrSplice::Node(node) => out.push(node),
                    NodeOrSplice::Splice(_) => {
                        return Err(MacroError::Message(
                            "unquote-splice not allowed in sets".to_string(),
                        ))
                    }
                }
            }
            Ok(NodeOrSplice::Node(Node::new(NodeKind::Set(out), node.span)))
        }
        NodeKind::Atom(Atom::Symbol { ns: None, name }) if depth >= 1 && name.ends_with('#') => {
            let base = name.trim_end_matches('#');
            let fresh = ctx.gensym(base);
            let node = Node::atom(
                Atom::Symbol {
                    ns: None,
                    name: fresh,
                },
                node.span,
            );
            Ok(NodeOrSplice::Node(node))
        }
        _ => Ok(NodeOrSplice::Node(node.clone())),
    }
}

fn eval_unquote(node: &Node, ctx: &mut ExpansionCtx) -> Result<SyntaxObj, MacroError> {
    match &node.kind {
        NodeKind::Atom(Atom::Symbol { ns: None, name }) => match ctx.bindings.get(name) {
            Some(MacroBinding::One(stx)) => Ok(stx.clone()),
            Some(MacroBinding::Many(_items)) => Err(MacroError::Message(format!(
                "cannot unquote-splice `{name}` with ~"
            ))),
            None => Err(MacroError::Message(format!(
                "unknown macro binding `{name}`"
            ))),
        },
        _ => Err(MacroError::Message(
            "unquote only supports symbol bindings for now".to_string(),
        )),
    }
}

fn eval_unquote_splice(node: &Node, ctx: &mut ExpansionCtx) -> Result<Vec<Node>, MacroError> {
    match &node.kind {
        NodeKind::Atom(Atom::Symbol { ns: None, name }) => match ctx.bindings.get(name) {
            Some(MacroBinding::Many(items)) => {
                Ok(items.iter().map(|stx| stx.node.clone()).collect())
            }
            Some(MacroBinding::One(stx)) => match &stx.node.kind {
                NodeKind::List(items) => Ok(items.clone()),
                _ => Err(MacroError::Message(format!(
                    "unquote-splice `{name}` must be a list"
                ))),
            },
            None => Err(MacroError::Message(format!(
                "unknown macro binding `{name}`"
            ))),
        },
        _ => Err(MacroError::Message(
            "unquote-splice only supports symbol bindings for now".to_string(),
        )),
    }
}

fn symbol_name(node: &Node) -> Option<&str> {
    match &node.kind {
        NodeKind::Atom(Atom::Symbol { ns: None, name }) => Some(name.as_str()),
        _ => None,
    }
}

fn make_do(forms: &[Node]) -> Node {
    let mut items = Vec::with_capacity(forms.len() + 1);
    items.push(Node::atom(
        Atom::Symbol {
            ns: None,
            name: "do".to_string(),
        },
        Span::synthetic(),
    ));
    items.extend(forms.iter().cloned());
    Node::new(NodeKind::List(items), Span::synthetic())
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexl_ast::FileId;
    use nexl_reader::read;

    fn normalize(node: &Node) -> Node {
        let kind = match &node.kind {
            NodeKind::Atom(atom) => NodeKind::Atom(atom.clone()),
            NodeKind::List(items) => NodeKind::List(items.iter().map(normalize).collect()),
            NodeKind::Vector(items) => NodeKind::Vector(items.iter().map(normalize).collect()),
            NodeKind::Map(pairs) => NodeKind::Map(
                pairs
                    .iter()
                    .map(|(k, v)| (normalize(k), normalize(v)))
                    .collect(),
            ),
            NodeKind::Set(items) => NodeKind::Set(items.iter().map(normalize).collect()),
            NodeKind::Quote(inner) => NodeKind::Quote(Box::new(normalize(inner))),
            NodeKind::Deref(inner) => NodeKind::Deref(Box::new(normalize(inner))),
            NodeKind::Discard(inner) => NodeKind::Discard(Box::new(normalize(inner))),
            NodeKind::Quasiquote(inner) => NodeKind::Quasiquote(Box::new(normalize(inner))),
            NodeKind::Unquote(inner) => NodeKind::Unquote(Box::new(normalize(inner))),
            NodeKind::UnquoteSplice(inner) => NodeKind::UnquoteSplice(Box::new(normalize(inner))),
        };
        Node::new(kind, Span::synthetic())
    }

    #[test]
    fn expand_defmacro_basic_unless() {
        let src = r#"
        (defmacro unless [cond & body]
          `(if (not ~cond) (do ~@body)))
        (unless true (println "ok"))
        "#;
        let nodes = read(src, FileId::SYNTHETIC).expect("parse");
        let expanded = expand_forms(&nodes).expect("expand");
        assert_eq!(expanded.len(), 1);

        let expected = read("(if (not true) (do (println \"ok\")))", FileId::SYNTHETIC)
            .expect("parse expected");

        assert_eq!(normalize(&expanded[0]), normalize(&expected[0]));
    }

    #[test]
    fn expand_defmacro_rest_splice() {
        let src = r#"
        (defmacro do2 [& body]
          `(do ~@body))
        (do2 1 2 3)
        "#;
        let nodes = read(src, FileId::SYNTHETIC).expect("parse");
        let expanded = expand_forms(&nodes).expect("expand");
        let expected = read("(do 1 2 3)", FileId::SYNTHETIC).expect("parse expected");
        assert_eq!(normalize(&expanded[0]), normalize(&expected[0]));
    }

    #[test]
    fn expand_defmacro_amp_form_returns_call() {
        let src = r#"
        (defmacro use-form [x] `(quote ~&form))
        (use-form 1)
        "#;
        let nodes = read(src, FileId::SYNTHETIC).expect("parse");
        let expanded = expand_forms(&nodes).expect("expand");
        let expected =
            read("(quote (use-form 1))", FileId::SYNTHETIC).expect("parse expected");
        assert_eq!(normalize(&expanded[0]), normalize(&expected[0]));
    }

    #[test]
    fn expand_defmacro_gensym_suffix_is_consistent() {
        let src = r#"
        (defmacro swap [a b]
          `(let [tmp# ~a
                 ~a ~b
                 ~b tmp#]
             tmp#))
        (swap x y)
        "#;
        let nodes = read(src, FileId::SYNTHETIC).expect("parse");
        let expanded = expand_forms(&nodes).expect("expand");

        let mut names = Vec::new();
        collect_symbols(&expanded[0], &mut names);
        let gensyms: Vec<String> = names
            .iter()
            .filter(|name| name.starts_with("tmp"))
            .cloned()
            .collect();
        assert!(!gensyms.is_empty(), "expected a tmp gensym");
        let first = &gensyms[0];
        for name in &gensyms {
            assert_eq!(name, first);
            assert_ne!(name, "tmp#");
        }
    }

    fn collect_symbols(node: &Node, out: &mut Vec<String>) {
        match &node.kind {
            NodeKind::Atom(Atom::Symbol { ns: None, name }) => out.push(name.clone()),
            NodeKind::List(items)
            | NodeKind::Vector(items)
            | NodeKind::Set(items) => {
                for item in items {
                    collect_symbols(item, out);
                }
            }
            NodeKind::Map(pairs) => {
                for (k, v) in pairs {
                    collect_symbols(k, out);
                    collect_symbols(v, out);
                }
            }
            NodeKind::Quote(inner)
            | NodeKind::Deref(inner)
            | NodeKind::Discard(inner)
            | NodeKind::Quasiquote(inner)
            | NodeKind::Unquote(inner)
            | NodeKind::UnquoteSplice(inner) => collect_symbols(inner, out),
            _ => {}
        }
    }
}
