(module $test_rust_c2e6cec606b20447e4c5accdc801c32098f0d7cda3811f8c4e38f16911210b8f.wasm
  (type (;0;) (func (param i64 i64) (result i64)))
  (func $entrypoint (;0;) (type 0) (param i64 i64) (result i64)
    local.get 0
    local.get 1
    i64.sub
  )
  (memory (;0;) 16)
  (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
  (global (;1;) i32 i32.const 1048576)
  (global (;2;) i32 i32.const 1048576)
  (export "memory" (memory 0))
  (export "entrypoint" (func $entrypoint))
  (export "__data_end" (global 1))
  (export "__heap_base" (global 2))
)