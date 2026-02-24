//! Minimal REPL logic: parse and evaluate one line of Nexl source.

use std::rc::Rc;

use nexl_runtime::Value;

use crate::Env;

/// Parse `src` as a sequence of Nexl forms and evaluate each one in `env`.
///
/// Returns one entry per top-level form. Parse errors produce a single
/// `Err` entry covering the whole line; eval errors produce an `Err` for
/// the failing form only.
///
/// Blank lines and comment-only lines produce an empty `Vec`.
pub fn eval_line(src: &str, env: &Rc<Env>) -> Vec<Result<Value, String>> {
    let nodes = match nexl_reader::read(src, meta::FileId::SYNTHETIC) {
        Ok(nodes) => nodes,
        Err(diag) => return vec![Err(diag.to_string())],
    };

    nodes
        .iter()
        .map(|node| crate::eval::eval(node, env).map_err(|e| e.to_string()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexl_runtime::Value;
    use crate::stdlib::standard_env;

    #[test]
    fn eval_line_single_expression() {
        let env = standard_env();
        let results = eval_line("(+ 1 2)", &env);
        assert_eq!(results, vec![Ok(Value::Int(3))]);
    }

    #[test]
    fn eval_line_empty_gives_empty() {
        let env = standard_env();
        assert!(eval_line("", &env).is_empty());
    }

    #[test]
    fn eval_line_comment_only_empty() {
        let env = standard_env();
        assert!(eval_line("; just a comment", &env).is_empty());
    }

    #[test]
    fn eval_line_eval_error_gives_err() {
        let env = standard_env();
        let results = eval_line("(/ 1 0)", &env);
        assert_eq!(results.len(), 1);
        assert!(results[0].is_err());
    }

    #[test]
    fn eval_line_parse_error_gives_err() {
        let env = standard_env();
        let results = eval_line("(+ 1", &env);
        assert_eq!(results.len(), 1);
        assert!(results[0].is_err());
    }

    #[test]
    fn eval_line_multiple_forms_sequential() {
        let env = standard_env();
        let results = eval_line("(def x 5) x", &env);
        assert_eq!(results, vec![Ok(Value::Unit), Ok(Value::Int(5))]);
    }

    #[test]
    fn eval_line_env_persists_across_calls() {
        let env = standard_env();
        let r1 = eval_line("(def y 7)", &env);
        let r2 = eval_line("y", &env);
        assert_eq!(r1, vec![Ok(Value::Unit)]);
        assert_eq!(r2, vec![Ok(Value::Int(7))]);
    }
}
