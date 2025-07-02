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
fn test_example_templates() {
    let _ = env_logger::Builder::from_env("MIDENC_TRACE")
        .is_test(true)
        .format_timestamp(None)
        .try_init();
    // Signal to `cargo-miden` that we're running in a test harness.
    //
    // This is necessary because cfg!(test) does not work for integration tests, so we're forced
    // to use an out-of-band signal like this instead
    env::set_var("TEST", "1");

    // Test basic-wallet example
    let wallet = build_example_project_from_template("basic-wallet");
    assert!(wallet.is_library());

    // Test counter-contract example
    let counter = build_example_project_from_template("counter-contract");
    assert!(counter.is_library());

    // Test fibonacci example
    let program = build_example_project_from_template("fibonacci");
    assert!(program.is_program());

    // Verify program projects don't have WIT files
    verify_no_wit_files_for_example_template("fibonacci");
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
        other => panic!("Expected NewCommandOutput, got {:?}", other),
    };
    env::set_current_dir(&new_project_path).unwrap();

    // Verify the wit directory does not exist or is empty for program template
    let wit_dir = new_project_path.join(WIT_DEPS_PATH);
    assert!(
        !wit_dir.exists() || wit_dir.read_dir().unwrap().count() == 0,
        "WIT directory should not exist or be empty for {} example",
        example_name
    );

    env::set_current_dir(restore_dir).unwrap();
    fs::remove_dir_all(temp_dir).unwrap();
}

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
        other => panic!("Expected NewCommandOutput, got {:?}", other),
    };
    dbg!(&new_project_path);
    assert!(new_project_path.exists());
    env::set_current_dir(&new_project_path).unwrap();

    // build with the dev profile
    let args = ["cargo", "miden", "build"].iter().map(|s| s.to_string());
    let output = run(args, OutputType::Masm)
        .expect("Failed to compile with the dev profile")
        .expect("'cargo miden build' should return Some(CommandOutput)");
    let expected_masm_path = match output {
        cargo_miden::CommandOutput::BuildCommandOutput { output } => match output {
            cargo_miden::BuildOutput::Masm { artifact_path } => artifact_path,
            other => panic!("Expected Masm output, got {:?}", other),
        },
        other => panic!("Expected BuildCommandOutput, got {:?}", other),
    };
    dbg!(&expected_masm_path);
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
            other => panic!("Expected Masm output, got {:?}", other),
        },
        other => panic!("Expected BuildCommandOutput, got {:?}", other),
    };
    dbg!(&expected_masm_path);
    assert!(expected_masm_path.exists());
    assert_eq!(expected_masm_path.extension().unwrap(), "masp");
    assert!(expected_masm_path.to_str().unwrap().contains("/release/"));
    assert!(expected_masm_path.metadata().unwrap().len() > 0);
    let package_bytes = fs::read(expected_masm_path).unwrap();
    let package = Package::read_from_bytes(&package_bytes).unwrap();

    env::set_current_dir(restore_dir).unwrap();
    fs::remove_dir_all(temp_dir).unwrap();
    package
}

#[test]
fn test_new_clean_templates() {
    let _ = env_logger::Builder::from_env("MIDENC_TRACE")
        .is_test(true)
        .format_timestamp(None)
        .try_init();
    // Signal to `cargo-miden` that we're running in a test harness.
    env::set_var("TEST", "1");

    // empty template means no template option is passing, thus using the default project template (program)
    let r#default = build_new_project_from_template("");
    assert!(r#default.is_program());

    let note = build_new_project_from_template("--note");
    assert!(note.is_program());

    let account = build_new_project_from_template("--account");
    assert!(account.is_library());

    let program = build_new_project_from_template("--program");
    assert!(program.is_program());

    // Verify program projects don't have WIT files
    verify_no_wit_files_for_new_template("--program");
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
        other => panic!("Expected NewCommandOutput, got {:?}", other),
    };
    env::set_current_dir(&new_project_path).unwrap();

    // Verify the wit directory does not exist or is empty for program template
    let wit_dir = new_project_path.join(WIT_DEPS_PATH);
    assert!(
        !wit_dir.exists() || wit_dir.read_dir().unwrap().count() == 0,
        "WIT directory should not exist or be empty for {} template",
        template
    );

    env::set_current_dir(restore_dir).unwrap();
    fs::remove_dir_all(new_project_path).unwrap();
}

fn build_new_project_from_template(template: &str) -> Package {
    let restore_dir = env::current_dir().unwrap();
    let temp_dir = env::temp_dir();
    env::set_current_dir(&temp_dir).unwrap();

    let project_name = "test_new_proj_underscore";
    let expected_new_project_dir = &temp_dir.join(project_name);
    dbg!(&expected_new_project_dir);
    if expected_new_project_dir.exists() {
        fs::remove_dir_all(expected_new_project_dir).unwrap();
    }

    let args = new_project_args(project_name, template);

    let output = run(args.into_iter(), OutputType::Masm)
        .expect("Failed to create new project")
        .expect("'cargo miden new' should return Some(CommandOutput)");
    let new_project_path = match output {
        cargo_miden::CommandOutput::NewCommandOutput { project_path } => {
            project_path.canonicalize().unwrap()
        }
        other => panic!("Expected NewCommandOutput, got {:?}", other),
    };
    dbg!(&new_project_path);
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
            other => panic!("Expected Masm output, got {:?}", other),
        },
        other => panic!("Expected BuildCommandOutput, got {:?}", other),
    };
    dbg!(&expected_masm_path);
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
            other => panic!("Expected Masm output, got {:?}", other),
        },
        other => panic!("Expected BuildCommandOutput, got {:?}", other),
    };
    dbg!(&expected_masm_path);
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
