;; RUN: midenc %s --emit=hir=- -Canalyze-only 2>&1 | filecheck %s --check-prefix=HIR
;; RUN: midenc %s --entrypoint=export_name_test::foo_3 --emit=masm=- 2>&1 | filecheck %s --check-prefix=MASM
;; RUN: midenc %s --entrypoint=export_name_test::foo_3 -o %t/out.masp
;;
;; Verify that export names become linkage names.

(module $export_name_test.wasm
  ;; WAT identifier: $foo_1
  ;; Name-section name: "foo_2"
  ;; Export name: "foo_3"
  (func $foo_1 (@name "foo_2") (result i32)
    i32.const 42
  )
  (export "foo_3" (func $foo_1))

  ;; Internal caller to verify calls resolve to the exported linkage symbol
  (func $caller (@name "caller_source") (result i32)
    call $foo_1
  )
  (export "caller" (func $caller))
)

;; HIR: builtin.function public extern("C") @foo_3() -> i32
;; HIR: di.subprogram = #di.subprogram<{ name = "foo_2", file = "unknown", line = 0, linkage = "foo_3", definition = true, local = false }>
;; HIR: builtin.function public extern("C") @caller() -> i32
;; HIR: hir.exec {{.*}}::@foo_3() : extern("C") () -> i32
;; HIR: di.subprogram = #di.subprogram<{ name = "caller_source", file = "unknown", line = 0, linkage = "caller", definition = true, local = false }>
;; HIR-NOT: builtin.function {{.*}} @foo_2

;; MASM: pub mod export_name_test
;; MASM: pub proc foo_3
;; MASM: pub proc caller
;; MASM: exec.::"root_ns:root@1.0.0"::export_name_test::foo_3
;; MASM-NOT: proc foo_2
