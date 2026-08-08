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
pub fn resolve(destination: &Path, fetch: Fetch) -> Result<Resolved> {
    if let Some(fetched) = fetch_released(fetch)? {
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

/// How hard to try for a released bundle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fetch {
    /// Use a released bundle when one is reachable, otherwise the embedded copy.
    IfAvailable,
    /// Insist on downloading a released bundle, and fail if that is not
    /// possible.
    ///
    /// Two uses: exercising the download path, which is otherwise only reached
    /// once a release newer than the embedded copy exists, and working around a
    /// bad embedded bundle in a binary already installed.
    Required,
}

struct Fetched {
    version: String,
    archive: Vec<u8>,
}

/// Decide what a failed lookup means.
///
/// Under [`Fetch::IfAvailable`] it means "use what is embedded", because
/// failing to create a project is far worse than creating one from slightly
/// older templates. Under [`Fetch::Required`] the caller asked for the released
/// bundle specifically, so silently substituting a different one would defeat
/// the request.
fn give_up<T>(fetch: Fetch, message: String) -> Result<Option<T>> {
    match fetch {
        Fetch::Required => bail!("{message}, and --force-download rules out the embedded copy"),
        Fetch::IfAvailable => {
            warn(&message);
            Ok(None)
        }
    }
}

/// One attempt at the newest compatible released bundle.
///
/// `Ok(None)` means "carry on with what is embedded"; `Err` is reserved for a
/// bundle that was found but is not what it claims to be, and for any failure
/// at all under [`Fetch::Required`].
fn fetch_released(fetch: Fetch) -> Result<Option<Fetched>> {
    let Some(accepted) = MinorSeries::of(VERSION) else {
        return give_up(fetch, format!("the embedded bundle version ({VERSION}) is unreadable"));
    };

    // Matching refs rather than the release list: this returns only template
    // tags, so it does not degrade as compiler and SDK releases accumulate.
    let Some(body) = http_get(&format!(
        "https://api.github.com/repos/{REPOSITORY}/git/matching-refs/tags/{TAG_PREFIX}"
    )) else {
        return give_up(fetch, "could not reach GitHub to look for templates".to_string());
    };
    let Ok(refs) = serde_json::from_slice::<Vec<serde_json::Value>>(&body) else {
        return give_up(fetch, "GitHub returned an unreadable list of template tags".to_string());
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
        return give_up(fetch, format!("no templates release exists in the {accepted} series"));
    };

    // Only ever move *forward*. Comparing for inequality instead would download
    // whatever the newest tag happens to be, which is a downgrade whenever this
    // build's bundle is ahead of the last published one -- the normal state of
    // any commit that has changed the templates but not yet released them. It
    // is also a downgrade attack: deleting a release, or tampering with the tag
    // list, walks clients back onto older templates, and the digest check
    // cannot object because the older release's digest is genuine.
    //
    // `--force-download` is exempt: it exists to fetch the released bundle, and
    // refusing to would defeat it. It says so out loud instead.
    let embedded = Version::parse(VERSION);
    if fetch == Fetch::IfAvailable && embedded.as_ref().is_some_and(|current| newest <= *current) {
        return Ok(None);
    }
    if embedded.as_ref().is_some_and(|current| newest < *current) {
        warn_always(&format!(
            "templates {} is older than the copy built into this binary ({VERSION})",
            newest.text
        ));
    }

    let Some(body) = http_get(&format!(
        "https://api.github.com/repos/{REPOSITORY}/releases/tags/{TAG_PREFIX}{}",
        newest.text
    )) else {
        // A tag with no release behind it is normal: tags are created before a
        // release is finalized, and an abandoned release leaves one behind.
        return give_up(
            fetch,
            format!("templates {} has a tag but no readable release", newest.text),
        );
    };
    let Ok(release) = serde_json::from_slice::<serde_json::Value>(&body) else {
        return give_up(fetch, "GitHub returned an unreadable release".to_string());
    };

    let Some(asset) = release["assets"]
        .as_array()
        .and_then(|assets| assets.iter().find(|asset| asset["name"] == "templates.tar.gz"))
    else {
        return give_up(fetch, format!("templates {} carries no templates.tar.gz", newest.text));
    };

    let Some(url) = asset["browser_download_url"].as_str() else {
        return give_up(fetch, format!("templates {} has no download URL", newest.text));
    };
    let Some(archive) = http_get(url) else {
        return give_up(fetch, format!("could not download templates {}", newest.text));
    };

    // The digest comes from the API response, not from an asset beside the
    // archive, so a substituted download cannot supply its own checksum.
    //
    // Absent or unrecognised is a hard failure, not a skip. GitHub documents
    // this field as nullable, so `null` is a legal response -- and treating
    // that as "no verification needed" would mean rendering unverified bytes
    // into code the user is about to compile, silently, exactly when the
    // integrity control is unavailable. `strip_prefix` rather than
    // `trim_start_matches`, which would also accept an unprefixed digest or a
    // repeated prefix.
    let Some(expected) = asset["digest"].as_str().and_then(|d| d.strip_prefix("sha256:")) else {
        bail!(
            "GitHub reports no usable sha256 digest for the templates {} archive, so it cannot be \
             verified; refusing to render templates from unverified bytes",
            newest.text
        );
    };
    let actual = sha256_hex(&archive);
    if actual != expected {
        bail!(
            "the templates {} archive does not match the digest GitHub reports for it (expected \
             {expected}, got {actual}); refusing to render templates from it",
            newest.text
        );
    }

    // Same version, different bytes. Only reachable under `--force-download`,
    // since otherwise an equal version never downloads -- and that is precisely
    // when someone is trying to find out what a release actually contains.
    if newest.text == VERSION && actual != SHA256 {
        warn_always(&format!(
            "templates {} was released with different content than the copy built into this \
             binary (released {}, embedded {}); the version was reused",
            newest.text,
            &actual[..16],
            &SHA256[..16]
        ));
    }

    Ok(Some(Fetched {
        version: newest.text,
        archive,
    }))
}

/// A version, kept beside its original text so a tag can be reconstructed.
///
/// `Eq` is derived from the ordering rather than from the fields, because the
/// two must agree: `0.32` and `0.32.0` are the same version with different text,
/// and a type whose `cmp` says `Equal` while `==` says `false` breaks the
/// contract `sort`, `max` and `dedup` rely on.
#[derive(Debug, Clone)]
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
        // A trailing `-` leaves an empty prerelease, which would otherwise read
        // as a stable release and let a stable build accept `0.14.0-`.
        if text.contains('-') && pre.is_empty() {
            return None;
        }

        let mut parts = core.split('.');
        let major = numeric(parts.next()?)?;
        let minor = numeric(parts.next()?)?;
        let patch = parts.next().map_or(Some(0), numeric)?;
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

/// A SemVer numeric identifier: digits only, and no leading zero.
///
/// `u64::from_str` alone would accept `+32` and `032`, whose text is then
/// reused verbatim to build a release URL.
fn numeric(field: &str) -> Option<u64> {
    if field.is_empty() || !field.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    if field.len() > 1 && field.starts_with('0') {
        return None;
    }
    field.parse().ok()
}

/// Compare prerelease identifiers by SemVer precedence.
///
/// Dot-separated, compared field by field: numeric identifiers numerically, and
/// a numeric identifier always below an alphanumeric one. A plain string
/// comparison puts `rc.10` below `rc.2`, which for a project releasing `-rc.N`
/// means the resolver stops finding the newest release at the tenth candidate.
fn compare_pre(left: &str, right: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;

    let mut left = left.split('.');
    let mut right = right.split('.');
    loop {
        return match (left.next(), right.next()) {
            (None, None) => Ordering::Equal,
            // A shorter run of identifiers has lower precedence.
            (None, Some(_)) => Ordering::Less,
            (Some(_), None) => Ordering::Greater,
            (Some(a), Some(b)) => {
                let ordering = match (numeric(a), numeric(b)) {
                    (Some(a), Some(b)) => a.cmp(&b),
                    (Some(_), None) => Ordering::Less,
                    (None, Some(_)) => Ordering::Greater,
                    (None, None) => a.cmp(b),
                };
                if ordering == Ordering::Equal {
                    continue;
                }
                ordering
            }
        };
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
                (false, false) => compare_pre(&self.pre, &other.pre),
            })
    }
}

impl PartialEq for Version {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == std::cmp::Ordering::Equal
    }
}

impl Eq for Version {}

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

impl std::fmt::Display for MinorSeries {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
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

/// A warning that is not about falling back to the embedded copy.
fn warn_always(message: &str) {
    eprintln!("warning: {message}");
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
        // The bundle is ~100 KB and release assets may be 2 GB. A timeout is
        // not a size bound: a fast link delivers a great deal in 20 seconds,
        // and the result is read entirely into memory.
        .args(["--max-filesize", "33554432"])
        .args(["--header", "User-Agent: cargo-miden"]);

    // An unauthenticated client shares a per-IP rate limit with everyone behind
    // the same NAT, and CI runners exhaust it routinely.
    //
    // The token goes to curl on **stdin**, never in argv: process arguments are
    // world-readable on Linux via /proc/<pid>/cmdline, and `cargo miden new` is
    // routinely run in CI where GITHUB_TOKEN carries write scopes.
    let token = std::env::var("GITHUB_TOKEN")
        .or_else(|_| std::env::var("GH_TOKEN"))
        .ok()
        .filter(|token| !token.is_empty());

    let mut config = String::from("header = \"Accept: application/vnd.github+json\"\n");
    if let Some(token) = &token {
        // curl's config format takes a double-quoted, backslash-escaped value.
        let escaped = token.replace('\\', "\\\\").replace('"', "\\\"");
        config.push_str(&format!("header = \"Authorization: Bearer {escaped}\"\n"));
    }
    command.args(["--config", "-"]);

    let output = run_with_stdin(command.arg(url), config.as_bytes()).ok()?;
    output.status.success().then_some(output.stdout)
}

fn run_with_stdin(
    command: &mut std::process::Command,
    stdin: &[u8],
) -> std::io::Result<std::process::Output> {
    use std::io::Write;
    let mut child = command
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;
    child.stdin.as_mut().expect("stdin was piped").write_all(stdin)?;
    child.wait_with_output()
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

    /// Prerelease identifiers are compared field by field, numerically where
    /// they are numeric. A string comparison puts `rc.10` below `rc.2`, so a
    /// project releasing `-rc.N` stops finding its newest release at the tenth.
    #[test]
    fn prerelease_identifiers_compare_numerically() {
        let mut versions = [
            v("0.32.0-rc.2"),
            v("0.32.0-rc.11"),
            v("0.32.0-rc.1"),
            v("0.32.0-rc.9"),
            v("0.32.0-rc.10"),
        ];
        versions.sort();
        let ordered: Vec<&str> = versions.iter().map(|version| version.text.as_str()).collect();
        assert_eq!(
            ordered,
            ["0.32.0-rc.1", "0.32.0-rc.2", "0.32.0-rc.9", "0.32.0-rc.10", "0.32.0-rc.11"]
        );

        // The remaining SemVer precedence rules for prerelease identifiers.
        assert!(v("1.0.0-alpha") < v("1.0.0-alpha.1"), "fewer identifiers rank lower");
        assert!(v("1.0.0-alpha.1") < v("1.0.0-alpha.beta"), "numeric ranks below alphanumeric");
        assert!(v("1.0.0-beta") < v("1.0.0-beta.2"));
        assert!(v("1.0.0-rc.1") < v("1.0.0"), "a prerelease ranks below its release");
    }

    /// `cmp` and `==` have to agree, or `sort`, `max` and `dedup` misbehave.
    /// `0.32` and `0.32.0` are the same version written two ways.
    #[test]
    fn equality_agrees_with_ordering() {
        assert_eq!(v("0.32"), v("0.32.0"));
        assert_eq!(v("0.32").cmp(&v("0.32.0")), std::cmp::Ordering::Equal);
        assert_ne!(v("0.32.0"), v("0.32.1"));
    }

    /// Malformed versions must not become candidates: `text` is reused verbatim
    /// to build the release URL, and a trailing `-` would otherwise read as a
    /// stable release.
    #[test]
    fn malformed_versions_are_rejected() {
        for bad in ["", "0.32.0-", "0.+32.0", "00.032.0", "0.32.0.1", "templates", "0.x.0"] {
            assert!(Version::parse(bad).is_none(), "'{bad}' should not parse");
        }
        assert!(Version::parse("0.32").is_some(), "a two-part version means patch 0");
        assert!(Version::parse("0.32.0-rc.1").is_some());
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
