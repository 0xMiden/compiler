;; Test that .debug_loc section shows DebugVar entries with source locations
;; from a real Rust project compiled with debug info.
;;
;; RUN: %cargo build --target-dir %target_dir/debugdump-source-location --release --target wasm32-unknown-unknown --manifest-path tests/lit/source-location/test-project/Cargo.toml 2>&1
;; RUN: /bin/sh -c "TMPDIR=$(mktemp -d) && TMPFILE=\"\$TMPDIR/out.masp\" && midenc '%target_dir/debugdump-source-location/wasm32-unknown-unknown/release/source_location_test.wasm' --lib --debug full -o \"\$TMPFILE\" && miden-objtool dump debug-info \"\$TMPFILE\" --section locations" | filecheck %s

;; Check header
;; CHECK: .debug_loc contents (DebugVar entries from MAST):
;; CHECK: Total DebugVar entries: 4
;; CHECK: Unique variable names: 3

;; Check variable "arg0" - parameter from test_assertion function
;; CHECK: Variable: "arg0"
;; CHECK: 1 location entries:
;; CHECK: FMP-4 (param #1)

;; Check variable "local3" - from panic handler
;; CHECK: Variable: "local3"
;; CHECK: 1 location entries:
;; CHECK: FMP-1

;; Check variable "x" - parameter from entrypoint function
;; CHECK: Variable: "x"
;; CHECK: 2 location entries:
;; CHECK: FMP-4 (param #1)
