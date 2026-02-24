//! Minimal Nexl REPL — reads from stdin, prints results to stdout.
//!
//! Each line is parsed as one or more top-level Nexl forms.  Evaluation
//! results are printed using the `Display` representation of `Value`.
//! Errors are printed prefixed with `error:`.  The session ends at EOF.

use std::io::{self, BufRead, Write};

use nexl_eval::{repl::eval_line, stdlib::standard_env};

fn main() {
    let env = standard_env();
    let stdin = io::stdin();
    let stdout = io::stdout();

    loop {
        // Prompt.
        {
            let mut out = stdout.lock();
            out.write_all(b"nexl> ").expect("write prompt");
            out.flush().expect("flush prompt");
        }

        // Read one line; EOF exits cleanly.
        let mut line = String::new();
        match stdin.lock().read_line(&mut line) {
            Ok(0) => {
                println!();
                break;
            }
            Ok(_) => {}
            Err(e) => {
                eprintln!("error reading input: {e}");
                break;
            }
        }

        // Evaluate and print each form on the line.
        for result in eval_line(line.trim_end_matches('\n'), &env) {
            match result {
                Ok(v) => println!("{v}"),
                Err(msg) => println!("error: {msg}"),
            }
        }
    }
}
