;; RUN: midenc %s --emit=hir=- -Canalyze-only 2>&1 | filecheck %s --check-prefix=HIR
;; RUN: midenc %s --emit=masm=- 2>&1 | filecheck %s --check-prefix=MASM
;;
;; Verify transparent casts retain their HIR meaning and checked physical scheduling.
;;
;; `intrinsics::felt::from_u32` becomes a `hir.bitcast`. Its lowering emits no instruction,
;; but the allocator brings its operand to the top and restores operand order for `sub`.
;; These two swaps belong to separate allocation boundaries, so a lowering-unit peephole
;; cannot cancel them without invalidating the verified plan.
;;
;; The load's alignment check and memory access likewise retain their allocation actions.
;; Pin these short sequences so changes to alias scheduling remain visible.

;; HIR-LABEL: builtin.function public extern("C") @sub_from_u32
;; HIR: hir.bitcast {{.*}} <{ ty = #builtin.type<felt> }>;
;; HIR: arith.sub

;; MASM-LABEL: pub proc sub_from_u32
;; MASM-NEXT: swap.1
;; MASM-NEXT: swap.1
;; MASM-NEXT: sub
;; MASM-NEXT: end

;; MASM-LABEL: pub proc load
;; MASM: assertz.err="pointer address does not meet minimum alignment for the type"
;; MASM-NEXT: push.0
;; MASM-NEXT: swap.1
;; MASM-NEXT: mem_load
;; MASM-NEXT: swap.1
;; MASM-NEXT: drop
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
