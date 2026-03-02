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
enum MacroKind {
    Proc(ProcMacro),
    Syntax(SyntaxMacro),
    Builtin(BuiltinMacro),
}

#[derive(Debug, Clone)]
struct ProcMacro {
    params: Vec<String>,
    rest: Option<String>,
    body: Node,
}

#[derive(Debug, Clone)]
struct SyntaxMacro {
    clauses: Vec<SyntaxClause>,
}

#[derive(Debug, Clone)]
struct SyntaxClause {
    params: Vec<String>,
    rest: Option<String>,
    template: Node,
}

#[derive(Debug, Clone, Copy)]
enum BuiltinMacro {
    When,
    Unless,
    Cond,
    ThreadFirst,
    ThreadLast,
    And,
    Or,
    Is,
    IsMatch,
    Deftest,
}

struct Expander {
    macros: HashMap<String, MacroKind>,
}

impl Expander {
    fn new() -> Self {
        let mut macros = HashMap::new();
        macros.insert("when".to_string(), MacroKind::Builtin(BuiltinMacro::When));
        macros.insert(
            "unless".to_string(),
            MacroKind::Builtin(BuiltinMacro::Unless),
        );
        macros.insert("cond".to_string(), MacroKind::Builtin(BuiltinMacro::Cond));
        macros.insert(
            "->".to_string(),
            MacroKind::Builtin(BuiltinMacro::ThreadFirst),
        );
        macros.insert(
            "->>".to_string(),
            MacroKind::Builtin(BuiltinMacro::ThreadLast),
        );
        macros.insert("and".to_string(), MacroKind::Builtin(BuiltinMacro::And));
        macros.insert("or".to_string(), MacroKind::Builtin(BuiltinMacro::Or));
        macros.insert("is".to_string(), MacroKind::Builtin(BuiltinMacro::Is));
        macros.insert(
            "is-match".to_string(),
            MacroKind::Builtin(BuiltinMacro::IsMatch),
        );
        macros.insert(
            "deftest".to_string(),
            MacroKind::Builtin(BuiltinMacro::Deftest),
        );
        Self { macros }
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
        if let Some(def) = parse_defreader_text(node)? {
            self.macros.insert(def.0, def.1);
            return Ok(None);
        }
        if let Some(def) = parse_defmacro_elab(node)? {
            self.macros.insert(def.0, def.1);
            return Ok(None);
        }
        if let Some(def) = parse_defmacro_syntax(node)? {
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
                    let call = match def {
                        MacroKind::Proc(def) => expand_macro_call(&def, node)?,
                        MacroKind::Syntax(def) => expand_syntax_macro(&def, node)?,
                        MacroKind::Builtin(def) => expand_builtin_macro(def, node)?,
                    };
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

fn parse_defmacro(node: &Node) -> Result<Option<(String, MacroKind)>, MacroError> {
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
        return Err(MacroError::Message(
            "defmacro requires name, params, body".to_string(),
        ));
    }
    let name = symbol_name(&items[1])
        .ok_or_else(|| MacroError::Message("defmacro name must be a symbol".to_string()))?
        .to_string();
    let params = match &items[2].kind {
        NodeKind::Vector(items) => items,
        _ => {
            return Err(MacroError::Message(
                "defmacro params must be a vector".to_string(),
            ));
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
        MacroKind::Proc(ProcMacro {
            params: args,
            rest,
            body,
        }),
    )))
}

fn parse_defmacro_elab(node: &Node) -> Result<Option<(String, MacroKind)>, MacroError> {
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
    if head_name != "defmacro-elab" {
        return Ok(None);
    }
    if items.len() < 4 {
        return Err(MacroError::Message(
            "defmacro-elab requires name, params, body".to_string(),
        ));
    }
    let name = symbol_name(&items[1])
        .ok_or_else(|| MacroError::Message("defmacro-elab name must be a symbol".to_string()))?
        .to_string();
    let params = match &items[2].kind {
        NodeKind::Vector(items) => items,
        _ => {
            return Err(MacroError::Message(
                "defmacro-elab params must be a vector".to_string(),
            ));
        }
    };
    let (args, rest) = parse_params_typed(params)?;

    let mut body_start = 3;
    if items.len() >= 6 && symbol_name(&items[3]) == Some("->") {
        body_start = 5;
    }
    if items.len() <= body_start {
        return Err(MacroError::Message(
            "defmacro-elab requires a body expression".to_string(),
        ));
    }

    let body = if items.len() == body_start + 1 {
        items[body_start].clone()
    } else if items.len() == body_start + 2
        && symbol_name(&items[body_start]) == Some("&")
        && symbol_name(&items[body_start + 1]) == Some("form")
    {
        Node::atom(
            Atom::Symbol {
                ns: None,
                name: "&form".to_string(),
            },
            items[body_start].span,
        )
    } else {
        make_do(&items[body_start..])
    };

    Ok(Some((
        name.clone(),
        MacroKind::Proc(ProcMacro {
            params: args,
            rest,
            body,
        }),
    )))
}

fn parse_defreader_text(node: &Node) -> Result<Option<(String, MacroKind)>, MacroError> {
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
    if head_name != "defreader-text" {
        return Ok(None);
    }
    if items.len() < 4 {
        return Err(MacroError::Message(
            "defreader-text requires tag, params, body".to_string(),
        ));
    }
    let raw_tag = symbol_name(&items[1])
        .ok_or_else(|| MacroError::Message("defreader-text tag must be a symbol".to_string()))?;
    let tag = raw_tag
        .strip_prefix('#')
        .ok_or_else(|| MacroError::Message("defreader-text tag must start with #".to_string()))?;
    if tag.is_empty() {
        return Err(MacroError::Message(
            "defreader-text tag cannot be empty".to_string(),
        ));
    }

    let params = match &items[2].kind {
        NodeKind::Vector(items) => items,
        _ => {
            return Err(MacroError::Message(
                "defreader-text params must be a vector".to_string(),
            ));
        }
    };
    let (args, rest) = parse_params_typed(params)?;

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
        tag.to_string(),
        MacroKind::Proc(ProcMacro {
            params: args,
            rest,
            body,
        }),
    )))
}

fn parse_defmacro_syntax(node: &Node) -> Result<Option<(String, MacroKind)>, MacroError> {
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
    if head_name != "defmacro-syntax" {
        return Ok(None);
    }
    if items.len() < 3 {
        return Err(MacroError::Message(
            "defmacro-syntax requires name and clauses".to_string(),
        ));
    }
    let name = symbol_name(&items[1])
        .ok_or_else(|| MacroError::Message("defmacro-syntax name must be a symbol".to_string()))?
        .to_string();

    let mut clauses = Vec::new();
    for clause in &items[2..] {
        let NodeKind::Vector(parts) = &clause.kind else {
            return Err(MacroError::Message(
                "defmacro-syntax clauses must be vectors".to_string(),
            ));
        };
        if parts.len() != 2 {
            return Err(MacroError::Message(
                "defmacro-syntax clause must have pattern and template".to_string(),
            ));
        }
        let (params, rest) = parse_syntax_pattern(&name, &parts[0])?;
        clauses.push(SyntaxClause {
            params,
            rest,
            template: parts[1].clone(),
        });
    }

    Ok(Some((
        name.clone(),
        MacroKind::Syntax(SyntaxMacro { clauses }),
    )))
}

fn parse_params(params: &[Node]) -> Result<(Vec<String>, Option<String>), MacroError> {
    let mut args = Vec::new();
    let mut rest = None;
    let mut iter = params.iter().peekable();
    while let Some(param) = iter.next() {
        match &param.kind {
            NodeKind::Atom(Atom::Symbol { ns: None, name }) if name == "&" => {
                let rest_node = iter.next().ok_or_else(|| {
                    MacroError::Message("expected rest param after &".to_string())
                })?;
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
                ));
            }
        }
    }
    Ok((args, rest))
}

fn parse_params_typed(params: &[Node]) -> Result<(Vec<String>, Option<String>), MacroError> {
    let mut args = Vec::new();
    let mut rest = None;
    let mut i = 0;
    while i < params.len() {
        let param = &params[i];
        match &param.kind {
            NodeKind::Atom(Atom::Symbol { ns: None, name }) if name == "&" => {
                i += 1;
                let rest_node = params.get(i).ok_or_else(|| {
                    MacroError::Message("expected rest param after &".to_string())
                })?;
                let rest_name = symbol_name(rest_node).ok_or_else(|| {
                    MacroError::Message("rest param must be a symbol".to_string())
                })?;
                i += 1;
                if i < params.len() && symbol_name(&params[i]) == Some(":") {
                    i += 1;
                    if i >= params.len() {
                        return Err(MacroError::Message(
                            "expected type annotation after :".to_string(),
                        ));
                    }
                    i += 1;
                }
                if i < params.len() {
                    return Err(MacroError::Message(
                        "rest param must be last in parameter list".to_string(),
                    ));
                }
                rest = Some(rest_name.to_string());
            }
            NodeKind::Atom(Atom::Symbol { ns: None, name }) => {
                args.push(name.clone());
                i += 1;
                if i < params.len() && symbol_name(&params[i]) == Some(":") {
                    i += 1;
                    if i >= params.len() {
                        return Err(MacroError::Message(
                            "expected type annotation after :".to_string(),
                        ));
                    }
                    i += 1;
                }
            }
            _ => {
                return Err(MacroError::Message(
                    "macro params must be symbols".to_string(),
                ));
            }
        }
    }
    Ok((args, rest))
}

fn expand_macro_call(def: &ProcMacro, call: &Node) -> Result<Node, MacroError> {
    let NodeKind::List(items) = &call.kind else {
        return Err(MacroError::Message("macro call must be a list".to_string()));
    };
    let args = &items[1..];
    if args.len() < def.params.len() {
        return Err(MacroError::Message(
            "macro call has too few arguments".to_string(),
        ));
    }
    if def.rest.is_none() && args.len() > def.params.len() {
        return Err(MacroError::Message(
            "macro call has too many arguments".to_string(),
        ));
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
        MacroBinding::One(
            SyntaxObj::new(call.clone(), ScopeSet::new()).add_scope_deep(intro_scope),
        ),
    );

    let mut ctx = ExpansionCtx::new(bindings);
    let result = eval_macro_body(&def.body, &mut ctx)?;
    let flipped = result.flip_scope_deep(intro_scope);
    Ok(flipped.node)
}

fn expand_syntax_macro(def: &SyntaxMacro, call: &Node) -> Result<Node, MacroError> {
    let NodeKind::List(items) = &call.kind else {
        return Err(MacroError::Message("macro call must be a list".to_string()));
    };
    let args = &items[1..];
    for clause in &def.clauses {
        if !pattern_arity_matches(clause, args.len()) {
            continue;
        }
        let mut bindings: HashMap<String, MacroBinding> = HashMap::new();
        let intro_scope = Scope::fresh();
        for (param, arg) in clause.params.iter().zip(args.iter()) {
            let stx = SyntaxObj::new(arg.clone(), ScopeSet::new()).add_scope_deep(intro_scope);
            bindings.insert(param.clone(), MacroBinding::One(stx));
        }
        if let Some(rest_name) = &clause.rest {
            let rest_args = args[clause.params.len()..]
                .iter()
                .cloned()
                .map(|arg| SyntaxObj::new(arg, ScopeSet::new()).add_scope_deep(intro_scope))
                .collect::<Vec<_>>();
            bindings.insert(rest_name.clone(), MacroBinding::Many(rest_args));
        }
        bindings.insert(
            "&form".to_string(),
            MacroBinding::One(
                SyntaxObj::new(call.clone(), ScopeSet::new()).add_scope_deep(intro_scope),
            ),
        );
        let mut ctx = ExpansionCtx::new(bindings);
        let result = eval_macro_body(&clause.template, &mut ctx)?;
        let flipped = result.flip_scope_deep(intro_scope);
        return Ok(flipped.node);
    }

    Err(MacroError::Message(
        "no matching defmacro-syntax clause".to_string(),
    ))
}

fn expand_builtin_macro(def: BuiltinMacro, call: &Node) -> Result<Node, MacroError> {
    let NodeKind::List(items) = &call.kind else {
        return Err(MacroError::Message("macro call must be a list".to_string()));
    };
    let args = &items[1..];
    let intro_scope = Scope::fresh();
    let args = args
        .iter()
        .cloned()
        .map(|arg| SyntaxObj::new(arg, ScopeSet::new()).add_scope_deep(intro_scope))
        .collect::<Vec<_>>();
    let mut gensym = Gensym::new();
    let node = match def {
        BuiltinMacro::When => expand_when(&args)?,
        BuiltinMacro::Unless => expand_unless(&args)?,
        BuiltinMacro::Cond => expand_cond(&args)?,
        BuiltinMacro::ThreadFirst => expand_thread(&args, ThreadPosition::First)?,
        BuiltinMacro::ThreadLast => expand_thread(&args, ThreadPosition::Last)?,
        BuiltinMacro::And => expand_and(&args, &mut gensym),
        BuiltinMacro::Or => expand_or(&args, &mut gensym),
        BuiltinMacro::Is => expand_is(&args, &mut gensym)?,
        BuiltinMacro::IsMatch => expand_is_match(&args)?,
        BuiltinMacro::Deftest => expand_deftest(&args)?,
    };
    let result = SyntaxObj::new(node, ScopeSet::new());
    let flipped = result.flip_scope_deep(intro_scope);
    Ok(flipped.node)
}

fn parse_syntax_pattern(
    name: &str,
    pattern: &Node,
) -> Result<(Vec<String>, Option<String>), MacroError> {
    let NodeKind::List(items) = &pattern.kind else {
        return Err(MacroError::Message(
            "defmacro-syntax pattern must be a list".to_string(),
        ));
    };
    let Some(head) = items.first() else {
        return Err(MacroError::Message(
            "defmacro-syntax pattern cannot be empty".to_string(),
        ));
    };
    let head_name = symbol_name(head).ok_or_else(|| {
        MacroError::Message("defmacro-syntax pattern head must be symbol".to_string())
    })?;
    if head_name != name {
        return Err(MacroError::Message(
            "defmacro-syntax pattern head must match macro name".to_string(),
        ));
    }
    let mut params = Vec::new();
    let mut rest = None;
    let tail = &items[1..];
    if tail.len() >= 2 {
        let ellipsis_len = if symbol_name(&tail[tail.len() - 1]) == Some("...") {
            Some(1)
        } else if tail.len() >= 4
            && symbol_name(&tail[tail.len() - 1]) == Some(".")
            && symbol_name(&tail[tail.len() - 2]) == Some(".")
            && symbol_name(&tail[tail.len() - 3]) == Some(".")
        {
            Some(3)
        } else {
            None
        };

        if let Some(ellipsis_len) = ellipsis_len {
            let rest_index = tail.len() - 1 - ellipsis_len;
            let rest_name = symbol_name(&tail[rest_index]).ok_or_else(|| {
                MacroError::Message("ellipsis must follow a symbol pattern".to_string())
            })?;
            params.extend(
                tail[..rest_index]
                    .iter()
                    .map(|node| {
                        symbol_name(node)
                            .ok_or_else(|| {
                                MacroError::Message(
                                    "defmacro-syntax params must be symbols".to_string(),
                                )
                            })
                            .map(|s| s.to_string())
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            );
            rest = Some(rest_name.to_string());
            return Ok((params, rest));
        }
    }
    for node in tail {
        let name = symbol_name(node).ok_or_else(|| {
            MacroError::Message("defmacro-syntax params must be symbols".to_string())
        })?;
        params.push(name.to_string());
    }
    Ok((params, rest))
}

fn pattern_arity_matches(clause: &SyntaxClause, arg_len: usize) -> bool {
    if clause.rest.is_some() {
        arg_len >= clause.params.len()
    } else {
        arg_len == clause.params.len()
    }
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
                    ));
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
                    ));
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
                        ));
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
                        ));
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
            Ok(NodeOrSplice::Node(Node::new(
                NodeKind::List(out),
                node.span,
            )))
        }
        NodeKind::Vector(items) => {
            let mut out = Vec::new();
            for item in items {
                match expand_quasiquote(item, depth, ctx)? {
                    NodeOrSplice::Node(node) => out.push(node),
                    NodeOrSplice::Splice(nodes) => out.extend(nodes),
                }
            }
            Ok(NodeOrSplice::Node(Node::new(
                NodeKind::Vector(out),
                node.span,
            )))
        }
        NodeKind::Map(pairs) => {
            let mut out = Vec::with_capacity(pairs.len());
            for (k, v) in pairs {
                let key = match expand_quasiquote(k, depth, ctx)? {
                    NodeOrSplice::Node(node) => node,
                    NodeOrSplice::Splice(_) => {
                        return Err(MacroError::Message(
                            "unquote-splice not allowed in map keys".to_string(),
                        ));
                    }
                };
                let value = match expand_quasiquote(v, depth, ctx)? {
                    NodeOrSplice::Node(node) => node,
                    NodeOrSplice::Splice(_) => {
                        return Err(MacroError::Message(
                            "unquote-splice not allowed in map values".to_string(),
                        ));
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
                        ));
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

struct Gensym {
    counter: u64,
}

impl Gensym {
    fn new() -> Self {
        Self { counter: 0 }
    }

    fn fresh(&mut self, base: &str) -> String {
        self.counter += 1;
        format!("{base}__{}__auto__", self.counter)
    }
}

fn expand_when(args: &[SyntaxObj]) -> Result<Node, MacroError> {
    if args.is_empty() {
        return Err(MacroError::Message("when requires a condition".to_string()));
    }
    let cond = args[0].node.clone();
    let body = &args[1..];
    let then_branch = if body.is_empty() {
        unit_node()
    } else {
        make_do(&body.iter().map(|stx| stx.node.clone()).collect::<Vec<_>>())
    };
    Ok(list_node(vec![
        symbol_node("if"),
        cond,
        then_branch,
        unit_node(),
    ]))
}

fn expand_unless(args: &[SyntaxObj]) -> Result<Node, MacroError> {
    if args.is_empty() {
        return Err(MacroError::Message(
            "unless requires a condition".to_string(),
        ));
    }
    let cond = args[0].node.clone();
    let body = &args[1..];
    let then_branch = if body.is_empty() {
        unit_node()
    } else {
        make_do(&body.iter().map(|stx| stx.node.clone()).collect::<Vec<_>>())
    };
    let not_cond = list_node(vec![symbol_node("not"), cond]);
    Ok(list_node(vec![
        symbol_node("if"),
        not_cond,
        then_branch,
        unit_node(),
    ]))
}

fn expand_cond(args: &[SyntaxObj]) -> Result<Node, MacroError> {
    if args.is_empty() {
        return Ok(panic_node("cond requires at least one clause"));
    }
    if !args.len().is_multiple_of(2) {
        return Err(MacroError::Message(
            "cond requires an even number of forms".to_string(),
        ));
    }

    expand_cond_pairs(args)
}

fn expand_cond_pairs(args: &[SyntaxObj]) -> Result<Node, MacroError> {
    if args.is_empty() {
        return Ok(panic_node("cond fell through"));
    }
    let test = &args[0];
    let expr = &args[1];

    if is_else_keyword(&test.node) {
        if args.len() > 2 {
            return Err(MacroError::Message(
                "cond :else must be the final clause".to_string(),
            ));
        }
        return Ok(expr.node.clone());
    }

    let else_branch = expand_cond_pairs(&args[2..])?;
    Ok(list_node(vec![
        symbol_node("if"),
        test.node.clone(),
        expr.node.clone(),
        else_branch,
    ]))
}

#[derive(Debug, Clone, Copy)]
enum ThreadPosition {
    First,
    Last,
}

fn expand_thread(args: &[SyntaxObj], position: ThreadPosition) -> Result<Node, MacroError> {
    if args.is_empty() {
        return Err(MacroError::Message(
            "threading macro requires an initial value".to_string(),
        ));
    }
    let mut acc = args[0].node.clone();
    for step in &args[1..] {
        acc = thread_step(&acc, &step.node, position)?;
    }
    Ok(acc)
}

fn thread_step(acc: &Node, step: &Node, position: ThreadPosition) -> Result<Node, MacroError> {
    match &step.kind {
        NodeKind::List(items) if items.is_empty() => Err(MacroError::Message(
            "threading macro step cannot be empty list".to_string(),
        )),
        NodeKind::List(items) => {
            let mut out = Vec::with_capacity(items.len() + 1);
            out.push(items[0].clone());
            match position {
                ThreadPosition::First => {
                    out.push(acc.clone());
                    out.extend(items[1..].iter().cloned());
                }
                ThreadPosition::Last => {
                    out.extend(items[1..].iter().cloned());
                    out.push(acc.clone());
                }
            }
            Ok(list_node(out))
        }
        _ => Ok(list_node(vec![step.clone(), acc.clone()])),
    }
}

fn expand_and(args: &[SyntaxObj], gensym: &mut Gensym) -> Node {
    match args.len() {
        0 => bool_node(true),
        1 => args[0].node.clone(),
        _ => {
            let name = gensym.fresh("tmp");
            let tmp = symbol_node(&name);
            let binding = vector_node(vec![tmp.clone(), args[0].node.clone()]);
            let rest = expand_and(&args[1..], gensym);
            let if_expr = list_node(vec![symbol_node("if"), tmp.clone(), rest, bool_node(false)]);
            list_node(vec![symbol_node("let"), binding, if_expr])
        }
    }
}

fn expand_or(args: &[SyntaxObj], gensym: &mut Gensym) -> Node {
    match args.len() {
        0 => bool_node(false),
        1 => args[0].node.clone(),
        _ => {
            let name = gensym.fresh("tmp");
            let tmp = symbol_node(&name);
            let binding = vector_node(vec![tmp.clone(), args[0].node.clone()]);
            let rest = expand_or(&args[1..], gensym);
            let if_expr = list_node(vec![symbol_node("if"), tmp.clone(), tmp.clone(), rest]);
            list_node(vec![symbol_node("let"), binding, if_expr])
        }
    }
}

/// Expand `(is expr)` or `(is expr "message")` into a power-assert expression.
///
/// The macro analyzes the expression's AST at expansion time and generates code
/// that captures sub-expression values and reports them on failure.
///
/// Recognized forms:
/// - `(= a b)` → captures both sides, reports `left: <a>  right: <b>`
/// - `(not= a b)` → reports "expected not-equal" with both values
/// - `(< a b)`, `(> a b)`, `(<= a b)`, `(>= a b)` → reports both values with operator
/// - `(pred? x)` or `(pred x)` — 1-arg predicate → reports predicate name and value
/// - Any other form → reports expression text and the boolean result
fn expand_is(args: &[SyntaxObj], gensym: &mut Gensym) -> Result<Node, MacroError> {
    if args.is_empty() {
        return Err(MacroError::Message(
            "`is` requires at least one argument".to_string(),
        ));
    }

    let expr = &args[0].node;
    // Optional explicit message (second arg)
    let extra_msg: Option<String> = args.get(1).and_then(|a| {
        if let NodeKind::Atom(Atom::Str(s)) = &a.node.kind {
            Some(s.clone())
        } else {
            None
        }
    });

    let expr_text = format!("{expr}");

    let failure_prefix = match &extra_msg {
        Some(msg) => format!("FAIL: {msg}\n  (is {expr_text})"),
        None => format!("assertion failed: (is {expr_text})"),
    };

    // Check if expr is a known binary comparison form
    if let NodeKind::List(items) = &expr.kind {
        if let Some(head_name) = items.first().and_then(|n| symbol_only_name(n)) {
            match head_name.as_str() {
                "=" | "not=" | "<" | ">" | "<=" | ">=" if items.len() == 3 => {
                    let lhs = items[1].clone();
                    let rhs = items[2].clone();
                    let lhs_text = format!("{lhs}");
                    let rhs_text = format!("{rhs}");
                    let lhs_var = gensym.fresh("is_lhs");
                    let rhs_var = gensym.fresh("is_rhs");

                    // (let [__lhs a __rhs b]
                    //   (if (op __lhs __rhs)
                    //     unit
                    //     (test/fail (str/format "..." __lhs __rhs))))
                    let op_sym = symbol_node(&head_name);
                    let cond = list_node(vec![
                        op_sym,
                        symbol_node(&lhs_var),
                        symbol_node(&rhs_var),
                    ]);

                    let fail_fmt = format!(
                        "{failure_prefix}\n  {lhs_text}: {{}}\n  {rhs_text}: {{}}",
                    );
                    let fail_call = test_fail_call(
                        str_format_call(&fail_fmt, vec![
                            symbol_node(&lhs_var),
                            symbol_node(&rhs_var),
                        ]),
                    );

                    let if_node = list_node(vec![
                        symbol_node("if"),
                        cond,
                        unit_node(),
                        fail_call,
                    ]);
                    let bindings = vector_node(vec![
                        symbol_node(&lhs_var),
                        lhs,
                        symbol_node(&rhs_var),
                        rhs,
                    ]);
                    return Ok(list_node(vec![symbol_node("let"), bindings, if_node]));
                }
                // 1-arg predicate: (pred expr)
                _ if items.len() == 2 => {
                    let val = items[1].clone();
                    let val_text = format!("{val}");
                    let val_var = gensym.fresh("is_val");

                    let cond = list_node(vec![
                        symbol_node(&head_name),
                        symbol_node(&val_var),
                    ]);

                    let fail_fmt =
                        format!("{failure_prefix}\n  {val_text}: {{}}  (expected {head_name} to be true)");
                    let fail_call = test_fail_call(str_format_call(
                        &fail_fmt,
                        vec![symbol_node(&val_var)],
                    ));

                    let if_node = list_node(vec![
                        symbol_node("if"),
                        cond,
                        unit_node(),
                        fail_call,
                    ]);
                    let bindings = vector_node(vec![symbol_node(&val_var), val]);
                    return Ok(list_node(vec![symbol_node("let"), bindings, if_node]));
                }
                _ => {}
            }
        }
    }

    // Generic fallback: (if expr unit (test/fail "assertion failed: <expr>"))
    let fail_call = test_fail_call(str_node(&failure_prefix));
    Ok(list_node(vec![
        symbol_node("if"),
        expr.clone(),
        unit_node(),
        fail_call,
    ]))
}

/// Return the symbol name if node is an unqualified symbol, else None.
fn symbol_only_name(node: &Node) -> Option<String> {
    match &node.kind {
        NodeKind::Atom(Atom::Symbol { ns: None, name }) => Some(name.clone()),
        _ => None,
    }
}

/// `(test/fail msg)` — call the test failure handler.
fn test_fail_call(msg: Node) -> Node {
    list_node(vec![
        Node::atom(
            Atom::Symbol {
                ns: Some("test".to_string()),
                name: "fail".to_string(),
            },
            Span::synthetic(),
        ),
        msg,
    ])
}

/// `(str/format fmt args...)` — format a string with runtime values.
fn str_format_call(fmt: &str, args: Vec<Node>) -> Node {
    let mut items = vec![
        Node::atom(
            Atom::Symbol {
                ns: Some("str".to_string()),
                name: "format".to_string(),
            },
            Span::synthetic(),
        ),
        str_node(fmt),
    ];
    items.extend(args);
    list_node(items)
}

/// A string literal node.
fn str_node(s: &str) -> Node {
    Node::atom(Atom::Str(s.to_string()), Span::synthetic())
}

/// Expand `(is-match pattern expr [:when guard] body...)`.
///
/// At the compile-time macro path, `is-match` is treated as a transparent pass-through:
/// the eval path handles the actual pattern matching semantics. This function returns
/// the form unchanged as a list node so the evaluator can dispatch it.
fn expand_is_match(args: &[SyntaxObj]) -> Result<Node, MacroError> {
    if args.len() < 2 {
        return Err(MacroError::Message(
            "`is-match` requires at least (is-match pattern expr)".to_string(),
        ));
    }
    // Return as-is: evaluator handles is-match as a special form
    let mut items = vec![symbol_node("is-match")];
    items.extend(args.iter().map(|a| a.node.clone()));
    Ok(list_node(items))
}

/// Expand `(deftest "name" body...)` into a test registration call.
///
/// Supported forms (spec §6.1–6.2):
/// - `(deftest "name" body...)`
///   → `(test/register! "name" (fn [] body...))`
/// - `(deftest "name" :skip body...)`
///   → `(test/register! "name" (fn [] (test/skip "skipped")))`
/// - `(deftest "name" :skip "reason" body...)`
///   → `(test/register! "name" (fn [] (test/skip "reason")))`
/// - `(deftest "name" :focus body...)`
///   → same as basic for now; focus filtering handled at runner level
fn expand_deftest(args: &[SyntaxObj]) -> Result<Node, MacroError> {
    if args.is_empty() {
        return Err(MacroError::Message(
            "`deftest` requires a name and a body".to_string(),
        ));
    }

    // args[0] must be a string literal — the test name
    let name_node = &args[0].node;
    let name = match &name_node.kind {
        NodeKind::Atom(Atom::Str(s)) => s.clone(),
        _ => {
            return Err(MacroError::Message(
                "`deftest` first argument must be a string name".to_string(),
            ));
        }
    };

    // Parse optional keyword metadata flags before the body
    let mut idx = 1usize;
    let mut skip_reason: Option<String> = None;
    // :focus — recognized but treated same as normal for now

    while idx < args.len() {
        if let NodeKind::Atom(Atom::Keyword { ns: None, name: kw }) = &args[idx].node.kind {
            match kw.as_str() {
                "skip" => {
                    idx += 1;
                    // Optional reason string
                    let reason = if idx < args.len() {
                        if let NodeKind::Atom(Atom::Str(s)) = &args[idx].node.kind {
                            let s = s.clone();
                            idx += 1;
                            s
                        } else {
                            "skipped".to_string()
                        }
                    } else {
                        "skipped".to_string()
                    };
                    skip_reason = Some(reason);
                }
                "focus" | "tags" | "timeout" | "flaky" => {
                    idx += 1;
                    // :tags takes a vector arg, :timeout/:flaky take a value — skip them
                    if matches!(kw.as_str(), "tags" | "timeout" | "flaky") && idx < args.len() {
                        idx += 1;
                    }
                }
                _ => break,
            }
        } else {
            break;
        }
    }

    let body_args = &args[idx..];

    // Build the body: if :skip, wrap in (test/skip reason)
    let body_node = if let Some(reason) = skip_reason {
        let skip_call = list_node(vec![
            Node::atom(
                Atom::Symbol {
                    ns: Some("test".to_string()),
                    name: "skip".to_string(),
                },
                Span::synthetic(),
            ),
            str_node(&reason),
        ]);
        list_node(vec![symbol_node("fn"), vector_node(vec![]), skip_call])
    } else if body_args.is_empty() {
        return Err(MacroError::Message(
            "`deftest` requires at least one body expression".to_string(),
        ));
    } else {
        let mut fn_items = vec![symbol_node("fn"), vector_node(vec![])];
        fn_items.extend(body_args.iter().map(|a| a.node.clone()));
        list_node(fn_items)
    };

    // (test/register! "name" (fn [] body...))
    Ok(list_node(vec![
        Node::atom(
            Atom::Symbol {
                ns: Some("test".to_string()),
                name: "register!".to_string(),
            },
            Span::synthetic(),
        ),
        str_node(&name),
        body_node,
    ]))
}

fn is_else_keyword(node: &Node) -> bool {
    matches!(
        node.kind,
        NodeKind::Atom(Atom::Keyword { ns: None, ref name }) if name == "else"
    )
}

fn unit_node() -> Node {
    Node::atom(Atom::Unit, Span::synthetic())
}

fn bool_node(value: bool) -> Node {
    Node::atom(Atom::Bool(value), Span::synthetic())
}

fn panic_node(msg: &str) -> Node {
    list_node(vec![
        symbol_node("panic"),
        Node::atom(Atom::Str(msg.to_string()), Span::synthetic()),
    ])
}

fn symbol_node(name: &str) -> Node {
    Node::atom(
        Atom::Symbol {
            ns: None,
            name: name.to_string(),
        },
        Span::synthetic(),
    )
}

fn list_node(items: Vec<Node>) -> Node {
    Node::new(NodeKind::List(items), Span::synthetic())
}

fn vector_node(items: Vec<Node>) -> Node {
    Node::new(NodeKind::Vector(items), Span::synthetic())
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
        let expected = read("(quote (use-form 1))", FileId::SYNTHETIC).expect("parse expected");
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

    #[test]
    fn expand_defmacro_syntax_selects_clause() {
        let src = r#"
        (defmacro-syntax my-or
          [(my-or) false]
          [(my-or e) e])
        (my-or true)
        "#;
        let nodes = read(src, FileId::SYNTHETIC).expect("parse");
        let expanded = expand_forms(&nodes).expect("expand");
        let expected = read("true", FileId::SYNTHETIC).expect("parse expected");
        assert_eq!(normalize(&expanded[0]), normalize(&expected[0]));
    }

    #[test]
    fn expand_defmacro_syntax_rest_pattern() {
        let src = r#"
        (defmacro-syntax collect
          [(collect x xs ...) `(list ~x ~@xs)])
        (collect 1 2 3)
        "#;
        let nodes = read(src, FileId::SYNTHETIC).expect("parse");
        let expanded = expand_forms(&nodes).expect("expand");
        let expected = read("(list 1 2 3)", FileId::SYNTHETIC).expect("parse expected");
        assert_eq!(normalize(&expanded[0]), normalize(&expected[0]));
    }

    #[test]
    fn expand_defmacro_elab_like_defmacro() {
        let src = r#"
        (defmacro-elab id [x] x)
        (id 1)
        "#;
        let nodes = read(src, FileId::SYNTHETIC).expect("parse");
        let expanded = expand_forms(&nodes).expect("expand");
        let expected = read("1", FileId::SYNTHETIC).expect("parse expected");
        assert_eq!(normalize(&expanded[0]), normalize(&expected[0]));
    }

    #[test]
    fn expand_defreader_text_basic() {
        let src = r#"
        (defreader-text #sql [text : Str loc : SrcLoc] text)
        #sql[SELECT 1]
        "#;
        let nodes = read(src, FileId::SYNTHETIC).expect("parse");
        let expanded = expand_forms(&nodes).expect("expand");
        let expected = read("\"SELECT 1\"", FileId::SYNTHETIC).expect("parse expected");
        assert_eq!(normalize(&expanded[0]), normalize(&expected[0]));
    }

    #[test]
    fn expand_when_basic() {
        let src = r#"
        (when ok (println "hi") (println "bye"))
        "#;
        let nodes = read(src, FileId::SYNTHETIC).expect("parse");
        let expanded = expand_forms(&nodes).expect("expand");
        let expected = read(
            "(if ok (do (println \"hi\") (println \"bye\")) unit)",
            FileId::SYNTHETIC,
        )
        .expect("parse expected");
        assert_eq!(normalize(&expanded[0]), normalize(&expected[0]));
    }

    #[test]
    fn expand_unless_basic() {
        let src = r#"
        (unless ok (println "no"))
        "#;
        let nodes = read(src, FileId::SYNTHETIC).expect("parse");
        let expanded = expand_forms(&nodes).expect("expand");
        let expected = read(
            "(if (not ok) (do (println \"no\")) unit)",
            FileId::SYNTHETIC,
        )
        .expect("parse expected");
        assert_eq!(normalize(&expanded[0]), normalize(&expected[0]));
    }

    #[test]
    fn expand_cond_with_else() {
        let src = r#"
        (cond (< x 0) :neg :else :pos)
        "#;
        let nodes = read(src, FileId::SYNTHETIC).expect("parse");
        let expanded = expand_forms(&nodes).expect("expand");
        let expected = read("(if (< x 0) :neg :pos)", FileId::SYNTHETIC).expect("parse expected");
        assert_eq!(normalize(&expanded[0]), normalize(&expected[0]));
    }

    #[test]
    fn expand_thread_first() {
        let src = r#"
        (-> x (f a) g)
        "#;
        let nodes = read(src, FileId::SYNTHETIC).expect("parse");
        let expanded = expand_forms(&nodes).expect("expand");
        let expected = read("(g (f x a))", FileId::SYNTHETIC).expect("parse expected");
        assert_eq!(normalize(&expanded[0]), normalize(&expected[0]));
    }

    #[test]
    fn expand_thread_last() {
        let src = r#"
        (->> x (f a) g)
        "#;
        let nodes = read(src, FileId::SYNTHETIC).expect("parse");
        let expanded = expand_forms(&nodes).expect("expand");
        let expected = read("(g (f a x))", FileId::SYNTHETIC).expect("parse expected");
        assert_eq!(normalize(&expanded[0]), normalize(&expected[0]));
    }

    #[test]
    fn expand_and_short_circuit() {
        let src = r#"
        (and a b c)
        "#;
        let nodes = read(src, FileId::SYNTHETIC).expect("parse");
        let expanded = expand_forms(&nodes).expect("expand");
        let expected = read(
            "(let [tmp__1__auto__ a]\n  (if tmp__1__auto__\n      (let [tmp__2__auto__ b]\n        (if tmp__2__auto__ c false))\n      false))",
            FileId::SYNTHETIC,
        )
        .expect("parse expected");
        assert_eq!(normalize(&expanded[0]), normalize(&expected[0]));
    }

    #[test]
    fn expand_or_short_circuit() {
        let src = r#"
        (or a b c)
        "#;
        let nodes = read(src, FileId::SYNTHETIC).expect("parse");
        let expanded = expand_forms(&nodes).expect("expand");
        let expected = read(
            "(let [tmp__1__auto__ a]\n  (if tmp__1__auto__\n      tmp__1__auto__\n      (let [tmp__2__auto__ b]\n        (if tmp__2__auto__ tmp__2__auto__ c))))",
            FileId::SYNTHETIC,
        )
        .expect("parse expected");
        assert_eq!(normalize(&expanded[0]), normalize(&expected[0]));
    }

    fn collect_symbols(node: &Node, out: &mut Vec<String>) {
        match &node.kind {
            NodeKind::Atom(Atom::Symbol { ns: None, name }) => out.push(name.clone()),
            NodeKind::List(items) | NodeKind::Vector(items) | NodeKind::Set(items) => {
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
