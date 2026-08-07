//! Where `cargo miden new` gets its templates.
//!
//! Templates are released on their own cadence, as `templates/v*`, so a
//! `cargo-miden` already installed has to be able to pick up template fixes
//! without being reinstalled. That is why they are resolved at runtime rather
//! than simply compiled in, and it is what the previous scheme achieved by
//! moving a git tag — a mutable pointer this replaces with immutable, digest-
//! verified releases.
//!
//! [`resolve`] makes **one** attempt at the newest release in the embedded
//! copy's minor series, and falls back to the embedded copy on any failure to
//! reach or read GitHub. The embedded copy is therefore a floor: `cargo miden
//! new` works offline, and works when GitHub does not.
//!
//! Deliberately absent for now, and specified in the design's §12.3–12.4: a
//! cache (so every invocation queries), walking down to older candidates when
//! the newest tag has no release behind it, `--template-version`, `--offline`,
//! and the `deny.json` retraction list.
//!
//! `release-tool lint` proves the committed archive still matches the template
//! sources, and a compiler release proves its embedded copy is byte-identical
//! to the released archive by comparing [`SHA256`].

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

include!(concat!(env!("OUT_DIR"), "/template_bundle.rs"));

/// The bundle archive, embedded at compile time.
pub const ARCHIVE: &[u8] = include_bytes!("../templates.tar.gz");

/// SHA-256 of [`ARCHIVE`].
pub const SHA256: &str = TEMPLATE_BUNDLE_SHA256;

/// Version of [`ARCHIVE`], and the version whose minor series this build accepts.
pub const VERSION: &str = TEMPLATE_BUNDLE_VERSION;

const REPOSITORY: &str = "0xMiden/compiler";
const TAG_PREFIX: &str = "templates/v";

/// Where the templates being rendered came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// A `templates/v*` release, newer than the embedded copy.
    Released {
        /// The bundle version that was fetched.
        version: String,
    },
    /// The copy compiled into this binary.
    Embedded,
}

/// Templates on disk, and where they came from.
#[derive(Debug)]
pub struct Resolved {
    /// Directory the bundle was extracted into.
    pub root: PathBuf,
    /// Which copy was used.
    pub source: Source,
}

/// Obtain templates, preferring a released bundle over the embedded one.
///
/// The embedded copy is a floor, not the answer: templates are released
/// independently of the compiler, so a `cargo-miden` in the field must be able
/// to pick up template fixes without being reinstalled. That is the whole
/// reason the bundle is resolved at runtime rather than simply compiled in.
///
/// Exactly one attempt is made. Any failure to reach or read GitHub — offline,
/// rate-limited, a tag with no release behind it — falls back to the embedded
/// bundle with a warning, because failing to create a project is a far worse
/// outcome than creating one from slightly older templates. A *digest mismatch*
/// is not such a failure and is not tolerated: it means the bytes are not the
/// ones the release claims, and quietly using the embedded copy instead would
/// hide that.
pub fn resolve(destination: &Path) -> Result<Resolved> {
    if let Some(fetched) = fetch_released()? {
        let root = extract_bytes(&fetched.archive, destination)
            .context("failed to extract the released template bundle")?;
        return Ok(Resolved {
            root,
            source: Source::Released {
                version: fetched.version,
            },
        });
    }

    let root = extract(destination)?;
    Ok(Resolved {
        root,
        source: Source::Embedded,
    })
}

struct Fetched {
    version: String,
    archive: Vec<u8>,
}

/// One attempt at the newest compatible released bundle.
///
/// `Ok(None)` means "carry on with what is embedded"; `Err` is reserved for a
/// bundle that was found but is not what it claims to be.
fn fetch_released() -> Result<Option<Fetched>> {
    let Some(accepted) = MinorSeries::of(VERSION) else {
        return Ok(None);
    };

    // Matching refs rather than the release list: this returns only template
    // tags, so it does not degrade as compiler and SDK releases accumulate.
    let Some(body) = http_get(&format!(
        "https://api.github.com/repos/{REPOSITORY}/git/matching-refs/tags/{TAG_PREFIX}"
    )) else {
        warn("could not reach GitHub to look for newer templates");
        return Ok(None);
    };
    let Ok(refs) = serde_json::from_slice::<Vec<serde_json::Value>>(&body) else {
        warn("GitHub returned an unreadable list of template tags");
        return Ok(None);
    };

    // The endpoint orders lexicographically, which puts v10 below v2, so the
    // ordering has to be redone here.
    let mut candidates: Vec<Version> = refs
        .iter()
        .filter_map(|entry| entry["ref"].as_str())
        .filter_map(|name| name.rsplit_once(TAG_PREFIX).map(|(_, version)| version))
        .filter_map(Version::parse)
        .filter(|version| accepted.accepts(version))
        .collect();
    candidates.sort();

    let Some(newest) = candidates.pop() else {
        // Nothing published in this series yet: the embedded copy is current.
        return Ok(None);
    };
    if newest.text == VERSION {
        // Already holding it; no download needed.
        return Ok(None);
    }

    let Some(body) = http_get(&format!(
        "https://api.github.com/repos/{REPOSITORY}/releases/tags/{TAG_PREFIX}{}",
        newest.text
    )) else {
        // A tag with no release behind it is normal: tags are created before a
        // release is finalized, and an abandoned release leaves one behind.
        warn(&format!("templates {} has a tag but no readable release", newest.text));
        return Ok(None);
    };
    let Ok(release) = serde_json::from_slice::<serde_json::Value>(&body) else {
        warn("GitHub returned an unreadable release");
        return Ok(None);
    };

    let Some(asset) = release["assets"]
        .as_array()
        .and_then(|assets| assets.iter().find(|asset| asset["name"] == "templates.tar.gz"))
    else {
        warn(&format!("templates {} carries no templates.tar.gz", newest.text));
        return Ok(None);
    };

    let Some(url) = asset["browser_download_url"].as_str() else {
        warn(&format!("templates {} has no download URL", newest.text));
        return Ok(None);
    };
    let Some(archive) = http_get(url) else {
        warn(&format!("could not download templates {}", newest.text));
        return Ok(None);
    };

    // The digest comes from the API response, not from an asset beside the
    // archive, so a substituted download cannot supply its own checksum.
    let expected = asset["digest"].as_str().map(|digest| digest.trim_start_matches("sha256:"));
    if let Some(expected) = expected {
        let actual = sha256_hex(&archive);
        if actual != expected {
            bail!(
                "the templates {} archive does not match the digest GitHub reports for it \
                 (expected {expected}, got {actual}); refusing to render templates from it",
                newest.text
            );
        }
    }

    Ok(Some(Fetched {
        version: newest.text,
        archive,
    }))
}

/// A version, kept beside its original text so a tag can be reconstructed.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Version {
    major: u64,
    minor: u64,
    patch: u64,
    /// Empty for a stable release.
    pre: String,
    text: String,
}

impl Version {
    fn parse(text: &str) -> Option<Self> {
        let (core, pre) = match text.split_once('-') {
            Some((core, pre)) => (core, pre.to_string()),
            None => (text, String::new()),
        };
        let mut parts = core.split('.');
        let major = parts.next()?.parse().ok()?;
        let minor = parts.next()?.parse().ok()?;
        let patch = parts.next().unwrap_or("0").parse().ok()?;
        if parts.next().is_some() {
            return None;
        }
        Some(Self {
            major,
            minor,
            patch,
            pre,
            text: text.to_string(),
        })
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // SemVer precedence: a prerelease sorts below its own release.
        (self.major, self.minor, self.patch)
            .cmp(&(other.major, other.minor, other.patch))
            .then_with(|| match (self.pre.is_empty(), other.pre.is_empty()) {
                (true, true) => std::cmp::Ordering::Equal,
                (true, false) => std::cmp::Ordering::Greater,
                (false, true) => std::cmp::Ordering::Less,
                (false, false) => self.pre.cmp(&other.pre),
            })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// The minor series this build accepts, derived from the embedded bundle.
struct MinorSeries {
    major: u64,
    minor: u64,
    /// Whether this build is itself a prerelease, and so may use prerelease
    /// bundles. A stable client must never be pulled onto one.
    prerelease: bool,
}

impl MinorSeries {
    fn of(version: &str) -> Option<Self> {
        let parsed = Version::parse(version)?;
        Some(Self {
            major: parsed.major,
            minor: parsed.minor,
            prerelease: !parsed.pre.is_empty(),
        })
    }

    fn accepts(&self, version: &Version) -> bool {
        version.major == self.major
            && version.minor == self.minor
            && (self.prerelease || version.pre.is_empty())
    }
}

fn warn(message: &str) {
    eprintln!("warning: {message}; using the templates built into cargo-miden ({VERSION})");
}

/// A single GET, yielding `None` for anything that is not a clean 200.
///
/// `curl` rather than an HTTP client crate: it keeps a TLS stack out of the
/// dependency graph of a published binary, and it is present wherever Cargo is.
fn http_get(url: &str) -> Option<Vec<u8>> {
    let mut command = std::process::Command::new("curl");
    command
        .args(["--silent", "--show-error", "--location", "--fail"])
        // Redirects are followed to reach the asset host, but only ever over
        // HTTPS.
        .args(["--proto", "=https", "--proto-redir", "=https"])
        .args(["--max-time", "20"])
        .args(["--header", "Accept: application/vnd.github+json"])
        .args(["--header", "User-Agent: cargo-miden"]);

    // An unauthenticated client shares a per-IP rate limit with everyone behind
    // the same NAT, and CI runners exhaust it routinely.
    if let Some(token) = std::env::var("GITHUB_TOKEN")
        .or_else(|_| std::env::var("GH_TOKEN"))
        .ok()
        .filter(|token| !token.is_empty())
    {
        command.args(["--header", &format!("Authorization: Bearer {token}")]);
    }

    let output = command.arg(url).output().ok()?;
    output.status.success().then_some(output.stdout)
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Extract the embedded bundle into `destination`, returning the root it was
/// written to.
pub fn extract(destination: &Path) -> Result<PathBuf> {
    extract_bytes(ARCHIVE, destination).context("failed to extract the embedded template bundle")
}

/// Extract a bundle archive into `destination`.
///
/// A bundle fetched from a release is data from the network, so extraction goes
/// through `tar`'s own path handling rather than anything hand-rolled here.
fn extract_bytes(archive: &[u8], destination: &Path) -> Result<PathBuf> {
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
            child.stdin.as_mut().expect("stdin was piped").write_all(archive)?;
            child.wait_with_output()
        })
        .context("failed to run tar")?;

    if !status.status.success() {
        bail!(
            "failed to extract the template bundle: {}",
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

    fn v(text: &str) -> Version {
        Version::parse(text).expect("parses")
    }

    #[test]
    fn the_embedded_version_matches_the_archive() {
        // Read out of the archive by the build script, so it cannot drift from
        // the bundle it describes.
        assert!(Version::parse(VERSION).is_some(), "VERSION is not a version: {VERSION}");
    }

    #[test]
    fn versions_order_by_precedence_not_lexicographically() {
        let mut versions = [v("0.32.0"), v("0.32.10"), v("0.32.2"), v("0.32.0-rc.1")];
        versions.sort();
        let ordered: Vec<&str> = versions.iter().map(|version| version.text.as_str()).collect();
        assert_eq!(
            ordered,
            ["0.32.0-rc.1", "0.32.0", "0.32.2", "0.32.10"],
            "a prerelease sorts below its release, and 10 above 2"
        );
    }

    /// The accepted range is "the embedded copy's minor series", so a template
    /// release can reach installed clients but a *minor* bump cannot drag them
    /// onto templates their compiler was never tested against.
    #[test]
    fn only_the_embedded_minor_series_is_accepted() {
        let stable = MinorSeries::of("0.32.0").unwrap();

        assert!(stable.accepts(&v("0.32.1")), "a patch in the series is the point");
        assert!(stable.accepts(&v("0.32.99")));
        assert!(!stable.accepts(&v("0.33.0")), "the next minor is a different series");
        assert!(!stable.accepts(&v("0.31.9")), "an older series is not an upgrade path");
        assert!(!stable.accepts(&v("1.32.0")), "a different major is unrelated");
    }

    /// A stable client must never be pulled onto a prerelease bundle; a
    /// prerelease client is already opted in and may use them.
    #[test]
    fn prereleases_are_reachable_only_from_a_prerelease_build() {
        let stable = MinorSeries::of("0.32.0").unwrap();
        let prerelease = MinorSeries::of("0.32.0-rc.1").unwrap();

        assert!(!stable.accepts(&v("0.32.1-rc.1")));
        assert!(prerelease.accepts(&v("0.32.1-rc.1")));
        assert!(prerelease.accepts(&v("0.32.1")), "a prerelease build may take a stable bundle");
    }

    #[test]
    fn a_nonsense_version_is_not_a_version() {
        assert!(Version::parse("").is_none());
        assert!(Version::parse("0.32").is_some(), "a two-part version means patch 0");
        assert!(Version::parse("0.32.0.1").is_none(), "four parts is not SemVer");
        assert!(Version::parse("templates").is_none());
    }

    /// The layout the resolver hands to the renderer has to match what is
    /// actually inside the archive, for both shapes of `cargo miden new`.
    #[test]
    fn the_bundle_layout_matches_what_new_asks_for() {
        let dir = std::env::temp_dir().join(format!("bundle-layout-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let root = extract(&dir).unwrap();

        // `cargo miden new NAME` renders `project/`.
        assert!(root.join("project").join("Cargo.toml").is_file());
        // `cargo miden new NAME --account` renders `rust/account/template/`.
        for template in ["account", "note", "program", "tx-script", "auth-component"] {
            assert!(
                root.join("rust").join(template).join("template").join("Cargo.toml").is_file(),
                "template '{template}' is not where new_project.rs looks for it"
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

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
