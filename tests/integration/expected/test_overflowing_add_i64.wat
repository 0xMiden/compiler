(module $test_rust_6ed5af737a3e9729eb235fcf34544b6d12759845617821b417872067ea766b40.wasm
  (type (;0;) (func (param i32 i64 i64)))
  (memory (;0;) 16)
  (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
  (global (;1;) i32 i32.const 1048576)
  (global (;2;) i32 i32.const 1048576)
  (export "memory" (memory 0))
  (export "entrypoint" (func $entrypoint))
  (export "__data_end" (global 1))
  (export "__heap_base" (global 2))
  (func $entrypoint (;0;) (type 0) (param i32 i64 i64)
    (local i64)
    local.get 0
    local.get 1
    local.get 2
    i64.add
    local.tee 3
    i64.store
    local.get 0
    local.get 2
    i64.const 0
    i64.lt_s
    local.get 3
    local.get 1
    i64.lt_s
    i32.xor
    i32.store8 offset=8
  )
)
