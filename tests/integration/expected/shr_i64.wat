(module $test_rust_186d93d83b74a241eb99ffa0ead88d2435a288695c19f22c9c35eecfda978717.wasm
  (type (;0;) (func (param i64 i64) (result i64)))
  (memory (;0;) 16)
  (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
  (global (;1;) i32 i32.const 1048576)
  (global (;2;) i32 i32.const 1048576)
  (export "memory" (memory 0))
  (export "entrypoint" (func $entrypoint))
  (export "__data_end" (global 1))
  (export "__heap_base" (global 2))
  (func $entrypoint (;0;) (type 0) (param i64 i64) (result i64)
    local.get 0
    local.get 1
    i64.shr_s
  )
)
