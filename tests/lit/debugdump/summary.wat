;; Test that miden-objtool --summary shows only summary output
;; RUN: /bin/sh -c "TMPDIR=$(mktemp -d) && TMPFILE=\"\$TMPDIR/out.masp\" && midenc '%s' --exe --debug full -o \"\$TMPFILE\" && miden-objtool dump debuginfo \"\$TMPFILE\" --summary" | filecheck %s

;; Check summary is present
;; CHECK: .debug_info summary:
;; CHECK: Strings:
;; CHECK: Types:{{.*}}entries
;; CHECK: Files:{{.*}}entries
;; CHECK: Functions:{{.*}}entries

;; Make sure full dump sections are NOT present with --summary
;; CHECK-NOT: .debug_str contents:
;; CHECK-NOT: .debug_types contents:
;; CHECK-NOT: .debug_files contents:
;; CHECK-NOT: .debug_functions contents:

(module
  (func $test (export "test") (param i32) (result i32)
    local.get 0
  )
)
