;; RUN: midenc %s --entrypoint=inline_ranges::entrypoint --debug full -o %t/out.masp
;; RUN: miden-objtool dump debug-info --verbose %t/out.masp | filecheck %s

(module $inline_ranges.wasm
  (func $inner (param i32) (result i32)
    local.get 0
    i32.const 2
    i32.mul)
  (func $entrypoint (export "entrypoint") (param i32) (result i32)
    local.get 0
    call $inner))

;; CHECK: Debug Info Version: 4
;; CHECK: Inline-call ranges ({{[0-9]+}}):
;; CHECK: ops[{{[0-9]+}}..{{[0-9]+}}) inlined at
