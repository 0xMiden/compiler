(module $test_rust_7d82f2e08b811f59ed036f725c1b72ca685b95ee301d765a74b8f7da3048058b.wasm
  (type (;0;) (func (param i64 i64) (result i32)))
  (func $entrypoint (;0;) (type 0) (param i64 i64) (result i32)
    local.get 0
    local.get 1
    i64.gt_s
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