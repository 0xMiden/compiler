;; RUN: midenc %s --emit=hir=- -Canalyze-only 2>&1 | filecheck %s --check-prefix=HIR
;; RUN: midenc %s --emit=masm=- 2>&1 | filecheck %s --check-prefix=MASM
;;
;; Verify that a cast at a stub boundary moves nothing, and that the stack peephole deletes an
;; identity operand scheduling leaves.
;;
;; `intrinsics::felt::from_u32` is a `hir.bitcast` of its `i32` argument, which sits below the
;; felt the `sub` takes as its right operand. Scheduled like any other op, the cast moved its
;; operand to the top (`swap.1`) and the `sub` moved it back (`swap.1`); lowered as a rename of
;; the operand's stack slot, it moves nothing, and the `sub` finds its operands in place.
;;
;; The alignment check of an `i32.load` leaves a `push.0 drop`, which the peephole deletes.

;; HIR-LABEL: builtin.function public extern("C") @sub_from_u32
;; HIR: hir.bitcast {{.*}} <{ ty = #builtin.type<felt> }>;
;; HIR: arith.sub

;; MASM-LABEL: pub proc sub_from_u32
;; MASM-NOT: swap
;; MASM: sub
;; MASM-NEXT: end

;; MASM-LABEL: pub proc load
;; MASM: assertz.err="pointer address does not meet minimum alignment for the type"
;; MASM-NEXT: mem_load
;; MASM-NEXT: end

(module $transparent_casts.wasm
  (type (;0;) (func (param f32 i32) (result f32)))
  (type (;1;) (func (param i32) (result f32)))
  (type (;2;) (func (param f32 f32) (result f32)))
  (type (;3;) (func (param i32) (result i32)))
  (memory 1)
  (export "sub_from_u32" (func $sub_from_u32))
  (export "load" (func $load))
  (func $sub_from_u32 (type 0) (param f32 i32) (result f32)
    local.get 1
    call $intrinsics::felt::from_u32
    local.get 0
    call $intrinsics::felt::sub)
  (func $load (type 3) (param i32) (result i32)
    local.get 0
    i32.load)
  ;; Linker stubs, inlined at their call sites
  (func $intrinsics::felt::from_u32 (type 1) (param i32) (result f32)
    unreachable)
  (func $intrinsics::felt::sub (type 2) (param f32 f32) (result f32)
    unreachable)
)
