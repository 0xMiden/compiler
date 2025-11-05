(module $test_rust_e6d553fb1c80aef6e5d6f2891701197bedac471cf510bd2495f99889d9543cd4.wasm
  (type (;0;) (func))
  (type (;1;) (func (result i32)))
  (memory (;0;) 17)
  (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
  (global (;1;) i32 i32.const 1048585)
  (global (;2;) i32 i32.const 1048592)
  (export "memory" (memory 0))
  (export "global_var_update" (func $global_var_update))
  (export "__main" (func $__main))
  (export "__data_end" (global 1))
  (export "__heap_base" (global 2))
  (func $global_var_update (;0;) (type 0)
    i32.const 0
    i32.const 0
    i32.load8_u offset=1048577
    i32.const 1
    i32.add
    i32.store8 offset=1048576
  )
  (func $__main (;1;) (type 1) (result i32)
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
  (data $.data (;0;) (i32.const 1048576) "\01\02\03\04\05\06\07\08\09")
)
