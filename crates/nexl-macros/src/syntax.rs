//! Syntax objects for hygienic macro expansion.
//!
//! A [`SyntaxObj`] wraps an AST [`Node`] together with scope information,
//! forming the value type that flows through the macro system (spec §7.2).

use crate::scope::{Scope, ScopeSet};
use nexl_ast::{Node, NodeKind, Span};

/// A syntax object: an AST node annotated with scope information for hygiene.
///
/// All values flowing through the macro system are syntax objects, not raw
/// s-expressions. A syntax object wraps a datum and carries source location
/// and scope information (spec §7.2).
#[derive(Debug, Clone, PartialEq)]
pub struct SyntaxObj {
    /// The underlying AST node.
    pub node: Node,
    /// The set of scopes active at this syntax position.
    pub scopes: ScopeSet,
}

impl SyntaxObj {
    /// Construct a new syntax object from a node and scope set.
    pub fn new(node: Node, scopes: ScopeSet) -> Self {
        Self { node, scopes }
    }

    /// Extract the underlying AST node (the "datum").
    pub fn datum(&self) -> &Node {
        &self.node
    }

    /// The source span of the underlying node.
    pub fn span(&self) -> Span {
        self.node.span
    }

    /// Wrap a plain node with scopes borrowed from a context syntax object.
    ///
    /// This is the `datum->syntax` operation from the spec (§7.6). Used for
    /// intentional hygiene breaking — e.g. anaphoric macros that introduce
    /// names visible at the call site.
    pub fn datum_to_syntax(ctx: &SyntaxObj, node: Node) -> SyntaxObj {
        SyntaxObj::new(node, ctx.scopes.clone())
    }

    /// Add a scope to this syntax object and all nested children.
    ///
    /// This is step 1 of the hygiene algorithm (spec §7.6): the expander adds
    /// a fresh macro-introduction scope to the entire input syntax.
    pub fn add_scope_deep(&self, scope: Scope) -> SyntaxObj {
        self.map_scopes_deep(|ss| ss.add(scope))
    }

    /// Remove a scope from this syntax object and all nested children.
    pub fn remove_scope_deep(&self, scope: Scope) -> SyntaxObj {
        self.map_scopes_deep(|ss| ss.remove(scope))
    }

    /// Flip (toggle) a scope on this syntax object and all nested children.
    ///
    /// This is step 3 of the hygiene algorithm (spec §7.6): after a macro
    /// transformer runs, the expander flips the introduction scope in the
    /// result. Identifiers the macro introduced (from templates) keep it;
    /// identifiers from user code (passed through `~`) lose it.
    pub fn flip_scope_deep(&self, scope: Scope) -> SyntaxObj {
        self.map_scopes_deep(|ss| ss.flip(scope))
    }

    /// Apply a scope-set mutation to this syntax object and all nested children.
    fn map_scopes_deep<F>(&self, f: F) -> SyntaxObj
    where
        F: Fn(&mut ScopeSet) + Copy,
    {
        let mut scopes = self.scopes.clone();
        f(&mut scopes);
        SyntaxObj {
            node: map_node_children(&self.node, |child| {
                SyntaxObj::new(child.clone(), self.scopes.clone())
                    .map_scopes_deep(f)
                    .node
            }),
            scopes,
        }
    }
}

/// Apply a function to the immediate children of a node, returning a new node
/// with the same kind but transformed children.
fn map_node_children<F>(node: &Node, mut f: F) -> Node
where
    F: FnMut(&Node) -> Node,
{
    let kind = match &node.kind {
        NodeKind::Atom(_) => node.kind.clone(),
        NodeKind::List(children) => NodeKind::List(children.iter().map(&mut f).collect()),
        NodeKind::Vector(children) => NodeKind::Vector(children.iter().map(&mut f).collect()),
        NodeKind::Map(pairs) => {
            NodeKind::Map(pairs.iter().map(|(k, v)| (f(k), f(v))).collect())
        }
        NodeKind::Set(children) => NodeKind::Set(children.iter().map(&mut f).collect()),
        NodeKind::Quote(inner) => NodeKind::Quote(Box::new(f(inner))),
        NodeKind::Deref(inner) => NodeKind::Deref(Box::new(f(inner))),
        NodeKind::Discard(inner) => NodeKind::Discard(Box::new(f(inner))),
        NodeKind::Quasiquote(inner) => NodeKind::Quasiquote(Box::new(f(inner))),
        NodeKind::Unquote(inner) => NodeKind::Unquote(Box::new(f(inner))),
        NodeKind::UnquoteSplice(inner) => NodeKind::UnquoteSplice(Box::new(f(inner))),
    };
    Node {
        kind,
        span: node.span,
        leading_comments: node.leading_comments.clone(),
        trailing_comment: node.trailing_comment.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scope::Scope;
    use nexl_ast::{Atom, Node, NodeKind, Span};

    fn sym(name: &str) -> Node {
        Node::atom(
            Atom::Symbol {
                ns: None,
                name: name.to_string(),
            },
            Span::synthetic(),
        )
    }

    #[test]
    fn test_syntax_obj_new_and_datum() {
        let node = sym("x");
        let mut scopes = ScopeSet::new();
        let s = Scope::fresh();
        scopes.add(s);

        let stx = SyntaxObj::new(node.clone(), scopes.clone());

        // datum() returns the underlying node
        assert_eq!(*stx.datum(), node);
        // scopes are preserved
        assert!(stx.scopes.contains(s));
        // span() delegates to node
        assert!(stx.span().is_synthetic());
    }

    fn list_of(children: Vec<Node>) -> Node {
        Node::new(NodeKind::List(children), Span::synthetic())
    }

    #[test]
    fn test_syntax_obj_add_scope_deep_list() {
        // Build: (f x y)
        let node = list_of(vec![sym("f"), sym("x"), sym("y")]);
        let stx = SyntaxObj::new(node, ScopeSet::new());
        let scope = Scope::fresh();

        let result = stx.add_scope_deep(scope);

        // The top-level scope set is updated
        assert!(result.scopes.contains(scope));

        // The children nodes are the same structurally, but the deep
        // operation means if we re-wrap them and check, scopes were
        // applied at each level. Since Node doesn't carry scopes,
        // we verify the structural integrity of the traversal.
        match &result.node.kind {
            NodeKind::List(children) => {
                assert_eq!(children.len(), 3);
                // Children are Nodes (not SyntaxObj), but the recursive
                // traversal visited them. When we move to SyntaxObj trees,
                // children will carry their own scopes.
                assert_eq!(
                    children[0].kind,
                    NodeKind::Atom(Atom::Symbol {
                        ns: None,
                        name: "f".to_string()
                    })
                );
            }
            _ => panic!("expected List"),
        }
    }

    #[test]
    fn test_syntax_obj_flip_scope_deep() {
        let scope = Scope::fresh();

        // Simulate: user code "x" starts with empty scopes.
        // Macro expansion adds the introduction scope.
        let mut user_scopes = ScopeSet::new();
        user_scopes.add(scope);
        let user_stx = SyntaxObj::new(sym("x"), user_scopes);

        // Simulate: template introduces "tmp" with the introduction scope.
        let mut template_scopes = ScopeSet::new();
        template_scopes.add(scope);
        let _template_stx = SyntaxObj::new(sym("tmp"), template_scopes);

        // Step 3: expander flips the introduction scope.
        // User code (received via ~) had scope added in step 1, so flip REMOVES it.
        let flipped_user = user_stx.flip_scope_deep(scope);
        assert!(
            !flipped_user.scopes.contains(scope),
            "user ident should lose the introduction scope after flip"
        );

        // Template-introduced idents: the macro produced "tmp" in its template,
        // which already had the scope. But flip toggles, so if it was present
        // before the macro ran (it wasn't — templates start fresh), it would be
        // removed. The real flow is:
        //   1. Template ident starts with NO introduction scope
        //   2. After flip, it GAINS the scope
        // Let's test that case:
        let fresh_template = SyntaxObj::new(sym("tmp"), ScopeSet::new());
        let flipped_template = fresh_template.flip_scope_deep(scope);
        assert!(
            flipped_template.scopes.contains(scope),
            "template-introduced ident should gain the introduction scope after flip"
        );
    }

    #[test]
    fn test_datum_to_syntax() {
        let scope_a = Scope::fresh();
        let scope_b = Scope::fresh();

        // Context syntax object has scopes {a, b}
        let mut ctx_scopes = ScopeSet::new();
        ctx_scopes.add(scope_a);
        ctx_scopes.add(scope_b);
        let ctx = SyntaxObj::new(sym("context"), ctx_scopes);

        // Wrap a plain node with scopes borrowed from the context
        let raw_node = sym("it");
        let result = SyntaxObj::datum_to_syntax(&ctx, raw_node.clone());

        // The result should have the context's scopes
        assert!(result.scopes.contains(scope_a));
        assert!(result.scopes.contains(scope_b));
        // And wrap the given node
        assert_eq!(*result.datum(), raw_node);
    }

    #[test]
    fn test_syntax_obj_add_scope_deep_atom() {
        let stx = SyntaxObj::new(sym("x"), ScopeSet::new());
        let scope = Scope::fresh();

        let result = stx.add_scope_deep(scope);
        assert!(result.scopes.contains(scope));
    }
}
