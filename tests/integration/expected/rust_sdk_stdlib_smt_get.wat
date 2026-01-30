(module $rust_sdk_stdlib_smt_get.wasm
  (type (;0;) (func (param i32 i32) (result i32)))
  (type (;1;) (func (param f32 f32 f32 f32 f32 f32 f32 f32)))
  (type (;2;) (func (param f32 f32) (result i32)))
  (type (;3;) (func (param i64) (result f32)))
  (type (;4;) (func (param f32 f32 f32 f32 f32 f32 f32 f32 i32)))
  (table (;0;) 1 1 funcref)
  (memory (;0;) 16)
  (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
  (export "memory" (memory 0))
  (export "entrypoint" (func $entrypoint))
  (func $<miden_stdlib_sys::intrinsics::word::Word as core::cmp::PartialEq>::eq (;0;) (type 0) (param i32 i32) (result i32)
    (local i32)
    i32.const 0
    local.set 2
    block ;; label = @1
      local.get 0
      f32.load
      local.get 1
      f32.load
      call $intrinsics::felt::eq
      i32.const 1
      i32.ne
      br_if 0 (;@1;)
      local.get 0
      f32.load offset=4
      local.get 1
      f32.load offset=4
      call $intrinsics::felt::eq
      i32.const 1
      i32.ne
      br_if 0 (;@1;)
      local.get 0
      f32.load offset=8
      local.get 1
      f32.load offset=8
      call $intrinsics::felt::eq
      i32.const 1
      i32.ne
      br_if 0 (;@1;)
      local.get 0
      f32.load offset=12
      local.get 1
      f32.load offset=12
      call $intrinsics::felt::eq
      i32.const 1
      i32.eq
      local.set 2
    end
    local.get 2
  )
  (func $entrypoint (;1;) (type 1) (param f32 f32 f32 f32 f32 f32 f32 f32)
    (local i32 i32 i64 i64 i64)
    global.get $__stack_pointer
    local.tee 8
    local.set 9
    local.get 8
    i32.const 96
    i32.sub
    i32.const -32
    i32.and
    local.tee 8
    global.set $__stack_pointer
    local.get 8
    local.get 7
    f32.store offset=28
    local.get 8
    local.get 6
    f32.store offset=24
    local.get 8
    local.get 5
    f32.store offset=20
    local.get 8
    local.get 4
    f32.store offset=16
    local.get 3
    local.get 2
    local.get 1
    local.get 0
    local.get 7
    local.get 6
    local.get 5
    local.get 4
    local.get 8
    i32.const 64
    i32.add
    call $miden::core::collections::smt::get
    local.get 8
    i64.load offset=88
    local.set 10
    local.get 8
    i64.load offset=80
    local.set 11
    local.get 8
    i64.load offset=72
    local.set 12
    local.get 8
    local.get 8
    i64.load offset=64
    i64.const 32
    i64.rotl
    i64.store offset=40
    local.get 8
    local.get 12
    i64.const 32
    i64.rotl
    i64.store offset=32
    local.get 8
    local.get 11
    i64.const 32
    i64.rotl
    i64.store offset=56
    local.get 8
    local.get 10
    i64.const 32
    i64.rotl
    i64.store offset=48
    i64.const 10
    call $intrinsics::felt::from_u64_unchecked
    local.set 7
    i64.const 11
    call $intrinsics::felt::from_u64_unchecked
    local.set 6
    i64.const 12
    call $intrinsics::felt::from_u64_unchecked
    local.set 5
    local.get 8
    i64.const 13
    call $intrinsics::felt::from_u64_unchecked
    f32.store offset=76
    local.get 8
    local.get 5
    f32.store offset=72
    local.get 8
    local.get 6
    f32.store offset=68
    local.get 8
    local.get 7
    f32.store offset=64
    block ;; label = @1
      local.get 8
      i32.const 32
      i32.add
      local.get 8
      i32.const 64
      i32.add
      call $<miden_stdlib_sys::intrinsics::word::Word as core::cmp::PartialEq>::eq
      i32.eqz
      br_if 0 (;@1;)
      local.get 8
      i32.const 48
      i32.add
      local.get 8
      i32.const 16
      i32.add
      call $<miden_stdlib_sys::intrinsics::word::Word as core::cmp::PartialEq>::eq
      i32.eqz
      br_if 0 (;@1;)
      local.get 9
      global.set $__stack_pointer
      return
    end
    unreachable
  )
  (func $intrinsics::felt::eq (;2;) (type 2) (param f32 f32) (result i32)
    unreachable
  )
  (func $intrinsics::felt::from_u64_unchecked (;3;) (type 3) (param i64) (result f32)
    unreachable
  )
  (func $miden::core::collections::smt::get (;4;) (type 4) (param f32 f32 f32 f32 f32 f32 f32 f32 i32)
    unreachable
  )
)
