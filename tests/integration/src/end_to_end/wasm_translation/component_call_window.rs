//! Rejects component exports whose results do not fit the cross-context call window.

use std::panic::{self, AssertUnwindSafe};

use crate::{CompilerTestBuilder, trap_helpers::panic_message};

/// Builds a component exporting `get-record`, which returns a record of `num_fields` `u64`
/// fields through the canonical result pointer.
fn u64_record_component(num_fields: usize) -> Vec<u8> {
    let fields: String = (0..num_fields).map(|i| format!(r#"(field "f{i}" u64) "#)).collect();
    wat::parse_str(format!(
        r#"
        (component
            (core module $m
                (memory (export "memory") 1)
                (func (export "get-record") (result i32) i32.const 1024))
            (core instance $i (instantiate $m))
            (type $record (record {fields}))
            (alias core export $i "memory" (core memory $memory))
            (func $lifted (result $record)
                (canon lift (core func $i "get-record") (memory $memory)))
            (component $exports
                (import "import-type-record" (type $record' (eq $record)))
                (import "import-func-get-record" (func $f (result $record')))
                (export $record-export "record" (type $record'))
                (export "get-record" (external-id "miden::test::call_window::get_record")
                    (func $f) (func (result $record-export))))
            (instance $instance
                (instantiate $exports
                    (with "import-type-record" (type $record))
                    (with "import-func-get-record" (func $lifted))))
            (export "miden:test/call-window@1.0.0" (instance $instance))
        )
        "#
    ))
    .expect("component fixture must be valid WebAssembly text")
}

/// Compiles the component of [`u64_record_component`] and returns the panic message if the
/// compilation fails.
fn compile_u64_record_component(num_fields: usize) -> Result<(), String> {
    let mut test =
        CompilerTestBuilder::from_wasm("call_window", u64_record_component(num_fields), []).build();
    panic::catch_unwind(AssertUnwindSafe(move || {
        test.compile_package();
    }))
    .map_err(panic_message)
}

/// A record of eight `u64` fields flattens to 16 felts, which fill the call window exactly.
#[test]
fn an_export_returning_sixteen_felts_compiles() {
    if let Err(err) = compile_u64_record_component(8) {
        panic!("a 16-felt result should compile: {err}");
    }
}

/// A record of nine `u64` fields is only nine values, but 18 felts, past the call window.
#[test]
fn an_export_returning_eighteen_felts_is_rejected() {
    let err = compile_u64_record_component(9).expect_err("an 18-felt result must be rejected");
    assert!(
        err.contains(
            "get_record': 18 result field elements exceed the 16-element context result ABI"
        ),
        "unexpected diagnostic: {err}"
    );
}
