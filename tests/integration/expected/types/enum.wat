(module $test_rust_dd1732f1cb2cdf8742b13cf624a182a3893ee7d1f0d94b11ef6b10382dffd79c.wasm
  (type (;0;) (func (result i32)))
  (type (;1;) (func (param i32 i32 i32) (result i32)))
  (memory (;0;) 16)
  (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
  (global (;1;) i32 i32.const 1048576)
  (global (;2;) i32 i32.const 1048576)
  (export "memory" (memory 0))
  (export "__main" (func $__main))
  (export "match_enum" (func $match_enum))
  (export "__data_end" (global 1))
  (export "__heap_base" (global 2))
  (func $__main (;0;) (type 0) (result i32)
    i32.const 3
    i32.const 5
    i32.const 0
    call $match_enum
    i32.const 3
    i32.const 5
    i32.const 1
    call $match_enum
    i32.add
    i32.const 3
    i32.const 5
    i32.const 2
    call $match_enum
    i32.add
  )
  (func $match_enum (;1;) (type 1) (param i32 i32 i32) (result i32)
    block ;; label = @1
      block ;; label = @2
        block ;; label = @3
          local.get 2
          i32.const 255
          i32.and
          br_table 0 (;@3;) 1 (;@2;) 2 (;@1;) 0 (;@3;)
        end
        local.get 1
        local.get 0
        i32.add
        return
      end
      local.get 0
      local.get 1
      i32.sub
      return
    end
    local.get 1
    local.get 0
    i32.mul
  )
)
