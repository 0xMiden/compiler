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
      (type (;7;) (record (field "inner" 1)))
      (export (;8;) "note-idx" (type (eq 7)))
    )
  )
  (import "miden:base/core-types@1.0.0" (instance (;0;) (type 0)))
  (alias export 0 "asset" (type (;1;)))
  (alias export 0 "note-idx" (type (;2;)))
  (type (;3;)
    (instance
      (alias outer 1 1 (type (;0;)))
      (export (;1;) "asset" (type (eq 0)))
      (alias outer 1 2 (type (;2;)))
      (export (;3;) "note-idx" (type (eq 2)))
      (type (;4;) (func (param "asset" 1) (param "note-idx" 3)))
      (export (;0;) "move-asset-to-note" (func (type 4)))
    )
  )
  (import "miden:basic-wallet/basic-wallet@1.0.0" (instance (;1;) (type 3)))
  (type (;4;)
    (instance
      (type (;0;) (func (result s32)))
      (export (;0;) "heap-base" (func (type 0)))
    )
  )
  (import "miden:core-intrinsics/intrinsics-mem@1.0.0" (instance (;2;) (type 4)))
  (type (;5;)
    (instance
      (type (;0;) (func (param "a" u64) (result f32)))
      (export (;0;) "from-u64-unchecked" (func (type 0)))
      (type (;1;) (func (param "a" u32) (result f32)))
      (export (;1;) "from-u32" (func (type 1)))
      (type (;2;) (func (param "a" f32) (result u64)))
      (export (;2;) "as-u64" (func (type 2)))
      (type (;3;) (func (param "a" f32) (param "b" f32)))
      (export (;3;) "assert-eq" (func (type 3)))
    )
  )
  (import "miden:core-intrinsics/intrinsics-felt@1.0.0" (instance (;3;) (type 5)))
  (type (;6;)
    (instance
      (type (;0;) (func (param "key0" f32) (param "key1" f32) (param "key2" f32) (param "key3" f32) (result f32)))
      (export (;0;) "adv-push-mapvaln" (func (type 0)))
    )
  )
  (import "miden:core-intrinsics/intrinsics-advice@1.0.0" (instance (;4;) (type 6)))
  (type (;7;)
    (instance
      (type (;0;) (func (param "num-words" f32) (param "result-ptr" s32) (param "c0" f32) (param "c1" f32) (param "c2" f32) (param "c3" f32) (result s32)))
      (export (;0;) "pipe-preimage-to-memory" (func (type 0)))
    )
  )
  (import "miden:core-stdlib/stdlib-mem@1.0.0" (instance (;5;) (type 7)))
  (type (;8;)
    (instance
      (type (;0;) (func (param "tag" f32) (param "aux" f32) (param "note-type" f32) (param "execution-hint" f32) (param "recipient0" f32) (param "recipient1" f32) (param "recipient2" f32) (param "recipient3" f32) (result f32)))
      (export (;0;) "create-note" (func (type 0)))
    )
  )
  (import "miden:core-base/tx@1.0.0" (instance (;6;) (type 8)))
  (core module (;0;)
    (type (;0;) (func (param f32 f32 f32 f32) (result f32)))
    (type (;1;) (func (param f32) (result i64)))
    (type (;2;) (func (param i32) (result f32)))
    (type (;3;) (func (param f32 f32)))
    (type (;4;) (func (param i64) (result f32)))
    (type (;5;) (func (param f32 i32 f32 f32 f32 f32) (result i32)))
    (type (;6;) (func (param f32 f32 f32 f32 f32)))
    (type (;7;) (func (result i32)))
    (type (;8;) (func (param f32 f32 f32 f32 f32 f32 f32 f32) (result f32)))
    (type (;9;) (func))
    (type (;10;) (func (param i32 i32 i32)))
    (type (;11;) (func (param i32 i32 i32 i32 i32)))
    (type (;12;) (func (param i32 i32) (result i32)))
    (type (;13;) (func (param f32 f32 f32 f32)))
    (type (;14;) (func (param i32 i32 i32) (result i32)))
    (type (;15;) (func (param f32 f32 f32 f32 i32) (result f32)))
    (type (;16;) (func (param i32 i32)))
    (type (;17;) (func (param i32 i32 i32 i32)))
    (import "miden:core-intrinsics/intrinsics-advice@1.0.0" "adv-push-mapvaln" (func $miden_stdlib_sys::intrinsics::advice::extern_adv_push_mapvaln (;0;) (type 0)))
    (import "miden:core-intrinsics/intrinsics-felt@1.0.0" "as-u64" (func $miden_stdlib_sys::intrinsics::felt::extern_as_u64 (;1;) (type 1)))
    (import "miden:core-intrinsics/intrinsics-felt@1.0.0" "from-u32" (func $miden_stdlib_sys::intrinsics::felt::extern_from_u32 (;2;) (type 2)))
    (import "miden:core-intrinsics/intrinsics-felt@1.0.0" "assert-eq" (func $miden_stdlib_sys::intrinsics::felt::extern_assert_eq (;3;) (type 3)))
    (import "miden:core-intrinsics/intrinsics-felt@1.0.0" "from-u64-unchecked" (func $miden_stdlib_sys::intrinsics::felt::extern_from_u64_unchecked (;4;) (type 4)))
    (import "miden:core-stdlib/stdlib-mem@1.0.0" "pipe-preimage-to-memory" (func $miden_stdlib_sys::stdlib::mem::extern_pipe_preimage_to_memory (;5;) (type 5)))
    (import "miden:basic-wallet/basic-wallet@1.0.0" "move-asset-to-note" (func $basic_wallet_tx_script::bindings::miden::basic_wallet::basic_wallet::move_asset_to_note::wit_import9 (;6;) (type 6)))
    (import "miden:core-intrinsics/intrinsics-mem@1.0.0" "heap-base" (func $miden_sdk_alloc::heap_base (;7;) (type 7)))
    (import "miden:core-base/tx@1.0.0" "create-note" (func $miden_base_sys::bindings::tx::extern_tx_create_note (;8;) (type 8)))
    (table (;0;) 2 2 funcref)
    (memory (;0;) 17)
    (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
    (global $GOT.data.internal.__memory_base (;1;) i32 i32.const 0)
    (export "memory" (memory 0))
    (export "miden:base/transaction-script@1.0.0#run" (func $miden:base/transaction-script@1.0.0#run))
    (elem (;0;) (i32.const 1) func $basic_wallet_tx_script::bindings::__link_custom_section_describing_imports)
    (func $__wasm_call_ctors (;9;) (type 9))
    (func $core::slice::index::slice_end_index_len_fail (;10;) (type 10) (param i32 i32 i32)
      local.get 0
      local.get 1
      local.get 2
      call $core::slice::<impl [T]>::copy_from_slice::len_mismatch_fail::do_panic::runtime
      unreachable
    )
    (func $<alloc::vec::Vec<T,A> as core::ops::index::Index<I>>::index (;11;) (type 11) (param i32 i32 i32 i32 i32)
      (local i32)
      block ;; label = @1
        local.get 3
        local.get 1
        i32.load offset=8
        local.tee 5
        i32.le_u
        br_if 0 (;@1;)
        local.get 3
        local.get 5
        local.get 4
        call $core::slice::index::slice_end_index_len_fail
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
    (func $basic_wallet_tx_script::bindings::__link_custom_section_describing_imports (;12;) (type 9))
    (func $__rustc::__rust_alloc (;13;) (type 12) (param i32 i32) (result i32)
      global.get $GOT.data.internal.__memory_base
      i32.const 1048728
      i32.add
      local.get 1
      local.get 0
      call $<miden_sdk_alloc::BumpAlloc as core::alloc::global::GlobalAlloc>::alloc
    )
    (func $__rustc::__rust_dealloc (;14;) (type 10) (param i32 i32 i32))
    (func $__rustc::__rust_alloc_zeroed (;15;) (type 12) (param i32 i32) (result i32)
      block ;; label = @1
        global.get $GOT.data.internal.__memory_base
        i32.const 1048728
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
    (func $miden:base/transaction-script@1.0.0#run (;16;) (type 13) (param f32 f32 f32 f32)
      (local i32 i64 f32 i32 i32 i32)
      global.get $__stack_pointer
      i32.const 80
      i32.sub
      local.tee 4
      global.set $__stack_pointer
      call $wit_bindgen_rt::run_ctors_once
      local.get 3
      local.get 2
      local.get 1
      local.get 0
      call $miden_stdlib_sys::intrinsics::advice::extern_adv_push_mapvaln
      call $miden_stdlib_sys::intrinsics::felt::extern_as_u64
      local.tee 5
      i32.wrap_i64
      i32.const 3
      i32.and
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      i32.const 0
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      call $miden_stdlib_sys::intrinsics::felt::extern_assert_eq
      local.get 4
      i32.const 64
      i32.add
      local.get 5
      i64.const 2
      i64.shr_u
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u64_unchecked
      local.tee 6
      call $miden_stdlib_sys::intrinsics::felt::extern_as_u64
      i32.wrap_i64
      i32.const 2
      i32.shl
      local.tee 7
      i32.const 0
      i32.const 4
      i32.const 4
      call $alloc::raw_vec::RawVecInner<A>::try_allocate_in
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
          call $miden_stdlib_sys::stdlib::mem::extern_pipe_preimage_to_memory
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
          global.get $GOT.data.internal.__memory_base
          local.set 7
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
          local.get 7
          i32.const 1048696
          i32.add
          call $<alloc::vec::Vec<T,A> as core::ops::index::Index<I>>::index
          local.get 4
          i32.load offset=12
          i32.const 4
          i32.ne
          br_if 1 (;@1;)
          local.get 4
          i32.load offset=8
          local.tee 9
          i64.load align=4
          local.set 5
          local.get 4
          i32.const 32
          i32.add
          i32.const 8
          i32.add
          local.get 9
          i32.const 8
          i32.add
          i64.load align=4
          i64.store
          local.get 4
          local.get 5
          i64.store offset=32
          global.get $GOT.data.internal.__memory_base
          local.set 9
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
          call $miden_base_sys::bindings::tx::create_note
          local.set 0
          local.get 4
          local.get 4
          i32.const 20
          i32.add
          i32.const 8
          i32.const 12
          local.get 9
          i32.const 1048712
          i32.add
          call $<alloc::vec::Vec<T,A> as core::ops::index::Index<I>>::index
          local.get 4
          i32.load offset=4
          i32.const 4
          i32.ne
          br_if 1 (;@1;)
          local.get 4
          i32.load
          local.tee 9
          i64.load align=4
          local.set 5
          local.get 4
          i32.const 48
          i32.add
          i32.const 8
          i32.add
          local.get 9
          i32.const 8
          i32.add
          i64.load align=4
          i64.store
          local.get 4
          local.get 5
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
          call $alloc::raw_vec::RawVecInner<A>::deallocate
          local.get 4
          i32.const 80
          i32.add
          global.set $__stack_pointer
          return
        end
        global.get $GOT.data.internal.__memory_base
        local.set 9
        local.get 8
        local.get 4
        i32.load offset=72
        local.get 9
        i32.const 1048680
        i32.add
        call $alloc::raw_vec::handle_error
      end
      unreachable
    )
    (func $__rustc::__rust_no_alloc_shim_is_unstable_v2 (;17;) (type 9)
      return
    )
    (func $wit_bindgen_rt::run_ctors_once (;18;) (type 9)
      (local i32)
      block ;; label = @1
        global.get $GOT.data.internal.__memory_base
        i32.const 1048732
        i32.add
        i32.load8_u
        br_if 0 (;@1;)
        global.get $GOT.data.internal.__memory_base
        local.set 0
        call $__wasm_call_ctors
        local.get 0
        i32.const 1048732
        i32.add
        i32.const 1
        i32.store8
      end
    )
    (func $<miden_sdk_alloc::BumpAlloc as core::alloc::global::GlobalAlloc>::alloc (;19;) (type 14) (param i32 i32 i32) (result i32)
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
          i32.const 268435456
          local.get 0
          i32.load
          local.tee 4
          i32.sub
          local.get 2
          i32.lt_u
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
    (func $miden_base_sys::bindings::tx::create_note (;20;) (type 15) (param f32 f32 f32 f32 i32) (result f32)
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
      call $miden_base_sys::bindings::tx::extern_tx_create_note
    )
    (func $<miden_base_sys::bindings::types::Asset as core::convert::From<[miden_stdlib_sys::intrinsics::felt::Felt; 4]>>::from (;21;) (type 16) (param i32 i32)
      local.get 0
      local.get 1
      i64.load offset=8 align=4
      i64.store offset=8
      local.get 0
      local.get 1
      i64.load align=4
      i64.store
    )
    (func $alloc::raw_vec::RawVecInner<A>::deallocate (;22;) (type 10) (param i32 i32 i32)
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
    (func $alloc::raw_vec::RawVecInner<A>::try_allocate_in (;23;) (type 11) (param i32 i32 i32 i32 i32)
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
    (func $<alloc::alloc::Global as core::alloc::Allocator>::allocate (;24;) (type 10) (param i32 i32 i32)
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
    (func $alloc::alloc::Global::alloc_impl (;25;) (type 17) (param i32 i32 i32 i32)
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
    (func $alloc::raw_vec::RawVecInner<A>::current_memory (;26;) (type 17) (param i32 i32 i32 i32)
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
    (func $<alloc::alloc::Global as core::alloc::Allocator>::deallocate (;27;) (type 10) (param i32 i32 i32)
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
    (func $alloc::raw_vec::handle_error (;28;) (type 10) (param i32 i32 i32)
      unreachable
    )
    (func $core::slice::<impl [T]>::copy_from_slice::len_mismatch_fail::do_panic::runtime (;29;) (type 10) (param i32 i32 i32)
      unreachable
    )
    (func $core::ptr::alignment::Alignment::max (;30;) (type 12) (param i32 i32) (result i32)
      local.get 0
      local.get 1
      local.get 0
      local.get 1
      i32.gt_u
      select
    )
    (data $.rodata (;0;) (i32.const 1048576) "miden-stdlib-sys-0.1.5/src/stdlib/mem.rs\00src/lib.rs\00")
    (data $.data (;1;) (i32.const 1048628) "\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00\00\00\10\00(\00\00\00\98\00\00\00!\00\00\00)\00\10\00\0a\00\00\00+\00\00\00)\00\00\00)\00\10\00\0a\00\00\003\00\00\00%\00\00\00")
  )
  (alias export 0 "word" (type (;9;)))
  (alias export 4 "adv-push-mapvaln" (func (;0;)))
  (core func (;0;) (canon lower (func 0)))
  (core instance (;0;)
    (export "adv-push-mapvaln" (func 0))
  )
  (alias export 3 "as-u64" (func (;1;)))
  (core func (;1;) (canon lower (func 1)))
  (alias export 3 "from-u32" (func (;2;)))
  (core func (;2;) (canon lower (func 2)))
  (alias export 3 "assert-eq" (func (;3;)))
  (core func (;3;) (canon lower (func 3)))
  (alias export 3 "from-u64-unchecked" (func (;4;)))
  (core func (;4;) (canon lower (func 4)))
  (core instance (;1;)
    (export "as-u64" (func 1))
    (export "from-u32" (func 2))
    (export "assert-eq" (func 3))
    (export "from-u64-unchecked" (func 4))
  )
  (alias export 5 "pipe-preimage-to-memory" (func (;5;)))
  (core func (;5;) (canon lower (func 5)))
  (core instance (;2;)
    (export "pipe-preimage-to-memory" (func 5))
  )
  (alias export 1 "move-asset-to-note" (func (;6;)))
  (core func (;6;) (canon lower (func 6)))
  (core instance (;3;)
    (export "move-asset-to-note" (func 6))
  )
  (alias export 2 "heap-base" (func (;7;)))
  (core func (;7;) (canon lower (func 7)))
  (core instance (;4;)
    (export "heap-base" (func 7))
  )
  (alias export 6 "create-note" (func (;8;)))
  (core func (;8;) (canon lower (func 8)))
  (core instance (;5;)
    (export "create-note" (func 8))
  )
  (core instance (;6;) (instantiate 0
      (with "miden:core-intrinsics/intrinsics-advice@1.0.0" (instance 0))
      (with "miden:core-intrinsics/intrinsics-felt@1.0.0" (instance 1))
      (with "miden:core-stdlib/stdlib-mem@1.0.0" (instance 2))
      (with "miden:basic-wallet/basic-wallet@1.0.0" (instance 3))
      (with "miden:core-intrinsics/intrinsics-mem@1.0.0" (instance 4))
      (with "miden:core-base/tx@1.0.0" (instance 5))
    )
  )
  (alias core export 6 "memory" (core memory (;0;)))
  (type (;10;) (func (param "arg" 9)))
  (alias core export 6 "miden:base/transaction-script@1.0.0#run" (core func (;9;)))
  (func (;9;) (type 10) (canon lift (core func 9)))
  (alias export 0 "felt" (type (;11;)))
  (alias export 0 "word" (type (;12;)))
  (component (;0;)
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
  (instance (;7;) (instantiate 0
      (with "import-func-run" (func 9))
      (with "import-type-felt" (type 11))
      (with "import-type-word" (type 12))
      (with "import-type-word0" (type 9))
    )
  )
  (export (;8;) "miden:base/transaction-script@1.0.0" (instance 7))
)
