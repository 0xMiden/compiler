//! Unit ordering, library closure, co-release, and asset routing over fixture
//! configurations.
//!
//! These use small synthetic repositories rather than this one, because the
//! point is that the tool no longer knows anything about this repository.

use std::path::PathBuf;

use midenc_release::config::Config;

pub fn load(body: &str, label: &str) -> Config {
    let dir = std::env::temp_dir().join(format!(
        "midenc-release-units-{label}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path: PathBuf = dir.join("config.toml");
    std::fs::write(&path, body).unwrap();
    Config::load(&path).unwrap()
}

/// The shape this repository has.
pub const THREE_UNITS: &str = r#"
schema-version = 2

[units.sdk]
kind = "crates"
version-source = "own"
tag = "sdk/v{version}"
changelog = "sdk/CHANGELOG.md"

[units.templates]
kind = "artifact"
tag = "templates/v{version}"
changelog = "templates/CHANGELOG.md"
assets = ["templates.tar.gz"]

[units.templates.source]
directory = "extra/templates"
manifest = "bundle.toml"

[units.templates.tracks.sdk]
packages = ["thesdk"]

[units.compiler]
kind = "crates"
version-source = "workspace"
tag = "v{version}"
changelog = "CHANGELOG.md"
after = ["sdk", "templates"]
latest = true
assets = ["tool-*.tar.gz"]

[units.private]
kind = "private"

[[packages]]
name = "thesdk"
unit = "sdk"

[[packages]]
name = "thetool"
unit = "compiler"

[[packages]]
name = "internal"
unit = "private"
"#;

#[test]
fn units_publish_in_dependency_order() {
    assert_eq!(load(THREE_UNITS, "order").order().unwrap(), ["sdk", "templates", "compiler"]);
}

#[test]
fn private_and_library_units_are_not_in_the_publish_order() {
    let order = load(THREE_UNITS, "order-private").order().unwrap();
    assert!(!order.contains(&"private".to_string()));
}

#[test]
fn independent_units_order_by_name_for_determinism() {
    let config = load(
        r#"
schema-version = 2

[units.zebra]
kind = "crates"
version-source = "own"
tag = "zebra/v{version}"
changelog = "zebra/CHANGELOG.md"

[units.alpha]
kind = "crates"
version-source = "own"
tag = "alpha/v{version}"
changelog = "alpha/CHANGELOG.md"

[[packages]]
name = "z"
unit = "zebra"

[[packages]]
name = "a"
unit = "alpha"
"#,
        "order-tiebreak",
    );
    assert_eq!(config.order().unwrap(), ["alpha", "zebra"]);
}

/// A repository whose dependency order is the reverse of the alphabetical one,
/// so the result cannot be mistaken for the tiebreak.
#[test]
fn declared_order_beats_the_name_tiebreak() {
    let config = load(
        r#"
schema-version = 2

[units.alpha]
kind = "crates"
version-source = "own"
tag = "alpha/v{version}"
changelog = "alpha/CHANGELOG.md"
after = ["zebra"]

[units.zebra]
kind = "crates"
version-source = "own"
tag = "zebra/v{version}"
changelog = "zebra/CHANGELOG.md"

[[packages]]
name = "a"
unit = "alpha"

[[packages]]
name = "z"
unit = "zebra"
"#,
        "order-declared",
    );
    assert_eq!(config.order().unwrap(), ["zebra", "alpha"]);
}

const TWO_UNITS_ONE_LIBRARY: &str = r#"
schema-version = 2

[units.tool-a]
kind = "crates"
version-source = "own"
tag = "tool-a/v{version}"
changelog = "a/CHANGELOG.md"
latest = true

[units.tool-b]
kind = "crates"
version-source = "own"
tag = "tool-b/v{version}"
changelog = "b/CHANGELOG.md"

[units.common]
kind = "library"
version-source = "workspace"

[[packages]]
name = "tool-a"
unit = "tool-a"

[[packages]]
name = "tool-b"
unit = "tool-b"

[[packages]]
name = "common"
unit = "common"
"#;

#[test]
fn a_library_unit_is_never_released_on_its_own() {
    let config = load(TWO_UNITS_ONE_LIBRARY, "lib-order");
    assert_eq!(config.order().unwrap(), ["tool-a", "tool-b"]);
}

#[test]
fn a_single_unit_repository_orders_trivially() {
    let config = load(
        r#"
schema-version = 2

[units.main]
kind = "crates"
version-source = "workspace"
tag = "v{version}"
changelog = "CHANGELOG.md"
latest = true

[[packages]]
name = "mytool"
unit = "main"
"#,
        "order-single",
    );
    assert_eq!(config.order().unwrap(), ["main"]);
}
