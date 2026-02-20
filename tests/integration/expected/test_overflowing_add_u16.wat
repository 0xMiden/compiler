(module $test_overflowing_add_u16.wasm
  (type (;0;) (func (param i32 i32 i32)))
  (table (;0;) 1 1 funcref)
  (memory (;0;) 16)
  (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
  (export "memory" (memory 0))
  (export "entrypoint" (func $entrypoint))
  (func $entrypoint (;0;) (type 0) (param i32 i32 i32)
    local.get 0
    local.get 2
    local.get 1
    i32.add
    local.tee 1
    i32.store16
    local.get 0
    local.get 1
    i32.const 65535
    i32.and
    local.get 1
    i32.ne
    i32.store8 offset=2
  )
)
