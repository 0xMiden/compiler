(module $test_rust_3ab387e5fafe4f8316074728a13b9c62413b6215a11492c80aa3e1d10383e008.wasm
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
    i32.const 15
    i32.and
    i32.shl
    i32.const 65535
    i32.and
  )
)
