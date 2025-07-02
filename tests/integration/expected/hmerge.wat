(module $hmerge.wasm
  (type (;0;) (func (param f32 f32 f32 f32 f32 f32 f32 f32 i32)))
  (type (;1;) (func (param i32 f32 f32 f32 f32 f32 f32 f32 f32)))
  (import "miden:core-intrinsics/intrinsics-crypto@1.0.0" "hmerge" (func $miden_stdlib_sys::intrinsics::crypto::extern_hmerge (;0;) (type 0)))
  (table (;0;) 1 1 funcref)
  (memory (;0;) 16)
  (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
  (export "memory" (memory 0))
  (export "entrypoint" (func $entrypoint))
  (func $entrypoint (;1;) (type 1) (param i32 f32 f32 f32 f32 f32 f32 f32 f32)
    local.get 1
    local.get 2
    local.get 3
    local.get 4
    local.get 5
    local.get 6
    local.get 7
    local.get 8
    local.get 0
    call $miden_stdlib_sys::intrinsics::crypto::extern_hmerge
  )
)
