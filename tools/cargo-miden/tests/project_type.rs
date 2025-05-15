use std::path::Path;

use cargo_metadata::MetadataCommand;
use cargo_miden::ProjectType;
use midenc_session::TargetEnv;

#[test]
fn test_project_type_detection() {
    // Define examples with both expected project type and target environment
    let examples = [
        // (example_name, expected_project_type, expected_target_environment)
        ("collatz", ProjectType::Program, TargetEnv::Base),
        ("counter-contract", ProjectType::Library, TargetEnv::Rollup),
        ("fibonacci", ProjectType::Program, TargetEnv::Base),
        ("is-prime", ProjectType::Program, TargetEnv::Base),
        ("storage-example", ProjectType::Library, TargetEnv::Rollup),
    ];

    for (example_name, expected_type, expected_env) in examples {
        let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        // Relative path from tools/cargo-miden/tests/ -> tools/cargo-miden/ -> examples/
        let example_manifest_path = manifest_dir
            .join("../../examples") // Go up two levels from crate root
            .join(example_name)
            .join("Cargo.toml")
            .canonicalize() // Resolve path for clearer error messages
            .unwrap_or_else(|e| {
                panic!("Failed to find manifest path for {}: {}", example_name, e)
            });

        println!("Testing project type detection for: {}", example_manifest_path.display());

        let metadata = MetadataCommand::new()
            .manifest_path(&example_manifest_path)
            .no_deps() // Avoid pulling deps for simple metadata read
            .exec()
            .unwrap_or_else(|e| {
                panic!(
                    "Failed to load metadata for {}: {}",
                    example_manifest_path.display(),
                    e
                )
            });

        // Test target environment detection
        let detected_env = cargo_miden::detect_target_environment(&metadata);
        assert_eq!(
            detected_env,
            expected_env,
            "Target environment mismatch for example '{}': expected {:?}, detected {:?} \
             (manifest: {})",
            example_name,
            expected_env,
            detected_env,
            example_manifest_path.display()
        );

        // Test project type detection
        let detected_type = cargo_miden::detect_project_type(&metadata);
        assert_eq!(
            detected_type,
            expected_type,
            "Project type mismatch for example '{}': expected {:?}, detected {:?} (manifest: {})",
            example_name,
            expected_type,
            detected_type,
            example_manifest_path.display()
        );

        // Verify that project type is correctly derived from target environment
        let derived_type = cargo_miden::target_environment_to_project_type(detected_env);
        assert_eq!(
            derived_type, detected_type,
            "Derived project type mismatch for example '{}': expected {:?}, derived {:?}",
            example_name, detected_type, derived_type
        );
    }
}
