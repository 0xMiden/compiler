;; RUN: midenc %s --entrypoint=event_free_calls::entrypoint --debug full --emit=masm=- 2>&1 | filecheck %s --implicit-check-not emit

(module $event_free_calls.wasm
  (func $inner (param i32) (result i32)
    local.get 0
    i32.const 2
    i32.add)
  (func $outer (param i32) (result i32)
    local.get 0
    call $inner
    i32.const 3
    i32.add)
  (func $entrypoint (export "entrypoint") (param i32) (result i32)
    local.get 0
    call $outer))

;; CHECK: exec.{{.*}}inner
;; CHECK: pub proc entrypoint
;; CHECK: exec.{{.*}}outer
;; CHECK-NEXT: end
