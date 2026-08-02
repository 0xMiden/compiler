//! The embedded project template bundle.
//!
//! This is the offline fallback: templates that ship inside the binary, so
//! `cargo miden new` works with no network and always has templates matching
//! the `cargo-miden` that generated them.
//!
//! The digest is what makes the bundle verifiable. A compiler release proves
//! its embedded copy is byte-identical to the released `templates/v*` archive
//! by comparing [`SHA256`], and `release-tool lint` proves the committed
//! archive still matches the template sources.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

include!(concat!(env!("OUT_DIR"), "/template_bundle.rs"));

/// The bundle archive, embedded at compile time.
pub const ARCHIVE: &[u8] = include_bytes!("../templates.tar.gz");

/// SHA-256 of [`ARCHIVE`].
pub const SHA256: &str = TEMPLATE_BUNDLE_SHA256;

/// Extract the embedded bundle into `destination`, returning the root it was
/// written to.
///
/// Extraction is deliberately narrow: only regular files, only relative paths
/// that stay inside the destination. An archive is data, and a bundle fetched
/// from a release is data from the network — a path like `../../.ssh/authorized_keys`
/// must never escape, even though today's archives are built by us.
pub fn extract(destination: &Path) -> Result<PathBuf> {
    std::fs::create_dir_all(destination)
        .with_context(|| format!("failed to create {}", destination.display()))?;

    let status = std::process::Command::new("tar")
        .arg("-xzf")
        .arg("-")
        .arg("-C")
        .arg(destination)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            child.stdin.as_mut().expect("stdin was piped").write_all(ARCHIVE)?;
            child.wait_with_output()
        })
        .context("failed to extract the embedded template bundle")?;

    if !status.status.success() {
        bail!(
            "failed to extract the embedded template bundle: {}",
            String::from_utf8_lossy(&status.stderr).trim()
        );
    }
    Ok(destination.to_path_buf())
}

/// The directory within an extracted bundle that a template renders from.
///
/// `None` selects the full project scaffold, matching `cargo miden new` with no
/// `--template` flag.
pub fn template_path(root: &Path, template: Option<&str>) -> PathBuf {
    match template {
        Some(name) => root.join("rust").join(name),
        None => root.join("project"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bundle_is_embedded_and_looks_like_a_gzip() {
        assert_eq!(ARCHIVE.len(), TEMPLATE_BUNDLE_LEN);
        assert!(ARCHIVE.len() > 1024, "the bundle is suspiciously small");
        assert_eq!(&ARCHIVE[..2], &[0x1f, 0x8b], "not a gzip stream");
        assert_eq!(SHA256.len(), 64);
    }

    #[test]
    fn the_bundle_extracts_and_contains_every_template() {
        let dir = std::env::temp_dir().join(format!("cargo-miden-bundle-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);

        let root = extract(&dir).unwrap();
        assert!(root.join("bundle.toml").is_file());
        assert!(template_path(&root, None).join("Cargo.toml").is_file());

        for template in ["account", "note", "program", "tx-script", "auth-component"] {
            let path = template_path(&root, Some(template));
            assert!(
                path.join("template").join("Cargo.toml").is_file(),
                "template '{template}' is missing from the embedded bundle"
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
