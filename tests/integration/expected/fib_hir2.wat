(module $miden_integration_tests_rust_fib_wasm.wasm
  (type (;0;) (func (param i32) (result i32)))
  (func $fib (;0;) (type 0) (param i32) (result i32)
    (local i32 i32 i32)
    i32.const 0
    local.set 1
    i32.const 1
    local.set 2
    loop (result i32) ;; label = @1
      local.get 2
      local.set 3
      block ;; label = @2
        local.get 0
        br_if 0 (;@2;)
        local.get 1
        return
      end
      local.get 0
      i32.const -1
      i32.add
      local.set 0
      local.get 1
      local.get 3
      i32.add
      local.set 2
      local.get 3
      local.set 1
      br 0 (;@1;)
    end
  )
  (memory (;0;) 16)
  (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
  (global (;1;) i32 i32.const 1048576)
  (global (;2;) i32 i32.const 1048576)
  (export "memory" (memory 0))
  (export "fib" (func $fib))
  (export "__data_end" (global 1))
  (export "__heap_base" (global 2))
)