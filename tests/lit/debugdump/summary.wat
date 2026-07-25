;; Test that miden-objtool --summary shows only summary output
;;
;; RUN: midenc %s --entrypoint=summary::test --debug full -o %t/out.masp
;; RUN: miden-objtool dump debug-info %t/out.masp --summary | filecheck %s

;; Check summary is present
;; CHECK: Summary:
;; CHECK: Strings:
;; CHECK: Types:
;; CHECK: Functions:
;; CHECK: Source Files:
;; CHECK: Locations:
;; CHECK: Source Nodes:

;; Make sure full dump sections are NOT present with --summary
;; CHECK-NOT: .debug_str contents:
;; CHECK-NOT: .debug_types contents:
;; CHECK-NOT: .debug_files contents:
;; CHECK-NOT: .debug_functions contents:

;; CHECK: Found 0 debug variable records

(module
  (func $test (export "test") (param i32) (result i32)
    local.get 0
  )
)
