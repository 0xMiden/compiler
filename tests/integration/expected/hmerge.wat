(module $hmerge.wasm
  (type (;0;) (func (param f32 f32 f32 f32 f32 f32 f32 f32 i32)))
  (type (;1;) (func (param i32 i32)))
  (import "miden:core-intrinsics/intrinsics-crypto@1.0.0" "hmerge" (func $miden_stdlib_sys::intrinsics::crypto::extern_hmerge (;0;) (type 0)))
  (table (;0;) 1 1 funcref)
  (memory (;0;) 16)
  (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
  (export "memory" (memory 0))
  (export "entrypoint" (func $entrypoint))
  (func $entrypoint (;1;) (type 1) (param i32 i32)
    local.get 1
    f32.load offset=32
    local.get 1
    f32.load offset=36
    local.get 1
    f32.load offset=40
    local.get 1
    f32.load offset=44
    local.get 1
    f32.load
    local.get 1
    f32.load offset=4
    local.get 1
    f32.load offset=8
    local.get 1
    f32.load offset=12
    local.get 0
    call $miden_stdlib_sys::intrinsics::crypto::extern_hmerge
  )
)
