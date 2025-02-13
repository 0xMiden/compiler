(module $felt_intrinsics.wasm
  (type (;0;) (func (param f32 f32) (result f32)))
  (import "miden:core-import/intrinsics-felt@1.0.0" "mul" (func $miden_stdlib_sys::intrinsics::felt::extern_mul (;0;) (type 0)))
  (import "miden:core-import/intrinsics-felt@1.0.0" "sub" (func $miden_stdlib_sys::intrinsics::felt::extern_sub (;1;) (type 0)))
  (import "miden:core-import/intrinsics-felt@1.0.0" "add" (func $miden_stdlib_sys::intrinsics::felt::extern_add (;2;) (type 0)))
  (import "miden:core-import/intrinsics-felt@1.0.0" "div" (func $miden_stdlib_sys::intrinsics::felt::extern_div (;3;) (type 0)))
  (func $entrypoint (;4;) (type 0) (param f32 f32) (result f32)
    local.get 0
    local.get 0
    local.get 1
    call $miden_stdlib_sys::intrinsics::felt::extern_mul
    local.get 0
    call $miden_stdlib_sys::intrinsics::felt::extern_sub
    local.get 1
    call $miden_stdlib_sys::intrinsics::felt::extern_add
    call $miden_stdlib_sys::intrinsics::felt::extern_div
  )
  (table (;0;) 1 1 funcref)
  (memory (;0;) 16)
  (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
  (export "memory" (memory 0))
  (export "entrypoint" (func $entrypoint))
)