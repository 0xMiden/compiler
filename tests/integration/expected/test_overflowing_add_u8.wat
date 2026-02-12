(module $test_rust_10c64afc4cff5140eb8bace16aa1791756ddfb6b76732b17a4cbc0b665764258.wasm
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
    local.get 2
    local.get 1
    i32.add
    local.tee 1
    i32.store8
    local.get 0
    local.get 1
    i32.const 255
    i32.and
    local.get 1
    i32.ne
    i32.store8 offset=1
  )
)
