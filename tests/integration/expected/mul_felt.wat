(module $mul_felt.wasm
  (type (;0;) (func (param f32 f32) (result f32)))
  (import "miden:core-import/intrinsics-felt@1.0.0" "mul" (func $miden_stdlib_sys::intrinsics::felt::extern_mul (;0;) (type 0)))
  (table (;0;) 1 1 funcref)
  (memory (;0;) 16)
  (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
  (export "memory" (memory 0))
  (export "entrypoint" (func $entrypoint))
  (func $entrypoint (;1;) (type 0) (param f32 f32) (result f32)
    local.get 0
    local.get 1
    call $miden_stdlib_sys::intrinsics::felt::extern_mul
  )
)
