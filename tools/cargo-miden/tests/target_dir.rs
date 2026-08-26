use std::{env, fs};

use cargo_miden::run;

use crate::utils::{RestoreEnvironment, current_dir_lock, project_template_arg};

/// A custom Midenc target is an umbrella for every artifact the compiler-owned build writes.
///
/// A regular file at the project's normal Cargo target path makes any accidental default Cargo
/// write fail deterministically on every platform. The build can therefore succeed only if the
/// manifest frontend redirects its nested Cargo invocation beneath the custom target.
#[test]
fn a_custom_midenc_target_contains_nested_cargo_artifacts() {
    let _cwd = current_dir_lock();
    let _restore_environment = RestoreEnvironment::new([
        "CARGO_TARGET_DIR",
        "CARGO_BUILD_TARGET_DIR",
        "MIDENC_TARGET_DIR",
        "TEST",
    ]);
    unsafe {
        env::remove_var("CARGO_TARGET_DIR");
        env::remove_var("CARGO_BUILD_TARGET_DIR");
        env::remove_var("MIDENC_TARGET_DIR");
        env::set_var("TEST", "1");
    }

    let scratch = env::temp_dir().join(format!(
        "cargo_miden_custom_target_{}_{:?}",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap()
    ));
    fs::create_dir_all(&scratch).unwrap();
    env::set_current_dir(&scratch).unwrap();

    let project_name = "custom-target";
    let created = run([
        "cargo".to_string(),
        "miden".to_string(),
        "new".to_string(),
        project_name.to_string(),
        project_template_arg("--program"),
    ]
    .into_iter())
    .expect("cargo miden new failed")
    .expect("expected NewCommandOutput");
    let project = match created {
        cargo_miden::CommandOutput::NewCommandOutput { project_path } => scratch.join(project_path),
        other => panic!("expected NewCommandOutput, got {other:?}"),
    };

    let blocked_default_target = project.join("target");
    fs::write(&blocked_default_target, b"the source-tree target must remain untouched").unwrap();
    let custom_target = scratch.join("writable").join("miden-target");
    env::set_current_dir(&project).unwrap();
    let output = run(["cargo", "miden", "build", "--target-dir", custom_target.to_str().unwrap()]
        .into_iter()
        .map(str::to_string))
    .expect("cargo miden build with a custom target failed")
    .expect("expected BuildCommandOutput")
    .unwrap_build_output();

    assert_eq!(output.len(), 1, "expected one compiled package, got {output:?}");
    assert!(
        output[0].starts_with(&custom_target),
        "the final Miden package must live under the custom target: {output:?}"
    );
    assert!(
        custom_target.join("cargo").is_dir(),
        "the nested Cargo target must be derived beneath the custom Midenc target"
    );
    assert_eq!(
        fs::read(&blocked_default_target).unwrap(),
        b"the source-tree target must remain untouched",
        "the build must not replace or write through the checkout's default target path"
    );

    env::set_current_dir(env::temp_dir()).unwrap();
    fs::remove_dir_all(scratch).unwrap();
}
