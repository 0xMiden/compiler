(module $test_overflowing_div_u32.wasm
  (type (;0;) (func (param i32 i32 i32)))
  (table (;0;) 1 1 funcref)
  (memory (;0;) 16)
  (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
  (export "memory" (memory 0))
  (export "entrypoint" (func $entrypoint))
  (func $entrypoint (;0;) (type 0) (param i32 i32 i32)
    block ;; label = @1
      local.get 2
      br_if 0 (;@1;)
      unreachable
    end
    local.get 0
    i32.const 0
    i32.store8 offset=4
    local.get 0
    local.get 1
    local.get 2
    i32.div_u
    i32.store
  )
)
