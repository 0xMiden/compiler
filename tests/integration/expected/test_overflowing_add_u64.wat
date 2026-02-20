(module $test_overflowing_add_u64.wasm
  (type (;0;) (func (param i32 i64 i64)))
  (table (;0;) 1 1 funcref)
  (memory (;0;) 16)
  (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
  (export "memory" (memory 0))
  (export "entrypoint" (func $entrypoint))
  (func $entrypoint (;0;) (type 0) (param i32 i64 i64)
    local.get 2
    i64.const 0
    local.get 1
    i64.const 0
    i64.add128
    local.set 2
    local.set 1
    local.get 0
    local.get 2
    i64.store8 offset=8
    local.get 0
    local.get 1
    i64.store
  )
)
