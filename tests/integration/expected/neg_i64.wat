(module $test_rust_7e6ecbf37c062a73130e37145b77eb9dd3cce8aa16128ac9fcca7dc837ca562c.wasm
  (type (;0;) (func (param i64) (result i64)))
  (func $entrypoint (;0;) (type 0) (param i64) (result i64)
    i64.const 0
    local.get 0
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