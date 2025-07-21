#![allow(unused)]

use std::{env, fs};

use cargo_miden::{run, OutputType, WIT_DEPS_PATH};
use miden_mast_package::Package;
use midenc_session::miden_assembly::utils::Deserializable;

fn example_project_args(example_name: &str) -> Vec<String> {
    vec![
        "cargo".to_string(),
        "miden".to_string(),
        "example".to_string(),
        example_name.to_string(),
    ]
}

fn new_project_args(project_name: &str, template: &str) -> Vec<String> {
    let template = if let Ok(templates_path) = std::env::var("TEST_LOCAL_TEMPLATES_PATH") {
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
    env::set_var("TEST", "1");

    // Test example templates
    // Test basic-wallet example (which also creates p2id-note)
    let (basic_wallet, p2id_note) = build_paired_example_projects(
        "basic-wallet",
        "basic-wallet",
        "p2id-note",
        "basic_wallet",
        "p2id",
    );
    assert!(basic_wallet.is_library());
    assert_eq!(basic_wallet.name, "basic_wallet");
    assert!(p2id_note.is_program());
    assert_eq!(p2id_note.name, "p2id");

    // Test counter-contract example (which also creates counter-note)
    let (counter_contract, counter_note) = build_paired_example_projects(
        "counter-contract",
        "counter-contract",
        "counter-note",
        "counter_contract",
        "counter_note",
    );
    assert!(counter_contract.is_library());
    assert_eq!(counter_contract.name, "counter_contract");
    assert!(counter_note.is_program());
    assert_eq!(counter_note.name, "counter_note");

    // Test fibonacci example
    let fibonacci = build_example_project_from_template("fibonacci");
    assert!(fibonacci.is_program());
    assert_eq!(fibonacci.name, "fibonacci");

    // Test collatz example
    let collatz = build_example_project_from_template("collatz");
    assert!(collatz.is_program());
    assert_eq!(collatz.name, "collatz");

    // Test is-prime example
    let is_prime = build_example_project_from_template("is-prime");
    assert!(is_prime.is_program());
    assert_eq!(is_prime.name, "is_prime");

    // Test storage-example
    let storage = build_example_project_from_template("storage-example");
    assert!(storage.is_library());
    assert_eq!(storage.name, "storage_example");

    // Verify program projects don't have WIT files
    verify_no_wit_files_for_example_template("fibonacci");
    verify_no_wit_files_for_example_template("collatz");
    verify_no_wit_files_for_example_template("is-prime");

    // Test new project templates
    // empty template means no template option is passing, thus using the default project template (account)
    let r#default = build_new_project_from_template("");
    assert!(r#default.is_library());

    let note = build_new_project_from_template("--note");
    assert!(note.is_program());

    let program = build_new_project_from_template("--program");
    assert!(program.is_program());

    // Verify program projects don't have WIT files
    verify_no_wit_files_for_new_template("--program");
}

/// Verify that WIT files are not present for program template
fn verify_no_wit_files_for_example_template(example_name: &str) {
    let restore_dir = env::current_dir().unwrap();
    let temp_dir = env::temp_dir().join(format!(
        "test_no_wit_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    ));
    fs::create_dir_all(&temp_dir).unwrap();
    env::set_current_dir(&temp_dir).unwrap();

    // Create the project - it will be named after the example
    let args = example_project_args(example_name);
    let output = run(args.into_iter(), OutputType::Masm)
        .expect("Failed to create new project")
        .expect("Expected build output");
    let new_project_path = match output {
        cargo_miden::CommandOutput::NewCommandOutput { project_path } => {
            project_path.canonicalize().unwrap()
        }
        other => panic!("Expected NewCommandOutput, got {other:?}"),
    };
    env::set_current_dir(&new_project_path).unwrap();

    // Verify the wit directory does not exist or is empty for program template
    let wit_dir = new_project_path.join(WIT_DEPS_PATH);
    assert!(
        !wit_dir.exists() || wit_dir.read_dir().unwrap().count() == 0,
        "WIT directory should not exist or be empty for {example_name} example"
    );

    env::set_current_dir(restore_dir).unwrap();
    fs::remove_dir_all(temp_dir).unwrap();
}

/// Build paired example projects (e.g., account and note script) and return their packages
/// Creates both projects in a subdirectory and builds them separately
fn build_paired_example_projects(
    example_name: &str,
    first_dir: &str,
    second_dir: &str,
    first_expected_name: &str,
    second_expected_name: &str,
) -> (Package, Package) {
    let restore_dir = env::current_dir().unwrap();
    let temp_dir = env::temp_dir().join(format!(
        "test_example_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    ));
    fs::create_dir_all(&temp_dir).unwrap();
    env::set_current_dir(&temp_dir).unwrap();

    // Create the project - it will create both projects
    let args = example_project_args(example_name);

    let output = run(args.into_iter(), OutputType::Masm)
        .expect("Failed to create new project")
        .expect("'cargo miden example' should return Some(CommandOutput)");
    let main_project_path = match output {
        cargo_miden::CommandOutput::NewCommandOutput { project_path } => {
            project_path.canonicalize().unwrap()
        }
        other => panic!("Expected NewCommandOutput, got {other:?}"),
    };
    assert!(main_project_path.exists());

    // Build first project
    let first_path = main_project_path.join(first_dir);
    assert!(first_path.exists());
    env::set_current_dir(&first_path).unwrap();

    let first_package = build_project_in_current_dir(first_expected_name);

    // Build second project
    let second_path = main_project_path.join(second_dir);
    assert!(second_path.exists());
    env::set_current_dir(&second_path).unwrap();

    let second_package = build_project_in_current_dir(second_expected_name);

    env::set_current_dir(restore_dir).unwrap();
    fs::remove_dir_all(temp_dir).unwrap();

    (first_package, second_package)
}

/// Build a project in the current directory and verify it compiles correctly
/// Tests both debug and release builds, returning the package from the release build
fn build_project_in_current_dir(expected_name: &str) -> Package {
    // build with the dev profile
    let args = ["cargo", "miden", "build"].iter().map(|s| s.to_string());
    let output = run(args, OutputType::Masm)
        .expect("Failed to compile with the dev profile")
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
    assert!(expected_masm_path.to_str().unwrap().contains(expected_name));
    assert!(expected_masm_path.metadata().unwrap().len() > 0);
    let package_bytes = fs::read(expected_masm_path).unwrap();
    Package::read_from_bytes(&package_bytes).unwrap()
}

/// Build an example project from the specified template and return its package
/// Creates the project in a temporary directory and builds it
fn build_example_project_from_template(example_name: &str) -> Package {
    let restore_dir = env::current_dir().unwrap();
    let temp_dir = env::temp_dir().join(format!(
        "test_example_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    ));
    fs::create_dir_all(&temp_dir).unwrap();
    env::set_current_dir(&temp_dir).unwrap();

    // Create the project - it will be named after the example
    let args = example_project_args(example_name);

    let output = run(args.into_iter(), OutputType::Masm)
        .expect("Failed to create new project")
        .expect("'cargo miden example' should return Some(CommandOutput)");
    let new_project_path = match output {
        cargo_miden::CommandOutput::NewCommandOutput { project_path } => {
            project_path.canonicalize().unwrap()
        }
        other => panic!("Expected NewCommandOutput, got {other:?}"),
    };
    assert!(new_project_path.exists());
    env::set_current_dir(&new_project_path).unwrap();

    // Convert hyphens to underscores for the expected package name
    let expected_name = example_name.replace("-", "_");
    let package = build_project_in_current_dir(&expected_name);

    env::set_current_dir(restore_dir).unwrap();
    fs::remove_dir_all(temp_dir).unwrap();
    package
}

/// Verify that WIT files are not present for program template
fn verify_no_wit_files_for_new_template(template: &str) {
    let restore_dir = env::current_dir().unwrap();
    let temp_dir = env::temp_dir();
    env::set_current_dir(&temp_dir).unwrap();
    let project_name = format!("test_new_no_wit_files_{}", template.replace("--", ""));
    let expected_new_project_dir = &temp_dir.join(&project_name);
    if expected_new_project_dir.exists() {
        fs::remove_dir_all(expected_new_project_dir).unwrap();
    }

    // Create the project
    let args = new_project_args(&project_name, template);
    let output = run(args.into_iter(), OutputType::Masm)
        .expect("Failed to create new project")
        .expect("Expected build output");
    let new_project_path = match output {
        cargo_miden::CommandOutput::NewCommandOutput { project_path } => {
            project_path.canonicalize().unwrap()
        }
        other => panic!("Expected NewCommandOutput, got {other:?}"),
    };
    env::set_current_dir(&new_project_path).unwrap();

    // Verify the wit directory does not exist or is empty for program template
    let wit_dir = new_project_path.join(WIT_DEPS_PATH);
    assert!(
        !wit_dir.exists() || wit_dir.read_dir().unwrap().count() == 0,
        "WIT directory should not exist or be empty for {template} template"
    );

    env::set_current_dir(restore_dir).unwrap();
    fs::remove_dir_all(new_project_path).unwrap();
}

/// Build a new project from the specified template and return its package
/// Handles special cases like note templates that require a contract dependency
fn build_new_project_from_template(template: &str) -> Package {
    let restore_dir = env::current_dir().unwrap();
    let temp_dir = env::temp_dir();
    env::set_current_dir(&temp_dir).unwrap();

    if template == "--note" {
        // create the counter contract cargo project since the note depends on it
        let project_name = "add-contract";
        let expected_new_project_dir = &temp_dir.join(project_name);
        if expected_new_project_dir.exists() {
            fs::remove_dir_all(expected_new_project_dir).unwrap();
        }
        let output = run(new_project_args(project_name, "--account").into_iter(), OutputType::Masm)
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
        .expect("Failed to compile with the dev profile")
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
