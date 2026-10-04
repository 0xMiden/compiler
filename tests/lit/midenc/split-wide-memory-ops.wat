;; RUN: midenc %s --emit=masm=- 2>&1 | filecheck %s
;;
;; Verify that a 64-bit store of two 32-bit halves, the shape in which LLVM's IR-level passes
;; store two 32-bit values they carry as one `i64`, is lowered as two 32-bit stores, and that a
;; 64-bit load read only for its high half is lowered as a 32-bit load: no 64-bit integer
;; operation touches the halves, which may be felts outside the `u32` range.

(module $split_wide_memory_ops.wasm
  (memory 1)
  (func $store_halves (param $p i32) (param $lo i32) (param $hi i32)
    (i64.store
      (local.get $p)
      (i64.or
        (i64.extend_i32_u (local.get $lo))
        (i64.shl (i64.extend_i32_u (local.get $hi)) (i64.const 32)))))
  (func $load_high_half (param $p i32) (result i32)
    (i32.wrap_i64 (i64.shr_u (i64.load (local.get $p)) (i64.const 32))))
  (export "store_halves" (func $store_halves))
  (export "load_high_half" (func $load_high_half))
)

;; The low half at `p`, the high half at `p + 4`, each with a 32-bit store
;; CHECK-LABEL: pub proc store_halves
;; CHECK-NOT: exec.::miden::core::math::u64
;; CHECK-NOT: exec.::intrinsics::mem::store_dw
;; CHECK: exec.::intrinsics::mem::store_sw
;; CHECK-NOT: exec.::miden::core::math::u64
;; CHECK-NOT: exec.::intrinsics::mem::store_dw
;; CHECK: push.4
;; CHECK-NEXT: add
;; CHECK-NEXT: u32assert
;; CHECK-NOT: exec.::miden::core::math::u64
;; CHECK-NOT: exec.::intrinsics::mem::store_dw
;; CHECK: exec.::intrinsics::mem::store_sw
;; CHECK-NOT: exec.::miden::core::math::u64
;; CHECK-NOT: exec.::intrinsics::mem::store_dw
;; CHECK: end

;; The high half read from `p + 4` with a 32-bit load, without the shift
;; CHECK-LABEL: pub proc load_high_half
;; CHECK-NOT: exec.::intrinsics::mem::load_dw
;; CHECK: push.4
;; CHECK-NEXT: add
;; CHECK-NEXT: u32assert
;; CHECK-NOT: exec.::miden::core::math::u64
;; CHECK-NOT: exec.::intrinsics::mem::load_dw
;; CHECK: exec.::intrinsics::mem::load_sw
;; CHECK-NOT: exec.::miden::core::math::u64
;; CHECK-NOT: exec.::intrinsics::mem::load_dw
;; CHECK: end
