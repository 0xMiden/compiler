;; RUN: midenc %s --entrypoint=inline_ranges::entrypoint --debug full -o %t/out.masp
;; RUN: miden-objtool dump debug-info --verbose %t/out.masp | filecheck %s
;; RUN: miden-objtool dump debug-info --section functions --verbose %t/out.masp | filecheck %s
;; RUN: miden-objtool dump debug-info --section functions --verbose --raw %t/out.masp | filecheck %s --check-prefix=RAW

;; WAT has no DWARF locations; its multiply intrinsic supplies named inline ranges.
;; CHECK: Debug Info Version: 4
;; CHECK: .debug_inline contents (operation ranges local to each execution node):
;; CHECK: [source-node#{{[0-9]+}} -> exec-node#{{[0-9]+}}] Inline-call ranges ({{[1-9][0-9]*}}):
;; CHECK: ::intrinsics::i32::overflowing_mul ops[{{[0-9]+}}..{{[1-9][0-9]*}}) inlined at ::intrinsics::i32:{{[0-9]+}}..{{[0-9]+}}

;; RAW: .debug_inline contents (operation ranges local to each execution node):
;; RAW: [source-node#{{[0-9]+}} -> exec-node#{{[0-9]+}}] Inline-call ranges ({{[1-9][0-9]*}}):
;; RAW: func[{{[0-9]+}}] ops[{{[0-9]+}}..{{[1-9][0-9]*}}) inlined at loc[{{[0-9]+}}]

(module $inline_ranges.wasm
  (func $inner (param i32) (result i32)
    local.get 0
    i32.const 2
    i32.mul)
  (func $entrypoint (export "entrypoint") (param i32) (result i32)
    local.get 0
    call $inner))
