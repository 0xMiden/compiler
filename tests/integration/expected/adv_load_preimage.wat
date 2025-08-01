(module $adv_load_preimage.wasm
  (type (;0;) (func (param f32 f32 f32 f32) (result f32)))
  (type (;1;) (func (param f32) (result i64)))
  (type (;2;) (func (param i32) (result f32)))
  (type (;3;) (func (param f32 f32)))
  (type (;4;) (func (param i64) (result f32)))
  (type (;5;) (func (param f32 i32 f32 f32 f32 f32) (result i32)))
  (type (;6;) (func (result i32)))
  (type (;7;) (func (param i32 f32 f32 f32 f32)))
  (type (;8;) (func (param i32 i32) (result i32)))
  (type (;9;) (func))
  (type (;10;) (func (param i32 i32 i32) (result i32)))
  (type (;11;) (func (param i32 i32 i32 i32 i32)))
  (type (;12;) (func (param i32 i32 i32)))
  (type (;13;) (func (param i32 i32 i32 i32)))
  (import "miden:core-intrinsics/intrinsics-advice@1.0.0" "adv-push-mapvaln" (func $miden_stdlib_sys::intrinsics::advice::extern_adv_push_mapvaln (;0;) (type 0)))
  (import "miden:core-intrinsics/intrinsics-felt@1.0.0" "as-u64" (func $miden_stdlib_sys::intrinsics::felt::extern_as_u64 (;1;) (type 1)))
  (import "miden:core-intrinsics/intrinsics-felt@1.0.0" "from-u32" (func $miden_stdlib_sys::intrinsics::felt::extern_from_u32 (;2;) (type 2)))
  (import "miden:core-intrinsics/intrinsics-felt@1.0.0" "assert-eq" (func $miden_stdlib_sys::intrinsics::felt::extern_assert_eq (;3;) (type 3)))
  (import "miden:core-intrinsics/intrinsics-felt@1.0.0" "from-u64-unchecked" (func $miden_stdlib_sys::intrinsics::felt::extern_from_u64_unchecked (;4;) (type 4)))
  (import "miden:core-stdlib/stdlib-mem@1.0.0" "pipe-preimage-to-memory" (func $miden_stdlib_sys::stdlib::mem::extern_pipe_preimage_to_memory (;5;) (type 5)))
  (import "miden:core-intrinsics/intrinsics-mem@1.0.0" "heap-base" (func $miden_sdk_alloc::heap_base (;6;) (type 6)))
  (table (;0;) 1 1 funcref)
  (memory (;0;) 17)
  (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
  (export "memory" (memory 0))
  (export "entrypoint" (func $entrypoint))
  (func $entrypoint (;7;) (type 7) (param i32 f32 f32 f32 f32)
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
    call $miden_stdlib_sys::intrinsics::advice::extern_adv_push_mapvaln
    call $miden_stdlib_sys::intrinsics::felt::extern_as_u64
    local.tee 6
    i32.wrap_i64
    i32.const 3
    i32.and
    call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
    i32.const 0
    call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
    call $miden_stdlib_sys::intrinsics::felt::extern_assert_eq
    local.get 5
    i32.const 4
    i32.add
    local.get 6
    i64.const 2
    i64.shr_u
    call $miden_stdlib_sys::intrinsics::felt::extern_from_u64_unchecked
    local.tee 7
    call $miden_stdlib_sys::intrinsics::felt::extern_as_u64
    i32.wrap_i64
    i32.const 2
    i32.shl
    local.tee 8
    i32.const 0
    i32.const 4
    i32.const 4
    call $alloc::raw_vec::RawVecInner<A>::try_allocate_in
    local.get 5
    i32.load offset=8
    local.set 9
    block ;; label = @1
      local.get 5
      i32.load offset=4
      i32.const 1
      i32.ne
      br_if 0 (;@1;)
      local.get 9
      local.get 5
      i32.load offset=12
      i32.const 1048620
      call $alloc::raw_vec::handle_error
      unreachable
    end
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
    call $miden_stdlib_sys::stdlib::mem::extern_pipe_preimage_to_memory
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
    local.get 5
    i32.const 16
    i32.add
    global.set $__stack_pointer
  )
  (func $__rustc::__rust_alloc (;8;) (type 8) (param i32 i32) (result i32)
    i32.const 1048636
    local.get 1
    local.get 0
    call $<miden_sdk_alloc::BumpAlloc as core::alloc::global::GlobalAlloc>::alloc
  )
  (func $__rustc::__rust_alloc_zeroed (;9;) (type 8) (param i32 i32) (result i32)
    block ;; label = @1
      i32.const 1048636
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
  (func $__rustc::__rust_no_alloc_shim_is_unstable_v2 (;10;) (type 9)
    return
  )
  (func $<miden_sdk_alloc::BumpAlloc as core::alloc::global::GlobalAlloc>::alloc (;11;) (type 10) (param i32 i32 i32) (result i32)
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
        call $miden_sdk_alloc::heap_base
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
  (func $alloc::raw_vec::RawVecInner<A>::try_allocate_in (;12;) (type 11) (param i32 i32 i32 i32 i32)
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
  (func $alloc::alloc::Global::alloc_impl (;14;) (type 13) (param i32 i32 i32 i32)
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
  (func $alloc::raw_vec::handle_error (;15;) (type 12) (param i32 i32 i32)
    unreachable
  )
  (func $core::ptr::alignment::Alignment::max (;16;) (type 8) (param i32 i32) (result i32)
    local.get 0
    local.get 1
    local.get 0
    local.get 1
    i32.gt_u
    select
  )
  (data $.rodata (;0;) (i32.const 1048576) "miden-stdlib-sys-0.1.5/src/stdlib/mem.rs\00\00\00\00\00\00\10\00(\00\00\00\98\00\00\00!\00\00\00")
)
