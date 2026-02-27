pub mod sandbox;
pub mod value;
pub use value::{NativeFn, Value};

use std::cell::RefCell;

/// Type for a function that can call any `Value` as a function.
pub type CallValueFn = fn(&Value, &[Value]) -> Result<Value, String>;

thread_local! {
    static CALL_VALUE: RefCell<Option<CallValueFn>> = const { RefCell::new(None) };
}

/// Register the evaluator's `apply_value` so that stdlib closures can call
/// arbitrary Value functions (including Nexl-defined `Function` values).
pub fn register_call_value(f: CallValueFn) {
    CALL_VALUE.with(|cell| {
        *cell.borrow_mut() = Some(f);
    });
}

/// Call a `Value` as a function with the given arguments.
///
/// For `NativeFunction` and `NativeClosure`, dispatches directly.
/// For `Function`, delegates to the registered evaluator callback.
pub fn call_value(callee: &Value, args: &[Value]) -> Result<Value, String> {
    match callee {
        Value::NativeFunction(native) => (native.f)(args),
        Value::NativeClosure { f, .. } => f(args),
        _ => {
            CALL_VALUE.with(|cell| {
                let guard = cell.borrow();
                match *guard {
                    Some(f) => f(callee, args),
                    None => Err("no evaluator registered for calling functions".into()),
                }
            })
        }
    }
}
