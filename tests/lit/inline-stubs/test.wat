;; RUN: bin/midenc %s --entrypoint=test_felt_add --emit=hir=- 2>&1 | filecheck %s --check-prefix=HIR-ADD
;; RUN: bin/midenc %s --entrypoint=test_felt_assert_eq --emit=hir=- 2>&1 | filecheck %s --check-prefix=HIR-ASSERT
;; RUN: bin/midenc %s --entrypoint=test_felt_arithmetic --emit=hir=- 2>&1 | filecheck %s --check-prefix=HIR-ARITH
;;
;; This test verifies that linker stubs (intrinsic functions like felt::add, felt::assert_eq)
;; are inlined at call sites rather than being emitted as separate function definitions.
;;

;; === Test 1: felt::add should be inlined ===
;; The arith.add operation should appear directly in test_felt_add,
;; NOT as a call to a separate intrinsics::felt::add function.

;; HIR-ADD: public builtin.function @test_felt_add
;; HIR-ADD: arith.add {{.*}} : felt
;; HIR-ADD: builtin.ret

;; Verify NO separate stub function is created for felt::add
;; HIR-ADD-NOT: builtin.function @intrinsics::felt::add

;; === Test 2: felt::assert_eq should be inlined ===
;; The hir.assert_eq operation should appear directly in test_felt_assert_eq.

;; HIR-ASSERT: public builtin.function @test_felt_assert_eq
;; HIR-ASSERT: hir.assert_eq
;; HIR-ASSERT: builtin.ret

;; Verify NO separate stub function is created for felt::assert_eq
;; HIR-ASSERT-NOT: builtin.function @intrinsics::felt::assert_eq

;; === Test 3: Multiple felt operations should all be inlined ===
;; All arithmetic operations should be inlined in test_felt_arithmetic.

;; HIR-ARITH: public builtin.function @test_felt_arithmetic
;; HIR-ARITH: arith.add {{.*}} : felt
;; HIR-ARITH: arith.sub {{.*}} : felt
;; HIR-ARITH: arith.mul {{.*}} : felt
;; HIR-ARITH: builtin.ret

;; Verify NO separate stub functions are created
;; HIR-ARITH-NOT: builtin.function @intrinsics::felt::add
;; HIR-ARITH-NOT: builtin.function @intrinsics::felt::sub
;; HIR-ARITH-NOT: builtin.function @intrinsics::felt::mul

(module $inline_stubs_test.wasm
  (type (;0;) (func (param f32 f32) (result f32)))
  (type (;1;) (func (param f32 f32)))
  (memory (;0;) 16)
  (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
  (export "memory" (memory 0))
  (export "test_felt_add" (func $test_felt_add))
  (export "test_felt_assert_eq" (func $test_felt_assert_eq))
  (export "test_felt_arithmetic" (func $test_felt_arithmetic))

  ;; Test function that calls felt::add stub
  ;; The stub should be inlined as arith.add
  (func $test_felt_add (;0;) (type 0) (param f32 f32) (result f32)
    local.get 0
    local.get 1
    call $intrinsics::felt::add
  )

  ;; Test function that calls felt::assert_eq stub
  ;; The stub should be inlined as hir.assert_eq
  (func $test_felt_assert_eq (;1;) (type 1) (param f32 f32)
    local.get 0
    local.get 1
    call $intrinsics::felt::assert_eq
  )

  ;; Test function that calls multiple felt stubs
  ;; All stubs should be inlined
  (func $test_felt_arithmetic (;2;) (type 0) (param f32 f32) (result f32)
    (local f32 f32)
    ;; sum = a + b
    local.get 0
    local.get 1
    call $intrinsics::felt::add
    local.set 2
    ;; diff = a - b
    local.get 0
    local.get 1
    call $intrinsics::felt::sub
    local.set 3
    ;; return sum * diff
    local.get 2
    local.get 3
    call $intrinsics::felt::mul
  )

  ;; Linker stubs - these have unreachable bodies and should be inlined at call sites
  (func $intrinsics::felt::add (;3;) (type 0) (param f32 f32) (result f32)
    unreachable
  )
  (func $intrinsics::felt::sub (;4;) (type 0) (param f32 f32) (result f32)
    unreachable
  )
  (func $intrinsics::felt::mul (;5;) (type 0) (param f32 f32) (result f32)
    unreachable
  )
  (func $intrinsics::felt::assert_eq (;6;) (type 1) (param f32 f32)
    unreachable
  )
)
