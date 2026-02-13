(module $hmerge.wasm
  (type (;0;) (func (param f32 f32 f32 f32 f32 f32 f32 f32) (result f32)))
  (type (;1;) (func (param i32 i32)))
  (type (;2;) (func (param i32 i32 i32) (result i32)))
  (table (;0;) 1 1 funcref)
  (memory (;0;) 17)
  (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
  (export "memory" (memory 0))
  (export "entrypoint" (func $entrypoint))
  (func $entrypoint (;0;) (type 0) (param f32 f32 f32 f32 f32 f32 f32 f32) (result f32)
    (local i32)
    global.get $__stack_pointer
    i32.const 64
    i32.sub
    local.tee 8
    global.set $__stack_pointer
    local.get 8
    local.get 3
    f32.store offset=60
    local.get 8
    local.get 2
    f32.store offset=56
    local.get 8
    local.get 1
    f32.store offset=52
    local.get 8
    local.get 0
    f32.store offset=48
    local.get 8
    i32.const 16
    i32.add
    local.get 8
    i32.const 48
    i32.add
    call $<miden_field::word::Word as core::convert::From<[miden_field::wasm_miden::Felt; 4]>>::from
    local.get 8
    local.get 7
    f32.store offset=60
    local.get 8
    local.get 6
    f32.store offset=56
    local.get 8
    local.get 5
    f32.store offset=52
    local.get 8
    local.get 4
    f32.store offset=48
    local.get 8
    local.get 8
    i32.const 48
    i32.add
    call $<miden_field::word::Word as core::convert::From<[miden_field::wasm_miden::Felt; 4]>>::from
    local.get 8
    i32.const 40
    i32.add
    local.get 8
    i64.load offset=8
    i64.store
    local.get 8
    local.get 8
    i64.load
    i64.store offset=32
    local.get 8
    i32.const 16
    i32.add
    local.get 8
    i32.const 48
    i32.add
    call $intrinsics::crypto::hmerge
    local.get 8
    i32.const 48
    i32.add
    i32.const 0
    i32.const 1048588
    call $<miden_field::word::Word as core::ops::index::Index<usize>>::index
    f32.load
    local.set 3
    local.get 8
    i32.const 64
    i32.add
    global.set $__stack_pointer
    local.get 3
  )
  (func $intrinsics::crypto::hmerge (;1;) (type 1) (param i32 i32)
    unreachable
  )
  (func $<miden_field::word::Word as core::ops::index::Index<usize>>::index (;2;) (type 2) (param i32 i32 i32) (result i32)
    block ;; label = @1
      local.get 1
      i32.const 3
      i32.gt_u
      br_if 0 (;@1;)
      local.get 0
      local.get 1
      i32.const 2
      i32.shl
      i32.add
      return
    end
    unreachable
  )
  (func $<miden_field::word::Word as core::convert::From<[miden_field::wasm_miden::Felt; 4]>>::from (;3;) (type 1) (param i32 i32)
    local.get 0
    local.get 1
    i64.load offset=8 align=4
    i64.store offset=8
    local.get 0
    local.get 1
    i64.load align=4
    i64.store
  )
  (data $.rodata (;0;) (i32.const 1048576) "<redacted>\00\00\00\00\10\00\0a\00\00\00\00\00\00\00\00\00\00\00")
)
