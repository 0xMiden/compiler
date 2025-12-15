;; Test that .debug_loc section shows DebugVar decorators with source locations
;; from a real Rust project compiled with debug info.
;;
;; RUN: cargo build --release --target wasm32-unknown-unknown --manifest-path %S/../source-location/test-project/Cargo.toml 2>&1
;; RUN: /bin/sh -c "TMPDIR=$(mktemp -d) && TMPFILE=\"\$TMPDIR/out.masp\" && bin/midenc '%S/../source-location/test-project/target/wasm32-unknown-unknown/release/source_location_test.wasm' --lib --debug full -Z trim-path-prefix='%S/../source-location/test-project' -o \"\$TMPFILE\" && target/debug/miden-debugdump \"\$TMPFILE\" --section locations" | filecheck %s

;; Check header
;; CHECK: .debug_loc contents (DebugVar decorators from MAST):
;; CHECK: Total DebugVar decorators: 5
;; CHECK: Unique variable names: 4

;; Check variable "arg0" - parameter from test_assertion function
;; CHECK: Variable: "arg0"
;; CHECK: 1 location entries:
;; CHECK: local[0] (param #2) @ {{.*}}test-project/src/lib.rs:10:1

;; Check variable "local1" - appears in both functions
;; CHECK: Variable: "local1"
;; CHECK: 2 location entries:
;; CHECK: stack[0] @ {{.*}}test-project/src/lib.rs:10:1
;; CHECK: stack[0] @ {{.*}}test-project/src/lib.rs:18:1

;; Check variable "local2" - from panic handler, no source location
;; CHECK: Variable: "local2"
;; CHECK: 1 location entries:
;; CHECK: stack[0]

;; Check variable "x" - parameter from entrypoint function
;; CHECK: Variable: "x"
;; CHECK: 1 location entries:
;; CHECK: local[0] (param #2) @ {{.*}}test-project/src/lib.rs:18:1
