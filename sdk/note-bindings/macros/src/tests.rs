//! Tests for macro expansion.

use midenc_expect_test::expect_file;
use syn::LitStr;

use crate::expand_from_wit_text;

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
    expect_file!["../../src/expected/p2id.rs"].assert_eq(&expand(P2ID_SCHEMA));
}

#[test]
fn expands_custom_schema_golden() {
    expect_file!["../../src/expected/custom.rs"].assert_eq(&expand(CUSTOM_SCHEMA));
}
