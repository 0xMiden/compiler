(module $test_rust_6b31118a71e100e1653e01e16012428a3ef4b388ea4b8aa8f00a0e4ff40c4ad9.wasm
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
    i32.le_u
  )
)
