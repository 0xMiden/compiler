(component
  (type (;0;)
    (instance
      (type (;0;) (record (field "inner" f32)))
      (export (;1;) "felt" (type (eq 0)))
      (type (;2;) (tuple 1 1 1 1))
      (type (;3;) (record (field "inner" 2)))
      (export (;4;) "word" (type (eq 3)))
      (type (;5;) (record (field "inner" 4)))
      (export (;6;) "asset" (type (eq 5)))
    )
  )
  (import "miden:base/core-types@1.0.0" (instance (;0;) (type 0)))
  (alias export 0 "asset" (type (;1;)))
  (type (;2;)
    (instance
      (alias outer 1 1 (type (;0;)))
      (export (;1;) "asset" (type (eq 0)))
      (type (;2;) (func (param "asset" 1)))
      (export (;0;) "receive-asset" (func (type 2)))
    )
  )
  (import "miden:basic-wallet/basic-wallet@1.0.0" (instance (;1;) (type 2)))
  (type (;3;)
    (instance
      (type (;0;) (func (result s32)))
      (export (;0;) "heap-base" (func (type 0)))
    )
  )
  (import "miden:core-intrinsics/intrinsics-mem@1.0.0" (instance (;2;) (type 3)))
  (type (;4;)
    (instance
      (type (;0;) (func (param "a" f32) (param "b" f32)))
      (export (;0;) "assert-eq" (func (type 0)))
    )
  )
  (import "miden:core-intrinsics/intrinsics-felt@1.0.0" (instance (;3;) (type 4)))
  (type (;5;)
    (instance
      (type (;0;) (func (param "result-ptr" s32)))
      (export (;0;) "get-id" (func (type 0)))
    )
  )
  (import "miden:core-base/account@1.0.0" (instance (;4;) (type 5)))
  (type (;6;)
    (instance
      (type (;0;) (func (param "ptr" s32) (result s32)))
      (export (;0;) "get-inputs" (func (type 0)))
      (export (;1;) "get-assets" (func (type 0)))
    )
  )
  (import "miden:core-base/note@1.0.0" (instance (;5;) (type 6)))
  (core module (;0;)
    (type (;0;) (func (param f32 f32)))
    (type (;1;) (func (param f32 f32 f32 f32)))
    (type (;2;) (func (result i32)))
    (type (;3;) (func (param i32)))
    (type (;4;) (func (param i32) (result i32)))
    (type (;5;) (func))
    (type (;6;) (func (param i32 i32) (result i32)))
    (type (;7;) (func (param i32 i32 i32)))
    (type (;8;) (func (param i32 i32 i32) (result i32)))
    (type (;9;) (func (param i32 i32 i32 i32)))
    (type (;10;) (func (param i32 i32 i32 i32 i32)))
    (import "miden:core-intrinsics/intrinsics-felt@1.0.0" "assert-eq" (func $miden_stdlib_sys::intrinsics::felt::extern_assert_eq (;0;) (type 0)))
    (import "miden:basic-wallet/basic-wallet@1.0.0" "receive-asset" (func $p2id::bindings::miden::basic_wallet::basic_wallet::receive_asset::wit_import7 (;1;) (type 1)))
    (import "miden:core-intrinsics/intrinsics-mem@1.0.0" "heap-base" (func $miden_sdk_alloc::heap_base (;2;) (type 2)))
    (import "miden:core-base/account@1.0.0" "get-id" (func $miden_base_sys::bindings::account::extern_account_get_id (;3;) (type 3)))
    (import "miden:core-base/note@1.0.0" "get-inputs" (func $miden_base_sys::bindings::note::extern_note_get_inputs (;4;) (type 4)))
    (import "miden:core-base/note@1.0.0" "get-assets" (func $miden_base_sys::bindings::note::extern_note_get_assets (;5;) (type 4)))
    (table (;0;) 2 2 funcref)
    (memory (;0;) 17)
    (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
    (global $GOT.data.internal.__memory_base (;1;) i32 i32.const 0)
    (export "memory" (memory 0))
    (export "miden:base/script@1.0.0#script" (func $miden:base/script@1.0.0#script))
    (elem (;0;) (i32.const 1) func $p2id::bindings::__link_custom_section_describing_imports)
    (func $__wasm_call_ctors (;6;) (type 5))
    (func $p2id::bindings::__link_custom_section_describing_imports (;7;) (type 5))
    (func $__rustc::__rust_alloc (;8;) (type 6) (param i32 i32) (result i32)
      global.get $GOT.data.internal.__memory_base
      i32.const 1048700
      i32.add
      local.get 1
      local.get 0
      call $<miden_sdk_alloc::BumpAlloc as core::alloc::global::GlobalAlloc>::alloc
    )
    (func $__rustc::__rust_dealloc (;9;) (type 7) (param i32 i32 i32))
    (func $__rustc::__rust_alloc_zeroed (;10;) (type 6) (param i32 i32) (result i32)
      block ;; label = @1
        global.get $GOT.data.internal.__memory_base
        i32.const 1048700
        i32.add
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
    (func $miden:base/script@1.0.0#script (;11;) (type 5)
      (local i32 i32 f32 f32 f32 i32 i32 i32)
      global.get $__stack_pointer
      i32.const 48
      i32.sub
      local.tee 0
      global.set $__stack_pointer
      call $wit_bindgen_rt::run_ctors_once
      local.get 0
      i32.const 16
      i32.add
      call $miden_base_sys::bindings::note::get_inputs
      block ;; label = @1
        block ;; label = @2
          local.get 0
          i32.load offset=24
          br_table 0 (;@2;) 0 (;@2;) 1 (;@1;)
        end
        unreachable
      end
      local.get 0
      i32.load offset=20
      local.tee 1
      f32.load offset=4
      local.set 2
      local.get 1
      f32.load
      local.set 3
      local.get 0
      i32.const 8
      i32.add
      call $miden_base_sys::bindings::account::get_id
      local.get 0
      f32.load offset=12
      local.set 4
      local.get 0
      f32.load offset=8
      local.get 3
      call $miden_stdlib_sys::intrinsics::felt::extern_assert_eq
      local.get 4
      local.get 2
      call $miden_stdlib_sys::intrinsics::felt::extern_assert_eq
      local.get 0
      i32.const 28
      i32.add
      call $miden_base_sys::bindings::note::get_assets
      local.get 0
      i32.load offset=36
      i32.const 4
      i32.shl
      local.set 5
      local.get 0
      i32.load offset=28
      local.set 6
      local.get 0
      i32.load offset=32
      local.tee 7
      local.set 1
      block ;; label = @1
        loop ;; label = @2
          local.get 5
          i32.eqz
          br_if 1 (;@1;)
          local.get 1
          f32.load
          local.get 1
          f32.load offset=4
          local.get 1
          f32.load offset=8
          local.get 1
          f32.load offset=12
          call $p2id::bindings::miden::basic_wallet::basic_wallet::receive_asset::wit_import7
          local.get 5
          i32.const -16
          i32.add
          local.set 5
          local.get 1
          i32.const 16
          i32.add
          local.set 1
          br 0 (;@2;)
        end
      end
      local.get 0
      local.get 7
      i32.store offset=44
      local.get 0
      local.get 6
      i32.store offset=40
      local.get 0
      i32.const 40
      i32.add
      i32.const 16
      i32.const 16
      call $alloc::raw_vec::RawVecInner<A>::deallocate
      local.get 0
      i32.const 16
      i32.add
      i32.const 4
      i32.const 4
      call $alloc::raw_vec::RawVecInner<A>::deallocate
      local.get 0
      i32.const 48
      i32.add
      global.set $__stack_pointer
    )
    (func $__rustc::__rust_no_alloc_shim_is_unstable_v2 (;12;) (type 5)
      return
    )
    (func $wit_bindgen_rt::run_ctors_once (;13;) (type 5)
      (local i32)
      block ;; label = @1
        global.get $GOT.data.internal.__memory_base
        i32.const 1048704
        i32.add
        i32.load8_u
        br_if 0 (;@1;)
        global.get $GOT.data.internal.__memory_base
        local.set 0
        call $__wasm_call_ctors
        local.get 0
        i32.const 1048704
        i32.add
        i32.const 1
        i32.store8
      end
    )
    (func $<miden_sdk_alloc::BumpAlloc as core::alloc::global::GlobalAlloc>::alloc (;14;) (type 8) (param i32 i32 i32) (result i32)
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
    (func $alloc::raw_vec::RawVecInner<A>::with_capacity_in (;15;) (type 9) (param i32 i32 i32 i32)
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
    (func $miden_base_sys::bindings::account::get_id (;16;) (type 3) (param i32)
      (local i32)
      global.get $__stack_pointer
      i32.const 16
      i32.sub
      local.tee 1
      global.set $__stack_pointer
      local.get 1
      i32.const 8
      i32.add
      call $miden_base_sys::bindings::account::extern_account_get_id
      local.get 0
      local.get 1
      i64.load offset=8 align=4
      i64.store
      local.get 1
      i32.const 16
      i32.add
      global.set $__stack_pointer
    )
    (func $miden_base_sys::bindings::note::get_inputs (;17;) (type 3) (param i32)
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
      global.get $GOT.data.internal.__memory_base
      i32.const 1048668
      i32.add
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
      call $miden_base_sys::bindings::note::extern_note_get_inputs
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
    (func $miden_base_sys::bindings::note::get_assets (;18;) (type 3) (param i32)
      (local i32 i32 i32)
      global.get $__stack_pointer
      i32.const 16
      i32.sub
      local.tee 1
      global.set $__stack_pointer
      local.get 1
      i32.const 8
      i32.add
      i32.const 16
      i32.const 16
      global.get $GOT.data.internal.__memory_base
      i32.const 1048684
      i32.add
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
      call $miden_base_sys::bindings::note::extern_note_get_assets
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
    (func $alloc::raw_vec::RawVecInner<A>::deallocate (;19;) (type 7) (param i32 i32 i32)
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
    (func $alloc::raw_vec::RawVecInner<A>::try_allocate_in (;20;) (type 10) (param i32 i32 i32 i32 i32)
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
    (func $<alloc::alloc::Global as core::alloc::Allocator>::allocate (;21;) (type 7) (param i32 i32 i32)
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
    (func $alloc::alloc::Global::alloc_impl (;22;) (type 9) (param i32 i32 i32 i32)
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
    (func $alloc::raw_vec::RawVecInner<A>::current_memory (;23;) (type 9) (param i32 i32 i32 i32)
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
    (func $<alloc::alloc::Global as core::alloc::Allocator>::deallocate (;24;) (type 7) (param i32 i32 i32)
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
    (func $alloc::raw_vec::handle_error (;25;) (type 7) (param i32 i32 i32)
      unreachable
    )
    (func $core::ptr::alignment::Alignment::max (;26;) (type 6) (param i32 i32) (result i32)
      local.get 0
      local.get 1
      local.get 0
      local.get 1
      i32.gt_u
      select
    )
    (data $.rodata (;0;) (i32.const 1048576) "miden-base-sys-0.1.5/src/bindings/note.rs\00")
    (data $.data (;1;) (i32.const 1048620) "\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00\00\00\10\00)\00\00\00\13\00\00\00!\00\00\00\00\00\10\00)\00\00\001\00\00\00\22\00\00\00")
  )
  (alias export 3 "assert-eq" (func (;0;)))
  (core func (;0;) (canon lower (func 0)))
  (core instance (;0;)
    (export "assert-eq" (func 0))
  )
  (alias export 1 "receive-asset" (func (;1;)))
  (core func (;1;) (canon lower (func 1)))
  (core instance (;1;)
    (export "receive-asset" (func 1))
  )
  (alias export 2 "heap-base" (func (;2;)))
  (core func (;2;) (canon lower (func 2)))
  (core instance (;2;)
    (export "heap-base" (func 2))
  )
  (alias export 4 "get-id" (func (;3;)))
  (core func (;3;) (canon lower (func 3)))
  (core instance (;3;)
    (export "get-id" (func 3))
  )
  (alias export 5 "get-inputs" (func (;4;)))
  (core func (;4;) (canon lower (func 4)))
  (alias export 5 "get-assets" (func (;5;)))
  (core func (;5;) (canon lower (func 5)))
  (core instance (;4;)
    (export "get-inputs" (func 4))
    (export "get-assets" (func 5))
  )
  (core instance (;5;) (instantiate 0
      (with "miden:core-intrinsics/intrinsics-felt@1.0.0" (instance 0))
      (with "miden:basic-wallet/basic-wallet@1.0.0" (instance 1))
      (with "miden:core-intrinsics/intrinsics-mem@1.0.0" (instance 2))
      (with "miden:core-base/account@1.0.0" (instance 3))
      (with "miden:core-base/note@1.0.0" (instance 4))
    )
  )
  (alias core export 5 "memory" (core memory (;0;)))
  (type (;7;) (func))
  (alias core export 5 "miden:base/script@1.0.0#script" (core func (;6;)))
  (func (;6;) (type 7) (canon lift (core func 6)))
  (component (;0;)
    (type (;0;) (func))
    (import "import-func-script" (func (;0;) (type 0)))
    (type (;1;) (func))
    (export (;1;) "script" (func 0) (func (type 1)))
  )
  (instance (;6;) (instantiate 0
      (with "import-func-script" (func 6))
    )
  )
  (export (;7;) "miden:base/script@1.0.0" (instance 6))
)
