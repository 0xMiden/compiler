(module $test_overflowing_sub_i32.wasm
  (type (;0;) (func (param i32 i32 i32)))
  (table (;0;) 1 1 funcref)
  (memory (;0;) 16)
  (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
  (export "memory" (memory 0))
  (export "entrypoint" (func $entrypoint))
  (func $entrypoint (;0;) (type 0) (param i32 i32 i32)
    (local i32)
    local.get 0
    local.get 1
    local.get 2
    i32.sub
    local.tee 3
    i32.store
    local.get 0
    local.get 2
    i32.const 0
    i32.gt_s
    local.get 3
    local.get 1
    i32.lt_s
    i32.xor
    i32.store8 offset=4
  )
)
