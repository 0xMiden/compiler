;; Test that .debug_loc section shows DebugVar decorators with source locations
;; from a real Rust project compiled with debug info.
;;
;; RUN: cargo build --release --target wasm32-unknown-unknown --manifest-path tests/lit/source-location/test-project/Cargo.toml 2>&1
;; RUN: /bin/sh -c "TMPDIR=$(mktemp -d) && TMPFILE=\"\$TMPDIR/out.masp\" && bin/midenc 'tests/lit/source-location/test-project/target/wasm32-unknown-unknown/release/source_location_test.wasm' --lib --debug full -o \"\$TMPFILE\" && target/debug/miden-debugdump \"\$TMPFILE\" --section locations" | filecheck %s

;; Check header
;; CHECK: .debug_loc contents (DebugVar decorators from MAST):
;; CHECK: Total DebugVar decorators: 3
;; CHECK: Unique variable names: 3

;; Check variable "arg0" - parameter from test_assertion function
;; CHECK: Variable: "arg0"
;; CHECK: 1 location entries:
;; CHECK: local[0] (param #2)

;; Check variable "local3" - from panic handler
;; CHECK: Variable: "local3"
;; CHECK: 1 location entries:
;; CHECK: stack[0]

;; Check variable "x" - parameter from entrypoint function
;; CHECK: Variable: "x"
;; CHECK: 1 location entries:
;; CHECK: local[0] (param #2)
