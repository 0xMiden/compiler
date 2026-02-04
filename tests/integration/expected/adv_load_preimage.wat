(module $adv_load_preimage.wasm
  (type (;0;) (func (param i32 i32) (result i32)))
  (type (;1;) (func (param i32 f32 f32 f32 f32)))
  (type (;2;) (func))
  (type (;3;) (func (param i32 i32 i32) (result i32)))
  (type (;4;) (func (result i32)))
  (type (;5;) (func (param f32 f32 f32 f32) (result f32)))
  (type (;6;) (func (param f32) (result i64)))
  (type (;7;) (func (param f32 f32)))
  (type (;8;) (func (param i32) (result f32)))
  (type (;9;) (func (param i64) (result f32)))
  (type (;10;) (func (param f32 i32 f32 f32 f32 f32) (result i32)))
  (type (;11;) (func (param i32 i32 i32 i32)))
  (type (;12;) (func (param i32 i32 i32)))
  (type (;13;) (func (param i32 i32 i32 i32 i32)))
  (type (;14;) (func (param i32 i32)))
  (table (;0;) 1 1 funcref)
  (memory (;0;) 17)
  (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
  (export "memory" (memory 0))
  (export "entrypoint" (func $entrypoint))
  (func $__rustc::__rust_alloc (;0;) (type 0) (param i32 i32) (result i32)
    i32.const 1048576
    local.get 1
    local.get 0
    call $<miden_sdk_alloc::BumpAlloc as core::alloc::global::GlobalAlloc>::alloc
  )
  (func $__rustc::__rust_alloc_zeroed (;1;) (type 0) (param i32 i32) (result i32)
    block ;; label = @1
      i32.const 1048576
      local.get 1
      local.get 0
      call $<miden_sdk_alloc::BumpAlloc as core::alloc::global::GlobalAlloc>::alloc
      local.tee 1
      i32.eqz
      br_if 0 (;@1;)
      local.get 0
      i32.eqz
      br_if 0 (;@1;)
      local.get 1
      i32.const 0
      local.get 0
      memory.fill
    end
    local.get 1
  )
  (func $entrypoint (;2;) (type 1) (param i32 f32 f32 f32 f32)
    (local i32 i64 f32 i32 i32 i32)
    global.get $__stack_pointer
    i32.const 16
    i32.sub
    local.tee 5
    global.set $__stack_pointer
    local.get 4
    local.get 3
    local.get 2
    local.get 1
    call $intrinsics::advice::adv_push_mapvaln
    call $intrinsics::felt::as_u64
    local.tee 6
    i32.wrap_i64
    i32.const 3
    i32.and
    call $intrinsics::felt::from_u32
    i32.const 0
    call $intrinsics::felt::from_u32
    call $intrinsics::felt::assert_eq
    local.get 5
    i32.const 4
    i32.add
    local.get 6
    i64.const 2
    i64.shr_u
    call $intrinsics::felt::from_u64_unchecked
    local.tee 7
    call $intrinsics::felt::as_u64
    i32.wrap_i64
    i32.const 2
    i32.shl
    local.tee 8
    i32.const 0
    i32.const 4
    i32.const 4
    call $<alloc::raw_vec::RawVecInner>::try_allocate_in
    local.get 5
    i32.load offset=8
    local.set 9
    block ;; label = @1
      block ;; label = @2
        local.get 5
        i32.load offset=4
        i32.const 1
        i32.eq
        br_if 0 (;@2;)
        local.get 7
        local.get 5
        i32.load offset=12
        local.tee 10
        i32.const 2
        i32.shr_u
        local.get 4
        local.get 3
        local.get 2
        local.get 1
        call $miden::core::mem::pipe_preimage_to_memory
        drop
        local.get 0
        local.get 8
        i32.store offset=8
        local.get 0
        local.get 10
        i32.store offset=4
        local.get 0
        local.get 9
        i32.store
        local.get 8
        i32.eqz
        br_if 1 (;@1;)
        local.get 10
        f32.load
        i32.const 1
        call $intrinsics::felt::from_u32
        call $intrinsics::felt::assert_eq
        local.get 10
        f32.load offset=4
        i32.const 2
        call $intrinsics::felt::from_u32
        call $intrinsics::felt::assert_eq
        local.get 8
        i32.const 5
        i32.le_u
        br_if 1 (;@1;)
        local.get 10
        f32.load offset=20
        i32.const 6
        call $intrinsics::felt::from_u32
        call $intrinsics::felt::assert_eq
        local.get 8
        i32.const 14
        i32.le_u
        br_if 1 (;@1;)
        local.get 10
        f32.load offset=56
        i32.const 15
        call $intrinsics::felt::from_u32
        call $intrinsics::felt::assert_eq
        local.get 5
        i32.const 16
        i32.add
        global.set $__stack_pointer
        return
      end
      local.get 9
      local.get 5
      i32.load offset=12
      call $alloc::raw_vec::handle_error
    end
    unreachable
  )
  (func $__rustc::__rust_no_alloc_shim_is_unstable_v2 (;3;) (type 2)
    return
  )
  (func $<miden_sdk_alloc::BumpAlloc as core::alloc::global::GlobalAlloc>::alloc (;4;) (type 3) (param i32 i32 i32) (result i32)
    (local i32 i32)
    block ;; label = @1
      local.get 1
      i32.const 16
      local.get 1
      i32.const 16
      i32.gt_u
      select
      local.tee 3
      local.get 3
      i32.const -1
      i32.add
      i32.and
      br_if 0 (;@1;)
      local.get 2
      i32.const -2147483648
      local.get 1
      local.get 3
      call $<core::ptr::alignment::Alignment>::max
      local.tee 1
      i32.sub
      i32.gt_u
      br_if 0 (;@1;)
      i32.const 0
      local.set 3
      local.get 2
      local.get 1
      i32.add
      i32.const -1
      i32.add
      i32.const 0
      local.get 1
      i32.sub
      i32.and
      local.set 2
      block ;; label = @2
        local.get 0
        i32.load
        br_if 0 (;@2;)
        local.get 0
        call $intrinsics::mem::heap_base
        memory.size
        i32.const 16
        i32.shl
        i32.add
        i32.store
      end
      block ;; label = @2
        local.get 2
        local.get 0
        i32.load
        local.tee 4
        i32.const -1
        i32.xor
        i32.gt_u
        br_if 0 (;@2;)
        local.get 0
        local.get 4
        local.get 2
        i32.add
        i32.store
        local.get 4
        local.get 1
        i32.add
        local.set 3
      end
      local.get 3
      return
    end
    unreachable
  )
  (func $intrinsics::mem::heap_base (;5;) (type 4) (result i32)
    unreachable
  )
  (func $intrinsics::advice::adv_push_mapvaln (;6;) (type 5) (param f32 f32 f32 f32) (result f32)
    unreachable
  )
  (func $intrinsics::felt::as_u64 (;7;) (type 6) (param f32) (result i64)
    unreachable
  )
  (func $intrinsics::felt::assert_eq (;8;) (type 7) (param f32 f32)
    unreachable
  )
  (func $intrinsics::felt::from_u32 (;9;) (type 8) (param i32) (result f32)
    unreachable
  )
  (func $intrinsics::felt::from_u64_unchecked (;10;) (type 9) (param i64) (result f32)
    unreachable
  )
  (func $miden::core::mem::pipe_preimage_to_memory (;11;) (type 10) (param f32 i32 f32 f32 f32 f32) (result i32)
    unreachable
  )
  (func $<alloc::alloc::Global>::alloc_impl (;12;) (type 11) (param i32 i32 i32 i32)
    block ;; label = @1
      local.get 2
      i32.eqz
      br_if 0 (;@1;)
      call $__rustc::__rust_no_alloc_shim_is_unstable_v2
      block ;; label = @2
        local.get 3
        br_if 0 (;@2;)
        local.get 2
        local.get 1
        call $__rustc::__rust_alloc
        local.set 1
        br 1 (;@1;)
      end
      local.get 2
      local.get 1
      call $__rustc::__rust_alloc_zeroed
      local.set 1
    end
    local.get 0
    local.get 2
    i32.store offset=4
    local.get 0
    local.get 1
    i32.store
  )
  (func $<alloc::alloc::Global as core::alloc::Allocator>::allocate (;13;) (type 12) (param i32 i32 i32)
    (local i32)
    global.get $__stack_pointer
    i32.const 16
    i32.sub
    local.tee 3
    global.set $__stack_pointer
    local.get 3
    i32.const 8
    i32.add
    local.get 1
    local.get 2
    i32.const 0
    call $<alloc::alloc::Global>::alloc_impl
    local.get 3
    i32.load offset=12
    local.set 2
    local.get 0
    local.get 3
    i32.load offset=8
    i32.store
    local.get 0
    local.get 2
    i32.store offset=4
    local.get 3
    i32.const 16
    i32.add
    global.set $__stack_pointer
  )
  (func $<alloc::raw_vec::RawVecInner>::try_allocate_in (;14;) (type 13) (param i32 i32 i32 i32 i32)
    (local i32 i64)
    global.get $__stack_pointer
    i32.const 16
    i32.sub
    local.tee 5
    global.set $__stack_pointer
    block ;; label = @1
      block ;; label = @2
        block ;; label = @3
          local.get 3
          local.get 4
          i32.add
          i32.const -1
          i32.add
          i32.const 0
          local.get 3
          i32.sub
          i32.and
          i64.extend_i32_u
          local.get 1
          i64.extend_i32_u
          i64.mul
          local.tee 6
          i64.const 32
          i64.shr_u
          i32.wrap_i64
          br_if 0 (;@3;)
          local.get 6
          i32.wrap_i64
          local.tee 4
          i32.const -2147483648
          local.get 3
          i32.sub
          i32.le_u
          br_if 1 (;@2;)
        end
        local.get 0
        i32.const 0
        i32.store offset=4
        i32.const 1
        local.set 3
        br 1 (;@1;)
      end
      block ;; label = @2
        local.get 4
        br_if 0 (;@2;)
        local.get 0
        local.get 3
        i32.store offset=8
        i32.const 0
        local.set 3
        local.get 0
        i32.const 0
        i32.store offset=4
        br 1 (;@1;)
      end
      block ;; label = @2
        block ;; label = @3
          local.get 2
          br_if 0 (;@3;)
          local.get 5
          i32.const 8
          i32.add
          local.get 3
          local.get 4
          call $<alloc::alloc::Global as core::alloc::Allocator>::allocate
          local.get 5
          i32.load offset=8
          local.set 2
          br 1 (;@2;)
        end
        local.get 5
        local.get 3
        local.get 4
        i32.const 1
        call $<alloc::alloc::Global>::alloc_impl
        local.get 5
        i32.load
        local.set 2
      end
      block ;; label = @2
        local.get 2
        br_if 0 (;@2;)
        local.get 0
        local.get 4
        i32.store offset=8
        local.get 0
        local.get 3
        i32.store offset=4
        i32.const 1
        local.set 3
        br 1 (;@1;)
      end
      local.get 0
      local.get 2
      i32.store offset=8
      local.get 0
      local.get 1
      i32.store offset=4
      i32.const 0
      local.set 3
    end
    local.get 0
    local.get 3
    i32.store
    local.get 5
    i32.const 16
    i32.add
    global.set $__stack_pointer
  )
  (func $alloc::raw_vec::handle_error (;15;) (type 14) (param i32 i32)
    unreachable
  )
  (func $<core::ptr::alignment::Alignment>::max (;16;) (type 0) (param i32 i32) (result i32)
    local.get 0
    local.get 1
    local.get 0
    local.get 1
    i32.gt_u
    select
  )
)
