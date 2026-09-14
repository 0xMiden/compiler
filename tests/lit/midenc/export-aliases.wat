;; RUN: midenc %s --emit=hir=- -Canalyze-only 2>&1 | filecheck %s --check-prefix=HIR
;; RUN: midenc %s --entrypoint=export_alias_test::foo --emit=masm=- 2>&1 | filecheck %s --check-prefix=MASM-FOO
;; RUN: midenc %s --entrypoint=export_alias_test::bar --emit=masm=- 2>&1 | filecheck %s --check-prefix=MASM-BAR
;; RUN: midenc %s --entrypoint=export_alias_test::foo -o %t/foo.masp
;; RUN: midenc %s --entrypoint=export_alias_test::bar -o %t/bar.masp
;;
;; Verify N:1 exports preserve the primary as a function and secondaries as function aliases.

(module $export_alias_test.wasm
  (func $impl (@name "impl_source") (result i32)
    i32.const 42
  )
  (export "foo" (func $impl))
  (export "bar" (func $impl))
)

;; First export name becomes the linkage name
;; HIR: builtin.function public extern("C") @foo() -> i32
;; HIR: builtin.function_alias public @bar -> {{.*}}@foo
;; HIR-NOT: builtin.function {{.*}} @impl_source

;; MASM-FOO: pub proc foo
;; MASM-FOO: pub proc bar
;; MASM-BAR: pub proc foo
;; MASM-BAR: pub proc bar
