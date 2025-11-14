;; RUN: cargo build --release --target wasm32-unknown-unknown --manifest-path %S/test-project/Cargo.toml 2>&1
;; RUN: env MIDENC_TRACE=debug bin/midenc %S/test-project/target/wasm32-unknown-unknown/release/source_location_test.wasm --entrypoint=source_location_test::test_assertion -Z trim-path-prefix=%S/test-project --emit=masm=- 2>&1 | filecheck %s
;; RUN: bin/midenc %S/test-project/target/wasm32-unknown-unknown/release/source_location_test.wasm --entrypoint=source_location_test::test_assertion -Z trim-path-prefix=%S/test-project -Z print-hir-source-locations --emit=hir=- 2>&1 | filecheck %s --check-prefix=HIR
;;
;; This test verifies that source location information from DWARF is correctly
;; resolved when trim-paths is enabled.
;;
;; The source_location_test example is compiled with:
;;   debug = true
;;   trim-paths = ["diagnostics", "object"]
;;
;; This causes DWARF to contain relative paths.
;;

;; CHECK: resolved source path './src/lib.rs'
;; CHECK: test-project/src/lib.rs
;; CHECK: pub proc test_assertion
;; CHECK-NOT: failed to resolve source path

;; Verify HIR output contains source locations with absolute paths
;; HIR: hir.bitcast {{.*}} #loc("/{{.*}}test-project/src/lib.rs":{{.*}})
;; HIR: arith.gt {{.*}} #loc("/{{.*}}test-project/src/lib.rs":{{.*}})
;; HIR: builtin.ret {{.*}} #loc("/{{.*}}test-project/src/lib.rs":{{.*}})
