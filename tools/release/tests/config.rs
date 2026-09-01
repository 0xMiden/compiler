//! Schema v2 loading and validation.
//!
//! The negative cases carry the weight. A configuration that loads proves
//! little; what matters is that one naming a unit that does not exist, or
//! declaring a cycle, is refused at load time rather than surfacing as a
//! confusing failure four phases into a release.

use std::path::PathBuf;

use midenc_release::config::{Config, UnitKind, VersionSource};

fn write(body: &str, label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "midenc-release-config-{label}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("config.toml");
    std::fs::write(&path, body).unwrap();
    path
}

fn load(body: &str, label: &str) -> anyhow::Result<Config> {
    Config::load(&write(body, label))
}

/// The whole error chain. `Display` alone gives only the outermost context.
fn err(body: &str, label: &str) -> String {
    format!("{:#}", load(body, label).unwrap_err())
}

const SINGLE_CRATE: &str = r#"
schema-version = 2

[units.main]
kind = "crates"
version-source = "workspace"
tag = "v{version}"
changelog = "CHANGELOG.md"
latest = true
assets = ["mytool-*.tar.gz"]
required-assets = ["mytool-*.tar.gz"]

[[packages]]
name = "mytool"
unit = "main"
"#;

#[test]
fn a_single_crate_repository_loads() {
    let config = load(SINGLE_CRATE, "single").unwrap();
    let unit = config.unit("main").unwrap();
    assert_eq!(unit.kind, UnitKind::Crates);
    assert_eq!(unit.version_source, Some(VersionSource::Workspace));
    assert_eq!(unit.tag(), "v{version}");
    assert!(unit.latest);
    assert_eq!(config.packages_in("main").count(), 1);
}

#[test]
fn headings_default_when_unset() {
    let config = load(SINGLE_CRATE, "headings").unwrap();
    assert_eq!(config.unit("main").unwrap().headings(), ["Added", "Changed", "Fixed"]);
}

#[test]
fn private_version_is_absent_by_default() {
    // Absent means the frozen-version check does not run. Defaulting it would
    // fail lint in a repository whose internal crates sit at 0.0.0, for a
    // policy that repository never opted into.
    assert!(load(SINGLE_CRATE, "privver").unwrap().private_version.is_none());
}

#[test]
fn the_two_kind_axes_are_independent() {
    assert!(UnitKind::Crates.publishes_crates() && UnitKind::Crates.is_releasable());
    assert!(UnitKind::Library.publishes_crates() && !UnitKind::Library.is_releasable());
    assert!(!UnitKind::Artifact.publishes_crates() && UnitKind::Artifact.is_releasable());
    assert!(!UnitKind::Private.publishes_crates() && !UnitKind::Private.is_releasable());
}

#[test]
fn a_wrong_schema_version_is_refused() {
    let body = SINGLE_CRATE.replace("schema-version = 2", "schema-version = 1");
    // This one bails inside `load` itself, before the context wrapper.
    assert!(err(&body, "schemaver").contains("schema version 1"));
}

#[test]
fn a_package_naming_an_undeclared_unit_is_refused() {
    let body = SINGLE_CRATE.replace(r#"unit = "main""#, r#"unit = "nope""#);
    assert!(err(&body, "badunit").contains("'nope'"));
}

#[test]
fn a_package_classified_twice_is_refused() {
    let body = format!("{SINGLE_CRATE}\n[[packages]]\nname = \"mytool\"\nunit = \"main\"\n");
    assert!(err(&body, "dupe").contains("more than once"));
}

#[test]
fn an_after_edge_to_an_undeclared_unit_is_refused() {
    // Written as a literal rather than by string surgery on SINGLE_CRATE: an
    // appended key lands in the [[packages]] table and is rejected for an
    // entirely different reason.
    let body = r#"
schema-version = 2

[units.main]
kind = "crates"
version-source = "workspace"
tag = "v{version}"
changelog = "CHANGELOG.md"
after = ["ghost"]

[[packages]]
name = "mytool"
unit = "main"
"#;
    assert!(err(body, "afteredge").contains("ghost"));
}

#[test]
fn a_cycle_in_after_is_refused() {
    let body = r#"
schema-version = 2

[units.a]
kind = "crates"
version-source = "own"
tag = "a/v{version}"
changelog = "a/CHANGELOG.md"
after = ["b"]

[units.b]
kind = "crates"
version-source = "own"
tag = "b/v{version}"
changelog = "b/CHANGELOG.md"
after = ["a"]

[[packages]]
name = "pa"
unit = "a"

[[packages]]
name = "pb"
unit = "b"
"#;
    assert!(err(body, "cyc").contains("cycle"));
}

#[test]
fn two_units_claiming_latest_are_refused() {
    let body = r#"
schema-version = 2

[units.a]
kind = "crates"
version-source = "own"
tag = "a/v{version}"
changelog = "a/CHANGELOG.md"
latest = true

[units.b]
kind = "crates"
version-source = "own"
tag = "b/v{version}"
changelog = "b/CHANGELOG.md"
latest = true

[[packages]]
name = "pa"
unit = "a"

[[packages]]
name = "pb"
unit = "b"
"#;
    assert!(err(body, "twolat").contains("latest"));
}

/// Sharing the root version means bumping one unit silently moves the other's
/// crates to an unpublished version, and nothing anywhere catches it.
#[test]
fn two_releasable_units_sharing_the_workspace_domain_are_refused() {
    let body = r#"
schema-version = 2

[units.a]
kind = "crates"
version-source = "workspace"
tag = "a/v{version}"
changelog = "a/CHANGELOG.md"

[units.b]
kind = "crates"
version-source = "workspace"
tag = "b/v{version}"
changelog = "b/CHANGELOG.md"

[[packages]]
name = "pa"
unit = "a"

[[packages]]
name = "pb"
unit = "b"
"#;
    assert!(err(body, "sharedws").contains("workspace"));
}

/// But a library unit may share it. That is the normal case, and it is safe
/// because a library unit is never released on its own.
#[test]
fn a_library_unit_may_share_the_workspace_domain() {
    let body = r#"
schema-version = 2

[units.a]
kind = "crates"
version-source = "workspace"
tag = "a/v{version}"
changelog = "a/CHANGELOG.md"

[units.common]
kind = "library"
version-source = "workspace"

[[packages]]
name = "pa"
unit = "a"

[[packages]]
name = "common"
unit = "common"
"#;
    let config = load(body, "libws").unwrap();
    assert_eq!(config.unit("common").unwrap().kind, UnitKind::Library);
    assert_eq!(config.order().unwrap(), ["a"], "a library unit is not released");
}

#[test]
fn a_library_unit_declaring_a_tag_is_refused() {
    let body = r#"
schema-version = 2

[units.common]
kind = "library"
version-source = "workspace"
tag = "common/v{version}"

[[packages]]
name = "common"
unit = "common"
"#;
    assert!(err(body, "libtag").contains("library"));
}

#[test]
fn a_crates_unit_with_no_packages_is_refused() {
    let body = r#"
schema-version = 2

[units.main]
kind = "crates"
version-source = "workspace"
tag = "v{version}"
changelog = "CHANGELOG.md"
"#;
    assert!(err(body, "nopkgs").contains("no packages"));
}

#[test]
fn an_artifact_unit_with_packages_is_refused() {
    let body = r#"
schema-version = 2

[units.bundle]
kind = "artifact"
tag = "bundle/v{version}"
changelog = "CHANGELOG.md"

[units.bundle.source]
directory = "templates"
include = ["demo"]

[[packages]]
name = "p"
unit = "bundle"
"#;
    assert!(err(body, "artpkgs").contains("artifact"));
}

#[test]
fn a_crates_unit_without_a_version_source_is_refused() {
    let body = SINGLE_CRATE.replace("version-source = \"workspace\"\n", "");
    assert!(err(&body, "novs").contains("version-source"));
}

#[test]
fn an_artifact_unit_without_a_source_is_refused() {
    let body = r#"
schema-version = 2

[units.bundle]
kind = "artifact"
tag = "bundle/v{version}"
changelog = "CHANGELOG.md"
"#;
    assert!(err(body, "nosrc").contains("source"));
}

#[test]
fn a_directory_source_needs_exactly_one_include_list() {
    let neither = r#"
schema-version = 2

[units.bundle]
kind = "artifact"
tag = "bundle/v{version}"
changelog = "CHANGELOG.md"

[units.bundle.source]
directory = "templates"
"#;
    assert!(err(neither, "noinc").contains("include"));

    let both = r#"
schema-version = 2

[units.bundle]
kind = "artifact"
tag = "bundle/v{version}"
changelog = "CHANGELOG.md"

[units.bundle.source]
directory = "templates"
include = ["demo"]
manifest = "bundle.toml"
"#;
    assert!(err(both, "bothinc").contains("include"));
}

#[test]
fn an_artifact_source_with_both_directory_and_file_is_refused() {
    let body = r#"
schema-version = 2

[units.bundle]
kind = "artifact"
tag = "bundle/v{version}"
changelog = "CHANGELOG.md"

[units.bundle.source]
directory = "templates"
include = ["demo"]
file = "dist/thing.tar.gz"
"#;
    assert!(err(body, "dirfile").contains("directory"));
}

#[test]
fn a_private_unit_declaring_a_tag_is_refused() {
    let body =
        format!("{SINGLE_CRATE}\n[units.private]\nkind = \"private\"\ntag = \"p/v{{version}}\"\n");
    assert!(err(&body, "privtag").contains("private"));
}

#[test]
fn tracks_targeting_a_non_crates_unit_is_refused() {
    let body = r#"
schema-version = 2

[units.a]
kind = "artifact"
tag = "a/v{version}"
changelog = "a/CHANGELOG.md"

[units.a.source]
directory = "a"
manifest = "bundle.toml"

[units.b]
kind = "artifact"
tag = "b/v{version}"
changelog = "b/CHANGELOG.md"

[units.b.source]
directory = "b"
manifest = "bundle.toml"

[units.a.tracks.b]
packages = ["x"]
"#;
    assert!(err(body, "trackart").contains("crates"));
}

#[test]
fn tracks_naming_a_package_outside_the_tracked_unit_is_refused() {
    let body = r#"
schema-version = 2

[units.lib]
kind = "crates"
version-source = "own"
tag = "lib/v{version}"
changelog = "lib/CHANGELOG.md"

[units.bundle]
kind = "artifact"
tag = "bundle/v{version}"
changelog = "bundle/CHANGELOG.md"

[units.bundle.source]
directory = "templates"
manifest = "bundle.toml"

[units.bundle.tracks.lib]
packages = ["elsewhere"]

[[packages]]
name = "thelib"
unit = "lib"
"#;
    assert!(err(body, "trackpkg").contains("elsewhere"));
}

/// `tracks` implies publish order, because a unit embedding a requirement on
/// another cannot resolve until that other unit is on the registry. It does
/// NOT imply co-release; that is decided from content, in `candidate::validate`.
#[test]
fn tracks_implies_after_but_not_release_when() {
    let body = r#"
schema-version = 2

[units.lib]
kind = "crates"
version-source = "own"
tag = "lib/v{version}"
changelog = "lib/CHANGELOG.md"

[units.bundle]
kind = "artifact"
tag = "bundle/v{version}"
changelog = "bundle/CHANGELOG.md"

[units.bundle.source]
directory = "templates"
manifest = "bundle.toml"

[units.bundle.tracks.lib]
packages = ["thelib"]

[[packages]]
name = "thelib"
unit = "lib"
"#;
    let config = load(body, "tracksafter").unwrap();
    let bundle = config.unit("bundle").unwrap();
    assert!(bundle.after_all().contains("lib"), "tracks must imply after");
    assert!(bundle.release_when.is_empty(), "tracks must not imply release-when");
    assert_eq!(bundle.requirement_key("lib"), "lib-requirement");
    assert_eq!(config.order().unwrap(), ["lib", "bundle"]);
}
