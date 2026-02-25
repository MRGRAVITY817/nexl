//! ANF (Administrative Normal Form) Intermediate Representation for the Nexl compiler.
//!
//! # Modules
//! - [`lower`] — lowering pass: reader AST → ANF IR
//!
//! After type inference and effect elaboration, the typed AST is lowered to this IR.
//! ANF ensures every intermediate computation is explicitly named: call arguments
//! must be [`Atom`]s (variables or constants), never nested expressions.
//!
//! Pipeline position: Lowering → **nexl-ir** → Optimization → WASM / Native / Bytecode.

pub mod lower;
pub use lower::{LowerError, Lowerer};

use std::rc::Rc;

// ── Identifiers ─────────────────────────────────────────────────────────────

/// A unique variable identifier in the ANF IR.
///
/// Every intermediate result bound by a [`LetBind`] gets a fresh `VarId`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VarId(pub u32);

/// A unique function identifier in the IR module.
///
/// Both top-level `defn` definitions and lambda-lifted closures get a `FuncId`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FuncId(pub u32);

/// Monotonically increasing generator for [`VarId`]s.
///
/// One `VarGen` is typically held per function during lowering.
#[derive(Debug, Default)]
pub struct VarGen(u32);

impl VarGen {
    /// Create a new generator starting at zero.
    pub fn new() -> Self {
        VarGen(0)
    }

    /// Return the next fresh [`VarId`] and advance the counter.
    pub fn fresh(&mut self) -> VarId {
        let id = VarId(self.0);
        self.0 += 1;
        id
    }
}

// ── Top-level module ─────────────────────────────────────────────────────────

/// A compiled IR module, corresponding to one Nexl source module.
///
/// Contains all lambda-lifted function definitions (including closures that
/// have been lifted to top-level functions with explicit environment parameters).
#[derive(Debug, Clone)]
pub struct Module {
    /// Module name (matches the Nexl `module` declaration).
    pub name: String,
    /// All function definitions in this module.
    pub funcs: Vec<FuncDef>,
}

/// A single function definition in the IR.
///
/// Both `defn` forms and lambda-lifted closures become `FuncDef`s.
/// Closures capture their environment as extra parameters prepended to `params`.
#[derive(Debug, Clone)]
pub struct FuncDef {
    /// Unique identifier for this function within the module.
    pub id: FuncId,
    /// Human-readable name for debug output (absent for anonymous closures).
    pub name: Option<String>,
    /// Parameter variable IDs in declaration order.
    pub params: Vec<VarId>,
    /// The function body in ANF block form.
    pub body: Block,
}

// ── ANF blocks ───────────────────────────────────────────────────────────────

/// An ANF block: a sequence of [`LetBind`]s followed by a [`Tail`] expression.
///
/// Every value computed in the block is explicitly named. The `Tail` determines
/// what happens at the end (return, branch, tail-call, …).
#[derive(Debug, Clone)]
pub struct Block {
    /// Ordered list of let-bindings that produce intermediate values.
    pub binds: Vec<LetBind>,
    /// The final action of this block.
    pub tail: Box<Tail>,
}

/// A single `let var = rhs` binding inside a [`Block`].
#[derive(Debug, Clone)]
pub struct LetBind {
    /// The variable being bound.
    pub var: VarId,
    /// The right-hand side computation.
    pub rhs: Rhs,
}

// ── Right-hand sides ─────────────────────────────────────────────────────────

/// The right-hand side of a [`LetBind`]: the computation that produces a value.
///
/// In ANF, only let-bindings may contain non-trivial computations.
/// All arguments to `Call` must be [`Atom`]s — never nested calls.
#[derive(Debug, Clone)]
pub enum Rhs {
    /// A trivial atomic value (no computation needed).
    Atom(Atom),

    /// A function application: `func(args...)`.
    ///
    /// `func` and every element of `args` must be [`Atom`]s.
    Call { func: Atom, args: Vec<Atom> },

    /// Create a closure: a code pointer (`func_id`) plus captured variables.
    ///
    /// `captures` maps each capture slot's parameter [`VarId`] to the [`Atom`]
    /// that provides the captured value at the closure-creation site.
    MakeClosure {
        func_id: FuncId,
        captures: Vec<(VarId, Atom)>,
    },

    /// Construct an ADT value: `Ctor(fields...)`.
    MakeTuple { ctor: String, fields: Vec<Atom> },

    /// Project one field out of an ADT or tuple: `base.index`.
    Project { base: Atom, index: usize },
}

// ── Atoms ────────────────────────────────────────────────────────────────────

/// A trivial, side-effect-free ANF value that is safe to duplicate.
///
/// Atoms are the only valid arguments to [`Rhs::Call`] and [`Tail::TailCall`].
/// Complex sub-expressions must be lifted to [`LetBind`]s first.
#[derive(Debug, Clone)]
pub enum Atom {
    /// A variable reference (must be in scope).
    Var(VarId),
    /// A signed 64-bit integer constant.
    Int(i64),
    /// A 64-bit float constant.
    Float(f64),
    /// A boolean constant.
    Bool(bool),
    /// The unit value (ADR-001).
    Unit,
    /// An interned string constant.
    Str(Rc<str>),
    /// A reference to a top-level function by ID.
    FuncRef(FuncId),
}

impl std::fmt::Display for Atom {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Atom::Var(VarId(n)) => write!(f, "%{n}"),
            Atom::Int(n) => write!(f, "{n}"),
            Atom::Float(n) => write!(f, "{n}"),
            Atom::Bool(b) => write!(f, "{b}"),
            Atom::Unit => write!(f, "unit"),
            Atom::Str(s) => write!(f, "\"{s}\""),
            Atom::FuncRef(FuncId(n)) => write!(f, "@fn{n}"),
        }
    }
}

// ── Tail expressions ─────────────────────────────────────────────────────────

/// The final action of a [`Block`]; determines control flow.
#[derive(Debug, Clone)]
pub enum Tail {
    /// Return `atom` to the caller.
    Return(Atom),

    /// Conditional branch on a boolean atom.
    If {
        cond: Atom,
        then_block: Block,
        else_block: Block,
    },

    /// Tail-position call enabling TCO.
    ///
    /// `func` is an [`Atom`] (variable or `FuncRef`).
    TailCall { func: Atom, args: Vec<Atom> },

    /// Decision-tree match on an ADT scrutinee.
    Match {
        scrutinee: Atom,
        arms: Vec<MatchArm>,
    },

    /// Abort the program with a message atom (from `panic` or failed `assert!`).
    Panic(Atom),
}

/// One arm of a [`Tail::Match`] decision tree.
#[derive(Debug, Clone)]
pub struct MatchArm {
    /// The constructor name being matched (e.g. `"Some"`, `"None"`).
    pub ctor: String,
    /// Variable IDs to bind the constructor's fields to.
    pub binds: Vec<VarId>,
    /// The body to execute when this arm is taken.
    pub body: Block,
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ─── 1. VarId equality ────────────────────────────────────────────────────
    #[test]
    fn ir_var_id_equality() {
        assert_eq!(VarId(0), VarId(0));
        assert_ne!(VarId(0), VarId(1));
    }

    // ─── 2. VarGen monotonic ─────────────────────────────────────────────────
    #[test]
    fn ir_var_gen_monotonic() {
        let mut vg = VarGen::new();
        assert_eq!(vg.fresh(), VarId(0));
        assert_eq!(vg.fresh(), VarId(1));
        assert_eq!(vg.fresh(), VarId(2));
    }

    // ─── 3. Module stores name ───────────────────────────────────────────────
    #[test]
    fn ir_module_stores_name() {
        let m = Module {
            name: "my_module".to_string(),
            funcs: vec![],
        };
        assert_eq!(m.name, "my_module");
    }

    // ─── 4. Atom::Int display ────────────────────────────────────────────────
    #[test]
    fn ir_atom_int_display() {
        assert_eq!(Atom::Int(42).to_string(), "42");
        assert_eq!(Atom::Int(-7).to_string(), "-7");
    }

    // ─── 5. Atom::Float display ──────────────────────────────────────────────
    #[test]
    fn ir_atom_float_display() {
        assert_eq!(Atom::Float(3.14).to_string(), "3.14");
    }

    // ─── 6. Atom::Bool display ───────────────────────────────────────────────
    #[test]
    fn ir_atom_bool_display() {
        assert_eq!(Atom::Bool(true).to_string(), "true");
        assert_eq!(Atom::Bool(false).to_string(), "false");
    }

    // ─── 7. Atom::Unit display ───────────────────────────────────────────────
    #[test]
    fn ir_atom_unit_display() {
        assert_eq!(Atom::Unit.to_string(), "unit");
    }

    // ─── 8. Atom::Str display ────────────────────────────────────────────────
    #[test]
    fn ir_atom_str_display() {
        assert_eq!(Atom::Str(Rc::from("hi")).to_string(), "\"hi\"");
    }

    // ─── 9. Atom::Var display ────────────────────────────────────────────────
    #[test]
    fn ir_atom_var_display() {
        assert_eq!(Atom::Var(VarId(5)).to_string(), "%5");
    }

    // ─── 10. Rhs::Call stores args ───────────────────────────────────────────
    #[test]
    fn ir_rhs_call_stores_args() {
        let rhs = Rhs::Call {
            func: Atom::Var(VarId(0)),
            args: vec![Atom::Int(1), Atom::Int(2)],
        };
        let Rhs::Call { args, .. } = rhs else {
            panic!("expected Call")
        };
        assert_eq!(args.len(), 2);
    }

    // ─── 11. Rhs::MakeClosure stores captures ────────────────────────────────
    #[test]
    fn ir_rhs_make_closure_captures() {
        let rhs = Rhs::MakeClosure {
            func_id: FuncId(3),
            captures: vec![
                (VarId(10), Atom::Int(99)),
                (VarId(11), Atom::Bool(true)),
            ],
        };
        let Rhs::MakeClosure { func_id, captures } = rhs else {
            panic!("expected MakeClosure")
        };
        assert_eq!(func_id, FuncId(3));
        assert_eq!(captures.len(), 2);
    }

    // ─── 12. Block binds count ───────────────────────────────────────────────
    #[test]
    fn ir_block_binds_count() {
        let block = Block {
            binds: vec![
                LetBind { var: VarId(0), rhs: Rhs::Atom(Atom::Int(1)) },
                LetBind { var: VarId(1), rhs: Rhs::Atom(Atom::Int(2)) },
            ],
            tail: Box::new(Tail::Return(Atom::Var(VarId(1)))),
        };
        assert_eq!(block.binds.len(), 2);
    }

    // ─── 13. Tail::If stores branches ────────────────────────────────────────
    #[test]
    fn ir_tail_if_stores_branches() {
        let make_return = |n: i64| Block {
            binds: vec![],
            tail: Box::new(Tail::Return(Atom::Int(n))),
        };
        let tail = Tail::If {
            cond: Atom::Bool(true),
            then_block: make_return(1),
            else_block: make_return(0),
        };
        let Tail::If { cond, then_block, else_block } = tail else {
            panic!("expected If")
        };
        assert_eq!(cond.to_string(), "true");
        assert!(matches!(*then_block.tail, Tail::Return(Atom::Int(1))));
        assert!(matches!(*else_block.tail, Tail::Return(Atom::Int(0))));
    }

    // ─── 14. Tail::Match arms count ──────────────────────────────────────────
    #[test]
    fn ir_tail_match_arms_count() {
        let make_arm = |ctor: &str| MatchArm {
            ctor: ctor.to_string(),
            binds: vec![],
            body: Block {
                binds: vec![],
                tail: Box::new(Tail::Return(Atom::Unit)),
            },
        };
        let tail = Tail::Match {
            scrutinee: Atom::Var(VarId(0)),
            arms: vec![make_arm("Some"), make_arm("None")],
        };
        let Tail::Match { arms, .. } = tail else {
            panic!("expected Match")
        };
        assert_eq!(arms.len(), 2);
    }

    // ─── 15. FuncDef params count ────────────────────────────────────────────
    #[test]
    fn ir_funcdef_params_count() {
        let func = FuncDef {
            id: FuncId(0),
            name: Some("add".to_string()),
            params: vec![VarId(0), VarId(1)],
            body: Block {
                binds: vec![],
                tail: Box::new(Tail::Return(Atom::Unit)),
            },
        };
        assert_eq!(func.params.len(), 2);
        assert_eq!(func.name.as_deref(), Some("add"));
    }

    // ─── 16. MatchArm ctor name ──────────────────────────────────────────────
    #[test]
    fn ir_match_arm_ctor_name() {
        let arm = MatchArm {
            ctor: "Ok".to_string(),
            binds: vec![VarId(7)],
            body: Block {
                binds: vec![],
                tail: Box::new(Tail::Return(Atom::Var(VarId(7)))),
            },
        };
        assert_eq!(arm.ctor, "Ok");
        assert_eq!(arm.binds.len(), 1);
    }
}
