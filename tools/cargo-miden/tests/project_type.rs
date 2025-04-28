use std::path::Path;

use cargo_metadata::MetadataCommand;
use cargo_miden::ProjectType;

#[test]
fn test_project_type_detection() {
    let examples = [
        ("collatz", ProjectType::Program),
        ("counter-contract", ProjectType::Library),
        ("fibonacci", ProjectType::Program),
        ("is-prime", ProjectType::Program),
        ("storage-example", ProjectType::Library),
    ];

    for (example_name, expected_type) in examples {
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

        let detected_type = cargo_miden::detect_project_type(&metadata);

        assert_eq!(
            detected_type,
            expected_type,
            "Mismatch for example '{}': expected {:?}, detected {:?} (manifest: {})",
            example_name,
            expected_type,
            detected_type,
            example_manifest_path.display()
        );
    }
}
