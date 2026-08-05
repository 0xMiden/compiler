//! Builds a new project from every template in this repository and verifies
//! that `cargo-miden` (from the `next` branch) compiles it without errors for
//! both the `dev` and `release` profiles.

use template_tests::build_new_project_from_template;

#[test]
fn account() {
    build_new_project_from_template("account");
}

#[test]
fn auth_component() {
    build_new_project_from_template("auth-component");
}

#[test]
fn note() {
    build_new_project_from_template("note");
}

#[test]
fn tx_script() {
    build_new_project_from_template("tx-script");
}

#[test]
fn program() {
    build_new_project_from_template("program");
}
