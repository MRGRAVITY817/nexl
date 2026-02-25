//! Core WASM emitter — walks the ANF IR and produces a WASM binary module.

use std::collections::HashMap;

use nexl_ir::{Atom, Block, FuncDef, Module, Rhs, Tail, VarId};
use wasm_encoder::{
    BlockType, CodeSection, ExportKind, ExportSection, Function, FunctionSection, Instruction,
    TypeSection, ValType,
};

// ── Error type ───────────────────────────────────────────────────────────────

/// An error produced during WASM emission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmitError(pub String);

impl std::fmt::Display for EmitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "emit error: {}", self.0)
    }
}

impl std::error::Error for EmitError {}

// ── WASM type mapping ────────────────────────────────────────────────────────

/// The WASM value type used for all Nexl values in this first-pass backend.
///
/// In M8 we treat every value as `i64` (integers, bools as 0/1, unit as 0).
/// Float-typed values will use `f64` in a later pass once type info is
/// threaded into the IR.
const DEFAULT_VAL: ValType = ValType::I64;

// ── Emitter ──────────────────────────────────────────────────────────────────

/// Stateless WASM emitter.
///
/// Create one with [`Emitter::new`] and call [`Emitter::emit`] to convert an
/// IR [`Module`] into a WASM binary blob.
pub struct Emitter;

impl Default for Emitter {
    fn default() -> Self {
        Emitter
    }
}

impl Emitter {
    /// Create a new emitter.
    pub fn new() -> Self {
        Emitter
    }

    /// Emit a WASM binary module from an IR [`Module`].
    ///
    /// Returns the raw bytes of a valid WebAssembly core module.
    pub fn emit(&self, module: &Module) -> Result<Vec<u8>, EmitError> {
        let mut wasm = wasm_encoder::Module::new();

        if module.funcs.is_empty() {
            return Ok(wasm.finish());
        }

        // ── Type section ────────────────────────────────────────────────────
        // One type entry per function: all params + result are DEFAULT_VAL.
        let mut types = TypeSection::new();
        for func in &module.funcs {
            let params: Vec<ValType> = func.params.iter().map(|_| DEFAULT_VAL).collect();
            types.ty().function(params, [DEFAULT_VAL]);
        }
        wasm.section(&types);

        // ── Function section ─────────────────────────────────────────────────
        let mut funcs_section = FunctionSection::new();
        for (type_idx, _) in module.funcs.iter().enumerate() {
            funcs_section.function(type_idx as u32);
        }
        wasm.section(&funcs_section);

        // ── Export section (named defns become exports) ──────────────────────
        let named: Vec<(usize, &FuncDef)> = module
            .funcs
            .iter()
            .enumerate()
            .filter(|(_, f)| f.name.is_some())
            .collect();

        if !named.is_empty() {
            let mut exports = ExportSection::new();
            for (idx, func) in &named {
                let name = func.name.as_deref().expect("filtered on is_some");
                exports.export(name, ExportKind::Func, *idx as u32);
            }
            wasm.section(&exports);
        }

        // ── Code section ─────────────────────────────────────────────────────
        let mut code = CodeSection::new();
        for func in &module.funcs {
            let wasm_func = emit_func(func)?;
            code.function(&wasm_func);
        }
        wasm.section(&code);

        Ok(wasm.finish())
    }
}

// ── Function emitter ─────────────────────────────────────────────────────────

fn emit_func(func: &FuncDef) -> Result<Function, EmitError> {
    // Assign WASM local indices to every VarId used in this function.
    // Parameters occupy indices 0..params.len(); let-binds follow.
    let mut local_map: HashMap<VarId, u32> = HashMap::new();
    let mut next_local = 0u32;

    for &var in &func.params {
        local_map.insert(var, next_local);
        next_local += 1;
    }
    collect_bind_vars(&func.body, &mut local_map, &mut next_local);

    let num_extra = next_local - func.params.len() as u32;
    let locals = if num_extra > 0 {
        vec![(num_extra, DEFAULT_VAL)]
    } else {
        vec![]
    };

    let mut wasm_func = Function::new(locals);
    emit_block(&func.body, &local_map, &mut wasm_func)?;
    wasm_func.instruction(&Instruction::End);

    Ok(wasm_func)
}

/// Recursively collect all let-bind VarIds so every local has an index
/// before we start emitting instructions.
fn collect_bind_vars(block: &Block, local_map: &mut HashMap<VarId, u32>, next: &mut u32) {
    for bind in &block.binds {
        local_map.entry(bind.var).or_insert_with(|| {
            let idx = *next;
            *next += 1;
            idx
        });
    }
    match block.tail.as_ref() {
        Tail::If { then_block, else_block, .. } => {
            collect_bind_vars(then_block, local_map, next);
            collect_bind_vars(else_block, local_map, next);
        }
        Tail::Match { arms, .. } => {
            for arm in arms {
                collect_bind_vars(&arm.body, local_map, next);
            }
        }
        _ => {}
    }
}

// ── Block / tail / atom emission ─────────────────────────────────────────────

fn emit_block(
    block: &Block,
    local_map: &HashMap<VarId, u32>,
    func: &mut Function,
) -> Result<(), EmitError> {
    for bind in &block.binds {
        emit_rhs(&bind.rhs, local_map, func)?;
        let idx = local_idx(bind.var, local_map)?;
        func.instruction(&Instruction::LocalSet(idx));
    }
    emit_tail(block.tail.as_ref(), local_map, func)
}

fn emit_tail(
    tail: &Tail,
    local_map: &HashMap<VarId, u32>,
    func: &mut Function,
) -> Result<(), EmitError> {
    match tail {
        Tail::Return(atom) => emit_atom(atom, local_map, func),
        Tail::If { cond, then_block, else_block } => {
            emit_atom(cond, local_map, func)?;
            // Condition is Bool (i32 1/0); WASM `if` expects an i32.
            // Since we store bools as i64(1)/i64(0), we need to convert.
            func.instruction(&Instruction::I32WrapI64);
            func.instruction(&Instruction::If(BlockType::Result(DEFAULT_VAL)));
            emit_block(then_block, local_map, func)?;
            func.instruction(&Instruction::Else);
            emit_block(else_block, local_map, func)?;
            func.instruction(&Instruction::End);
            Ok(())
        }
        Tail::Panic(_) => {
            func.instruction(&Instruction::Unreachable);
            Ok(())
        }
        Tail::TailCall { func: f_atom, args } => {
            for arg in args {
                emit_atom(arg, local_map, func)?;
            }
            // For now emit as a regular call (TailCall optimisation comes later).
            emit_call_atom(f_atom, local_map, func)
        }
        Tail::Match { .. } => Err(EmitError(
            "match codegen not yet implemented (requires ADT runtime)".to_string(),
        )),
    }
}

fn emit_rhs(
    rhs: &Rhs,
    local_map: &HashMap<VarId, u32>,
    func: &mut Function,
) -> Result<(), EmitError> {
    match rhs {
        Rhs::Atom(atom) => emit_atom(atom, local_map, func),
        Rhs::Call { func: f_atom, args } => {
            for arg in args {
                emit_atom(arg, local_map, func)?;
            }
            emit_call_atom(f_atom, local_map, func)
        }
        Rhs::MakeClosure { .. } => Err(EmitError(
            "closure codegen not yet implemented".to_string(),
        )),
        Rhs::MakeTuple { .. } => Err(EmitError(
            "ADT codegen not yet implemented".to_string(),
        )),
        Rhs::Project { .. } => Err(EmitError(
            "field projection codegen not yet implemented".to_string(),
        )),
    }
}

fn emit_atom(
    atom: &Atom,
    local_map: &HashMap<VarId, u32>,
    func: &mut Function,
) -> Result<(), EmitError> {
    match atom {
        Atom::Int(n) => {
            func.instruction(&Instruction::I64Const(*n));
            Ok(())
        }
        Atom::Float(f) => {
            func.instruction(&Instruction::F64Const((*f).into()));
            Ok(())
        }
        Atom::Bool(b) => {
            func.instruction(&Instruction::I64Const(if *b { 1 } else { 0 }));
            Ok(())
        }
        Atom::Unit => {
            func.instruction(&Instruction::I64Const(0));
            Ok(())
        }
        Atom::Var(var) => {
            let idx = local_idx(*var, local_map)?;
            func.instruction(&Instruction::LocalGet(idx));
            Ok(())
        }
        Atom::Str(_) => Err(EmitError(
            "string literals not yet supported in codegen".to_string(),
        )),
        Atom::FuncRef(fid) => {
            // Will be used for direct calls; not a push-to-stack operation in WASM.
            Err(EmitError(format!("bare FuncRef({}) cannot be an atom value", fid.0)))
        }
    }
}

/// Emit a direct-call instruction for a callee atom.
///
/// Only [`Atom::FuncRef`] is supported for now (direct calls); closure
/// dispatch via `Atom::Var` requires `call_indirect` and is deferred.
fn emit_call_atom(
    f_atom: &Atom,
    _local_map: &HashMap<VarId, u32>,
    func: &mut Function,
) -> Result<(), EmitError> {
    match f_atom {
        Atom::FuncRef(fid) => {
            func.instruction(&Instruction::Call(fid.0));
            Ok(())
        }
        _ => Err(EmitError(
            "indirect calls through closures not yet implemented".to_string(),
        )),
    }
}

fn local_idx(var: VarId, local_map: &HashMap<VarId, u32>) -> Result<u32, EmitError> {
    local_map.get(&var).copied().ok_or_else(|| {
        EmitError(format!("unresolved local variable VarId({})", var.0))
    })
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use nexl_ir::{Lowerer, Module as IrModule};

    fn lower(src: &str) -> IrModule {
        let nodes = nexl_reader::read(src, meta::FileId::SYNTHETIC)
            .expect("parse error in test");
        Lowerer::new("test").lower_module(&nodes).expect("lower error in test")
    }

    fn emit(src: &str) -> Vec<u8> {
        let m = lower(src);
        Emitter::new().emit(&m).expect("emit error in test")
    }

    const WASM_MAGIC: [u8; 4] = [0x00, 0x61, 0x73, 0x6D];
    const WASM_VERSION: [u8; 4] = [0x01, 0x00, 0x00, 0x00];

    // ─── 1. Emitter constructs ────────────────────────────────────────────────
    #[test]
    fn emitter_new() {
        let _ = Emitter::new();
    }

    // ─── 2. Empty module magic ────────────────────────────────────────────────
    #[test]
    fn emit_empty_module_has_magic() {
        let m = IrModule { name: "empty".to_string(), funcs: vec![] };
        let bytes = Emitter::new().emit(&m).unwrap();
        assert!(bytes.len() >= 8);
        assert_eq!(&bytes[..4], &WASM_MAGIC, "WASM magic bytes");
    }

    // ─── 3. Empty module version ─────────────────────────────────────────────
    #[test]
    fn emit_empty_module_has_version() {
        let m = IrModule { name: "empty".to_string(), funcs: vec![] };
        let bytes = Emitter::new().emit(&m).unwrap();
        assert_eq!(&bytes[4..8], &WASM_VERSION, "WASM version bytes");
    }

    // ─── 4. Single literal-return function ───────────────────────────────────
    #[test]
    fn emit_single_literal_func() {
        let bytes = emit("(defn answer [] 42)");
        assert_eq!(&bytes[..4], &WASM_MAGIC);
        // Must have at least the header + some sections
        assert!(bytes.len() > 8, "non-trivial WASM for a function");
    }

    // ─── 5. Single param function ────────────────────────────────────────────
    #[test]
    fn emit_single_param_func() {
        let bytes = emit("(defn id [x] x)");
        assert_eq!(&bytes[..4], &WASM_MAGIC);
        assert!(bytes.len() > 8);
    }

    // ─── 6. Let binding ──────────────────────────────────────────────────────
    #[test]
    fn emit_let_binding() {
        let bytes = emit("(defn f [] (let [x 42] x))");
        assert_eq!(&bytes[..4], &WASM_MAGIC);
        assert!(bytes.len() > 8);
    }

    // ─── 7. Sequential let bindings ──────────────────────────────────────────
    #[test]
    fn emit_sequential_lets() {
        let bytes = emit("(defn f [] (let [x 1 y 2] y))");
        assert_eq!(&bytes[..4], &WASM_MAGIC);
        assert!(bytes.len() > 8);
    }

    // ─── 8. if branch ────────────────────────────────────────────────────────
    #[test]
    fn emit_if_branch() {
        // choose(b): if b then 10 else 20 — exercises Tail::If codegen
        let bytes = emit("(defn choose [b] (if b 10 20))");
        assert_eq!(&bytes[..4], &WASM_MAGIC);
        assert!(bytes.len() > 8);
    }

    // ─── 9. Direct inter-function call ───────────────────────────────────────
    #[test]
    fn emit_direct_call() {
        // double calls identity — exercises Rhs::Call with Atom::FuncRef
        let bytes = emit("(defn identity [x] x)\n(defn double [x] (identity x))");
        assert_eq!(&bytes[..4], &WASM_MAGIC);
        assert!(bytes.len() > 8);
    }

    // ─── 10. Export names appear in bytes ────────────────────────────────────
    #[test]
    fn emit_exports_named_function() {
        let bytes = emit("(defn my-answer [] 42)");
        // The function name should appear literally in the export section.
        let name_bytes = b"my-answer";
        let found = bytes.windows(name_bytes.len()).any(|w| w == name_bytes);
        assert!(found, "export name 'my-answer' not found in WASM bytes");
    }
}
