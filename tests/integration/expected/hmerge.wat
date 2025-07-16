(module $hmerge.wasm
  (type (;0;) (func (param i32 i32)))
  (type (;1;) (func (param f32 f32 f32 f32 f32 f32 f32 f32) (result f32)))
  (import "miden:core-intrinsics/intrinsics-crypto@1.0.0" "hmerge" (func $miden_stdlib_sys::intrinsics::crypto::extern_hmerge (;0;) (type 0)))
  (table (;0;) 1 1 funcref)
  (memory (;0;) 16)
  (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
  (export "memory" (memory 0))
  (export "entrypoint" (func $entrypoint))
  (func $entrypoint (;1;) (type 1) (param f32 f32 f32 f32 f32 f32 f32 f32) (result f32)
    (local i32)
    global.get $__stack_pointer
    i32.const 48
    i32.sub
    local.tee 8
    global.set $__stack_pointer
    local.get 8
    local.get 7
    f32.store offset=28
    local.get 8
    local.get 6
    f32.store offset=24
    local.get 8
    local.get 5
    f32.store offset=20
    local.get 8
    local.get 4
    f32.store offset=16
    local.get 8
    local.get 3
    f32.store offset=12
    local.get 8
    local.get 2
    f32.store offset=8
    local.get 8
    local.get 1
    f32.store offset=4
    local.get 8
    local.get 0
    f32.store
    local.get 8
    local.get 8
    i32.const 32
    i32.add
    call $miden_stdlib_sys::intrinsics::crypto::extern_hmerge
    local.get 8
    f32.load offset=32
    local.set 7
    local.get 8
    i32.const 48
    i32.add
    global.set $__stack_pointer
    local.get 7
  )
)
