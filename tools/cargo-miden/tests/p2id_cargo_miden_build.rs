use std::{env, time::SystemTime};

use cargo_miden::run;

use crate::utils::{current_dir_lock, workspace_root};

/// A caller-provided `MIDENC_PACKAGE_CACHE` materializes a build's Miden dependencies on disk.
///
/// The `p2id-note` example depends on the `basic-wallet` example as a Miden dependency. When
/// `cargo miden build` compiles `p2id-note`, it compiles `basic-wallet` as a dependency and
/// publishes it into its package cache. A cache the compiler mints itself is deleted when the
/// build ends; a caller-provided one must be adopted and left in place, so consumers that run
/// after the compiler exits can read the packages.
#[test]
fn p2id_build_materializes_basic_wallet_dependency() {
    let _cwd_lock = current_dir_lock();
    let _ = midenc_log::Builder::from_env("MIDENC_TRACE")
        .is_test(true)
        .format_timestamp(None)
        .try_init();

    // `Makefile.toml` sets `CARGO_TARGET_DIR` to the workspace target directory. Unset it so each
    // example project uses its own `target/` directory, where dependency packages are expected to
    // be materialized.
    let restore_target_dir = env::var_os("CARGO_TARGET_DIR");
    unsafe {
        env::remove_var("CARGO_TARGET_DIR");
    }

    let examples = workspace_root().join("examples");
    let p2id_note_dir = examples.join("p2id-note");

    // Build the p2id-note project, which pulls in basic-wallet as a Miden dependency.
    let restore_dir = env::current_dir().unwrap();
    env::set_current_dir(&p2id_note_dir).unwrap();
    let build_started_at = SystemTime::now();
    let export_dir = crate::utils::exported_packages_dir(&p2id_note_dir);
    let result = crate::utils::with_package_cache_env(&export_dir, || {
        run(["cargo", "miden", "build", "--release"].into_iter().map(|s| s.to_string()))
    });
    env::set_current_dir(&restore_dir).unwrap();

    // Restore `CARGO_TARGET_DIR` before asserting, so a build failure doesn't leak the unset state.
    match restore_target_dir {
        Some(val) => unsafe { env::set_var("CARGO_TARGET_DIR", val) },
        None => unsafe { env::remove_var("CARGO_TARGET_DIR") },
    }

    let output = result
        .expect("cargo miden build for p2id-note failed")
        .expect("expected BuildCommandOutput")
        .unwrap_build_output();
    assert_eq!(output.len(), 1, "expected a single p2id-note package artifact, got {output:?}");

    // The build must have published the basic-wallet dependency package into the adopted
    // stable directory and left it in place.
    let dep_package = export_dir.join("basic-wallet.masp");
    assert!(
        dep_package.exists(),
        "expected basic-wallet dependency package to be materialized at {}",
        dep_package.display()
    );
    crate::utils::assert_written_by_this_build(&dep_package, build_started_at);

    // The root's watch list must carry the Rust dependency's inputs from its recorded
    // cargo dep-info — a source file cargo actually consumed — plus the lockfile, which
    // dep-info does not cover.
    let watch_file = export_dir.join("miden-deps").join("p2id.watch");
    let watch = std::fs::read_to_string(&watch_file).unwrap_or_else(|err| {
        panic!("expected the compiler-recorded watch list at {}: {err}", watch_file.display())
    });
    let watch_has =
        |suffix: &str| watch.lines().any(|line| std::path::Path::new(line).ends_with(suffix));
    assert!(
        watch_has("basic-wallet/src/lib.rs"),
        "expected a dep-info-derived source watch in:\n{watch}"
    );
    assert!(
        watch_has("basic-wallet/Cargo.lock"),
        "expected the dependency lockfile watch in:\n{watch}"
    );
}
