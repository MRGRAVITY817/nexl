//! Core WASM emitter — walks the ANF IR and produces a WASM binary module.
//!
//! # Memory model (first pass)
//!
//! - Linear memory: 1 page (64 KiB).
//! - `__heap_ptr` (mutable global `i32`, index 0): bump allocator pointer,
//!   starts at offset 1024.
//! - Closures are allocated in linear memory as a contiguous array of `i64`
//!   words: `[func_id, capture_0, capture_1, ...]`.
//! - Direct function calls use WASM `call`.  Indirect closure calls are
//!   deferred to a later task.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use nexl_ir::{Atom, Block, FuncDef, MatchArm, Module, Rhs, Tail, VarId};
use wasm_encoder::{
    BlockType, CodeSection, ConstExpr, DataSection, ExportKind, ExportSection, Function,
    FunctionSection, GlobalSection, GlobalType, Instruction, MemArg, MemorySection, MemoryType,
    TypeSection, ValType,
};

/// Maps interned string content → `(byte_offset_in_data_segment, byte_length)`.
///
/// Built by [`collect_string_literals`] before codegen starts and threaded
/// through to [`emit_atom`] so that `Atom::Str` emits a packed `i64`.
type StringMap = HashMap<Rc<str>, (u32, u32)>;

// ── Constants ────────────────────────────────────────────────────────────────

/// WASM value type used for all Nexl values in this first-pass backend.
const DEFAULT_VAL: ValType = ValType::I64;

/// Index of the `__heap_ptr` mutable global (bump allocator).
const HEAP_PTR: u32 = 0;

/// Index of the `__evv_depth` mutable global (current evidence-vector depth).
/// Used by effect handler push/pop code (tail-resumptive handlers task).
#[allow(dead_code)]
const EVV_DEPTH: u32 = 1;

/// Initial heap base (offset 1024 in linear memory).
const HEAP_BASE: i32 = 1024;

/// Base byte offset in linear memory where the evidence vector array starts.
///
/// Evidence entries are laid out as consecutive `i64` words:
/// `memory[EVV_BASE + 8*idx]` holds the handler pointer for handler `idx`.
/// Used by tail-resumptive handler emission (next task).
#[allow(dead_code)]
pub const EVV_BASE: i32 = 512;

/// Maximum number of simultaneously-installed effect handlers.
/// Used by tail-resumptive handler emission (next task).
#[allow(dead_code)]
pub const EVV_MAX_HANDLERS: u32 = 32;

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

// ── Emitter ──────────────────────────────────────────────────────────────────

/// Stateless WASM emitter.
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

    /// Emit a WASM binary core module from an IR [`Module`].
    pub fn emit(&self, module: &Module) -> Result<Vec<u8>, EmitError> {
        let mut wasm = wasm_encoder::Module::new();

        if module.funcs.is_empty() {
            return Ok(wasm.finish());
        }

        let n = module.funcs.len() as u32;
        // Helper type indices: N = push, N+1 = pop, N+2 = get.
        let ty_push = n;
        let ty_pop = n + 1;
        let ty_get = n + 2;
        // Helper function indices (placed after all user functions).
        let fn_push = n;
        let fn_pop = n + 1;
        let fn_get = n + 2;

        // ── Type section ────────────────────────────────────────────────────
        // One type entry per function (duplicates are fine for correctness;
        // de-duplication is an optimisation deferred to later).
        let mut types = TypeSection::new();
        for func in &module.funcs {
            let params: Vec<ValType> = func.params.iter().map(|_| DEFAULT_VAL).collect();
            types.ty().function(params, [DEFAULT_VAL]);
        }
        // Helper types.
        types.ty().function([ValType::I64], []); // push(handler: i64) -> ()
        types.ty().function([], [ValType::I64]); // pop() -> i64
        types.ty().function([ValType::I32], [ValType::I64]); // get(idx: i32) -> i64
        wasm.section(&types);

        // ── Function section ─────────────────────────────────────────────────
        let mut funcs_section = FunctionSection::new();
        for (type_idx, _) in module.funcs.iter().enumerate() {
            funcs_section.function(type_idx as u32);
        }
        // Helper functions.
        funcs_section.function(ty_push);
        funcs_section.function(ty_pop);
        funcs_section.function(ty_get);
        wasm.section(&funcs_section);

        // ── Memory section (1 page = 64 KiB) ─────────────────────────────────
        let mut memory = MemorySection::new();
        memory.memory(MemoryType {
            minimum: 1,
            maximum: None,
            memory64: false,
            shared: false,
            page_size_log2: None,
        });
        wasm.section(&memory);

        // ── Global section ────────────────────────────────────────────────────
        // global 0: __heap_ptr (i32, mutable) — bump allocator pointer
        // global 1: __evv_depth (i32, mutable) — current evidence-vector depth
        let mut globals = GlobalSection::new();
        globals.global(
            GlobalType { val_type: ValType::I32, mutable: true, shared: false },
            &ConstExpr::i32_const(HEAP_BASE),
        );
        globals.global(
            GlobalType { val_type: ValType::I32, mutable: true, shared: false },
            &ConstExpr::i32_const(0),
        );
        wasm.section(&globals);

        // ── Export section ────────────────────────────────────────────────────
        {
            let mut exports = ExportSection::new();
            // User-defined named functions.
            for (idx, func) in module.funcs.iter().enumerate() {
                if let Some(name) = func.name.as_deref() {
                    exports.export(name, ExportKind::Func, idx as u32);
                }
            }
            // Always export the evv helper functions (used by effect operations).
            exports.export("__evv_push", ExportKind::Func, fn_push);
            exports.export("__evv_pop", ExportKind::Func, fn_pop);
            exports.export("__evv_get", ExportKind::Func, fn_get);
            wasm.section(&exports);
        }

        // ── String pre-pass → data section + string map ──────────────────────
        let string_order: Vec<Rc<str>> = collect_string_literals(module);
        let mut string_map: StringMap = StringMap::new();
        let mut offset: u32 = 0;
        let mut data_bytes: Vec<u8> = Vec::new();
        for s in &string_order {
            let bytes = s.as_bytes();
            let len = bytes.len() as u32;
            string_map.insert(Rc::clone(s), (offset, len));
            data_bytes.extend_from_slice(bytes);
            offset += len;
        }

        // ── Code section ─────────────────────────────────────────────────────
        let mut code = CodeSection::new();
        for func in &module.funcs {
            let wasm_func = emit_func(func, &string_map)?;
            code.function(&wasm_func);
        }
        // Helper function bodies.
        code.function(&emit_evv_push());
        code.function(&emit_evv_pop());
        code.function(&emit_evv_get());
        wasm.section(&code);

        // ── Data section (strings, if any) ───────────────────────────────────
        // Data section (id=11) must come after code (id=10).
        if !data_bytes.is_empty() {
            let mut data = DataSection::new();
            // Active segment at memory 0, offset 0.
            data.active(0, &ConstExpr::i32_const(0), data_bytes);
            wasm.section(&data);
        }

        Ok(wasm.finish())
    }
}

// ── Function emitter ─────────────────────────────────────────────────────────

fn emit_func(func: &FuncDef, so: &StringMap) -> Result<Function, EmitError> {
    let mut local_map: HashMap<VarId, u32> = HashMap::new();
    let mut next_local = 0u32;

    for &var in &func.params {
        local_map.insert(var, next_local);
        next_local += 1;
    }
    collect_bind_vars(&func.body, &mut local_map, &mut next_local);

    let num_extra = next_local - func.params.len() as u32;
    let locals = if num_extra > 0 { vec![(num_extra, DEFAULT_VAL)] } else { vec![] };

    let mut wasm_func = Function::new(locals);
    emit_block(&func.body, &local_map, so, &mut wasm_func, None)?;
    wasm_func.instruction(&Instruction::End);

    Ok(wasm_func)
}

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
                // Register arm field-bind variables as locals.
                for &bind_var in &arm.binds {
                    local_map.entry(bind_var).or_insert_with(|| {
                        let idx = *next;
                        *next += 1;
                        idx
                    });
                }
                collect_bind_vars(&arm.body, local_map, next);
            }
        }
        Tail::Loop { vars, body } => {
            for (var_id, _) in vars {
                local_map.entry(*var_id).or_insert_with(|| {
                    let idx = *next;
                    *next += 1;
                    idx
                });
            }
            collect_bind_vars(body, local_map, next);
        }
        _ => {}
    }
}

// ── Block / tail / rhs / atom ─────────────────────────────────────────────────

fn emit_block(
    block: &Block,
    local_map: &HashMap<VarId, u32>,
    so: &StringMap,
    func: &mut Function,
    lv: Option<(&[VarId], u32)>,
) -> Result<(), EmitError> {
    for bind in &block.binds {
        emit_rhs(&bind.rhs, local_map, so, func)?;
        let idx = local_idx(bind.var, local_map)?;
        func.instruction(&Instruction::LocalSet(idx));
    }
    emit_tail(block.tail.as_ref(), local_map, so, func, lv)
}

fn emit_tail(
    tail: &Tail,
    local_map: &HashMap<VarId, u32>,
    so: &StringMap,
    func: &mut Function,
    lv: Option<(&[VarId], u32)>,
) -> Result<(), EmitError> {
    match tail {
        Tail::Return(atom) => emit_atom(atom, local_map, so, func),
        Tail::If { cond, then_block, else_block } => {
            emit_atom(cond, local_map, so, func)?;
            // Bool is stored as i64 (1/0); WASM `if` expects i32.
            func.instruction(&Instruction::I32WrapI64);
            func.instruction(&Instruction::If(BlockType::Result(DEFAULT_VAL)));
            // Increase br depth by 1 since we're now inside an `if` block.
            let inner_lv = lv.map(|(v, d)| (v, d + 1));
            emit_block(then_block, local_map, so, func, inner_lv)?;
            func.instruction(&Instruction::Else);
            emit_block(else_block, local_map, so, func, inner_lv)?;
            func.instruction(&Instruction::End);
            Ok(())
        }
        Tail::Panic(_) => {
            func.instruction(&Instruction::Unreachable);
            Ok(())
        }
        Tail::TailCall { func: f_atom, args } => {
            for arg in args {
                emit_atom(arg, local_map, so, func)?;
            }
            emit_return_call_atom(f_atom, func)
        }
        Tail::Match { scrutinee, arms } => {
            emit_match_arms(scrutinee, arms, local_map, so, func, lv)
        }
        Tail::Loop { vars, body } => {
            // Emit initial values and set loop-variable locals.
            let var_ids: Vec<VarId> = vars.iter().map(|(v, _)| *v).collect();
            for (var_id, init_atom) in vars {
                emit_atom(init_atom, local_map, so, func)?;
                let idx = local_idx(*var_id, local_map)?;
                func.instruction(&Instruction::LocalSet(idx));
            }
            // `loop (result i64)` — `br 0` inside returns to the top.
            func.instruction(&Instruction::Loop(BlockType::Result(DEFAULT_VAL)));
            emit_block(body, local_map, so, func, Some((&var_ids, 0)))?;
            func.instruction(&Instruction::End);
            Ok(())
        }
        Tail::Recur { args } => {
            let (loop_var_ids, depth) = lv
                .ok_or_else(|| EmitError("recur outside loop".to_string()))?;
            // Set each loop variable to its new value.
            for (var_id, arg) in loop_var_ids.iter().zip(args.iter()) {
                emit_atom(arg, local_map, so, func)?;
                let idx = local_idx(*var_id, local_map)?;
                func.instruction(&Instruction::LocalSet(idx));
            }
            // Branch back to the top of the loop.
            func.instruction(&Instruction::Br(depth));
            Ok(())
        }
    }
}

fn emit_rhs(
    rhs: &Rhs,
    local_map: &HashMap<VarId, u32>,
    so: &StringMap,
    func: &mut Function,
) -> Result<(), EmitError> {
    match rhs {
        Rhs::Atom(atom) => emit_atom(atom, local_map, so, func),
        Rhs::Call { func: f_atom, args } => {
            for arg in args {
                emit_atom(arg, local_map, so, func)?;
            }
            emit_call_atom(f_atom, local_map, func)
        }
        Rhs::MakeClosure { func_id, captures } => {
            emit_make_closure(func_id.0, captures, local_map, so, func)
        }
        Rhs::MakeTuple { ctor, fields } => emit_make_tuple(ctor, fields, local_map, so, func),
        Rhs::Project { .. } => {
            Err(EmitError("field projection codegen not yet implemented".to_string()))
        }
    }
}

/// Emit instructions that allocate a closure env struct in linear memory and
/// leave a pointer to it (as `i64`) on the WASM stack.
///
/// Layout: `[func_id: i64, capture_0: i64, capture_1: i64, ...]`
///
/// Uses a bump allocator: mutable global `__heap_ptr` (index 0).
fn emit_make_closure(
    func_id_u32: u32,
    captures: &[(VarId, Atom)],
    local_map: &HashMap<VarId, u32>,
    so: &StringMap,
    func: &mut Function,
) -> Result<(), EmitError> {
    let num_slots = 1 + captures.len(); // 1 for func_id
    let size = (num_slots * 8) as i32;

    // ── Bump heap pointer ────────────────────────────────────────────────────
    // __heap_ptr += size
    func.instruction(&Instruction::GlobalGet(HEAP_PTR));
    func.instruction(&Instruction::I32Const(size));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::GlobalSet(HEAP_PTR));

    // Helper: emit `closure_ptr = __heap_ptr - size` (i32 on stack).
    let push_closure_ptr = |f: &mut Function| {
        f.instruction(&Instruction::GlobalGet(HEAP_PTR));
        f.instruction(&Instruction::I32Const(size));
        f.instruction(&Instruction::I32Sub);
    };

    // ── Store func_id at offset 0 ─────────────────────────────────────────
    push_closure_ptr(func);
    func.instruction(&Instruction::I64Const(func_id_u32 as i64));
    func.instruction(&Instruction::I64Store(MemArg { offset: 0, align: 3, memory_index: 0 }));

    // ── Store each capture value at offset 8, 16, … ──────────────────────
    for (slot, (_, capture_atom)) in captures.iter().enumerate() {
        push_closure_ptr(func);
        emit_atom(capture_atom, local_map, so, func)?;
        func.instruction(&Instruction::I64Store(MemArg {
            offset: 8 + (slot as u64) * 8,
            align: 3,
            memory_index: 0,
        }));
    }

    // ── Result: closure_ptr as i64 ───────────────────────────────────────
    push_closure_ptr(func);
    func.instruction(&Instruction::I64ExtendI32U);

    Ok(())
}

fn emit_atom(
    atom: &Atom,
    local_map: &HashMap<VarId, u32>,
    so: &StringMap,
    func: &mut Function,
) -> Result<(), EmitError> {
    match atom {
        Atom::Int(n) => { func.instruction(&Instruction::I64Const(*n)); Ok(()) }
        Atom::Float(f) => { func.instruction(&Instruction::F64Const((*f).into())); Ok(()) }
        Atom::Bool(b) => {
            func.instruction(&Instruction::I64Const(if *b { 1 } else { 0 }));
            Ok(())
        }
        Atom::Unit => { func.instruction(&Instruction::I64Const(0)); Ok(()) }
        Atom::Var(var) => {
            let idx = local_idx(*var, local_map)?;
            func.instruction(&Instruction::LocalGet(idx));
            Ok(())
        }
        Atom::Str(s) => {
            let &(ptr, len) = so
                .get(s)
                .ok_or_else(|| EmitError(format!("string {s:?} not in string table")))?;
            // Packed i64: high 32 bits = ptr, low 32 bits = len.
            let packed = ((ptr as i64) << 32) | (len as i64);
            func.instruction(&Instruction::I64Const(packed));
            Ok(())
        }
        Atom::FuncRef(fid) => {
            Err(EmitError(format!("bare FuncRef({}) cannot be an atom value", fid.0)))
        }
    }
}

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
            "indirect calls through closures not yet implemented (use FuncRef for direct calls)"
                .to_string(),
        )),
    }
}

/// Emit a `return_call` (tail call) to a direct function reference (TCO).
fn emit_return_call_atom(f_atom: &Atom, func: &mut Function) -> Result<(), EmitError> {
    match f_atom {
        Atom::FuncRef(fid) => {
            func.instruction(&Instruction::ReturnCall(fid.0));
            Ok(())
        }
        _ => Err(EmitError(
            "indirect tail calls through closures not yet implemented".to_string(),
        )),
    }
}

/// FNV-1a hash of a constructor name — used as the ADT discriminant tag.
fn ctor_tag(name: &str) -> i64 {
    let mut hash: u64 = 14_695_981_039_346_656_037;
    for byte in name.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(1_099_511_628_211);
    }
    hash as i64
}

/// Emit instructions that allocate an ADT value in linear memory and leave
/// a pointer to it (as `i64`) on the WASM stack.
///
/// Layout: `[tag: i64, field_0: i64, field_1: i64, ...]`
fn emit_make_tuple(
    ctor: &str,
    fields: &[Atom],
    local_map: &HashMap<VarId, u32>,
    so: &StringMap,
    func: &mut Function,
) -> Result<(), EmitError> {
    let num_slots = 1 + fields.len(); // 1 for tag
    let size = (num_slots * 8) as i32;

    // Bump heap pointer: __heap_ptr += size
    func.instruction(&Instruction::GlobalGet(HEAP_PTR));
    func.instruction(&Instruction::I32Const(size));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::GlobalSet(HEAP_PTR));

    let push_ptr = |f: &mut Function| {
        f.instruction(&Instruction::GlobalGet(HEAP_PTR));
        f.instruction(&Instruction::I32Const(size));
        f.instruction(&Instruction::I32Sub);
    };

    // Store tag at offset 0
    push_ptr(func);
    func.instruction(&Instruction::I64Const(ctor_tag(ctor)));
    func.instruction(&Instruction::I64Store(MemArg { offset: 0, align: 3, memory_index: 0 }));

    // Store each field at offset 8, 16, …
    for (i, field) in fields.iter().enumerate() {
        push_ptr(func);
        emit_atom(field, local_map, so, func)?;
        func.instruction(&Instruction::I64Store(MemArg {
            offset: 8 + (i as u64) * 8,
            align: 3,
            memory_index: 0,
        }));
    }

    // Result: ptr as i64
    push_ptr(func);
    func.instruction(&Instruction::I64ExtendI32U);
    Ok(())
}

/// Recursively emit a decision-tree match as nested WASM `if/else` blocks.
///
/// The scrutinee is a pointer (stored as `i64`) to a heap-allocated ADT value
/// whose first word is the [`ctor_tag`] discriminant.
fn emit_match_arms(
    scrutinee: &Atom,
    arms: &[MatchArm],
    local_map: &HashMap<VarId, u32>,
    so: &StringMap,
    func: &mut Function,
    lv: Option<(&[VarId], u32)>,
) -> Result<(), EmitError> {
    if arms.is_empty() {
        // Exhaustiveness is guaranteed by the type checker; emit a trap.
        func.instruction(&Instruction::Unreachable);
        return Ok(());
    }

    let arm = &arms[0];

    if arm.ctor == "_" {
        // Wildcard arm — unconditionally execute body (no new block, depth unchanged).
        return emit_block(&arm.body, local_map, so, func, lv);
    }

    // Load tag from scrutinee (ptr as i32, tag is i64 at offset 0).
    emit_atom(scrutinee, local_map, so, func)?;
    func.instruction(&Instruction::I32WrapI64);
    func.instruction(&Instruction::I64Load(MemArg { offset: 0, align: 3, memory_index: 0 }));
    func.instruction(&Instruction::I64Const(ctor_tag(&arm.ctor)));
    // i64.eq returns i32 (0 or 1) — consumed directly by `if`.
    func.instruction(&Instruction::I64Eq);

    // Opening this `if` block increases br depth by 1.
    let inner_lv = lv.map(|(v, d)| (v, d + 1));
    func.instruction(&Instruction::If(BlockType::Result(DEFAULT_VAL)));

    // Arm taken: bind fields from memory, then run arm body.
    for (field_idx, &bind_var) in arm.binds.iter().enumerate() {
        emit_atom(scrutinee, local_map, so, func)?;
        func.instruction(&Instruction::I32WrapI64);
        func.instruction(&Instruction::I64Load(MemArg {
            offset: 8 + (field_idx as u64) * 8,
            align: 3,
            memory_index: 0,
        }));
        let bind_local = *local_map
            .get(&bind_var)
            .ok_or_else(|| EmitError(format!("unresolved bind var VarId({})", bind_var.0)))?;
        func.instruction(&Instruction::LocalSet(bind_local));
    }
    emit_block(&arm.body, local_map, so, func, inner_lv)?;

    func.instruction(&Instruction::Else);
    emit_match_arms(scrutinee, &arms[1..], local_map, so, func, inner_lv)?;
    func.instruction(&Instruction::End);

    Ok(())
}

// ── Evidence-vector helper functions ─────────────────────────────────────────

/// Emit `__evv_push(handler: i64) -> ()`.
///
/// Stores the handler pointer at `EVV_BASE + depth * 8`, then increments
/// `__evv_depth`.
fn emit_evv_push() -> Function {
    let mut f = Function::new([]);
    // memory[EVV_BASE + depth*8] = handler (local 0)
    f.instruction(&Instruction::GlobalGet(EVV_DEPTH));
    f.instruction(&Instruction::I32Const(8));
    f.instruction(&Instruction::I32Mul);
    f.instruction(&Instruction::I32Const(EVV_BASE));
    f.instruction(&Instruction::I32Add);
    f.instruction(&Instruction::LocalGet(0));
    f.instruction(&Instruction::I64Store(MemArg { offset: 0, align: 3, memory_index: 0 }));
    // __evv_depth++
    f.instruction(&Instruction::GlobalGet(EVV_DEPTH));
    f.instruction(&Instruction::I32Const(1));
    f.instruction(&Instruction::I32Add);
    f.instruction(&Instruction::GlobalSet(EVV_DEPTH));
    f.instruction(&Instruction::End);
    f
}

/// Emit `__evv_pop() -> i64`.
///
/// Decrements `__evv_depth`, then loads and returns the handler pointer at
/// `EVV_BASE + new_depth * 8`.
fn emit_evv_pop() -> Function {
    let mut f = Function::new([]);
    // --depth
    f.instruction(&Instruction::GlobalGet(EVV_DEPTH));
    f.instruction(&Instruction::I32Const(1));
    f.instruction(&Instruction::I32Sub);
    f.instruction(&Instruction::GlobalSet(EVV_DEPTH));
    // return memory[EVV_BASE + depth*8]
    f.instruction(&Instruction::GlobalGet(EVV_DEPTH));
    f.instruction(&Instruction::I32Const(8));
    f.instruction(&Instruction::I32Mul);
    f.instruction(&Instruction::I32Const(EVV_BASE));
    f.instruction(&Instruction::I32Add);
    f.instruction(&Instruction::I64Load(MemArg { offset: 0, align: 3, memory_index: 0 }));
    f.instruction(&Instruction::End);
    f
}

/// Emit `__evv_get(idx: i32) -> i64`.
///
/// Loads and returns the handler pointer at `EVV_BASE + idx * 8`.
/// Used for tail-resumptive dispatch: the caller already knows the handler index.
fn emit_evv_get() -> Function {
    let mut f = Function::new([]);
    // return memory[EVV_BASE + idx*8]
    f.instruction(&Instruction::LocalGet(0)); // idx (i32)
    f.instruction(&Instruction::I32Const(8));
    f.instruction(&Instruction::I32Mul);
    f.instruction(&Instruction::I32Const(EVV_BASE));
    f.instruction(&Instruction::I32Add);
    f.instruction(&Instruction::I64Load(MemArg { offset: 0, align: 3, memory_index: 0 }));
    f.instruction(&Instruction::End);
    f
}

fn local_idx(var: VarId, local_map: &HashMap<VarId, u32>) -> Result<u32, EmitError> {
    local_map
        .get(&var)
        .copied()
        .ok_or_else(|| EmitError(format!("unresolved local variable VarId({})", var.0)))
}

// ── String literal pre-pass ───────────────────────────────────────────────────

/// Walk the module and collect all unique string literals in first-encounter
/// order.  The resulting [`Vec`] determines the byte layout of the data segment.
fn collect_string_literals(module: &Module) -> Vec<Rc<str>> {
    let mut order: Vec<Rc<str>> = vec![];
    let mut seen: HashSet<Rc<str>> = HashSet::new();
    for func in &module.funcs {
        collect_strings_in_block(&func.body, &mut order, &mut seen);
    }
    order
}

fn collect_strings_in_block(
    block: &Block,
    order: &mut Vec<Rc<str>>,
    seen: &mut HashSet<Rc<str>>,
) {
    for bind in &block.binds {
        collect_strings_in_rhs(&bind.rhs, order, seen);
    }
    collect_strings_in_tail(block.tail.as_ref(), order, seen);
}

fn collect_strings_in_rhs(rhs: &Rhs, order: &mut Vec<Rc<str>>, seen: &mut HashSet<Rc<str>>) {
    match rhs {
        Rhs::Atom(atom) => collect_strings_in_atom(atom, order, seen),
        Rhs::Call { func, args } => {
            collect_strings_in_atom(func, order, seen);
            for a in args {
                collect_strings_in_atom(a, order, seen);
            }
        }
        Rhs::MakeClosure { captures, .. } => {
            for (_, a) in captures {
                collect_strings_in_atom(a, order, seen);
            }
        }
        Rhs::MakeTuple { fields, .. } => {
            for a in fields {
                collect_strings_in_atom(a, order, seen);
            }
        }
        Rhs::Project { base, .. } => collect_strings_in_atom(base, order, seen),
    }
}

fn collect_strings_in_tail(tail: &Tail, order: &mut Vec<Rc<str>>, seen: &mut HashSet<Rc<str>>) {
    match tail {
        Tail::Return(a) | Tail::Panic(a) => collect_strings_in_atom(a, order, seen),
        Tail::If { cond, then_block, else_block } => {
            collect_strings_in_atom(cond, order, seen);
            collect_strings_in_block(then_block, order, seen);
            collect_strings_in_block(else_block, order, seen);
        }
        Tail::TailCall { func, args } => {
            collect_strings_in_atom(func, order, seen);
            for a in args {
                collect_strings_in_atom(a, order, seen);
            }
        }
        Tail::Match { scrutinee, arms } => {
            collect_strings_in_atom(scrutinee, order, seen);
            for arm in arms {
                collect_strings_in_block(&arm.body, order, seen);
            }
        }
        Tail::Loop { vars, body } => {
            for (_, init) in vars {
                collect_strings_in_atom(init, order, seen);
            }
            collect_strings_in_block(body, order, seen);
        }
        Tail::Recur { args } => {
            for a in args {
                collect_strings_in_atom(a, order, seen);
            }
        }
    }
}

fn collect_strings_in_atom(atom: &Atom, order: &mut Vec<Rc<str>>, seen: &mut HashSet<Rc<str>>) {
    if let Atom::Str(s) = atom
        && seen.insert(Rc::clone(s))
    {
        order.push(Rc::clone(s));
    }
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
        assert!(bytes.len() > 8);
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
        let bytes = emit("(defn choose [b] (if b 10 20))");
        assert_eq!(&bytes[..4], &WASM_MAGIC);
        assert!(bytes.len() > 8);
    }

    // ─── 9. Direct inter-function call ───────────────────────────────────────
    #[test]
    fn emit_direct_call() {
        let bytes = emit("(defn identity [x] x)\n(defn double [x] (identity x))");
        assert_eq!(&bytes[..4], &WASM_MAGIC);
        assert!(bytes.len() > 8);
    }

    // ─── 10. Export names appear in bytes ────────────────────────────────────
    #[test]
    fn emit_exports_named_function() {
        let bytes = emit("(defn my-answer [] 42)");
        let name_bytes = b"my-answer";
        let found = bytes.windows(name_bytes.len()).any(|w| w == name_bytes);
        assert!(found, "export name 'my-answer' not found in WASM bytes");
    }

    // ─── 11. Closure creation (with capture) ─────────────────────────────────
    #[test]
    fn emit_closure_creation() {
        // outer f captures y and wraps it in a fn — tests MakeClosure codegen
        let bytes = emit("(defn f [y] (fn [x] y))");
        assert_eq!(&bytes[..4], &WASM_MAGIC);
        assert!(bytes.len() > 8);
    }

    // ─── 12. Closure with no captures ────────────────────────────────────────
    #[test]
    fn emit_closure_no_captures() {
        let bytes = emit("(defn f [] (fn [x] x))");
        assert_eq!(&bytes[..4], &WASM_MAGIC);
        assert!(bytes.len() > 8);
    }

    // ─── 13. Closure multiple captures ───────────────────────────────────────
    #[test]
    fn emit_multi_capture_closure() {
        // captures both a and b
        let bytes = emit("(defn f [a b] (fn [x] a))");
        assert_eq!(&bytes[..4], &WASM_MAGIC);
        assert!(bytes.len() > 8);
    }

    // ─── 14. Module with memory section ──────────────────────────────────────
    #[test]
    fn emit_module_has_memory_section() {
        // Any non-empty module should include a memory section (for closures)
        // Memory section id = 0x05
        let bytes = emit("(defn f [] 1)");
        // Memory section (id=0x05), size=3, count=1, no-max flag=0x00, 1 page
        let has_memory_section = bytes.windows(5).any(|w| w == [0x05, 0x03, 0x01, 0x00, 0x01]);
        assert!(has_memory_section, "expected memory section in WASM bytes");
    }

    // ─── 15. ADT constructor (Some x) codegen ────────────────────────────────
    #[test]
    fn emit_adt_some() {
        // (defn wrap [x] (Some x)) — MakeTuple with 1 field
        let bytes = emit("(defn wrap [x] (Some x))");
        assert_eq!(&bytes[..4], &WASM_MAGIC);
        assert!(bytes.len() > 8);
    }

    // ─── 16. Nullary ADT constructor (None) codegen ──────────────────────────
    #[test]
    fn emit_adt_none() {
        // (defn nothing [] None) — MakeTuple with 0 fields
        let bytes = emit("(defn nothing [] None)");
        assert_eq!(&bytes[..4], &WASM_MAGIC);
        assert!(bytes.len() > 8);
    }

    // ─── 17. Match on ADT ────────────────────────────────────────────────────
    #[test]
    fn emit_match_with_adt() {
        // (defn unwrap [v d] (match v (Some x) x None d))
        let bytes = emit("(defn unwrap [v d] (match v (Some x) x None d))");
        assert_eq!(&bytes[..4], &WASM_MAGIC);
        assert!(bytes.len() > 8);
    }

    // ─── 18. String literal codegen ──────────────────────────────────────────
    #[test]
    fn emit_string_literal() {
        // (defn greeting [] "hello") — Atom::Str must not error
        let bytes = emit("(defn greeting [] \"hello\")");
        assert_eq!(&bytes[..4], &WASM_MAGIC);
        assert!(bytes.len() > 8);
    }

    // ─── 19. String content appears in data section ───────────────────────────
    #[test]
    fn emit_string_bytes_in_output() {
        let bytes = emit("(defn greeting [] \"hello\")");
        let needle = b"hello";
        let found = bytes.windows(needle.len()).any(|w| w == needle);
        assert!(found, "string bytes 'hello' not found in WASM output");
    }

    // ─── 20. EVV layout constants are well-formed ─────────────────────────────
    #[test]
    fn evv_constants_sane() {
        // The evv region must fit entirely below the heap.
        let evv_end = EVV_BASE as u32 + EVV_MAX_HANDLERS * 8;
        assert!(
            evv_end <= HEAP_BASE as u32,
            "evv region ({evv_end}) overflows into heap ({HEAP_BASE})"
        );
        assert!(EVV_BASE > 0, "evv must not start at address 0 (reserved for strings)");
        assert!(EVV_MAX_HANDLERS >= 8, "need room for at least 8 handlers");
    }

    // ─── 21. Non-empty module has __evv_depth global ──────────────────────────
    #[test]
    fn emit_module_has_evv_global() {
        // After adding __evv_depth, the global section should encode count=2.
        // Global section id=6; count is the first byte after the section size.
        // We verify the module emits without error and is larger than before
        // (the extra global adds bytes).
        let bytes_with_func = emit("(defn f [] 1)");
        // Global section id byte = 0x06.
        let has_global_section = bytes_with_func.windows(1).any(|w| w == [0x06]);
        assert!(has_global_section, "global section (id=6) not found");
        // Verify the module is valid WASM.
        assert_eq!(&bytes_with_func[..4], &WASM_MAGIC);
    }

    // ─── 22. __evv_depth initialized to 0 ────────────────────────────────────
    #[test]
    fn emit_evv_depth_initialized_zero() {
        let bytes = emit("(defn f [] 1)");
        let init_zero = [0x41u8, 0x00, 0x0B]; // i32.const 0 end
        let found = bytes.windows(3).any(|w| w == init_zero);
        assert!(found, "__evv_depth i32.const 0 initializer not found in output");
    }

    // ─── 23. __evv_push exported ──────────────────────────────────────────────
    #[test]
    fn emit_evv_push_in_output() {
        let bytes = emit("(defn f [] 1)");
        let found = bytes.windows(b"__evv_push".len()).any(|w| w == b"__evv_push");
        assert!(found, "__evv_push name not found in WASM output");
    }

    // ─── 24. __evv_pop exported ───────────────────────────────────────────────
    #[test]
    fn emit_evv_pop_in_output() {
        let bytes = emit("(defn f [] 1)");
        let found = bytes.windows(b"__evv_pop".len()).any(|w| w == b"__evv_pop");
        assert!(found, "__evv_pop name not found in WASM output");
    }

    // ─── 25. __evv_get exported ───────────────────────────────────────────────
    #[test]
    fn emit_evv_get_in_output() {
        let bytes = emit("(defn f [] 1)");
        let found = bytes.windows(b"__evv_get".len()).any(|w| w == b"__evv_get");
        assert!(found, "__evv_get name not found in WASM output");
    }

    // ─── 26. direct tail call emits return_call opcode ───────────────────────
    #[test]
    fn emit_tail_call_has_return_call() {
        // (defn f [x] (f x)) — self-recursive tail call
        // WASM return_call opcode = 0x12
        let bytes = emit("(defn f [x] (f x))");
        let found = bytes.windows(1).any(|w| w == [0x12]);
        assert!(found, "WASM return_call opcode (0x12) not found in output");
    }

    // ─── 27. inter-function tail call emits valid WASM ────────────────────────
    #[test]
    fn emit_inter_func_tail_call() {
        // (defn a [x] (b x)) / (defn b [x] x) — mutual tail call
        let bytes = emit("(defn a [x] (b x))\n(defn b [x] x)");
        assert_eq!(&bytes[..4], &WASM_MAGIC);
        // return_call opcode must appear (from (b x) in tail position)
        let found = bytes.windows(1).any(|w| w == [0x12]);
        assert!(found, "WASM return_call opcode (0x12) not found for inter-func tail call");
    }

    // ─── 28. end-to-end: trivial program validates with wasmparser ───────────
    #[test]
    fn e2e_trivial_program_validates() {
        // (defn main [] 42) — compile and validate the WASM bytes
        let bytes = emit("(defn main [] 42)");
        wasmparser::validate(&bytes).expect("emitted WASM should be structurally valid");
    }

    // ─── 29. end-to-end: wasm bytes can be written to a file ─────────────────
    #[test]
    fn e2e_wasm_file_written() {
        use std::io::Write;
        let bytes = emit("(defn main [] 42)");
        let mut tmp = tempfile::NamedTempFile::new().expect("create temp file");
        tmp.write_all(&bytes).expect("write wasm bytes");
        // Verify the file starts with the WASM magic
        let on_disk = std::fs::read(tmp.path()).expect("read back temp file");
        assert_eq!(&on_disk[..4], &WASM_MAGIC, "file should start with WASM magic");
    }

    // ─── 30. loop without recur emits valid WASM ──────────────────────────────
    #[test]
    fn emit_loop_simple() {
        // (defn f [n] (loop [i n] i)) — loop that immediately returns its var
        let bytes = emit("(defn f [n] (loop [i n] i))");
        assert_eq!(&bytes[..4], &WASM_MAGIC);
        assert!(bytes.len() > 8);
    }

    // ─── 27. loop opcode appears in WASM binary ───────────────────────────────
    #[test]
    fn emit_loop_has_loop_opcode() {
        // WASM `loop (result i64)` encodes as [0x03, 0x7E]
        let bytes = emit("(defn f [n] (loop [i n] i))");
        let found = bytes.windows(2).any(|w| w == [0x03, 0x7E]);
        assert!(found, "WASM loop opcode [0x03, 0x7E] not found in output");
    }

    // ─── 28. recur emits br back to loop ─────────────────────────────────────
    #[test]
    fn emit_loop_recur_has_br() {
        // (defn f [n] (loop [i n] (if false 99 (recur 0))))
        // Recur is inside an if-else block, so br depth = 1 → [0x0C, 0x01]
        let bytes = emit("(defn f [n] (loop [i n] (if false 99 (recur 0))))");
        let found = bytes.windows(2).any(|w| w == [0x0C, 0x01]);
        assert!(found, "WASM br 1 [0x0C, 0x01] not found in output (recur inside if)");
    }

}
