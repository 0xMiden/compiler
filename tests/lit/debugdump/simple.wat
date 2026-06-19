;; Test that miden-objtool correctly parses and displays debug info from a .masp file
;;
;; RUN: midenc %s --entrypoint=simple::multiply --debug full -o %t/out.masp
;; RUN: miden-objtool dump debug-info %t/out.masp | filecheck %s
;; XFAIL: *

;; CHECK: Name: simple
;; CHECK-NEXT: Version: 0.0.0
;; CHECK-NEXT: Kind: library

;; Check summary section is present
;; CHECK: Summary:
;; CHECK: Types:
;; CHECK: Sources:
;; CHECK: Functions:

;; Check that we have functions from the WAT
;; CHECK: .debug_functions contents:
;; CHECK: FUNCTION: add
;; CHECK: FUNCTION: multiply

(module
  (func $add (export "add") (param $a i32) (param $b i32) (result i32)
    local.get $a
    local.get $b
    i32.add
  )

  (func $multiply (export "multiply") (param $x i32) (param $y i32) (result i32)
    local.get $x
    local.get $y
    i32.mul
  )
)
