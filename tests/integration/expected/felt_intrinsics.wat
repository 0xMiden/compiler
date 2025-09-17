(module $felt_intrinsics.wasm
  (type (;0;) (func (param f32 f32) (result f32)))
  (table (;0;) 1 1 funcref)
  (memory (;0;) 16)
  (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
  (export "memory" (memory 0))
  (export "entrypoint" (func $entrypoint))
  (func $entrypoint (;0;) (type 0) (param f32 f32) (result f32)
    local.get 0
    local.get 0
    local.get 1
    call $intrinsics::felt::mul
    local.get 0
    call $intrinsics::felt::sub
    local.get 1
    call $intrinsics::felt::add
    call $intrinsics::felt::div
  )
  (func $intrinsics::felt::add (;1;) (type 0) (param f32 f32) (result f32)
    unreachable
  )
  (func $intrinsics::felt::sub (;2;) (type 0) (param f32 f32) (result f32)
    unreachable
  )
  (func $intrinsics::felt::mul (;3;) (type 0) (param f32 f32) (result f32)
    unreachable
  )
  (func $intrinsics::felt::div (;4;) (type 0) (param f32 f32) (result f32)
    unreachable
  )
)
