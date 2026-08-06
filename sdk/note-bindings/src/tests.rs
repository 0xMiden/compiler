//! Tests for macro expansion and package discovery.

use std::{fs, thread, time::Duration};

use midenc_expect_test::expect_file;
use syn::LitStr;

use crate::{expand_from_wit_text, freshest_project_package, missing_project_package_message};

const P2ID_SCHEMA: &str = r#"
package example:p2id-schema@1.0.0;

use miden:base/core-types@1.0.0;

interface note-storage {
    use core-types.{account-id};

    record p2id-note {
        target-account-id: account-id,
    }

    type storage = p2id-note;
}

package miden:base@1.0.0 {
    interface core-types {
        record felt { inner: f32 }
        record account-id { prefix: felt, suffix: felt }
    }
}
"#;

const CUSTOM_SCHEMA: &str = r#"
package example:dex-schema@1.0.0;

use miden:base/core-types@1.0.0;

interface note-storage {
    use core-types.{account-id};

    /// A ratio used as an order limit.
    record limit-price {
        numerator: u64,
        denominator: u64,
    }

    /// Selects order execution.
    variant order-kind {
        market,
        limit(limit-price),
    }

    record dex-note {
        target: account-id,
        kind: order-kind,
    }

    type storage = dex-note;
}

package miden:base@1.0.0 {
    interface core-types {
        record felt { inner: f32 }
        record account-id { prefix: felt, suffix: felt }
    }
}
"#;

/// Formats one WIT-text macro expansion as Rust source.
fn expand(wit: &str) -> String {
    let literal = LitStr::new(wit, proc_macro2::Span::call_site());
    let tokens = expand_from_wit_text(&literal).unwrap();
    let file: syn::File = syn::parse2(tokens).unwrap();
    prettyplease::unparse(&file)
}

#[test]
fn expands_p2id_schema_golden() {
    expect_file!["expected/p2id.rs"].assert_eq(&expand(P2ID_SCHEMA));
}

#[test]
fn expands_custom_schema_golden() {
    expect_file!["expected/custom.rs"].assert_eq(&expand(CUSTOM_SCHEMA));
}

#[test]
fn selects_the_freshest_package_across_profiles() {
    let temp = tempfile::tempdir().unwrap();
    let debug = temp.path().join("target/miden/debug");
    let release = temp.path().join("target/miden/release");
    fs::create_dir_all(&debug).unwrap();
    fs::create_dir_all(&release).unwrap();
    fs::write(debug.join("note.masp"), b"old").unwrap();
    thread::sleep(Duration::from_millis(20));
    fs::write(release.join("note.masp"), b"new").unwrap();

    let selected = freshest_project_package(temp.path(), proc_macro2::Span::call_site())
        .unwrap()
        .unwrap();
    assert_eq!(selected, release.join("note.masp"));
}

#[test]
fn missing_package_diagnostic_names_build_command() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join("Cargo.toml"), "[package]\nname='note'\nversion='0.1.0'").unwrap();
    let message = missing_project_package_message(temp.path());
    assert!(message.contains("cargo miden build --manifest-path"));
    assert!(message.contains("--release"));
}
