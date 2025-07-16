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
    (local i32 i32)
    global.get $__stack_pointer
    local.tee 8
    i32.const 64
    i32.sub
    i32.const -32
    i32.and
    local.tee 9
    global.set $__stack_pointer
    local.get 9
    local.get 7
    f32.store offset=60
    local.get 9
    local.get 6
    f32.store offset=56
    local.get 9
    local.get 5
    f32.store offset=52
    local.get 9
    local.get 4
    f32.store offset=48
    local.get 9
    local.get 3
    f32.store offset=44
    local.get 9
    local.get 2
    f32.store offset=40
    local.get 9
    local.get 1
    f32.store offset=36
    local.get 9
    local.get 0
    f32.store offset=32
    local.get 9
    i32.const 32
    i32.add
    local.get 9
    call $miden_stdlib_sys::intrinsics::crypto::extern_hmerge
    local.get 9
    f32.load
    local.set 7
    local.get 8
    global.set $__stack_pointer
    local.get 7
  )
)
