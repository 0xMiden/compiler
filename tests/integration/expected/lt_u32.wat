(module $test_rust_1fcb2f151e54a93413cfbf27b720161db50a02b59d060ed448c3ffe2c3c9cc6c.wasm
  (type (;0;) (func (param i32 i32) (result i32)))
  (memory (;0;) 16)
  (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
  (global (;1;) i32 i32.const 1048576)
  (global (;2;) i32 i32.const 1048576)
  (export "memory" (memory 0))
  (export "entrypoint" (func $entrypoint))
  (export "__data_end" (global 1))
  (export "__heap_base" (global 2))
  (func $entrypoint (;0;) (type 0) (param i32 i32) (result i32)
    local.get 0
    local.get 1
    i32.lt_u
  )
)
