;; RUN: midenc %s --entrypoint=test --emit=hir=- -Canalyze-only 2>&1 | filecheck %s
;;
;; This test verifies that function names duplicated in the Wasm name section are made unique.

(module $duplicate_func_names_test.wasm
  (type (;0;) (func (param i32) (result i32)))
  (type (;1;) (func (result i32)))
  (memory (;0;) 16)
  (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
  (export "memory" (memory 0))
  (export "test" (func $test))

  ;; Both functions carry the same name-section name
  (func $first (@name "foo") (;0;) (type 0) (param i32) (result i32)
    local.get 0
  )
  (func $second (@name "foo") (;1;) (type 0) (param i32) (result i32)
    local.get 0
  )
  (func $test (;2;) (type 1) (result i32)
    i32.const 1
    call $first
    i32.const 2
    call $second
    i32.add
  )
)

;; Both members of the duplicate group are renamed with their function index
;; CHECK: builtin.function private extern("C") @foo_func0(
;; CHECK: builtin.function private extern("C") @foo_func1(
;; The unique name is left untouched
;; CHECK: builtin.function public extern("C") @test(

;; Calls resolve to the renamed functions
;; CHECK: hir.exec {{.*}}::@foo_func0(
;; CHECK: hir.exec {{.*}}::@foo_func1(
