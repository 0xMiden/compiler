(module $test_rust_661f326c571b472b3f5afbc1f710009a76f4a75f71fca9a4e50c0ac7ae44f6e6.wasm
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
    i32.store
    local.get 0
    local.get 1
    local.get 2
    i32.lt_u
    i32.store8 offset=4
  )
)
