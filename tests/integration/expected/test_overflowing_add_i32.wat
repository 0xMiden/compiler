(module $test_rust_ff6e71408b9f9c38c7473bca110f3fb773c6050dcf5d56f9a91b26ea80b340b5.wasm
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
    (local i32)
    local.get 0
    local.get 1
    local.get 2
    i32.add
    local.tee 3
    i32.store
    local.get 0
    local.get 2
    i32.const 0
    i32.lt_s
    local.get 3
    local.get 1
    i32.lt_s
    i32.xor
    i32.store8 offset=4
  )
)
