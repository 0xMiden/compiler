;; Test that miden-debugdump correctly parses and displays debug info from a .masp file
;; RUN: /bin/sh -c "TMPDIR=$(mktemp -d) && TMPFILE=\"\$TMPDIR/out.masp\" && bin/midenc '%s' --exe --debug full -o \"\$TMPFILE\" && target/debug/miden-debugdump \"\$TMPFILE\"" | filecheck %s

;; Check header
;; CHECK: DEBUG INFO DUMP:
;; CHECK: Debug info version: 1

;; Check summary section is present
;; CHECK: .debug_info summary:
;; CHECK: Strings:
;; CHECK: Types:
;; CHECK: Files:
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
