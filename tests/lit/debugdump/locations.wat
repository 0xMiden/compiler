;; Test that .debug_loc section is present and handles empty case
;; RUN: /bin/sh -c "TMPDIR=$(mktemp -d) && TMPFILE=\"\$TMPDIR/out.masp\" && bin/midenc '%s' --exe --debug full -o \"\$TMPFILE\" && target/debug/miden-debugdump \"\$TMPFILE\" --section locations" | filecheck %s

;; Check header for .debug_loc section
;; CHECK: .debug_loc contents (DebugVar decorators from MAST):
;; For raw WAT files without debug info, we expect no decorators
;; CHECK: (no DebugVar decorators found)

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
