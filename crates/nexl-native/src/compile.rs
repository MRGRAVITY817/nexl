//! IR → Cranelift IR translation.
//!
//! Translates an [`nexl_ir::Module`] into native machine code via Cranelift.
//! All Nexl values are represented as `i64` (tagged pointers, see [`crate::value`]).

use std::collections::HashMap;

use cranelift_codegen::ir::types::I64;
use cranelift_codegen::ir::{AbiParam, Function, InstBuilder, Value};
use cranelift_codegen::isa::OwnedTargetIsa;
use cranelift_codegen::settings::{self, Configurable};
use cranelift_codegen::{self as codegen, Context};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
use cranelift_module::{FuncId, Linkage, Module};
use cranelift_object::{ObjectBuilder, ObjectModule};

use crate::value;

/// Errors during native compilation.
#[derive(Debug)]
pub enum CompileError {
    /// Cranelift module-level error.
    Module(Box<cranelift_module::ModuleError>),
    /// Cranelift codegen error.
    Codegen(Box<codegen::CodegenError>),
}

impl From<cranelift_module::ModuleError> for CompileError {
    fn from(e: cranelift_module::ModuleError) -> Self {
        CompileError::Module(Box::new(e))
    }
}

impl From<codegen::CodegenError> for CompileError {
    fn from(e: codegen::CodegenError) -> Self {
        CompileError::Codegen(Box::new(e))
    }
}

impl std::fmt::Display for CompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CompileError::Module(e) => write!(f, "module error: {e}"),
            CompileError::Codegen(e) => write!(f, "codegen error: {e}"),
        }
    }
}

impl std::error::Error for CompileError {}

/// Create a Cranelift target ISA for the current host.
pub fn host_isa() -> OwnedTargetIsa {
    let mut flag_builder = settings::builder();
    flag_builder
        .set("opt_level", "speed")
        .expect("valid setting");
    let isa_builder = cranelift_codegen::isa::lookup(target_lexicon::Triple::host())
        .expect("host target supported");
    isa_builder
        .finish(settings::Flags::new(flag_builder))
        .expect("valid ISA")
}

/// Build a Cranelift signature for an IR function.
///
/// All params and return values are `i64` (tagged pointers).
/// Uses the `tail` calling convention to enable `return_call` (spec §13.6).
fn build_signature(module: &ObjectModule, func_def: &nexl_ir::FuncDef) -> codegen::ir::Signature {
    let mut sig = module.make_signature();
    sig.call_conv = codegen::isa::CallConv::Tail;
    for _ in &func_def.params {
        sig.params.push(AbiParam::new(I64));
    }
    sig.returns.push(AbiParam::new(I64));
    sig
}

/// Native code compiler: translates ANF IR modules to machine code.
pub struct Compiler {
    module: ObjectModule,
    ctx: Context,
    func_builder_ctx: FunctionBuilderContext,
    /// Cranelift FuncId for the runtime allocator (`nexl_alloc(size_bytes: i64) -> i64`).
    alloc_func: Option<FuncId>,
    /// Cranelift FuncId for `nexl_rc_inc(ptr: i64)`.
    /// Used when emitting inc calls at copy sites (future: Perceus insertion pass).
    #[allow(dead_code)]
    rc_inc_func: Option<FuncId>,
    /// Cranelift FuncId for `nexl_rc_dec(ptr: i64)`.
    /// Used when emitting dec calls at drop sites (future: Perceus insertion pass).
    #[allow(dead_code)]
    rc_dec_func: Option<FuncId>,
}

impl Default for Compiler {
    fn default() -> Self {
        Self::new()
    }
}

impl Compiler {
    /// Create a new compiler targeting the host platform.
    pub fn new() -> Self {
        let isa = host_isa();
        let libcall_names: Box<dyn Fn(codegen::ir::LibCall) -> String + Send + Sync> =
            Box::new(|lc| lc.to_string());
        let builder =
            ObjectBuilder::new(isa, "nexl_module", libcall_names).expect("valid object builder");
        let mut module = ObjectModule::new(builder);
        // Declare the runtime allocator: nexl_alloc(size_bytes: i64) -> i64
        let mut alloc_sig = module.make_signature();
        alloc_sig.params.push(AbiParam::new(I64));
        alloc_sig.returns.push(AbiParam::new(I64));
        let alloc_func = module
            .declare_function("nexl_alloc", Linkage::Import, &alloc_sig)
            .expect("declare nexl_alloc");

        // Declare nexl_rc_inc(ptr: i64)
        let mut rc_inc_sig = module.make_signature();
        rc_inc_sig.params.push(AbiParam::new(I64));
        let rc_inc_func = module
            .declare_function("nexl_rc_inc", Linkage::Import, &rc_inc_sig)
            .expect("declare nexl_rc_inc");

        // Declare nexl_rc_dec(ptr: i64)
        let mut rc_dec_sig = module.make_signature();
        rc_dec_sig.params.push(AbiParam::new(I64));
        let rc_dec_func = module
            .declare_function("nexl_rc_dec", Linkage::Import, &rc_dec_sig)
            .expect("declare nexl_rc_dec");

        Self {
            module,
            ctx: Context::new(),
            func_builder_ctx: FunctionBuilderContext::new(),
            alloc_func: Some(alloc_func),
            rc_inc_func: Some(rc_inc_func),
            rc_dec_func: Some(rc_dec_func),
        }
    }

    /// Compile an entire IR module.
    pub fn compile_module(&mut self, ir: &nexl_ir::Module) -> Result<(), CompileError> {
        // First pass: declare all functions so they can reference each other.
        let mut func_ids: HashMap<nexl_ir::FuncId, FuncId> = HashMap::new();
        for func_def in &ir.funcs {
            let sig = build_signature(&self.module, func_def);
            let anon_name = format!("_anon_{}", func_def.id.0);
            let name = func_def.name.as_deref().unwrap_or(&anon_name);
            let id = self.module.declare_function(name, Linkage::Local, &sig)?;
            func_ids.insert(func_def.id, id);
        }

        let alloc_func = self.alloc_func;

        // Second pass: define each function.
        for func_def in &ir.funcs {
            let cl_func_id = func_ids[&func_def.id];
            self.compile_func(func_def, &func_ids, alloc_func)?;
            self.module.define_function(cl_func_id, &mut self.ctx)?;
            self.ctx.clear();
        }

        Ok(())
    }

    /// Compile a single IR function definition into `self.ctx`.
    fn compile_func(
        &mut self,
        func_def: &nexl_ir::FuncDef,
        func_ids: &HashMap<nexl_ir::FuncId, FuncId>,
        alloc_func: Option<FuncId>,
    ) -> Result<(), CompileError> {
        let sig = build_signature(&self.module, func_def);
        self.ctx.func = Function::with_name_signature(codegen::ir::UserFuncName::default(), sig);

        let mut builder = FunctionBuilder::new(&mut self.ctx.func, &mut self.func_builder_ctx);
        let entry_block = builder.create_block();
        builder.append_block_params_for_function_params(entry_block);
        builder.switch_to_block(entry_block);
        builder.seal_block(entry_block);

        // Map IR VarIds → Cranelift Variables.
        let mut var_map: HashMap<nexl_ir::VarId, Variable> = HashMap::new();
        let mut next_var = 0u32;

        // Bind function parameters.
        for (i, &param_id) in func_def.params.iter().enumerate() {
            let var = Variable::from_u32(next_var);
            next_var += 1;
            builder.declare_var(var, I64);
            let param_val = builder.block_params(entry_block)[i];
            builder.def_var(var, param_val);
            var_map.insert(param_id, var);
        }

        // Translate the function body.
        translate_block(
            &func_def.body,
            &mut self.module,
            &mut builder,
            &mut var_map,
            &mut next_var,
            func_ids,
            alloc_func,
        );

        builder.finalize();
        Ok(())
    }

    /// Consume the compiler and produce the object file bytes.
    pub fn finish(self) -> Vec<u8> {
        let product = self.module.finish();
        product.emit().expect("object file emission")
    }
}

// ── Free translation functions (avoid borrow conflicts with FunctionBuilder) ──

/// Translate an ANF block (let-bindings + tail).
#[allow(clippy::too_many_arguments)]
fn translate_block(
    block: &nexl_ir::Block,
    module: &mut ObjectModule,
    builder: &mut FunctionBuilder,
    var_map: &mut HashMap<nexl_ir::VarId, Variable>,
    next_var: &mut u32,
    func_ids: &HashMap<nexl_ir::FuncId, FuncId>,
    alloc_func: Option<FuncId>,
) {
    for bind in &block.binds {
        let val = translate_rhs(&bind.rhs, module, builder, var_map, func_ids, alloc_func);
        let var = Variable::from_u32(*next_var);
        *next_var += 1;
        builder.declare_var(var, I64);
        builder.def_var(var, val);
        var_map.insert(bind.var, var);
    }

    translate_tail(
        &block.tail,
        module,
        builder,
        var_map,
        next_var,
        func_ids,
        alloc_func,
    );
}

/// Translate an atom to a Cranelift value.
fn translate_atom(
    atom: &nexl_ir::Atom,
    builder: &mut FunctionBuilder,
    var_map: &HashMap<nexl_ir::VarId, Variable>,
) -> Value {
    match atom {
        nexl_ir::Atom::Var(id) => builder.use_var(var_map[id]),
        nexl_ir::Atom::Int(n) => {
            let tagged = value::NativeValue::small_int(*n).raw();
            builder.ins().iconst(I64, tagged as i64)
        }
        nexl_ir::Atom::Float(f) => {
            // Store float bits as tagged i64 (unboxed for now).
            let bits = f.to_bits();
            builder.ins().iconst(I64, bits as i64)
        }
        nexl_ir::Atom::Bool(b) => {
            let tagged = value::NativeValue::bool(*b).raw();
            builder.ins().iconst(I64, tagged as i64)
        }
        nexl_ir::Atom::Unit => {
            let tagged = value::NativeValue::unit().raw();
            builder.ins().iconst(I64, tagged as i64)
        }
        nexl_ir::Atom::Str(_s) => {
            // TODO: string constants need a data section.
            builder.ins().iconst(I64, 0)
        }
        nexl_ir::Atom::FuncRef(fid) => {
            // TODO: function references need proper handling.
            builder.ins().iconst(I64, fid.0 as i64)
        }
    }
}

/// Translate a right-hand side computation.
fn translate_rhs(
    rhs: &nexl_ir::Rhs,
    module: &mut ObjectModule,
    builder: &mut FunctionBuilder,
    var_map: &HashMap<nexl_ir::VarId, Variable>,
    func_ids: &HashMap<nexl_ir::FuncId, FuncId>,
    alloc_func: Option<FuncId>,
) -> Value {
    match rhs {
        nexl_ir::Rhs::Atom(atom) => translate_atom(atom, builder, var_map),
        nexl_ir::Rhs::Call { func, args } => {
            let sig = {
                let mut sig = module.make_signature();
                for _ in args {
                    sig.params.push(AbiParam::new(I64));
                }
                sig.returns.push(AbiParam::new(I64));
                sig
            };

            match func {
                nexl_ir::Atom::FuncRef(fid) => {
                    let cl_func_id = func_ids[fid];
                    let func_ref = module.declare_func_in_func(cl_func_id, builder.func);
                    let arg_vals: Vec<Value> = args
                        .iter()
                        .map(|a| translate_atom(a, builder, var_map))
                        .collect();
                    let call = builder.ins().call(func_ref, &arg_vals);
                    builder.inst_results(call)[0]
                }
                _ => {
                    let sig_ref = builder.import_signature(sig);
                    let callee = translate_atom(func, builder, var_map);
                    let arg_vals: Vec<Value> = args
                        .iter()
                        .map(|a| translate_atom(a, builder, var_map))
                        .collect();
                    let call = builder.ins().call_indirect(sig_ref, callee, &arg_vals);
                    builder.inst_results(call)[0]
                }
            }
        }
        nexl_ir::Rhs::MakeClosure { func_id, captures } => {
            let layout = crate::closure::ClosureLayout::new(captures.len());
            let size = layout.size_bytes() as i64;

            // Call nexl_alloc(size) to get a heap pointer.
            let alloc_id = alloc_func.expect("nexl_alloc must be declared for closures");
            let alloc_ref = module.declare_func_in_func(alloc_id, builder.func);
            let size_val = builder.ins().iconst(I64, size);
            let call = builder.ins().call(alloc_ref, &[size_val]);
            let ptr = builder.inst_results(call)[0];

            // Store header.
            let header = value::HeapHeader::new(value::HeapTag::Closure, layout.field_count());
            let header_val = builder.ins().iconst(I64, header.raw() as i64);
            builder
                .ins()
                .store(codegen::ir::MemFlags::new(), header_val, ptr, 0);

            // Initialize refcount to 1 (Perceus: freshly allocated = unique).
            let rc_val = builder.ins().iconst(I64, crate::rc::INITIAL_RC);
            builder.ins().store(
                codegen::ir::MemFlags::new(),
                rc_val,
                ptr,
                crate::rc::RC_OFFSET,
            );

            // Store code pointer (func_id as integer placeholder).
            let code_ptr = builder.ins().iconst(I64, func_id.0 as i64);
            builder.ins().store(
                codegen::ir::MemFlags::new(),
                code_ptr,
                ptr,
                layout.code_ptr_offset() as i32,
            );

            // Store arity (number of params of the target function — not available here,
            // so store capture count as a proxy; the real arity comes from the func signature).
            let arity_val = builder.ins().iconst(I64, captures.len() as i64);
            builder.ins().store(
                codegen::ir::MemFlags::new(),
                arity_val,
                ptr,
                layout.arity_offset() as i32,
            );

            // Store captured values.
            for (i, (_cap_var, cap_atom)) in captures.iter().enumerate() {
                let cap_val = translate_atom(cap_atom, builder, var_map);
                builder.ins().store(
                    codegen::ir::MemFlags::new(),
                    cap_val,
                    ptr,
                    layout.capture_offset(i) as i32,
                );
            }

            // Return the raw heap pointer (tag = 000, already aligned).
            ptr
        }
        nexl_ir::Rhs::MakeTuple { fields, .. } => {
            // TODO: tuple/ADT construction.
            let _ = fields;
            builder.ins().iconst(I64, 0)
        }
        nexl_ir::Rhs::Project { .. } => {
            // TODO: field projection.
            builder.ins().iconst(I64, 0)
        }
    }
}

/// Translate a tail expression (control flow).
#[allow(clippy::too_many_arguments)]
fn translate_tail(
    tail: &nexl_ir::Tail,
    module: &mut ObjectModule,
    builder: &mut FunctionBuilder,
    var_map: &mut HashMap<nexl_ir::VarId, Variable>,
    next_var: &mut u32,
    func_ids: &HashMap<nexl_ir::FuncId, FuncId>,
    alloc_func: Option<FuncId>,
) {
    match tail {
        nexl_ir::Tail::Return(atom) => {
            let val = translate_atom(atom, builder, var_map);
            builder.ins().return_(&[val]);
        }
        nexl_ir::Tail::If {
            cond,
            then_block,
            else_block,
        } => {
            let cond_val = translate_atom(cond, builder, var_map);
            // Compare tagged bool against `true` encoding (0xA).
            let true_val = builder
                .ins()
                .iconst(I64, value::NativeValue::bool(true).raw() as i64);
            let cmp = builder
                .ins()
                .icmp(codegen::ir::condcodes::IntCC::Equal, cond_val, true_val);

            let then_bb = builder.create_block();
            let else_bb = builder.create_block();

            builder.ins().brif(cmp, then_bb, &[], else_bb, &[]);

            builder.switch_to_block(then_bb);
            builder.seal_block(then_bb);
            translate_block(
                then_block, module, builder, var_map, next_var, func_ids, alloc_func,
            );

            builder.switch_to_block(else_bb);
            builder.seal_block(else_bb);
            translate_block(
                else_block, module, builder, var_map, next_var, func_ids, alloc_func,
            );
        }
        nexl_ir::Tail::TailCall { func, args } => {
            // Use Cranelift's return_call for true tail call optimization (spec §13.6).
            match func {
                nexl_ir::Atom::FuncRef(fid) => {
                    let cl_func_id = func_ids[fid];
                    let func_ref = module.declare_func_in_func(cl_func_id, builder.func);
                    let arg_vals: Vec<Value> = args
                        .iter()
                        .map(|a| translate_atom(a, builder, var_map))
                        .collect();
                    builder.ins().return_call(func_ref, &arg_vals);
                }
                _ => {
                    let sig = {
                        let mut sig = module.make_signature();
                        for _ in args {
                            sig.params.push(AbiParam::new(I64));
                        }
                        sig.returns.push(AbiParam::new(I64));
                        sig
                    };
                    let sig_ref = builder.import_signature(sig);
                    let callee = translate_atom(func, builder, var_map);
                    let arg_vals: Vec<Value> = args
                        .iter()
                        .map(|a| translate_atom(a, builder, var_map))
                        .collect();
                    builder
                        .ins()
                        .return_call_indirect(sig_ref, callee, &arg_vals);
                }
            }
        }
        nexl_ir::Tail::Match { .. } => {
            // TODO: match compilation.
            let zero = builder.ins().iconst(I64, 0);
            builder.ins().return_(&[zero]);
        }
        nexl_ir::Tail::Panic(atom) => {
            let _val = translate_atom(atom, builder, var_map);
            builder
                .ins()
                .trap(codegen::ir::TrapCode::user(0).expect("valid trap code"));
        }
        nexl_ir::Tail::Loop { vars, body } => {
            // Bind initial values.
            let mut loop_vars: Vec<(nexl_ir::VarId, Variable)> = Vec::new();
            for (vid, init_atom) in vars {
                let init_val = translate_atom(init_atom, builder, var_map);
                let var = Variable::from_u32(*next_var);
                *next_var += 1;
                builder.declare_var(var, I64);
                builder.def_var(var, init_val);
                var_map.insert(*vid, var);
                loop_vars.push((*vid, var));
            }

            let loop_bb = builder.create_block();
            builder.ins().jump(loop_bb, &[]);
            builder.switch_to_block(loop_bb);
            // Don't seal yet — Recur will jump back.

            translate_block_for_loop(
                body, module, builder, var_map, next_var, func_ids, loop_bb, &loop_vars, alloc_func,
            );
            builder.seal_block(loop_bb);
        }
        nexl_ir::Tail::Recur { .. } => {
            // Recur outside Loop — shouldn't happen in well-formed IR.
            builder
                .ins()
                .trap(codegen::ir::TrapCode::user(1).expect("valid trap code"));
        }
    }
}

/// Translate a block inside a loop context, handling Recur.
#[allow(clippy::too_many_arguments)]
fn translate_block_for_loop(
    block: &nexl_ir::Block,
    module: &mut ObjectModule,
    builder: &mut FunctionBuilder,
    var_map: &mut HashMap<nexl_ir::VarId, Variable>,
    next_var: &mut u32,
    func_ids: &HashMap<nexl_ir::FuncId, FuncId>,
    loop_bb: codegen::ir::Block,
    loop_vars: &[(nexl_ir::VarId, Variable)],
    alloc_func: Option<FuncId>,
) {
    for bind in &block.binds {
        let val = translate_rhs(&bind.rhs, module, builder, var_map, func_ids, alloc_func);
        let var = Variable::from_u32(*next_var);
        *next_var += 1;
        builder.declare_var(var, I64);
        builder.def_var(var, val);
        var_map.insert(bind.var, var);
    }

    match block.tail.as_ref() {
        nexl_ir::Tail::Recur { args } => {
            // Update loop variables and jump back.
            let new_vals: Vec<Value> = args
                .iter()
                .map(|a| translate_atom(a, builder, var_map))
                .collect();
            for ((_vid, var), val) in loop_vars.iter().zip(new_vals) {
                builder.def_var(*var, val);
            }
            builder.ins().jump(loop_bb, &[]);
        }
        other => {
            translate_tail(
                other, module, builder, var_map, next_var, func_ids, alloc_func,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: make a simple IR module with the given functions.
    fn make_module(name: &str, funcs: Vec<nexl_ir::FuncDef>) -> nexl_ir::Module {
        nexl_ir::Module {
            name: name.to_string(),
            funcs,
        }
    }

    /// Helper: make a function that just returns an atom.
    fn return_func(
        id: u32,
        name: &str,
        params: Vec<nexl_ir::VarId>,
        ret: nexl_ir::Atom,
    ) -> nexl_ir::FuncDef {
        nexl_ir::FuncDef {
            id: nexl_ir::FuncId(id),
            name: Some(name.to_string()),
            params,
            body: nexl_ir::Block {
                binds: vec![],
                tail: Box::new(nexl_ir::Tail::Return(ret)),
            },
        }
    }

    #[test]
    fn test_compiler_creation() {
        let _compiler = Compiler::new();
    }

    #[test]
    fn test_compile_return_int() {
        // fn f0() { return 42 }
        let ir = make_module(
            "test",
            vec![return_func(0, "f0", vec![], nexl_ir::Atom::Int(42))],
        );
        let mut compiler = Compiler::new();
        compiler.compile_module(&ir).expect("compilation succeeds");
    }

    #[test]
    fn test_compile_return_param() {
        // fn f0(v0) { return v0 }
        let ir = make_module(
            "test",
            vec![return_func(
                0,
                "identity",
                vec![nexl_ir::VarId(0)],
                nexl_ir::Atom::Var(nexl_ir::VarId(0)),
            )],
        );
        let mut compiler = Compiler::new();
        compiler.compile_module(&ir).expect("compilation succeeds");
    }

    #[test]
    fn test_compile_let_atom() {
        // fn f0() { let v0 = 42; return v0 }
        let ir = make_module(
            "test",
            vec![nexl_ir::FuncDef {
                id: nexl_ir::FuncId(0),
                name: Some("f0".to_string()),
                params: vec![],
                body: nexl_ir::Block {
                    binds: vec![nexl_ir::LetBind {
                        var: nexl_ir::VarId(0),
                        rhs: nexl_ir::Rhs::Atom(nexl_ir::Atom::Int(42)),
                    }],
                    tail: Box::new(nexl_ir::Tail::Return(nexl_ir::Atom::Var(nexl_ir::VarId(0)))),
                },
            }],
        );
        let mut compiler = Compiler::new();
        compiler.compile_module(&ir).expect("compilation succeeds");
    }

    #[test]
    fn test_compile_call() {
        // fn f0(v0) { return v0 }
        // fn f1() { let v0 = call f0(42); return v0 }
        let ir = make_module(
            "test",
            vec![
                return_func(
                    0,
                    "f0",
                    vec![nexl_ir::VarId(0)],
                    nexl_ir::Atom::Var(nexl_ir::VarId(0)),
                ),
                nexl_ir::FuncDef {
                    id: nexl_ir::FuncId(1),
                    name: Some("f1".to_string()),
                    params: vec![],
                    body: nexl_ir::Block {
                        binds: vec![nexl_ir::LetBind {
                            var: nexl_ir::VarId(0),
                            rhs: nexl_ir::Rhs::Call {
                                func: nexl_ir::Atom::FuncRef(nexl_ir::FuncId(0)),
                                args: vec![nexl_ir::Atom::Int(42)],
                            },
                        }],
                        tail: Box::new(nexl_ir::Tail::Return(nexl_ir::Atom::Var(nexl_ir::VarId(
                            0,
                        )))),
                    },
                },
            ],
        );
        let mut compiler = Compiler::new();
        compiler.compile_module(&ir).expect("compilation succeeds");
    }

    #[test]
    fn test_compile_if_branch() {
        // fn f0() { if true { return 1 } else { return 0 } }
        let ir = make_module(
            "test",
            vec![nexl_ir::FuncDef {
                id: nexl_ir::FuncId(0),
                name: Some("f0".to_string()),
                params: vec![],
                body: nexl_ir::Block {
                    binds: vec![],
                    tail: Box::new(nexl_ir::Tail::If {
                        cond: nexl_ir::Atom::Bool(true),
                        then_block: nexl_ir::Block {
                            binds: vec![],
                            tail: Box::new(nexl_ir::Tail::Return(nexl_ir::Atom::Int(1))),
                        },
                        else_block: nexl_ir::Block {
                            binds: vec![],
                            tail: Box::new(nexl_ir::Tail::Return(nexl_ir::Atom::Int(0))),
                        },
                    }),
                },
            }],
        );
        let mut compiler = Compiler::new();
        compiler.compile_module(&ir).expect("compilation succeeds");
    }

    #[test]
    fn test_compile_tail_call() {
        // fn f0(v0) { tailcall f0(v0) }
        let ir = make_module(
            "test",
            vec![nexl_ir::FuncDef {
                id: nexl_ir::FuncId(0),
                name: Some("f0".to_string()),
                params: vec![nexl_ir::VarId(0)],
                body: nexl_ir::Block {
                    binds: vec![],
                    tail: Box::new(nexl_ir::Tail::TailCall {
                        func: nexl_ir::Atom::FuncRef(nexl_ir::FuncId(0)),
                        args: vec![nexl_ir::Atom::Var(nexl_ir::VarId(0))],
                    }),
                },
            }],
        );
        let mut compiler = Compiler::new();
        compiler.compile_module(&ir).expect("compilation succeeds");
    }

    #[test]
    fn test_compile_make_closure() {
        // fn f0(v0) { return v0 }   ← the closure body
        // fn f1() { let v0 = MakeClosure(f0, [42]); return v0 }
        let ir = make_module(
            "test",
            vec![
                return_func(
                    0,
                    "f0",
                    vec![nexl_ir::VarId(0)],
                    nexl_ir::Atom::Var(nexl_ir::VarId(0)),
                ),
                nexl_ir::FuncDef {
                    id: nexl_ir::FuncId(1),
                    name: Some("f1".to_string()),
                    params: vec![],
                    body: nexl_ir::Block {
                        binds: vec![nexl_ir::LetBind {
                            var: nexl_ir::VarId(0),
                            rhs: nexl_ir::Rhs::MakeClosure {
                                func_id: nexl_ir::FuncId(0),
                                captures: vec![(nexl_ir::VarId(10), nexl_ir::Atom::Int(42))],
                            },
                        }],
                        tail: Box::new(nexl_ir::Tail::Return(nexl_ir::Atom::Var(nexl_ir::VarId(
                            0,
                        )))),
                    },
                },
            ],
        );
        let mut compiler = Compiler::new();
        compiler.compile_module(&ir).expect("compilation succeeds");
    }

    #[test]
    fn test_compile_module_two_funcs() {
        // fn f0() { return 1 }
        // fn f1() { return 2 }
        // Both compile and the module finishes.
        let ir = make_module(
            "test",
            vec![
                return_func(0, "f0", vec![], nexl_ir::Atom::Int(1)),
                return_func(1, "f1", vec![], nexl_ir::Atom::Int(2)),
            ],
        );
        let mut compiler = Compiler::new();
        compiler.compile_module(&ir).expect("compilation succeeds");
        let bytes = compiler.finish();
        assert!(!bytes.is_empty(), "object file should have content");
    }

    #[test]
    fn test_object_file_format() {
        // Verify the emitted bytes start with the correct magic for the host OS.
        let ir = make_module(
            "test",
            vec![return_func(0, "main", vec![], nexl_ir::Atom::Int(0))],
        );
        let mut compiler = Compiler::new();
        compiler.compile_module(&ir).expect("compilation succeeds");
        let bytes = compiler.finish();

        if cfg!(target_os = "macos") {
            // Mach-O magic: 0xFEEDFACF (64-bit) or 0xCFFAEDFE (little-endian)
            assert!(bytes.len() >= 4, "object file too small for Mach-O header");
            let magic = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
            assert!(
                magic == 0xFEED_FACF || magic == 0xCFFA_EDFE,
                "expected Mach-O magic, got {magic:#010X}"
            );
        } else if cfg!(target_os = "linux") {
            // ELF magic: 0x7F 'E' 'L' 'F'
            assert!(bytes.len() >= 4, "object file too small for ELF header");
            assert_eq!(&bytes[0..4], b"\x7FELF", "expected ELF magic");
        }
    }
}
