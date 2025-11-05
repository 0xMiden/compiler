(module $test_rust_aef84d8c0f31ef839006490963a826949aaedc9315fd0ca99dc18ddcd7c7205b.wasm
  (type (;0;) (func (param i32 i64 i64 i64 i64)))
  (memory (;0;) 16)
  (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
  (global (;1;) i32 i32.const 1048576)
  (global (;2;) i32 i32.const 1048576)
  (export "memory" (memory 0))
  (export "entrypoint" (func $entrypoint))
  (export "__data_end" (global 1))
  (export "__heap_base" (global 2))
  (func $entrypoint (;0;) (type 0) (param i32 i64 i64 i64 i64)
    (local i64)
    local.get 0
    local.get 3
    local.get 1
    i64.mul_wide_u
    local.set 5
    i64.store
    local.get 0
    local.get 5
    local.get 3
    local.get 2
    i64.mul
    i64.add
    local.get 4
    local.get 1
    i64.mul
    i64.add
    i64.store offset=8
  )
)
