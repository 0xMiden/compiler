(module $rust_sdk_stdlib_smt_set.wasm
  (type (;0;) (func (param i32 i32) (result i32)))
  (type (;1;) (func (param f32 f32 f32 f32 f32 f32 f32 f32 f32 f32 f32 f32)))
  (type (;2;) (func (param f32 f32) (result i32)))
  (type (;3;) (func (param i64) (result f32)))
  (type (;4;) (func (param f32 f32 f32 f32 f32 f32 f32 f32 f32 f32 f32 f32 i32)))
  (type (;5;) (func (param i32 i32 i32) (result i32)))
  (table (;0;) 1 1 funcref)
  (memory (;0;) 17)
  (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
  (export "memory" (memory 0))
  (export "entrypoint" (func $entrypoint))
  (func $<miden_field::word::Word as core::cmp::PartialEq>::eq (;0;) (type 0) (param i32 i32) (result i32)
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
  (func $entrypoint (;1;) (type 1) (param f32 f32 f32 f32 f32 f32 f32 f32 f32 f32 f32 f32)
    (local i32 i32 i64 i64 i64)
    global.get $__stack_pointer
    local.tee 12
    local.set 13
    local.get 12
    i32.const 128
    i32.sub
    i32.const -32
    i32.and
    local.tee 12
    global.set $__stack_pointer
    local.get 12
    local.get 3
    f32.store offset=12
    local.get 12
    local.get 2
    f32.store offset=8
    local.get 12
    local.get 1
    f32.store offset=4
    local.get 12
    local.get 0
    f32.store
    local.get 12
    local.get 7
    f32.store offset=28
    local.get 12
    local.get 6
    f32.store offset=24
    local.get 12
    local.get 5
    f32.store offset=20
    local.get 12
    local.get 4
    f32.store offset=16
    local.get 12
    local.get 11
    f32.store offset=44
    local.get 12
    local.get 10
    f32.store offset=40
    local.get 12
    local.get 9
    f32.store offset=36
    local.get 12
    local.get 8
    f32.store offset=32
    local.get 12
    i32.const 3
    i32.const 1048588
    call $<miden_field::word::Word as core::ops::index::Index<usize>>::index
    f32.load
    local.get 12
    i32.const 2
    i32.const 1048588
    call $<miden_field::word::Word as core::ops::index::Index<usize>>::index
    f32.load
    local.get 12
    i32.const 1
    i32.const 1048588
    call $<miden_field::word::Word as core::ops::index::Index<usize>>::index
    f32.load
    local.get 12
    i32.const 0
    i32.const 1048588
    call $<miden_field::word::Word as core::ops::index::Index<usize>>::index
    f32.load
    local.get 12
    i32.const 16
    i32.add
    i32.const 3
    i32.const 1048588
    call $<miden_field::word::Word as core::ops::index::Index<usize>>::index
    f32.load
    local.get 12
    i32.const 16
    i32.add
    i32.const 2
    i32.const 1048588
    call $<miden_field::word::Word as core::ops::index::Index<usize>>::index
    f32.load
    local.get 12
    i32.const 16
    i32.add
    i32.const 1
    i32.const 1048588
    call $<miden_field::word::Word as core::ops::index::Index<usize>>::index
    f32.load
    local.get 12
    i32.const 16
    i32.add
    i32.const 0
    i32.const 1048588
    call $<miden_field::word::Word as core::ops::index::Index<usize>>::index
    f32.load
    local.get 12
    i32.const 32
    i32.add
    i32.const 3
    i32.const 1048588
    call $<miden_field::word::Word as core::ops::index::Index<usize>>::index
    f32.load
    local.get 12
    i32.const 32
    i32.add
    i32.const 2
    i32.const 1048588
    call $<miden_field::word::Word as core::ops::index::Index<usize>>::index
    f32.load
    local.get 12
    i32.const 32
    i32.add
    i32.const 1
    i32.const 1048588
    call $<miden_field::word::Word as core::ops::index::Index<usize>>::index
    f32.load
    local.get 12
    i32.const 32
    i32.add
    i32.const 0
    i32.const 1048588
    call $<miden_field::word::Word as core::ops::index::Index<usize>>::index
    f32.load
    local.get 12
    i32.const 96
    i32.add
    call $miden::core::collections::smt::set
    local.get 12
    i64.load offset=120
    local.set 14
    local.get 12
    i64.load offset=112
    local.set 15
    local.get 12
    i64.load offset=104
    local.set 16
    local.get 12
    local.get 12
    i64.load offset=96
    i64.const 32
    i64.rotl
    i64.store offset=56
    local.get 12
    local.get 16
    i64.const 32
    i64.rotl
    i64.store offset=48
    local.get 12
    local.get 15
    i64.const 32
    i64.rotl
    i64.store offset=72
    local.get 12
    local.get 14
    i64.const 32
    i64.rotl
    i64.store offset=64
    i64.const 0
    call $intrinsics::felt::from_u64_unchecked
    local.set 3
    i64.const 0
    call $intrinsics::felt::from_u64_unchecked
    local.set 2
    i64.const 0
    call $intrinsics::felt::from_u64_unchecked
    local.set 1
    local.get 12
    i64.const 0
    call $intrinsics::felt::from_u64_unchecked
    f32.store offset=92
    local.get 12
    local.get 1
    f32.store offset=88
    local.get 12
    local.get 2
    f32.store offset=84
    local.get 12
    local.get 3
    f32.store offset=80
    i64.const -8162549007765693629
    call $intrinsics::felt::from_u64_unchecked
    local.set 3
    i64.const 2968936853898945160
    call $intrinsics::felt::from_u64_unchecked
    local.set 2
    i64.const 4556576161526575002
    call $intrinsics::felt::from_u64_unchecked
    local.set 1
    local.get 12
    i64.const 900237903234600995
    call $intrinsics::felt::from_u64_unchecked
    f32.store offset=108
    local.get 12
    local.get 1
    f32.store offset=104
    local.get 12
    local.get 2
    f32.store offset=100
    local.get 12
    local.get 3
    f32.store offset=96
    block ;; label = @1
      local.get 12
      i32.const 48
      i32.add
      local.get 12
      i32.const 80
      i32.add
      call $<miden_field::word::Word as core::cmp::PartialEq>::eq
      i32.eqz
      br_if 0 (;@1;)
      local.get 12
      i32.const 64
      i32.add
      local.get 12
      i32.const 96
      i32.add
      call $<miden_field::word::Word as core::cmp::PartialEq>::eq
      i32.eqz
      br_if 0 (;@1;)
      local.get 13
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
  (func $miden::core::collections::smt::set (;4;) (type 4) (param f32 f32 f32 f32 f32 f32 f32 f32 f32 f32 f32 f32 i32)
    unreachable
  )
  (func $<miden_field::word::Word as core::ops::index::Index<usize>>::index (;5;) (type 5) (param i32 i32 i32) (result i32)
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
  (data $.rodata (;0;) (i32.const 1048576) "<redacted>\00\00\00\00\10\00\0a\00\00\00\00\00\00\00\00\00\00\00")
)
