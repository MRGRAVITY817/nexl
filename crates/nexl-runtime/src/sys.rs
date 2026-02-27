//! System state accessible to stdlib functions.
//!
//! Stores program arguments passed from the CLI so that `sys/args` can access them.

use std::cell::RefCell;

thread_local! {
    static PROGRAM_ARGS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// Set the program arguments (called by the CLI before evaluation).
pub fn set_program_args(args: Vec<String>) {
    PROGRAM_ARGS.with(|cell| {
        *cell.borrow_mut() = args;
    });
}

/// Get the program arguments.
pub fn get_program_args() -> Vec<String> {
    PROGRAM_ARGS.with(|cell| cell.borrow().clone())
}
