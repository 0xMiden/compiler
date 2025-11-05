(module $test_rust_345f3c1e58fe5c9ef31421da6a362d9cadbf519ad68045840229e8b76f58a4f4.wasm
  (type (;0;) (func (param i64) (result i64)))
  (memory (;0;) 16)
  (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
  (global (;1;) i32 i32.const 1048576)
  (global (;2;) i32 i32.const 1048576)
  (export "memory" (memory 0))
  (export "entrypoint" (func $entrypoint))
  (export "__data_end" (global 1))
  (export "__heap_base" (global 2))
  (func $entrypoint (;0;) (type 0) (param i64) (result i64)
    local.get 0
    i64.const -1
    i64.xor
  )
)
