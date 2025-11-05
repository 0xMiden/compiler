(module $test_rust_f0bb65319ffababec660ada9dd2dd5f137503f60cf9c37332d6f7e171f275824.wasm
  (type (;0;) (func (param i32 i32 i32) (result i32)))
  (type (;1;) (func (result i32)))
  (memory (;0;) 16)
  (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
  (global (;1;) i32 i32.const 1048576)
  (global (;2;) i32 i32.const 1048576)
  (export "memory" (memory 0))
  (export "match_enum" (func $match_enum))
  (export "__main" (func $__main))
  (export "__data_end" (global 1))
  (export "__heap_base" (global 2))
  (func $match_enum (;0;) (type 0) (param i32 i32 i32) (result i32)
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
  (func $__main (;1;) (type 1) (result i32)
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
)
