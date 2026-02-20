(module $test_overflowing_mul_u8.wasm
  (type (;0;) (func (param i32 i32 i32)))
  (table (;0;) 1 1 funcref)
  (memory (;0;) 16)
  (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
  (export "memory" (memory 0))
  (export "entrypoint" (func $entrypoint))
  (func $entrypoint (;0;) (type 0) (param i32 i32 i32)
    local.get 0
    local.get 1
    local.get 2
    i32.mul
    local.tee 2
    i32.store8
    local.get 0
    local.get 2
    i32.const 8
    i32.shr_u
    i32.const 0
    i32.ne
    i32.store8 offset=1
  )
)
