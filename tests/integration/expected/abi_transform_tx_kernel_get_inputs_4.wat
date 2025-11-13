(module $abi_transform_tx_kernel_get_inputs_4.wasm
  (type (;0;) (func))
  (type (;1;) (func (param i32 i32) (result i32)))
  (type (;2;) (func (param i32 i32 i32)))
  (type (;3;) (func (param i32 i32 i32 i32) (result i32)))
  (type (;4;) (func (param i32 i32 i32) (result i32)))
  (type (;5;) (func (result i32)))
  (type (;6;) (func (param i32 i32 i32 i32)))
  (type (;7;) (func (param i32)))
  (type (;8;) (func (param i32) (result f32)))
  (type (;9;) (func (param f32 f32)))
  (type (;10;) (func (param i32 i32 i32 i32 i32)))
  (type (;11;) (func (param i32) (result i32)))
  (table (;0;) 2 2 funcref)
  (memory (;0;) 17)
  (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
  (export "memory" (memory 0))
  (export "entrypoint" (func $entrypoint))
  (export "cabi_realloc_wit_bindgen_0_46_0" (func $cabi_realloc_wit_bindgen_0_46_0))
  (export "cabi_realloc" (func $cabi_realloc))
  (elem (;0;) (i32.const 1) func $cabi_realloc)
  (func $entrypoint (;0;) (type 0)
    (local i32 i32 i32)
    global.get $__stack_pointer
    i32.const 16
    i32.sub
    local.tee 0
    global.set $__stack_pointer
    local.get 0
    i32.const 4
    i32.add
    call $miden_base_sys::bindings::active_note::get_inputs
    local.get 0
    i32.load offset=12
    local.tee 1
    call $<miden_stdlib_sys::intrinsics::felt::Felt as core::convert::From<u32>>::from
    i32.const 4
    call $intrinsics::felt::from_u32
    call $intrinsics::felt::assert_eq
    block ;; label = @1
      local.get 1
      i32.eqz
      br_if 0 (;@1;)
      local.get 0
      i32.load offset=8
      local.tee 2
      f32.load
      i32.const -1
      call $intrinsics::felt::from_u32
      call $intrinsics::felt::assert_eq
      local.get 1
      i32.const 1
      i32.eq
      br_if 0 (;@1;)
      local.get 2
      f32.load offset=4
      i32.const 1
      call $intrinsics::felt::from_u32
      call $intrinsics::felt::assert_eq
      local.get 1
      i32.const 2
      i32.le_u
      br_if 0 (;@1;)
      local.get 2
      f32.load offset=8
      i32.const 2
      call $intrinsics::felt::from_u32
      call $intrinsics::felt::assert_eq
      local.get 1
      i32.const 3
      i32.eq
      br_if 0 (;@1;)
      local.get 2
      f32.load offset=12
      i32.const 3
      call $intrinsics::felt::from_u32
      call $intrinsics::felt::assert_eq
      local.get 0
      i32.const 4
      i32.add
      i32.const 4
      i32.const 4
      call $alloc::raw_vec::RawVecInner<A>::deallocate
      local.get 0
      i32.const 16
      i32.add
      global.set $__stack_pointer
      return
    end
    unreachable
  )
  (func $__rustc::__rust_alloc (;1;) (type 1) (param i32 i32) (result i32)
    i32.const 1048648
    local.get 1
    local.get 0
    call $<miden_sdk_alloc::BumpAlloc as core::alloc::global::GlobalAlloc>::alloc
  )
  (func $__rustc::__rust_dealloc (;2;) (type 2) (param i32 i32 i32))
  (func $__rustc::__rust_realloc (;3;) (type 3) (param i32 i32 i32 i32) (result i32)
    block ;; label = @1
      i32.const 1048648
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
  (func $__rustc::__rust_alloc_zeroed (;4;) (type 1) (param i32 i32) (result i32)
    block ;; label = @1
      i32.const 1048648
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
  (func $__rustc::__rust_no_alloc_shim_is_unstable_v2 (;5;) (type 0)
    return
  )
  (func $<miden_sdk_alloc::BumpAlloc as core::alloc::global::GlobalAlloc>::alloc (;6;) (type 4) (param i32 i32 i32) (result i32)
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
  (func $intrinsics::mem::heap_base (;7;) (type 5) (result i32)
    unreachable
  )
  (func $alloc::raw_vec::RawVecInner<A>::with_capacity_in (;8;) (type 6) (param i32 i32 i32 i32)
    (local i32)
    global.get $__stack_pointer
    i32.const 16
    i32.sub
    local.tee 4
    global.set $__stack_pointer
    local.get 4
    i32.const 4
    i32.add
    i32.const 256
    i32.const 0
    local.get 1
    local.get 2
    call $alloc::raw_vec::RawVecInner<A>::try_allocate_in
    local.get 4
    i32.load offset=8
    local.set 2
    block ;; label = @1
      local.get 4
      i32.load offset=4
      i32.const 1
      i32.ne
      br_if 0 (;@1;)
      local.get 2
      local.get 4
      i32.load offset=12
      local.get 3
      call $alloc::raw_vec::handle_error
      unreachable
    end
    local.get 0
    local.get 4
    i32.load offset=12
    i32.store offset=4
    local.get 0
    local.get 2
    i32.store
    local.get 4
    i32.const 16
    i32.add
    global.set $__stack_pointer
  )
  (func $miden_base_sys::bindings::active_note::get_inputs (;9;) (type 7) (param i32)
    (local i32 i32 i32)
    global.get $__stack_pointer
    i32.const 16
    i32.sub
    local.tee 1
    global.set $__stack_pointer
    local.get 1
    i32.const 8
    i32.add
    i32.const 4
    i32.const 4
    i32.const 1048628
    call $alloc::raw_vec::RawVecInner<A>::with_capacity_in
    local.get 1
    i32.load offset=8
    local.set 2
    local.get 0
    local.get 1
    i32.load offset=12
    local.tee 3
    i32.const 2
    i32.shr_u
    call $miden::active_note::get_inputs
    i32.store offset=8
    local.get 0
    local.get 3
    i32.store offset=4
    local.get 0
    local.get 2
    i32.store
    local.get 1
    i32.const 16
    i32.add
    global.set $__stack_pointer
  )
  (func $<miden_stdlib_sys::intrinsics::felt::Felt as core::convert::From<u32>>::from (;10;) (type 8) (param i32) (result f32)
    local.get 0
    f32.reinterpret_i32
  )
  (func $intrinsics::felt::from_u32 (;11;) (type 8) (param i32) (result f32)
    unreachable
  )
  (func $intrinsics::felt::assert_eq (;12;) (type 9) (param f32 f32)
    unreachable
  )
  (func $alloc::raw_vec::RawVecInner<A>::deallocate (;13;) (type 2) (param i32 i32 i32)
    (local i32)
    global.get $__stack_pointer
    i32.const 16
    i32.sub
    local.tee 3
    global.set $__stack_pointer
    local.get 3
    i32.const 4
    i32.add
    local.get 0
    local.get 1
    local.get 2
    call $alloc::raw_vec::RawVecInner<A>::current_memory
    block ;; label = @1
      local.get 3
      i32.load offset=8
      local.tee 2
      i32.eqz
      br_if 0 (;@1;)
      local.get 3
      i32.load offset=4
      local.get 2
      local.get 3
      i32.load offset=12
      call $<alloc::alloc::Global as core::alloc::Allocator>::deallocate
    end
    local.get 3
    i32.const 16
    i32.add
    global.set $__stack_pointer
  )
  (func $alloc::raw_vec::RawVecInner<A>::try_allocate_in (;14;) (type 10) (param i32 i32 i32 i32 i32)
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
  (func $<alloc::alloc::Global as core::alloc::Allocator>::allocate (;15;) (type 2) (param i32 i32 i32)
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
  (func $alloc::alloc::Global::alloc_impl (;16;) (type 6) (param i32 i32 i32 i32)
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
  (func $alloc::raw_vec::RawVecInner<A>::current_memory (;17;) (type 6) (param i32 i32 i32 i32)
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
  (func $<alloc::alloc::Global as core::alloc::Allocator>::deallocate (;18;) (type 2) (param i32 i32 i32)
    block ;; label = @1
      local.get 2
      i32.eqz
      br_if 0 (;@1;)
      local.get 0
      local.get 2
      local.get 1
      call $__rustc::__rust_dealloc
    end
  )
  (func $alloc::raw_vec::handle_error (;19;) (type 2) (param i32 i32 i32)
    unreachable
  )
  (func $core::ptr::alignment::Alignment::max (;20;) (type 1) (param i32 i32) (result i32)
    local.get 0
    local.get 1
    local.get 0
    local.get 1
    i32.gt_u
    select
  )
  (func $miden::active_note::get_inputs (;21;) (type 11) (param i32) (result i32)
    unreachable
  )
  (func $cabi_realloc (;22;) (type 3) (param i32 i32 i32 i32) (result i32)
    local.get 0
    local.get 1
    local.get 2
    local.get 3
    call $cabi_realloc_wit_bindgen_0_46_0
  )
  (func $alloc::alloc::alloc (;23;) (type 1) (param i32 i32) (result i32)
    call $__rustc::__rust_no_alloc_shim_is_unstable_v2
    local.get 1
    local.get 0
    call $__rustc::__rust_alloc
  )
  (func $cabi_realloc_wit_bindgen_0_46_0 (;24;) (type 3) (param i32 i32 i32 i32) (result i32)
    local.get 0
    local.get 1
    local.get 2
    local.get 3
    call $wit_bindgen::rt::cabi_realloc
  )
  (func $wit_bindgen::rt::cabi_realloc (;25;) (type 3) (param i32 i32 i32 i32) (result i32)
    block ;; label = @1
      block ;; label = @2
        block ;; label = @3
          local.get 1
          br_if 0 (;@3;)
          local.get 3
          i32.eqz
          br_if 2 (;@1;)
          local.get 2
          local.get 3
          call $alloc::alloc::alloc
          local.set 2
          br 1 (;@2;)
        end
        local.get 0
        local.get 1
        local.get 2
        local.get 3
        call $__rustc::__rust_realloc
        local.set 2
      end
      local.get 2
      br_if 0 (;@1;)
      unreachable
    end
    local.get 2
  )
  (data $.rodata (;0;) (i32.const 1048576) "miden-base-sys-0.8.0/src/bindings/active_note.rs\00\00\00\00\00\00\10\000\00\00\00\1f\00\00\00!\00\00\00\01\00\00\00")
)
