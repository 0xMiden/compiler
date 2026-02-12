(module $test_rust_1cfe03a9674ad8024f366214b10f5cae9c09f1cdc7bf3b349b66b0ccadded955.wasm
  (type (;0;) (func (param i32 i32 i32)))
  (memory (;0;) 16)
  (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
  (global (;1;) i32 i32.const 1048576)
  (global (;2;) i32 i32.const 1048576)
  (export "memory" (memory 0))
  (export "entrypoint" (func $entrypoint))
  (export "__data_end" (global 1))
  (export "__heap_base" (global 2))
  (func $entrypoint (;0;) (type 0) (param i32 i32 i32)
    (local i64)
    local.get 0
    local.get 1
    i64.extend_i32_s
    local.get 2
    i64.extend_i32_s
    i64.mul
    local.tee 3
    i32.wrap_i64
    local.tee 2
    i32.store
    local.get 0
    local.get 3
    i64.const 32
    i64.shr_u
    i32.wrap_i64
    local.get 2
    i32.const 31
    i32.shr_s
    i32.ne
    i32.store8 offset=4
  )
)
