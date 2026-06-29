;; Test that miden-objtool correctly parses and displays debug info from a .masp file
;;
;; RUN: midenc %s --entrypoint=simple::multiply --debug full -o %t/out.masp
;; RUN: miden-objtool dump debug-info %t/out.masp | filecheck %s

;; CHECK: Package Info:
;; CHECK: Name:    simple:simple
;; CHECK-NEXT: Version: 0.0.0
;; CHECK-NEXT: Kind:    executable

;; Check summary section is present
;; CHECK: Summary:
;; CHECK: Types:
;; CHECK: Sources:
;; CHECK: Functions:
;; CHECK: records:   4
;; CHECK: Found 0 debug variable records

;; Check that debug functions are present for the emitted code
;; CHECK: .debug_functions contents:
;; CHECK: FUNCTION: ::intrinsics::i32::unchecked_neg
;; CHECK: FUNCTION: ::intrinsics::i32::is_signed
;; CHECK: FUNCTION: ::intrinsics::i32::overflowing_mul
;; CHECK: FUNCTION: ::intrinsics::i32::wrapping_mul

;; CHECK: .debug_loc contents (DebugVar entries from MAST):
;; CHECK: (no DebugVar entries found)

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
