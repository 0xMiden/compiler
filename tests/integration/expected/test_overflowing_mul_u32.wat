(module $test_rust_a332e98753eaa727b86ea68c0da32bb6bbe8b6a7cc84cb279e4d21d824527a27.wasm
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
    i64.extend_i32_u
    local.get 2
    i64.extend_i32_u
    i64.mul
    local.tee 3
    i64.store32
    local.get 0
    local.get 3
    i64.const 32
    i64.shr_u
    i32.wrap_i64
    i32.const 0
    i32.ne
    i32.store8 offset=4
  )
)
