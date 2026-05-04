use crate::compiler_test::{sdk_alloc_crate_path, sdk_crate_path};

pub(super) fn cargo_toml(name: &str) -> String {
    let sdk_alloc_path = sdk_alloc_crate_path();
    let sdk_path = sdk_crate_path();
    format!(
        r#"
                [package]
                name = "{name}"
                version = "0.0.1"
                edition = "2024"
                authors = []

                [lib]
                crate-type = ["cdylib"]

                [dependencies]
                miden-sdk-alloc = {{ path = "{sdk_alloc_path}" }}
                miden = {{ path = "{sdk_path}" }}

                [profile.release]
                # optimize the output for size
                opt-level = "z"
                panic = "abort"

                [profile.dev]
                panic = "abort"
                opt-level = 1
                debug-assertions = true
                overflow-checks = false
                debug = false

            "#,
        sdk_alloc_path = sdk_alloc_path.display(),
        sdk_path = sdk_path.display(),
    )
}
