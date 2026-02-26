//! Native code generation for the Nexl compiler via Cranelift.
//!
//! Translates the ANF IR (from [`nexl_ir`]) to native machine code using
//! Cranelift.  Supports x86-64 and aarch64, emitting ELF (Linux) or
//! Mach-O (macOS) object files.
//!
//! Pipeline position: nexl-ir → **nexl-native** → object file → linker → binary.

pub mod closure;
pub mod compile;
pub mod continuation;
pub mod evidence;
pub mod rc;
pub mod reuse;
pub mod value;

#[cfg(test)]
mod tests {
    #[test]
    fn crate_compiles() {
        // Smoke test: importing the Cranelift crates succeeds.
        use cranelift_codegen::ir::types;
        let _ = types::I64;
    }
}
