(module $test_rust_0cd87c9311a6c4443098d959e79bb2f03e49ac6420204bf285af60162abb7723.wasm
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
    i32.mul
    local.tee 2
    i32.store16
    local.get 0
    local.get 2
    i32.const 16
    i32.shr_u
    i32.const 0
    i32.ne
    i32.store8 offset=2
  )
)
