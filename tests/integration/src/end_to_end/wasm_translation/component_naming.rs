//! Rejects components whose core modules are named like a procedure the component generates.

use std::panic::{self, AssertUnwindSafe};

use crate::{CompilerTestBuilder, trap_helpers::panic_message};

/// A core module named `init` is rejected: it would clash with the component's generated `init`
/// procedure, which the module's globals and data segment make codegen emit next to the exports.
#[test]
fn a_core_module_named_init_is_rejected() {
    let wasm = wat::parse_str(
        r#"
        (component
            (core module $init
                (memory 1)
                (global $count (mut i32) (i32.const 7))
                (data (i32.const 1024) "init")
                (func (export "get-count") (result i32) global.get $count))
            (core instance $i (instantiate $init))
            (func $lifted (result u32) (canon lift (core func $i "get-count")))
            (component $exports
                (import "import-func-get-count" (func $f (result u32)))
                (export "get-count" (external-id "miden::counter::counter::get_count")
                    (func $f)))
            (instance $counter
                (instantiate $exports (with "import-func-get-count" (func $lifted))))
            (export "miden:counter/counter@0.1.0" (instance $counter))
        )
        "#,
    )
    .expect("component fixture must be valid WebAssembly text");
    let mut test = CompilerTestBuilder::from_wasm("init", wasm, []).build();
    let err = panic::catch_unwind(AssertUnwindSafe(move || test.compile_package()))
        .map(|_| ())
        .expect_err("a core module named `init` must be rejected");
    let err = panic_message(err);
    assert!(
        err.contains("is named like the generated `init` procedure"),
        "unexpected diagnostic: {err}"
    );
}
