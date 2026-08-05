//! Deterministic `.tar.gz` archives.
//!
//! Release artifacts are identified by digest, so an archive built twice from
//! the same bytes must *be* the same bytes. Everything a tar archive normally
//! records about the moment of creation — timestamps, ownership, the order the
//! filesystem happened to return entries in — is therefore fixed, and gzip's
//! own header fields are zeroed.
//!
//! The tar writer is hand-rolled because the determinism rules are the whole
//! point and are easier to guarantee than to verify through a library. Gzip is
//! delegated, because DEFLATE is a solved problem and reimplementing it would
//! buy nothing.

use std::io::Write;

use anyhow::{Result, bail};
use flate2::{Compression, GzBuilder};

/// One file in an archive.
pub struct Entry {
    /// Path inside the archive. Always relative, always `/`-separated.
    pub path: String,
    pub bytes: Vec<u8>,
    /// Executables need the bit; everything else does not.
    pub executable: bool,
}

/// Build a `.tar.gz` whose bytes depend only on its contents.
///
/// Entries are sorted by path, so the caller cannot make the output depend on
/// directory iteration order by accident.
pub fn tar_gz(mut entries: Vec<Entry>) -> Result<Vec<u8>> {
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    entries.dedup_by(|a, b| a.path == b.path);

    let mut tar = Vec::new();
    for entry in &entries {
        write_entry(&mut tar, entry)?;
    }
    // A tar archive ends with two zero blocks.
    tar.extend_from_slice(&[0u8; 1024]);

    let mut gz = GzBuilder::new()
        // No filename and no timestamp: both would leak the moment of creation
        // into the archive's digest.
        .mtime(0)
        .write(Vec::new(), Compression::new(6));
    gz.write_all(&tar)?;
    Ok(gz.finish()?)
}

fn write_entry(out: &mut Vec<u8>, entry: &Entry) -> Result<()> {
    if entry.path.len() > 100 {
        // ustar can split long paths across the `prefix` field. Nothing this
        // repository ships comes close, so the case is refused rather than
        // half-implemented.
        bail!(
            "archive path '{}' exceeds 100 bytes; ustar prefix splitting is not implemented",
            entry.path
        );
    }

    let mut header = [0u8; 512];
    write_str(&mut header[0..100], &entry.path);
    write_octal(&mut header[100..108], if entry.executable { 0o755 } else { 0o644 });
    write_octal(&mut header[108..116], 0); // uid
    write_octal(&mut header[116..124], 0); // gid
    write_octal(&mut header[124..136], entry.bytes.len() as u64);
    write_octal(&mut header[136..148], 0); // mtime
    header[156] = b'0'; // regular file
    header[257..263].copy_from_slice(b"ustar\0");
    header[263..265].copy_from_slice(b"00");
    // Owner and group names are left empty: a name would record who happened to
    // run the build.

    // The checksum is computed with its own field treated as spaces.
    header[148..156].fill(b' ');
    let checksum: u32 = header.iter().map(|byte| *byte as u32).sum();
    write_octal(&mut header[148..154], checksum as u64);
    header[154] = 0;
    header[155] = b' ';

    out.extend_from_slice(&header);
    out.extend_from_slice(&entry.bytes);

    // Pad the data out to a 512-byte boundary.
    let remainder = entry.bytes.len() % 512;
    if remainder != 0 {
        out.extend(std::iter::repeat_n(0u8, 512 - remainder));
    }
    Ok(())
}

fn write_str(field: &mut [u8], value: &str) {
    field[..value.len()].copy_from_slice(value.as_bytes());
}

fn write_octal(field: &mut [u8], value: u64) {
    let text = format!("{:0width$o}", value, width = field.len() - 1);
    field[..text.len()].copy_from_slice(text.as_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(path: &str, bytes: &[u8]) -> Entry {
        Entry {
            path: path.to_string(),
            bytes: bytes.to_vec(),
            executable: false,
        }
    }

    #[test]
    fn archives_are_byte_identical_across_builds() {
        let build = || tar_gz(vec![entry("b.txt", b"second"), entry("a.txt", b"first")]).unwrap();
        let first = build();
        for _ in 0..8 {
            assert_eq!(build(), first, "an archive's bytes must depend only on its contents");
        }
    }

    #[test]
    fn entry_order_does_not_affect_the_result() {
        let one = tar_gz(vec![entry("a.txt", b"first"), entry("b.txt", b"second")]).unwrap();
        let two = tar_gz(vec![entry("b.txt", b"second"), entry("a.txt", b"first")]).unwrap();
        assert_eq!(one, two, "entries are sorted, so caller order cannot leak in");
    }

    #[test]
    fn different_contents_produce_different_archives() {
        let one = tar_gz(vec![entry("a.txt", b"first")]).unwrap();
        let two = tar_gz(vec![entry("a.txt", b"changed")]).unwrap();
        assert_ne!(one, two);
    }

    #[test]
    fn the_result_is_a_real_tar_gz() {
        // Round-trip through the system tar, so this is not just self-consistent.
        let archive = tar_gz(vec![
            entry("dir/file.txt", b"contents"),
            Entry {
                path: "bin/tool".into(),
                bytes: b"#!/bin/sh\n".to_vec(),
                executable: true,
            },
        ])
        .unwrap();

        let dir = std::env::temp_dir().join(format!("archive-roundtrip-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test.tar.gz");
        std::fs::write(&path, &archive).unwrap();

        let status = std::process::Command::new("tar")
            .arg("-xzf")
            .arg(&path)
            .current_dir(&dir)
            .status()
            .unwrap();
        assert!(status.success(), "the system tar must be able to read what we produce");

        assert_eq!(std::fs::read(dir.join("dir/file.txt")).unwrap(), b"contents");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dir.join("bin/tool")).unwrap().permissions().mode();
            assert_eq!(mode & 0o111, 0o111, "the executable bit survives");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn overlong_paths_are_refused_rather_than_silently_truncated() {
        let long = format!("{}.txt", "a".repeat(120));
        let err = tar_gz(vec![entry(&long, b"x")]).unwrap_err().to_string();
        assert!(err.contains("exceeds 100 bytes"), "{err}");
    }
}
