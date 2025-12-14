(component
  (type $ty-miden:base/core-types@1.0.0 (;0;)
    (instance
      (type (;0;) (record (field "inner" f32)))
      (export (;1;) "felt" (type (eq 0)))
      (type (;2;) (tuple 1 1 1 1))
      (type (;3;) (record (field "inner" 2)))
      (export (;4;) "word" (type (eq 3)))
      (type (;5;) (record (field "inner" 4)))
      (export (;6;) "asset" (type (eq 5)))
      (type (;7;) (record (field "inner" 1)))
      (export (;8;) "note-idx" (type (eq 7)))
    )
  )
  (import "miden:base/core-types@1.0.0" (instance $miden:base/core-types@1.0.0 (;0;) (type $ty-miden:base/core-types@1.0.0)))
  (alias export $miden:base/core-types@1.0.0 "asset" (type $asset (;1;)))
  (alias export $miden:base/core-types@1.0.0 "note-idx" (type $note-idx (;2;)))
  (type $ty-miden:basic-wallet/basic-wallet@0.1.0 (;3;)
    (instance
      (alias outer 1 $asset (type (;0;)))
      (export (;1;) "asset" (type (eq 0)))
      (alias outer 1 $note-idx (type (;2;)))
      (export (;3;) "note-idx" (type (eq 2)))
      (type (;4;) (func (param "asset" 1) (param "note-idx" 3)))
      (export (;0;) "move-asset-to-note" (func (type 4)))
    )
  )
  (import "miden:basic-wallet/basic-wallet@0.1.0" (instance $miden:basic-wallet/basic-wallet@0.1.0 (;1;) (type $ty-miden:basic-wallet/basic-wallet@0.1.0)))
  (core module $main (;0;)
    (type (;0;) (func (param f32 f32 f32 f32 f32)))
    (type (;1;) (func))
    (type (;2;) (func (param i32 i32) (result i32)))
    (type (;3;) (func (param i32 i32 i32)))
    (type (;4;) (func (param i32 i32 i32 i32)))
    (type (;5;) (func (param f32 f32 f32 f32)))
    (type (;6;) (func (param i32 i32 i32) (result i32)))
    (type (;7;) (func (result i32)))
    (type (;8;) (func (param f32 f32 f32 f32 i32) (result f32)))
    (type (;9;) (func (param i32 i32)))
    (type (;10;) (func (param f32 f32 f32 f32) (result f32)))
    (type (;11;) (func (param f32) (result i64)))
    (type (;12;) (func (param f32 f32)))
    (type (;13;) (func (param i32) (result f32)))
    (type (;14;) (func (param i64) (result f32)))
    (type (;15;) (func (param f32 i32 f32 f32 f32 f32) (result i32)))
    (type (;16;) (func (param i32 i32 i32 i32 i32)))
    (type (;17;) (func (param f32 f32 f32 f32 f32 f32 f32 f32) (result f32)))
    (import "miden:basic-wallet/basic-wallet@0.1.0" "move-asset-to-note" (func $basic_wallet_tx_script::bindings::miden::basic_wallet::basic_wallet::move_asset_to_note::wit_import9 (;0;) (type 0)))
    (table (;0;) 2 2 funcref)
    (memory (;0;) 17)
    (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
    (global $GOT.data.internal.__memory_base (;1;) i32 i32.const 0)
    (export "memory" (memory 0))
    (export "miden:base/transaction-script@1.0.0#run" (func $miden:base/transaction-script@1.0.0#run))
    (elem (;0;) (i32.const 1) func $basic_wallet_tx_script::bindings::__link_custom_section_describing_imports)
    (func $__wasm_call_ctors (;1;) (type 1))
    (func $__rustc::__rust_alloc (;2;) (type 2) (param i32 i32) (result i32)
      global.get $GOT.data.internal.__memory_base
      i32.const 1048616
      i32.add
      local.get 1
      local.get 0
      call $<miden_sdk_alloc::BumpAlloc as core::alloc::global::GlobalAlloc>::alloc
    )
    (func $__rustc::__rust_dealloc (;3;) (type 3) (param i32 i32 i32))
    (func $__rustc::__rust_alloc_zeroed (;4;) (type 2) (param i32 i32) (result i32)
      block ;; label = @1
        global.get $GOT.data.internal.__memory_base
        i32.const 1048616
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
    (func $basic_wallet_tx_script::bindings::__link_custom_section_describing_imports (;5;) (type 1))
    (func $core::slice::index::slice_index_fail (;6;) (type 3) (param i32 i32 i32)
      block ;; label = @1
        block ;; label = @2
          local.get 0
          local.get 2
          i32.gt_u
          br_if 0 (;@2;)
          local.get 1
          local.get 2
          i32.gt_u
          br_if 1 (;@1;)
          local.get 1
          local.get 2
          global.get $GOT.data.internal.__memory_base
          i32.const 1048600
          i32.add
          call $core::slice::index::slice_index_fail::do_panic::runtime
          unreachable
        end
        local.get 0
        local.get 2
        global.get $GOT.data.internal.__memory_base
        i32.const 1048600
        i32.add
        call $core::slice::index::slice_index_fail::do_panic::runtime
        unreachable
      end
      local.get 1
      local.get 2
      global.get $GOT.data.internal.__memory_base
      i32.const 1048600
      i32.add
      call $core::slice::index::slice_index_fail::do_panic::runtime
      unreachable
    )
    (func $<alloc::vec::Vec<miden_stdlib_sys::intrinsics::felt::Felt> as core::ops::index::Index<core::ops::range::Range<usize>>>::index (;7;) (type 4) (param i32 i32 i32 i32)
      (local i32)
      block ;; label = @1
        local.get 3
        local.get 1
        i32.load offset=8
        local.tee 4
        i32.le_u
        br_if 0 (;@1;)
        local.get 2
        local.get 3
        local.get 4
        call $core::slice::index::slice_index_fail
        unreachable
      end
      local.get 0
      local.get 3
      local.get 2
      i32.sub
      i32.store offset=4
      local.get 0
      local.get 1
      i32.load offset=4
      local.get 2
      i32.const 2
      i32.shl
      i32.add
      i32.store
    )
    (func $miden:base/transaction-script@1.0.0#run (;8;) (type 5) (param f32 f32 f32 f32)
      (local i32 i64 f32 i32 i32 i32)
      global.get $__stack_pointer
      i32.const 80
      i32.sub
      local.tee 4
      global.set $__stack_pointer
      call $wit_bindgen::rt::run_ctors_once
      local.get 3
      local.get 2
      local.get 1
      local.get 0
      call $intrinsics::advice::adv_push_mapvaln
      call $intrinsics::felt::as_u64
      local.tee 5
      i32.wrap_i64
      i32.const 3
      i32.and
      call $intrinsics::felt::from_u32
      i32.const 0
      call $intrinsics::felt::from_u32
      call $intrinsics::felt::assert_eq
      local.get 4
      i32.const 64
      i32.add
      local.get 5
      i64.const 2
      i64.shr_u
      call $intrinsics::felt::from_u64_unchecked
      local.tee 6
      call $intrinsics::felt::as_u64
      i32.wrap_i64
      i32.const 2
      i32.shl
      local.tee 7
      i32.const 0
      i32.const 4
      i32.const 4
      call $<alloc::raw_vec::RawVecInner>::try_allocate_in
      local.get 4
      i32.load offset=68
      local.set 8
      block ;; label = @1
        block ;; label = @2
          local.get 4
          i32.load offset=64
          i32.const 1
          i32.eq
          br_if 0 (;@2;)
          local.get 6
          local.get 4
          i32.load offset=72
          local.tee 9
          i32.const 2
          i32.shr_u
          local.get 3
          local.get 2
          local.get 1
          local.get 0
          call $std::mem::pipe_preimage_to_memory
          drop
          local.get 4
          local.get 7
          i32.store offset=28
          local.get 4
          local.get 9
          i32.store offset=24
          local.get 4
          local.get 8
          i32.store offset=20
          local.get 7
          i32.eqz
          br_if 1 (;@1;)
          local.get 9
          f32.load offset=12
          local.set 0
          local.get 9
          f32.load offset=8
          local.set 1
          local.get 9
          f32.load offset=4
          local.set 2
          local.get 9
          f32.load
          local.set 3
          local.get 4
          i32.const 8
          i32.add
          local.get 4
          i32.const 20
          i32.add
          i32.const 4
          i32.const 8
          call $<alloc::vec::Vec<miden_stdlib_sys::intrinsics::felt::Felt> as core::ops::index::Index<core::ops::range::Range<usize>>>::index
          local.get 4
          i32.load offset=12
          i32.const 4
          i32.ne
          br_if 1 (;@1;)
          local.get 4
          i32.const 32
          i32.add
          i32.const 8
          i32.add
          local.get 4
          i32.load offset=8
          local.tee 9
          i32.const 8
          i32.add
          i64.load align=4
          i64.store
          local.get 4
          local.get 9
          i64.load align=4
          i64.store offset=32
          local.get 4
          i32.const 64
          i32.add
          local.get 4
          i32.const 32
          i32.add
          call $<miden_base_sys::bindings::types::Asset as core::convert::From<[miden_stdlib_sys::intrinsics::felt::Felt; 4]>>::from
          local.get 3
          local.get 2
          local.get 1
          local.get 0
          local.get 4
          i32.const 64
          i32.add
          call $miden_base_sys::bindings::output_note::create
          local.set 0
          local.get 4
          local.get 4
          i32.const 20
          i32.add
          i32.const 8
          i32.const 12
          call $<alloc::vec::Vec<miden_stdlib_sys::intrinsics::felt::Felt> as core::ops::index::Index<core::ops::range::Range<usize>>>::index
          local.get 4
          i32.load offset=4
          i32.const 4
          i32.ne
          br_if 1 (;@1;)
          local.get 4
          i32.const 48
          i32.add
          i32.const 8
          i32.add
          local.get 4
          i32.load
          local.tee 9
          i32.const 8
          i32.add
          i64.load align=4
          i64.store
          local.get 4
          local.get 9
          i64.load align=4
          i64.store offset=48
          local.get 4
          i32.const 64
          i32.add
          local.get 4
          i32.const 48
          i32.add
          call $<miden_base_sys::bindings::types::Asset as core::convert::From<[miden_stdlib_sys::intrinsics::felt::Felt; 4]>>::from
          local.get 4
          f32.load offset=64
          local.get 4
          f32.load offset=68
          local.get 4
          f32.load offset=72
          local.get 4
          f32.load offset=76
          local.get 0
          call $basic_wallet_tx_script::bindings::miden::basic_wallet::basic_wallet::move_asset_to_note::wit_import9
          local.get 4
          i32.const 20
          i32.add
          i32.const 4
          i32.const 4
          call $<alloc::raw_vec::RawVecInner>::deallocate
          local.get 4
          i32.const 80
          i32.add
          global.set $__stack_pointer
          return
        end
        local.get 8
        local.get 4
        i32.load offset=72
        call $alloc::raw_vec::handle_error
      end
      unreachable
    )
    (func $__rustc::__rust_no_alloc_shim_is_unstable_v2 (;9;) (type 1)
      return
    )
    (func $wit_bindgen::rt::run_ctors_once (;10;) (type 1)
      (local i32)
      block ;; label = @1
        global.get $GOT.data.internal.__memory_base
        i32.const 1048620
        i32.add
        i32.load8_u
        br_if 0 (;@1;)
        global.get $GOT.data.internal.__memory_base
        local.set 0
        call $__wasm_call_ctors
        local.get 0
        i32.const 1048620
        i32.add
        i32.const 1
        i32.store8
      end
    )
    (func $<miden_sdk_alloc::BumpAlloc as core::alloc::global::GlobalAlloc>::alloc (;11;) (type 6) (param i32 i32 i32) (result i32)
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
    (func $intrinsics::mem::heap_base (;12;) (type 7) (result i32)
      unreachable
    )
    (func $miden_base_sys::bindings::output_note::create (;13;) (type 8) (param f32 f32 f32 f32 i32) (result f32)
      local.get 0
      local.get 1
      local.get 2
      local.get 3
      local.get 4
      f32.load offset=12
      local.get 4
      f32.load offset=8
      local.get 4
      f32.load offset=4
      local.get 4
      f32.load
      call $miden::output_note::create
    )
    (func $<miden_base_sys::bindings::types::Asset as core::convert::From<[miden_stdlib_sys::intrinsics::felt::Felt; 4]>>::from (;14;) (type 9) (param i32 i32)
      local.get 0
      local.get 1
      i64.load offset=8 align=4
      i64.store offset=8
      local.get 0
      local.get 1
      i64.load align=4
      i64.store
    )
    (func $intrinsics::advice::adv_push_mapvaln (;15;) (type 10) (param f32 f32 f32 f32) (result f32)
      unreachable
    )
    (func $intrinsics::felt::as_u64 (;16;) (type 11) (param f32) (result i64)
      unreachable
    )
    (func $intrinsics::felt::assert_eq (;17;) (type 12) (param f32 f32)
      unreachable
    )
    (func $intrinsics::felt::from_u32 (;18;) (type 13) (param i32) (result f32)
      unreachable
    )
    (func $intrinsics::felt::from_u64_unchecked (;19;) (type 14) (param i64) (result f32)
      unreachable
    )
    (func $std::mem::pipe_preimage_to_memory (;20;) (type 15) (param f32 i32 f32 f32 f32 f32) (result i32)
      unreachable
    )
    (func $<alloc::alloc::Global>::alloc_impl (;21;) (type 4) (param i32 i32 i32 i32)
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
    (func $<alloc::raw_vec::RawVecInner>::deallocate (;22;) (type 3) (param i32 i32 i32)
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
      call $<alloc::raw_vec::RawVecInner>::current_memory
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
    (func $<alloc::raw_vec::RawVecInner>::current_memory (;23;) (type 4) (param i32 i32 i32 i32)
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
    (func $<alloc::alloc::Global as core::alloc::Allocator>::deallocate (;24;) (type 3) (param i32 i32 i32)
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
    (func $<alloc::alloc::Global as core::alloc::Allocator>::allocate (;25;) (type 3) (param i32 i32 i32)
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
    (func $<alloc::raw_vec::RawVecInner>::try_allocate_in (;26;) (type 16) (param i32 i32 i32 i32 i32)
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
    (func $alloc::raw_vec::handle_error (;27;) (type 9) (param i32 i32)
      unreachable
    )
    (func $core::slice::index::slice_index_fail::do_panic::runtime (;28;) (type 3) (param i32 i32 i32)
      unreachable
    )
    (func $<core::ptr::alignment::Alignment>::max (;29;) (type 2) (param i32 i32) (result i32)
      local.get 0
      local.get 1
      local.get 0
      local.get 1
      i32.gt_u
      select
    )
    (func $miden::output_note::create (;30;) (type 17) (param f32 f32 f32 f32 f32 f32 f32 f32) (result f32)
      unreachable
    )
    (data $.rodata (;0;) (i32.const 1048576) "<redacted>\00")
    (data $.data (;1;) (i32.const 1048588) "\01\00\00\00\01\00\00\00\01\00\00\00\00\00\10\00\0a\00\00\00\00\00\00\00\00\00\00\00")
  )
  (alias export $miden:base/core-types@1.0.0 "word" (type $word (;4;)))
  (alias export $miden:basic-wallet/basic-wallet@0.1.0 "move-asset-to-note" (func $move-asset-to-note (;0;)))
  (core func $move-asset-to-note (;0;) (canon lower (func $move-asset-to-note)))
  (core instance $miden:basic-wallet/basic-wallet@0.1.0 (;0;)
    (export "move-asset-to-note" (func $move-asset-to-note))
  )
  (core instance $main (;1;) (instantiate $main
      (with "miden:basic-wallet/basic-wallet@0.1.0" (instance $miden:basic-wallet/basic-wallet@0.1.0))
    )
  )
  (alias core export $main "memory" (core memory $memory (;0;)))
  (type (;5;) (func (param "arg" $word)))
  (alias core export $main "miden:base/transaction-script@1.0.0#run" (core func $miden:base/transaction-script@1.0.0#run (;1;)))
  (func $run (;1;) (type 5) (canon lift (core func $miden:base/transaction-script@1.0.0#run)))
  (alias export $miden:base/core-types@1.0.0 "felt" (type $felt (;6;)))
  (alias export $miden:base/core-types@1.0.0 "word" (type $"#type7 word" (@name "word") (;7;)))
  (component $miden:base/transaction-script@1.0.0-shim-component (;0;)
    (type (;0;) (record (field "inner" f32)))
    (import "import-type-felt" (type (;1;) (eq 0)))
    (type (;2;) (tuple 1 1 1 1))
    (type (;3;) (record (field "inner" 2)))
    (import "import-type-word" (type (;4;) (eq 3)))
    (import "import-type-word0" (type (;5;) (eq 4)))
    (type (;6;) (func (param "arg" 5)))
    (import "import-func-run" (func (;0;) (type 6)))
    (export (;7;) "word" (type 4))
    (type (;8;) (func (param "arg" 7)))
    (export (;1;) "run" (func 0) (func (type 8)))
  )
  (instance $miden:base/transaction-script@1.0.0-shim-instance (;2;) (instantiate $miden:base/transaction-script@1.0.0-shim-component
      (with "import-func-run" (func $run))
      (with "import-type-felt" (type $felt))
      (with "import-type-word" (type $"#type7 word"))
      (with "import-type-word0" (type $word))
    )
  )
  (export $miden:base/transaction-script@1.0.0 (;3;) "miden:base/transaction-script@1.0.0" (instance $miden:base/transaction-script@1.0.0-shim-instance))
)
