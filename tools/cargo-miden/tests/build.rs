use std::{env, fs};

use cargo_miden::{OutputType, run};
use miden_mast_package::Package;
use midenc_session::miden_assembly::utils::Deserializable;

fn new_project_args(project_name: &str, template: &str) -> Vec<String> {
    let template = if template.is_empty() {
        if let Ok(project_template_path) = std::env::var("TEST_LOCAL_PROJECT_TEMPLATE_PATH") {
            &format!("--template-path={project_template_path}")
        } else {
            template
        }
    } else if let Ok(templates_path) = std::env::var("TEST_LOCAL_TEMPLATES_PATH") {
        &format!("--template-path={templates_path}/{}", template.strip_prefix("--").unwrap())
    } else {
        template
    };
    let args: Vec<String> = ["cargo", "miden", "new", project_name, template]
        .into_iter()
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect();
    args
}

// NOTE: This test sets the current working directory so don't run it in parallel with tests
// that depend on the current directory

#[test]
fn test_all_templates() {
    let _ = env_logger::Builder::from_env("MIDENC_TRACE")
        .is_test(true)
        .format_timestamp(None)
        .try_init();
    // Signal to `cargo-miden` that we're running in a test harness.
    //
    // This is necessary because cfg!(test) does not work for integration tests, so we're forced
    // to use an out-of-band signal like this instead
    unsafe { env::set_var("TEST", "1") };

    // Test new project templates
    let account = build_new_project_from_template("--account");
    assert!(account.is_library());

    let note = build_new_project_from_template("--note");
    assert!(note.is_program());

    let tx_script = build_new_project_from_template("--tx-script");
    assert!(tx_script.is_program());

    let program = build_new_project_from_template("--program");
    assert!(program.is_program());

    let auth_comp = build_new_project_from_template("--auth-component");
    assert!(auth_comp.is_library());

    let expected_function = "auth__procedure";
    let lib = auth_comp.unwrap_library();
    assert!(
        lib.exports()
            .any(|export| export.path().as_ref().last() == Some(expected_function)),
        "expected one of the authentication component exports to contain  function \
         '{expected_function}'"
    );
}

/// Build a new project from the specified template and return its package
/// Handles special cases like note templates that require a contract dependency
fn build_new_project_from_template(template: &str) -> Package {
    let restore_dir = env::current_dir().unwrap();
    let temp_dir = env::temp_dir();
    env::set_current_dir(&temp_dir).unwrap();

    if template == "--note" || template == "--tx-script" {
        // create the counter contract cargo project since the note and tx-script depend on it
        let project_name = "add-contract";
        let expected_new_project_dir = &temp_dir.join(project_name);
        if expected_new_project_dir.exists() {
            fs::remove_dir_all(expected_new_project_dir).unwrap();
        }
        let _ = run(new_project_args(project_name, "--account").into_iter(), OutputType::Masm)
            .expect("Failed to create new add-contract dependency project")
            .expect("'cargo miden new' should return Some(CommandOutput)");
    }

    let project_name = "test_proj_underscore";
    let expected_new_project_dir = &temp_dir.join(project_name);
    if expected_new_project_dir.exists() {
        fs::remove_dir_all(expected_new_project_dir).unwrap();
    }

    let args = new_project_args(project_name, template);

    let output = run(args.into_iter(), OutputType::Masm)
        .expect("Failed to create new project from {template} template")
        .expect("'cargo miden new' should return Some(CommandOutput)");
    let new_project_path = match output {
        cargo_miden::CommandOutput::NewCommandOutput { project_path } => {
            project_path.canonicalize().unwrap()
        }
        other => panic!("Expected NewCommandOutput, got {other:?}"),
    };
    assert!(new_project_path.exists());
    assert_eq!(new_project_path, expected_new_project_dir.canonicalize().unwrap());
    env::set_current_dir(&new_project_path).unwrap();

    // build with the dev profile
    let args = ["cargo", "miden", "build"].iter().map(|s| s.to_string());
    let output = run(args, OutputType::Masm)
        .unwrap_or_else(|e| {
            panic!(
                "Failed to compile with the dev profile for template: {template} \nwith error: {e}"
            )
        })
        .expect("'cargo miden build' should return Some(CommandOutput)");
    let expected_masm_path = match output {
        cargo_miden::CommandOutput::BuildCommandOutput { output } => match output {
            cargo_miden::BuildOutput::Masm { artifact_path } => artifact_path,
            other => panic!("Expected Masm output, got {other:?}"),
        },
        other => panic!("Expected BuildCommandOutput, got {other:?}"),
    };
    assert!(expected_masm_path.exists());
    assert!(expected_masm_path.to_str().unwrap().contains("/debug/"));
    assert_eq!(expected_masm_path.extension().unwrap(), "masp");
    assert!(expected_masm_path.metadata().unwrap().len() > 0);

    // build with the release profile
    let args = ["cargo", "miden", "build", "--release"].iter().map(|s| s.to_string());
    let output = run(args, OutputType::Masm)
        .expect("Failed to compile with the release profile")
        .expect("'cargo miden build --release' should return Some(CommandOutput)");
    let expected_masm_path = match output {
        cargo_miden::CommandOutput::BuildCommandOutput { output } => match output {
            cargo_miden::BuildOutput::Masm { artifact_path } => artifact_path,
            other => panic!("Expected Masm output, got {other:?}"),
        },
        other => panic!("Expected BuildCommandOutput, got {other:?}"),
    };
    assert!(expected_masm_path.exists());
    assert_eq!(expected_masm_path.extension().unwrap(), "masp");
    assert!(expected_masm_path.to_str().unwrap().contains("/release/"));
    assert!(expected_masm_path.metadata().unwrap().len() > 0);
    let package_bytes = fs::read(expected_masm_path).unwrap();
    let package = Package::read_from_bytes(&package_bytes).unwrap();

    env::set_current_dir(restore_dir).unwrap();
    fs::remove_dir_all(new_project_path).unwrap();
    package
}

#[test]
fn new_project_integration_tests_pass() {
    let _ = env_logger::Builder::from_env("MIDENC_TRACE")
        .is_test(true)
        .format_timestamp(None)
        .try_init();
    unsafe { env::set_var("TEST", "1") };

    let restore_dir = env::current_dir().unwrap();
    let temp_dir = env::temp_dir().join(format!(
        "cargo_miden_integration_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    ));
    if temp_dir.exists() {
        fs::remove_dir_all(&temp_dir).unwrap();
    }
    fs::create_dir_all(&temp_dir).unwrap();
    env::set_current_dir(&temp_dir).unwrap();

    let project_name = format!(
        "integration_project_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_micros()
    );
    let args = new_project_args(&project_name, "");

    let output = run(args.into_iter(), OutputType::Masm)
        .expect("Failed to create project with `cargo miden new`")
        .expect("'cargo miden new' should return Some(CommandOutput)");
    let project_path = match output {
        cargo_miden::CommandOutput::NewCommandOutput { project_path } => {
            project_path.canonicalize().unwrap()
        }
        other => panic!("Expected NewCommandOutput, got {other:?}"),
    };
    assert!(project_path.exists());

    let integration_dir = project_path.join("integration");
    assert!(
        integration_dir.exists(),
        "expected integration workspace at {}",
        integration_dir.display()
    );

    let output = std::process::Command::new("cargo")
        .arg("test")
        .current_dir(&integration_dir)
        .output()
        .expect("failed to spawn `cargo test` inside integration directory");
    if !output.status.success() {
        panic!(
            "`cargo test` failed in {} with status {:?}\nstdout:\n{}\nstderr:\n{}",
            integration_dir.display(),
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    env::set_current_dir(restore_dir).unwrap();
    fs::remove_dir_all(&project_path).unwrap();
    if temp_dir.exists() {
        fs::remove_dir_all(&temp_dir).unwrap();
    }
}
