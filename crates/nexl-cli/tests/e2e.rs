//! End-to-end test harness for the Nexl CLI.
//!
//! Each `.nx` file in `tests/fixtures/` is run via the `nexl` binary.
//! If a companion `.expected` file exists, stdout is compared against it.
//! If no `.expected` file exists, the test just verifies exit code 0.
//!
//! Run with: `cargo test -p nexl-cli --test e2e`

use std::path::{Path, PathBuf};
use std::process::Command;

fn nexl_binary() -> PathBuf {
    let mut path = std::env::current_exe()
        .expect("current_exe")
        .parent()
        .expect("parent of test binary")
        .parent()
        .expect("parent of deps dir")
        .to_path_buf();
    path.push("nexl-cli");
    path
}

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
}

fn run_fixture(name: &str) {
    let fixtures = fixtures_dir();
    let input = fixtures.join(format!("{name}.nx"));
    assert!(input.exists(), "fixture not found: {}", input.display());

    let output = Command::new(nexl_binary())
        .arg("run")
        .arg(&input)
        .output()
        .expect("failed to execute nexl");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    let expected_path = fixtures.join(format!("{name}.expected"));
    if expected_path.exists() {
        let expected = std::fs::read_to_string(&expected_path).expect("read .expected");
        assert!(
            output.status.success(),
            "nexl run failed for {name}:\nstdout: {stdout}\nstderr: {stderr}"
        );
        assert_eq!(
            stdout.as_ref(),
            expected.as_str(),
            "output mismatch for {name}:\n--- expected ---\n{expected}\n--- actual ---\n{stdout}\n--- stderr ---\n{stderr}"
        );
    } else {
        assert!(
            output.status.success(),
            "nexl run failed for {name}:\nstdout: {stdout}\nstderr: {stderr}"
        );
    }
}

#[test]
fn all_fixtures_pass() {
    let fixtures = fixtures_dir();
    if !fixtures.exists() {
        panic!("fixtures directory not found: {}", fixtures.display());
    }

    let mut entries: Vec<_> = std::fs::read_dir(&fixtures)
        .expect("read fixtures dir")
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "nx"))
        .collect();

    entries.sort_by_key(|e| e.file_name());

    if entries.is_empty() {
        eprintln!("  no .nx fixtures found, skipping");
        return;
    }

    for entry in entries {
        let name = entry
            .path()
            .file_stem()
            .expect("file stem")
            .to_string_lossy()
            .to_string();
        eprintln!("  running fixture: {name}");
        run_fixture(&name);
    }
}
