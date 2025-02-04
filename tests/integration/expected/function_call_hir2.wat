(module $function_call_hir2.wasm
  (type (;0;) (func (param i32 i32) (result i32)))
  (func $add (;0;) (type 0) (param i32 i32) (result i32)
    local.get 1
    local.get 0
    i32.add
  )
  (func $entrypoint (;1;) (type 0) (param i32 i32) (result i32)
    local.get 0
    local.get 1
    call $add
  )
  (memory (;0;) 16)
  (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
  (export "memory" (memory 0))
  (export "add" (func $add))
  (export "entrypoint" (func $entrypoint))
)