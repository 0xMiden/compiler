(module $test_rust_d63291a98b435c53f58385d5782fb46f0b0b78bee8e860843e7223106d66f7d6.wasm
  (type (;0;) (func (param i32 i32) (result i32)))
  (type (;1;) (func (result i32)))
  (memory (;0;) 17)
  (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
  (global (;1;) i32 i32.const 1048616)
  (global (;2;) i32 i32.const 1048624)
  (export "memory" (memory 0))
  (export "sum_arr" (func $sum_arr))
  (export "__main" (func $__main))
  (export "__data_end" (global 1))
  (export "__heap_base" (global 2))
  (func $sum_arr (;0;) (type 0) (param i32 i32) (result i32)
    (local i32)
    i32.const 0
    local.set 2
    block ;; label = @1
      local.get 1
      i32.eqz
      br_if 0 (;@1;)
      loop ;; label = @2
        local.get 0
        i32.load
        local.get 2
        i32.add
        local.set 2
        local.get 0
        i32.const 4
        i32.add
        local.set 0
        local.get 1
        i32.const -1
        i32.add
        local.tee 1
        br_if 0 (;@2;)
      end
    end
    local.get 2
  )
  (func $__main (;1;) (type 1) (result i32)
    i32.const 1048576
    i32.const 5
    call $sum_arr
    i32.const 1048596
    i32.const 5
    call $sum_arr
    i32.add
  )
  (data $.rodata (;0;) (i32.const 1048576) "\01\00\00\00\02\00\00\00\03\00\00\00\04\00\00\00\05\00\00\00\06\00\00\00\07\00\00\00\08\00\00\00\09\00\00\00\0a\00\00\00")
)
