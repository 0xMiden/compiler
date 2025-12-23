use std::{env, fs};

use cargo_miden::{OutputType, run};
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
fn test_all_templates_and_examples() {
    let _ = env_logger::Builder::from_env("MIDENC_TRACE")
        .is_test(true)
        .format_timestamp(None)
        .try_init();
    // Signal to `cargo-miden` that we're running in a test harness.
    //
    // This is necessary because cfg!(test) does not work for integration tests, so we're forced
    // to use an out-of-band signal like this instead
    unsafe { env::set_var("TEST", "1") };

    // Test example templates

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

    // Test basic-wallet-tx-script example using different entry points
    // Test 1: Using "basic-wallet-tx-script" as the example name
    let (tx_script, wallet, p2id) = build_triple_example_projects(
        "basic-wallet-tx-script",
        "basic-wallet-tx-script",
        "basic-wallet",
        "p2id-note",
        "basic_wallet_tx_script",
        "basic_wallet",
        "p2id",
    );
    assert!(tx_script.is_program());
    assert_eq!(tx_script.name, "basic_wallet_tx_script");
    assert!(wallet.is_library());
    assert_eq!(wallet.name, "basic_wallet");
    assert!(p2id.is_program());
    assert_eq!(p2id.name, "p2id");

    // Test 2: Using "basic-wallet" as the example name (should create all three)
    let (tx_script2, wallet2, p2id2) = build_triple_example_projects(
        "basic-wallet",
        "basic-wallet-tx-script",
        "basic-wallet",
        "p2id-note",
        "basic_wallet_tx_script",
        "basic_wallet",
        "p2id",
    );
    assert!(tx_script2.is_program());
    assert_eq!(tx_script2.name, "basic_wallet_tx_script");
    assert!(wallet2.is_library());
    assert_eq!(wallet2.name, "basic_wallet");
    assert!(p2id2.is_program());
    assert_eq!(p2id2.name, "p2id");

    // Test 3: Using "p2id-note" as the example name (should create all three)
    let (tx_script3, wallet3, p2id3) = build_triple_example_projects(
        "p2id-note",
        "basic-wallet-tx-script",
        "basic-wallet",
        "p2id-note",
        "basic_wallet_tx_script",
        "basic_wallet",
        "p2id",
    );
    assert!(tx_script3.is_program());
    assert_eq!(tx_script3.name, "basic_wallet_tx_script");
    assert!(wallet3.is_library());
    assert_eq!(wallet3.name, "basic_wallet");
    assert!(p2id3.is_program());
    assert_eq!(p2id3.name, "p2id");

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
        lib.exports().any(|export| { export.name.name.as_str() == expected_function }),
        "expected one of the authentication component exports to contain  function \
         '{expected_function}'"
    );
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

/// Build triple example projects (e.g., tx-script, account and note script) and return their packages
/// Creates all three projects in a subdirectory and builds them separately
fn build_triple_example_projects(
    example_name: &str,
    first_dir: &str,
    second_dir: &str,
    third_dir: &str,
    first_expected_name: &str,
    second_expected_name: &str,
    third_expected_name: &str,
) -> (Package, Package, Package) {
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

    // Create the project - it will create all three projects
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

    // Build second project (basic-wallet)
    let second_path = main_project_path.join(second_dir);
    assert!(second_path.exists());
    env::set_current_dir(&second_path).unwrap();
    let second_package = build_project_in_current_dir(second_expected_name);

    // Build third project (p2id-note)
    let third_path = main_project_path.join(third_dir);
    assert!(third_path.exists());
    env::set_current_dir(&third_path).unwrap();
    let third_package = build_project_in_current_dir(third_expected_name);

    // Build first project (basic-wallet-tx-script)
    let first_path = main_project_path.join(first_dir);
    assert!(first_path.exists());
    env::set_current_dir(&first_path).unwrap();
    let first_package = build_project_in_current_dir(first_expected_name);

    env::set_current_dir(restore_dir).unwrap();
    fs::remove_dir_all(temp_dir).unwrap();

    (first_package, second_package, third_package)
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
