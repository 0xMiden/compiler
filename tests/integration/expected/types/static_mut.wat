(module $test_rust_2512f26a084edd4aa11742f4c2ef0985a5681361fee42216136cb00d26224778.wasm
  (type (;0;) (func (result i32)))
  (type (;1;) (func))
  (memory (;0;) 17)
  (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
  (global (;1;) i32 i32.const 1048585)
  (global (;2;) i32 i32.const 1048592)
  (export "memory" (memory 0))
  (export "__main" (func $__main))
  (export "global_var_update" (func $global_var_update))
  (export "__data_end" (global 1))
  (export "__heap_base" (global 2))
  (func $__main (;0;) (type 0) (result i32)
    (local i32 i32 i32)
    call $global_var_update
    i32.const 0
    local.set 0
    i32.const -9
    local.set 1
    loop ;; label = @1
      local.get 1
      i32.const 1048585
      i32.add
      i32.load8_u
      local.get 0
      i32.add
      local.set 0
      local.get 1
      i32.const 1
      i32.add
      local.tee 2
      local.set 1
      local.get 2
      br_if 0 (;@1;)
    end
    local.get 0
    i32.const 255
    i32.and
  )
  (func $global_var_update (;1;) (type 1)
    i32.const 0
    i32.const 0
    i32.load8_u offset=1048577
    i32.const 1
    i32.add
    i32.store8 offset=1048576
  )
  (data $.data (;0;) (i32.const 1048576) "\01\02\03\04\05\06\07\08\09")
)
