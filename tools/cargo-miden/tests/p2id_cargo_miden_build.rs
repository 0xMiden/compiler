use std::{
    env,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use cargo_miden::run;

use crate::utils::{current_dir_lock, workspace_root};

/// Building a project materializes its compiled Miden dependencies on disk.
///
/// The `p2id-note` example depends on the `basic-wallet` example as a Miden dependency. When
/// `cargo miden build` compiles `p2id-note`, it compiles `basic-wallet` as a dependency. The
/// resulting dependency package must be materialized under p2id-note's
/// `target/miden/packages/<build-fingerprint>/` package cache rather than only living in the
/// in-memory package registry.
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
    let result = run(["cargo", "miden", "build", "--release"].into_iter().map(|s| s.to_string()));
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

    // The build must have materialized the basic-wallet dependency package on disk, inside the
    // build's single fingerprint directory.
    let dep_package =
        crate::utils::package_cache_fingerprint_dir(&p2id_note_dir, "basic-wallet.masp")
            .join("basic-wallet.masp");
    assert!(
        dep_package.exists(),
        "expected basic-wallet dependency package to be materialized at {}",
        dep_package.display()
    );
    let modified = dep_package.metadata().unwrap().modified().unwrap();
    let attribution_floor =
        build_started_at.checked_sub(Duration::from_secs(1)).unwrap_or(UNIX_EPOCH);
    assert!(
        modified >= attribution_floor,
        "expected this build to rewrite {}, but its modification time {modified:?} predates the \
         one-second-tolerant build attribution floor {attribution_floor:?}",
        dep_package.display()
    );
}
