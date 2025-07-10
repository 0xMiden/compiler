(module $test_rust_c232a9b0045c768c9a1fca4d8a419280b591a50dcbebe88fe280529cdd423ec0.wasm
  (type (;0;) (func (param i32 i64 i64 i64 i64)))
  (memory (;0;) 16)
  (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
  (global (;1;) i32 i32.const 1048576)
  (global (;2;) i32 i32.const 1048576)
  (export "memory" (memory 0))
  (export "entrypoint" (func $entrypoint))
  (export "__data_end" (global 1))
  (export "__heap_base" (global 2))
  (func $entrypoint (;0;) (type 0) (param i32 i64 i64 i64 i64)
    local.get 3
    local.get 4
    local.get 1
    local.get 2
    i64.add128
    local.set 1
    local.set 2
    local.get 0
    local.get 1
    i64.store offset=8
    local.get 0
    local.get 2
    i64.store
  )
)
