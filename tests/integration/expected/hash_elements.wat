(module $hash_elements.wasm
  (type (;0;) (func (param i32 i32 i32)))
  (type (;1;) (func (param f32 f32 f32 f32 f32 f32 f32 f32) (result f32)))
  (import "miden:core-import/stdlib-crypto-hashes-rpo@1.0.0" "hash-memory" (func $miden_stdlib_sys::stdlib::crypto::hashes::extern_hash_memory (;0;) (type 0)))
  (table (;0;) 1 1 funcref)
  (memory (;0;) 16)
  (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
  (export "memory" (memory 0))
  (export "entrypoint" (func $entrypoint))
  (func $entrypoint (;1;) (type 1) (param f32 f32 f32 f32 f32 f32 f32 f32) (result f32)
    (local i32 i32 i32)
    global.get $__stack_pointer
    local.tee 8
    local.set 9
    local.get 8
    i32.const 64
    i32.sub
    i32.const -32
    i32.and
    local.tee 10
    global.set $__stack_pointer
    i32.const 0
    local.set 8
    block ;; label = @1
      loop ;; label = @2
        local.get 8
        i32.const 32
        i32.eq
        br_if 1 (;@1;)
        local.get 10
        local.get 8
        i32.add
        local.get 0
        f32.store
        local.get 8
        i32.const 4
        i32.add
        local.set 8
        br 0 (;@2;)
      end
    end
    local.get 10
    i32.const 8
    local.get 10
    i32.const 32
    i32.add
    call $miden_stdlib_sys::stdlib::crypto::hashes::extern_hash_memory
    local.get 10
    f32.load offset=32
    local.set 0
    local.get 9
    global.set $__stack_pointer
    local.get 0
  )
)
