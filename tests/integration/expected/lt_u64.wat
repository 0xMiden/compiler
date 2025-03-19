(module $test_rust_416d83d84b3ae500f053222602966b59099d85bd2706dbbaf20b4eb34e76cf0e.wasm
  (type (;0;) (func (param i64 i64) (result i32)))
  (func $entrypoint (;0;) (type 0) (param i64 i64) (result i32)
    local.get 0
    local.get 1
    i64.lt_u
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