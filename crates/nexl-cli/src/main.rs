//! `nexl` — compile a Nexl source file to a WebAssembly binary.
//!
//! Usage: `nexl <input.nexl> [output.wasm]`
//!
//! If no output path is given, the output file is derived from the input
//! by replacing the extension with `.wasm`.

use std::path::PathBuf;
use std::process;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: nexl <input.nexl> [output.wasm]");
        process::exit(1);
    }

    let input_path = PathBuf::from(&args[1]);
    let output_path = if args.len() >= 3 {
        PathBuf::from(&args[2])
    } else {
        input_path.with_extension("wasm")
    };

    let source = match std::fs::read_to_string(&input_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("nexl: cannot read {:?}: {e}", input_path);
            process::exit(1);
        }
    };

    let module_name = input_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("module")
        .to_string();

    // Parse
    let nodes = match nexl_reader::read(&source, meta::FileId::SYNTHETIC) {
        Ok(n) => n,
        Err(e) => {
            eprintln!("nexl: parse error: {e}");
            process::exit(1);
        }
    };

    // Lower to ANF IR
    let ir_module = match nexl_ir::Lowerer::new(&module_name).lower_module(&nodes) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("nexl: lowering error: {e}");
            process::exit(1);
        }
    };

    // Emit WASM
    let wasm_bytes = match nexl_wasm::Emitter::new().emit(&ir_module) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("nexl: codegen error: {e}");
            process::exit(1);
        }
    };

    // Write output
    if let Err(e) = std::fs::write(&output_path, &wasm_bytes) {
        eprintln!("nexl: cannot write {:?}: {e}", output_path);
        process::exit(1);
    }

    println!("nexl: wrote {} bytes to {:?}", wasm_bytes.len(), output_path);
}
