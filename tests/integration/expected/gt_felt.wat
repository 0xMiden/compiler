(module $gt_felt.wasm
  (type (;0;) (func (param f32 f32) (result i32)))
  (table (;0;) 1 1 funcref)
  (memory (;0;) 16)
  (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
  (export "memory" (memory 0))
  (export "entrypoint" (func $entrypoint))
  (func $entrypoint (;0;) (type 0) (param f32 f32) (result i32)
    local.get 0
    local.get 1
    call $intrinsics::felt::gt
    i32.const 0
    i32.ne
  )
  (func $intrinsics::felt::gt (;1;) (type 0) (param f32 f32) (result i32)
    unreachable
  )
)
