use std::process::Command;

/// `cargo miden build` forwards its arguments to `midenc`, help included.
///
/// The wrapper parses nothing of its own, so the help a user gets is the compiler's — which is
/// the only place the forwarded options are documented. Two options no wrapper stub ever had
/// stand in for "this is midenc's help": if they are there, the arguments reached the driver.
#[test]
fn build_help_is_the_compilers_help() {
    let output = Command::new(env!("CARGO_BIN_EXE_cargo-miden"))
        .args(["miden", "build", "--help"])
        .output()
        .expect("cargo-miden should run");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "`cargo miden build --help` failed: {}\n{stdout}",
        String::from_utf8_lossy(&output.stderr)
    );
    for option in ["--manifest-path", "--working-dir"] {
        assert!(
            stdout.contains(option),
            "expected midenc's help, which documents `{option}`, but got:\n{stdout}"
        );
    }
}
