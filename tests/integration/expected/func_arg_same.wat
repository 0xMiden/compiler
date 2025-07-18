(module $func_arg_same.wasm
  (type (;0;) (func (param i32 i32) (result i32)))
  (table (;0;) 1 1 funcref)
  (memory (;0;) 16)
  (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
  (export "memory" (memory 0))
  (export "entrypoint" (func $entrypoint))
  (export "intrinsic" (func $intrinsic))
  (func $entrypoint (;0;) (type 0) (param i32 i32) (result i32)
    local.get 0
    local.get 0
    call $intrinsic
  )
  (func $intrinsic (;1;) (type 0) (param i32 i32) (result i32)
    local.get 0
  )
)
