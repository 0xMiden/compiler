(module $test_rust_4a304ec72db272fa9f8651c55af48f6a136a1107366ae9102fb8e3b63eda2d10.wasm
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
    local.get 0
    local.get 1
    local.get 2
    i32.lt_u
    i32.store8 offset=2
    local.get 0
    local.get 1
    local.get 2
    i32.sub
    i32.store16
  )
)
