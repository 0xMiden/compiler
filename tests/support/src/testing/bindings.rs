//! The SDK crates commit the bindings `miden-sdk-bindgen` generates from the toolchain's
//! packages; their tests regenerate the text and compare it with the committed files here.

use std::path::Path;

/// The environment variable that makes [`check_generated`] rewrite the committed files.
pub const UPDATE_BINDINGS: &str = "UPDATE_BINDINGS";

/// Asserts that the committed file `path`, relative to `manifest_dir`, holds exactly `fresh`,
/// the text the generator produces now. With [`UPDATE_BINDINGS`] set, writes `fresh` to the file
/// instead.
///
/// # Panics
///
/// If the file differs from `fresh` (naming the first line that differs), or cannot be written.
pub fn check_generated(manifest_dir: &Path, path: &str, fresh: &str) {
    let path = manifest_dir.join(path);
    if std::env::var_os(UPDATE_BINDINGS).is_some() {
        std::fs::write(&path, fresh)
            .unwrap_or_else(|err| panic!("cannot write {}: {err}", path.display()));
        return;
    }
    let committed = std::fs::read_to_string(&path).unwrap_or_default();
    if committed == fresh {
        return;
    }
    let line = committed
        .lines()
        .zip(fresh.lines())
        .position(|(committed, fresh)| committed != fresh)
        .unwrap_or_else(|| committed.lines().count().min(fresh.lines().count()))
        + 1;
    panic!(
        "{} is stale from line {line}: run the test with {UPDATE_BINDINGS}=1 to regenerate it",
        path.display()
    );
}
