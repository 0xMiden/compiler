use std::{env, fs, path::Path};

use cargo_miden::{run, BuildOutput, OutputType};

/// Creates a minimal Cargo workspace at `root` with a single member named `member_name`.
fn write_workspace_root(root: &Path, member_name: &str) {
    let ws_toml = format!(
        r#"[workspace]
resolver = "2"
members = ["{member_name}"]

[workspace.package]
version = "0.1.0"
edition = "2021"
authors = ["Miden Contributors"]
license = "MIT"
repository = "https://example.com/test"
"#
    );
    fs::write(root.join("Cargo.toml"), ws_toml).expect("write workspace Cargo.toml");
}

fn new_project_args(project_name: &str, template: &str) -> Vec<String> {
    let template = if let Ok(templates_path) = std::env::var("TEST_LOCAL_TEMPLATES_PATH") {
        &format!("--template-path={templates_path}/{}", template.trim_start_matches("--"))
    } else {
        template
    };
    ["cargo", "miden", "new", project_name, template]
        .into_iter()
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect()
}

#[test]
fn build_workspace_member_account_project() {
    let _ = env_logger::Builder::from_env("MIDENC_TRACE")
        .is_test(true)
        .format_timestamp(None)
        .try_init();
    // signal integration tests to the cargo-miden code path
    env::set_var("TEST", "1");

    // create temp workspace root
    let restore_dir = env::current_dir().unwrap();
    let ws_root = env::temp_dir().join(format!(
        "cargo_miden_ws_test_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    ));
    if ws_root.exists() {
        fs::remove_dir_all(&ws_root).unwrap();
    }
    fs::create_dir_all(&ws_root).unwrap();
    env::set_current_dir(&ws_root).unwrap();

    // write workspace manifest
    let member_name = "member_account";
    write_workspace_root(&ws_root, member_name);

    // create account project as a workspace member
    let output = run(new_project_args(member_name, "--account").into_iter(), OutputType::Masm)
        .expect("cargo miden new failed")
        .expect("expected NewCommandOutput");
    let project_path = match output {
        cargo_miden::CommandOutput::NewCommandOutput { project_path } => project_path,
        other => panic!("Expected NewCommandOutput, got {other:?}"),
    };
    assert!(project_path.ends_with(member_name));

    // change into the member directory and try to build using cargo-miden
    env::set_current_dir(&project_path).unwrap();
    let output =
        run(["cargo", "miden", "build"].into_iter().map(|s| s.to_string()), OutputType::Masm)
            .unwrap()
            .unwrap()
            .unwrap_build_output();
    assert!(matches!(output, BuildOutput::Masm { .. }));

    // cleanup
    env::set_current_dir(restore_dir).unwrap();
    fs::remove_dir_all(ws_root).unwrap();
}

#[test]
fn build_from_workspace_root_is_rejected() {
    let _ = env_logger::Builder::from_env("MIDENC_TRACE")
        .is_test(true)
        .format_timestamp(None)
        .try_init();
    env::set_var("TEST", "1");

    // create temp workspace root
    let restore_dir = env::current_dir().unwrap();
    let ws_root = env::temp_dir().join(format!(
        "cargo_miden_ws_root_test_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    ));
    if ws_root.exists() {
        fs::remove_dir_all(&ws_root).unwrap();
    }
    fs::create_dir_all(&ws_root).unwrap();
    env::set_current_dir(&ws_root).unwrap();

    // write workspace manifest and scaffold a member
    let member_name = "member_account";
    write_workspace_root(&ws_root, member_name);
    let _ = run(
        ["cargo", "miden", "new", member_name, "--account"]
            .into_iter()
            .map(|s| s.to_string()),
        OutputType::Masm,
    )
    .expect("cargo miden new failed")
    .expect("expected NewCommandOutput");

    // Run cargo miden build at the workspace root without selecting a package
    env::set_current_dir(&ws_root).unwrap();
    let err = run(["cargo", "miden", "build"].into_iter().map(|s| s.to_string()), OutputType::Masm)
        .expect_err("expected workspace root build to be rejected");
    let msg = err.to_string();
    assert!(
        msg.contains("unable to determine package") && msg.contains("member"),
        "unexpected error message: {msg}"
    );

    // cleanup
    env::set_current_dir(restore_dir).unwrap();
    fs::remove_dir_all(ws_root).unwrap();
}
