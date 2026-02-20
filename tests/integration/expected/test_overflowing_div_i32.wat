(module $test_overflowing_div_i32.wasm
  (type (;0;) (func (param i32 i32 i32)))
  (table (;0;) 1 1 funcref)
  (memory (;0;) 16)
  (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
  (export "memory" (memory 0))
  (export "entrypoint" (func $entrypoint))
  (func $entrypoint (;0;) (type 0) (param i32 i32 i32)
    (local i32 i32)
    i32.const -2147483648
    local.set 3
    block ;; label = @1
      block ;; label = @2
        local.get 1
        i32.const -2147483648
        i32.eq
        local.get 2
        i32.const -1
        i32.eq
        i32.and
        local.tee 4
        br_if 0 (;@2;)
        local.get 2
        i32.eqz
        br_if 1 (;@1;)
        local.get 1
        local.get 2
        i32.div_s
        local.set 3
      end
      local.get 0
      local.get 4
      i32.store8 offset=4
      local.get 0
      local.get 3
      i32.store
      return
    end
    unreachable
  )
)
