(module $onchain_two_felts_struct.wasm
  (type (;0;) (func (param i32 f32)))
  (type (;1;) (func (param i32)))
  (type (;2;) (func (param i32 i32)))
  (type (;3;) (func (param i32 i32) (result i32)))
  (type (;4;) (func (param i32 i32 i32 i32) (result i32)))
  (type (;5;) (func))
  (type (;6;) (func (param i32 i32 i32) (result i32)))
  (type (;7;) (func (result i32)))
  (type (;8;) (func (param i32 i32 i32 i32 i32 i32)))
  (type (;9;) (func (param i32 i32 i32 i32 i32)))
  (type (;10;) (func (param i32 i32 i32)))
  (type (;11;) (func (param i32 i32 i32 i32)))
  (table (;0;) 1 1 funcref)
  (memory (;0;) 17)
  (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
  (export "memory" (memory 0))
  (export "entrypoint" (func $entrypoint))
  (func $alloc::vec::Vec<T,A>::push (;0;) (type 0) (param i32 f32)
    (local i32)
    block ;; label = @1
      local.get 0
      i32.load offset=8
      local.tee 2
      local.get 0
      i32.load
      i32.ne
      br_if 0 (;@1;)
      local.get 0
      call $alloc::raw_vec::RawVec<T,A>::grow_one
    end
    local.get 0
    i32.load offset=4
    local.get 2
    i32.const 2
    i32.shl
    i32.add
    local.get 1
    f32.store
    local.get 0
    local.get 2
    i32.const 1
    i32.add
    i32.store offset=8
  )
  (func $alloc::raw_vec::RawVec<T,A>::grow_one (;1;) (type 1) (param i32)
    (local i32)
    global.get $__stack_pointer
    i32.const 16
    i32.sub
    local.tee 1
    global.set $__stack_pointer
    local.get 1
    i32.const 8
    i32.add
    local.get 0
    local.get 0
    i32.load
    i32.const 1
    i32.const 4
    i32.const 4
    call $alloc::raw_vec::RawVecInner<A>::grow_amortized
    block ;; label = @1
      local.get 1
      i32.load offset=8
      local.tee 0
      i32.const -2147483647
      i32.eq
      br_if 0 (;@1;)
      local.get 0
      local.get 1
      i32.load offset=12
      i32.const 1048588
      call $alloc::raw_vec::handle_error
      unreachable
    end
    local.get 1
    i32.const 16
    i32.add
    global.set $__stack_pointer
  )
  (func $entrypoint (;2;) (type 2) (param i32 i32)
    (local i32 f32 f32 i32)
    global.get $__stack_pointer
    i32.const 32
    i32.sub
    local.tee 2
    global.set $__stack_pointer
    local.get 1
    f32.load offset=4
    local.set 3
    local.get 1
    f32.load
    local.set 4
    local.get 2
    i32.const 20
    i32.add
    i32.const 256
    i32.const 0
    i32.const 4
    i32.const 4
    call $alloc::raw_vec::RawVecInner<A>::try_allocate_in
    local.get 2
    i32.load offset=24
    local.set 1
    block ;; label = @1
      local.get 2
      i32.load offset=20
      i32.const 1
      i32.ne
      br_if 0 (;@1;)
      local.get 1
      local.get 2
      i32.load offset=28
      i32.const 1048588
      call $alloc::raw_vec::handle_error
      unreachable
    end
    local.get 2
    i32.const 8
    i32.add
    i32.const 8
    i32.add
    local.tee 5
    i32.const 0
    i32.store
    local.get 2
    local.get 2
    i32.load offset=28
    i32.store offset=12
    local.get 2
    local.get 1
    i32.store offset=8
    local.get 2
    i32.const 8
    i32.add
    local.get 4
    call $alloc::vec::Vec<T,A>::push
    local.get 2
    i32.const 8
    i32.add
    local.get 3
    call $alloc::vec::Vec<T,A>::push
    local.get 0
    i32.const 8
    i32.add
    local.get 5
    i32.load
    i32.store
    local.get 0
    local.get 2
    i64.load offset=8 align=4
    i64.store align=4
    local.get 2
    i32.const 32
    i32.add
    global.set $__stack_pointer
  )
  (func $__rustc::__rust_alloc (;3;) (type 3) (param i32 i32) (result i32)
    i32.const 1048604
    local.get 1
    local.get 0
    call $<miden_sdk_alloc::BumpAlloc as core::alloc::global::GlobalAlloc>::alloc
  )
  (func $__rustc::__rust_realloc (;4;) (type 4) (param i32 i32 i32 i32) (result i32)
    block ;; label = @1
      i32.const 1048604
      local.get 2
      local.get 3
      call $<miden_sdk_alloc::BumpAlloc as core::alloc::global::GlobalAlloc>::alloc
      local.tee 2
      i32.eqz
      br_if 0 (;@1;)
      local.get 3
      local.get 1
      local.get 3
      local.get 1
      i32.lt_u
      select
      local.tee 3
      i32.eqz
      br_if 0 (;@1;)
      local.get 2
      local.get 0
      local.get 3
      memory.copy
    end
    local.get 2
  )
  (func $__rustc::__rust_alloc_zeroed (;5;) (type 3) (param i32 i32) (result i32)
    block ;; label = @1
      i32.const 1048604
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
  (func $__rustc::__rust_no_alloc_shim_is_unstable_v2 (;6;) (type 5)
    return
  )
  (func $<miden_sdk_alloc::BumpAlloc as core::alloc::global::GlobalAlloc>::alloc (;7;) (type 6) (param i32 i32 i32) (result i32)
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
      call $core::ptr::alignment::Alignment::max
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
  (func $intrinsics::mem::heap_base (;8;) (type 7) (result i32)
    unreachable
  )
  (func $alloc::raw_vec::RawVecInner<A>::grow_amortized (;9;) (type 8) (param i32 i32 i32 i32 i32 i32)
    (local i32 i32 i32 i64)
    global.get $__stack_pointer
    i32.const 32
    i32.sub
    local.tee 6
    global.set $__stack_pointer
    i32.const 0
    local.set 7
    block ;; label = @1
      block ;; label = @2
        local.get 5
        i32.eqz
        br_if 0 (;@2;)
        local.get 2
        local.get 3
        i32.add
        local.tee 3
        local.get 2
        i32.lt_u
        br_if 1 (;@1;)
        i32.const 0
        local.set 7
        local.get 4
        local.get 5
        i32.add
        i32.const -1
        i32.add
        i32.const 0
        local.get 4
        i32.sub
        i32.and
        i64.extend_i32_u
        local.get 3
        local.get 1
        i32.load
        i32.const 1
        i32.shl
        local.tee 8
        local.get 3
        local.get 8
        i32.gt_u
        select
        local.tee 8
        i32.const 8
        i32.const 4
        i32.const 1
        local.get 5
        i32.const 1025
        i32.lt_u
        select
        local.get 5
        i32.const 1
        i32.eq
        select
        local.tee 2
        local.get 8
        local.get 2
        i32.gt_u
        select
        local.tee 2
        i64.extend_i32_u
        i64.mul
        local.tee 9
        i64.const 32
        i64.shr_u
        i32.wrap_i64
        br_if 0 (;@2;)
        local.get 9
        i32.wrap_i64
        local.tee 3
        i32.const -2147483648
        local.get 4
        i32.sub
        i32.gt_u
        br_if 1 (;@1;)
        local.get 6
        i32.const 20
        i32.add
        local.get 1
        local.get 4
        local.get 5
        call $alloc::raw_vec::RawVecInner<A>::current_memory
        local.get 6
        i32.const 8
        i32.add
        local.get 4
        local.get 3
        local.get 6
        i32.const 20
        i32.add
        local.get 0
        call $alloc::raw_vec::finish_grow
        local.get 6
        i32.load offset=12
        local.set 7
        block ;; label = @3
          local.get 6
          i32.load offset=8
          i32.eqz
          br_if 0 (;@3;)
          local.get 6
          i32.load offset=16
          local.set 8
          br 2 (;@1;)
        end
        local.get 1
        local.get 2
        i32.store
        local.get 1
        local.get 7
        i32.store offset=4
        i32.const -2147483647
        local.set 7
        br 1 (;@1;)
      end
    end
    local.get 0
    local.get 8
    i32.store offset=4
    local.get 0
    local.get 7
    i32.store
    local.get 6
    i32.const 32
    i32.add
    global.set $__stack_pointer
  )
  (func $alloc::raw_vec::RawVecInner<A>::try_allocate_in (;10;) (type 9) (param i32 i32 i32 i32 i32)
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
        call $alloc::alloc::Global::alloc_impl
        local.get 5
        i32.load
        local.set 2
      end
      block ;; label = @2
        local.get 2
        i32.eqz
        br_if 0 (;@2;)
        local.get 0
        local.get 2
        i32.store offset=8
        local.get 0
        local.get 1
        i32.store offset=4
        i32.const 0
        local.set 3
        br 1 (;@1;)
      end
      local.get 0
      local.get 4
      i32.store offset=8
      local.get 0
      local.get 3
      i32.store offset=4
      i32.const 1
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
  (func $<alloc::alloc::Global as core::alloc::Allocator>::allocate (;11;) (type 10) (param i32 i32 i32)
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
    call $alloc::alloc::Global::alloc_impl
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
  (func $alloc::alloc::Global::alloc_impl (;12;) (type 11) (param i32 i32 i32 i32)
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
  (func $alloc::raw_vec::RawVecInner<A>::current_memory (;13;) (type 11) (param i32 i32 i32 i32)
    (local i32 i32 i32)
    i32.const 0
    local.set 4
    i32.const 4
    local.set 5
    block ;; label = @1
      local.get 3
      i32.eqz
      br_if 0 (;@1;)
      local.get 1
      i32.load
      local.tee 6
      i32.eqz
      br_if 0 (;@1;)
      local.get 0
      local.get 2
      i32.store offset=4
      local.get 0
      local.get 1
      i32.load offset=4
      i32.store
      local.get 6
      local.get 3
      i32.mul
      local.set 4
      i32.const 8
      local.set 5
    end
    local.get 0
    local.get 5
    i32.add
    local.get 4
    i32.store
  )
  (func $alloc::raw_vec::finish_grow (;14;) (type 9) (param i32 i32 i32 i32 i32)
    (local i32 i32)
    global.get $__stack_pointer
    i32.const 16
    i32.sub
    local.tee 5
    global.set $__stack_pointer
    i32.const 0
    local.set 6
    block ;; label = @1
      block ;; label = @2
        local.get 2
        i32.const 0
        i32.ge_s
        br_if 0 (;@2;)
        i32.const 1
        local.set 2
        i32.const 4
        local.set 3
        br 1 (;@1;)
      end
      block ;; label = @2
        block ;; label = @3
          local.get 3
          i32.load offset=4
          i32.eqz
          br_if 0 (;@3;)
          block ;; label = @4
            local.get 3
            i32.load offset=8
            local.tee 6
            br_if 0 (;@4;)
            local.get 5
            i32.const 8
            i32.add
            local.get 1
            local.get 2
            i32.const 0
            call $alloc::alloc::Global::alloc_impl
            local.get 5
            i32.load offset=12
            local.set 6
            local.get 5
            i32.load offset=8
            local.set 3
            br 2 (;@2;)
          end
          local.get 3
          i32.load
          local.get 6
          local.get 1
          local.get 2
          call $__rustc::__rust_realloc
          local.set 3
          local.get 2
          local.set 6
          br 1 (;@2;)
        end
        local.get 5
        local.get 1
        local.get 2
        call $<alloc::alloc::Global as core::alloc::Allocator>::allocate
        local.get 5
        i32.load offset=4
        local.set 6
        local.get 5
        i32.load
        local.set 3
      end
      local.get 0
      local.get 3
      local.get 1
      local.get 3
      select
      i32.store offset=4
      local.get 6
      local.get 2
      local.get 3
      select
      local.set 6
      local.get 3
      i32.eqz
      local.set 2
      i32.const 8
      local.set 3
    end
    local.get 0
    local.get 3
    i32.add
    local.get 6
    i32.store
    local.get 0
    local.get 2
    i32.store
    local.get 5
    i32.const 16
    i32.add
    global.set $__stack_pointer
  )
  (func $alloc::raw_vec::handle_error (;15;) (type 10) (param i32 i32 i32)
    unreachable
  )
  (func $core::ptr::alignment::Alignment::max (;16;) (type 3) (param i32 i32) (result i32)
    local.get 0
    local.get 1
    local.get 0
    local.get 1
    i32.gt_u
    select
  )
  (data $.rodata (;0;) (i32.const 1048576) "<redacted>\00\00\00\00\10\00\0a\00\00\00\00\00\00\00\00\00\00\00")
)
