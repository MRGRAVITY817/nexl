# M1: Tree-Walk Interpreter + Core Forms

## Done

## In Progress

## Todo

### nexl-runtime — Value Representation
- [x] `Value` enum: Int (i64), Float (f64), Bool, Str (Rc<str>), Unit, Char, Keyword, Symbol, Ratio
- [x] `Display` impl (REPL output format for all variants)
- [x] `type_name()` method for error messages
- [x] `PartialEq` (derived)
- [x] `Value::Function` variant — closure representation (Rc<Function>)

### nexl-eval — Tree-Walk Evaluator
- [x] Add `nexl-eval` crate to workspace
- [x] `Env` type — lexical environment (name → Value, parent chain)
- [x] `eval(node: &Node, env: &mut Env) -> Result<Value, EvalError>` signature
- [x] Atom evaluation: Int/Float/Ratio/Bool/Char/Str/Unit literals → Value
- [x] Atom evaluation: Keyword/Symbol lookup → Value
- [x] `def` form — bind name in current env
- [x] `let` form — sequential bindings in a new scope
- [x] `do` form — evaluate forms in sequence, return last
- [x] `if` form — Bool-only conditional (ADR-004); error on non-Bool condition
- [x] `fn` form — anonymous function with closure capture
- [ ] `defn` form — named function (sugar for def + fn)
- [ ] Function application — call a Value::Function with arguments
- [ ] `loop` / `recur` — tail-recursive loop with TCO
- [ ] `var` / `set!` — mutable locals within function scope

### nexl-runtime — Built-in Functions
- [ ] Arithmetic: `+`, `-`, `*`, `/`, `mod` (Int-only and Float-only; ADR-006)
- [ ] Comparison: `=`, `<`, `>`, `<=`, `>=`
- [ ] Logic: `not`, `and`, `or`
- [ ] String: `str` (concatenation/coercion), `count`

### Test Suite
- [x] Unit tests for `Env` (bind, lookup, scoping, parent chain)
- [ ] Unit tests for atom evaluation (each literal type)
- [ ] Unit tests for each core form
- [ ] Unit tests for arithmetic/comparison builtins
- [ ] Integration test: Fibonacci via `loop`/`recur`

### Minimal REPL (stretch)
- [ ] `nexl-eval` binary: stdin → reader → eval → print loop

## Blocked
(none)
