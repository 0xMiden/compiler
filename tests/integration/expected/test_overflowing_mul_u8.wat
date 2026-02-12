(module $test_rust_80b957c04bce574d463a00ffe63c65630186bbabae95ad08ab5093907321c965.wasm
  (type (;0;) (func (param i32 i32 i32)))
  (memory (;0;) 16)
  (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
  (global (;1;) i32 i32.const 1048576)
  (global (;2;) i32 i32.const 1048576)
  (export "memory" (memory 0))
  (export "entrypoint" (func $entrypoint))
  (export "__data_end" (global 1))
  (export "__heap_base" (global 2))
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
