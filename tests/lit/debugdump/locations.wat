;; Test that .debug_loc section is present and handles empty case
;;
;; RUN: midenc %s --entrypoint=locations::entrypoint --debug full -o %t/out.masp
;; RUN: miden-objtool dump debug-info %t/out.masp --section locations | filecheck %s

;; Check header for .debug_loc section
;; CHECK: .debug_loc contents (DebugVar entries from MAST):
;; For raw WAT files without debug info, we expect no decorators
;; CHECK: (no DebugVar entries found)

(module
  (func $add (export "add") (param i32 i32) (result i32)
    local.get 0
    local.get 1
    i32.add
  )

  (func $entrypoint (export "entrypoint")
    i32.const 5
    i32.const 3
    call $add
    drop
  )
)
