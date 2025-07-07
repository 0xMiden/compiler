(module $hash_elements.wasm
  (type (;0;) (func (param i32 i32 i32)))
  (type (;1;) (func (param i32 f32 f32 f32 f32 f32 f32 f32 f32)))
  (import "miden:core-import/stdlib-crypto-hashes-rpo@1.0.0" "hash-memory" (func $miden_stdlib_sys::stdlib::crypto::hashes::extern_hash_memory (;0;) (type 0)))
  (table (;0;) 1 1 funcref)
  (memory (;0;) 16)
  (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
  (export "memory" (memory 0))
  (export "entrypoint" (func $entrypoint))
  (func $entrypoint (;1;) (type 1) (param i32 f32 f32 f32 f32 f32 f32 f32 f32)
    (local i32)
    global.get $__stack_pointer
    i32.const 32
    i32.sub
    local.tee 9
    global.set $__stack_pointer
    local.get 9
    local.get 8
    f32.store offset=28
    local.get 9
    local.get 7
    f32.store offset=24
    local.get 9
    local.get 6
    f32.store offset=20
    local.get 9
    local.get 5
    f32.store offset=16
    local.get 9
    local.get 4
    f32.store offset=12
    local.get 9
    local.get 3
    f32.store offset=8
    local.get 9
    local.get 2
    f32.store offset=4
    local.get 9
    local.get 1
    f32.store
    local.get 9
    i32.const 8
    local.get 0
    call $miden_stdlib_sys::stdlib::crypto::hashes::extern_hash_memory
    local.get 9
    i32.const 32
    i32.add
    global.set $__stack_pointer
  )
)
