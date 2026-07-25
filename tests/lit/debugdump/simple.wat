;; Test that miden-objtool correctly parses and displays debug info from a .masp file
;;
;; RUN: midenc %s --entrypoint=simple::multiply --debug full -o %t/out.masp
;; RUN: miden-objtool dump debug-info %t/out.masp | filecheck %s

;; CHECK: Package Info:
;; CHECK: Name:               simple:simple
;; CHECK-NEXT: Version:            0.0.0
;; CHECK-NEXT: Kind:               executable
;; CHECK-NEXT: Debug Info Version: 2

;; Check summary section is present
;; CHECK: Summary:

;; CHECK: Strings:

;; CHECK: Types:

;; CHECK: Functions:
;; CHECK-NEXT: records:          72
;; CHECK-NEXT: with source info: 2
;; CHECK-NEXT: w/o source info:  70

;; CHECK: Source Files:
;; CHECK-NEXT: records: 4

;; CHECK: Locations:

;; CHECK: Source Nodes:
;; CHECK-NEXT: records: 17
;; CHECK-NEXT: roots:   2
;; CHECK-NEXT: debug variables (total): 0
;; CHECK-NEXT: inline calls (total):    0

;; CHECK: Found 0 debug variable records

;; CHECK: .debug_str contents:

;; CHECK: .debug_types contents:

;; CHECK: .debug_files contents:

;; Check that debug functions are present for the emitted code
;; CHECK: .debug_functions contents:
;; CHECK: FUNCTION: ::intrinsics::i32::overflowing_mul
;; CHECK: FUNCTION: ::intrinsics::i32::wrapping_mul

;; CHECK: .debug_loc contents (DebugLoc entries from MAST):
;; CHECK: Total DebugVar entries: 0

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
