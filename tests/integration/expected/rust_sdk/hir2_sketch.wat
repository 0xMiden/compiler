(component
  (type (;0;)
    (instance
      (type (;0;) (record (field "inner" float32)))
      (export (;1;) "felt" (type (eq 0)))
      (type (;2;) (tuple 1 1 1 1))
      (type (;3;) (record (field "inner" 2)))
      (export (;4;) "word" (type (eq 3)))
      (type (;5;) (record (field "inner" 4)))
      (export (;6;) "core-asset" (type (eq 5)))
      (type (;7;) (record (field "inner" 1)))
      (export (;8;) "tag" (type (eq 7)))
      (type (;9;) (record (field "inner" 4)))
      (export (;10;) "recipient" (type (eq 9)))
      (type (;11;) (record (field "inner" 1)))
      (export (;12;) "note-type" (type (eq 11)))
    )
  )
  (import "miden:base/core-types@1.0.0" (instance (;0;) (type 0)))
  (type (;1;)
    (instance
      (type (;0;) (func (result s32)))
      (export (;0;) "heap-base" (func (type 0)))
    )
  )
  (import "miden:core-import/intrinsics-mem@1.0.0" (instance (;1;) (type 1)))
  (type (;2;)
    (instance
      (type (;0;) (func (param "a" float32) (param "b" float32) (result float32)))
      (export (;0;) "add" (func (type 0)))
    )
  )
  (import "miden:core-import/intrinsics-felt@1.0.0" (instance (;2;) (type 2)))
  (type (;3;)
    (instance
      (type (;0;) (func (param "a0" s32) (param "a1" s32) (param "a2" s32) (param "a3" s32) (param "a4" s32) (param "a5" s32) (param "a6" s32) (param "a7" s32) (param "result-ptr" s32)))
      (export (;0;) "hash-one-to-one" (func (type 0)))
    )
  )
  (import "miden:core-import/stdlib-crypto-hashes-blake3@1.0.0" (instance (;3;) (type 3)))
  (type (;4;)
    (instance
      (type (;0;) (func (param "asset0" float32) (param "asset1" float32) (param "asset2" float32) (param "asset3" float32) (param "result-ptr" s32)))
      (export (;0;) "add-asset" (func (type 0)))
      (export (;1;) "remove-asset" (func (type 0)))
    )
  )
  (import "miden:core-import/account@1.0.0" (instance (;4;) (type 4)))
  (type (;5;)
    (instance
      (type (;0;) (func (param "asset0" float32) (param "asset1" float32) (param "asset2" float32) (param "asset3" float32) (param "tag" float32) (param "note-type" float32) (param "recipient0" float32) (param "recipient1" float32) (param "recipient2" float32) (param "recipient3" float32) (result float32)))
      (export (;0;) "create-note" (func (type 0)))
    )
  )
  (import "miden:core-import/tx@1.0.0" (instance (;5;) (type 5)))
  (core module (;0;)
    (type (;0;) (func (param f32 f32) (result f32)))
    (type (;1;) (func (param i32 i32 i32 i32 i32 i32 i32 i32 i32)))
    (type (;2;) (func (result i32)))
    (type (;3;) (func (param f32 f32 f32 f32 i32)))
    (type (;4;) (func (param f32 f32 f32 f32 f32 f32 f32 f32 f32 f32) (result f32)))
    (type (;5;) (func))
    (type (;6;) (func (param i32 i32) (result i32)))
    (type (;7;) (func (param i32 i32 i32)))
    (type (;8;) (func (param i32 i32 i32 i32) (result i32)))
    (type (;9;) (func (param f32 f32 f32 f32)))
    (type (;10;) (func (param f32 f32 f32 f32 f32 f32 f32 f32 f32 f32)))
    (type (;11;) (func (param i32)))
    (type (;12;) (func (param f32 f32 f32 f32) (result i32)))
    (type (;13;) (func (param i32 i32 i32) (result i32)))
    (type (;14;) (func (param i32 i32)))
    (type (;15;) (func (param i32 f32 f32 i32) (result f32)))
    (type (;16;) (func (param i32 i32 i32 i32)))
    (import "miden:core-import/intrinsics-felt@1.0.0" "add" (func $miden_stdlib_sys::intrinsics::felt::extern_add (;0;) (type 0)))
    (import "miden:core-import/stdlib-crypto-hashes-blake3@1.0.0" "hash-one-to-one" (func $miden_stdlib_sys::stdlib::crypto::hashes::extern_blake3_hash_1to1 (;1;) (type 1)))
    (import "miden:core-import/intrinsics-mem@1.0.0" "heap-base" (func $miden_sdk_alloc::heap_base (;2;) (type 2)))
    (import "miden:core-import/account@1.0.0" "add-asset" (func $miden_base_sys::bindings::account::extern_account_add_asset (;3;) (type 3)))
    (import "miden:core-import/account@1.0.0" "remove-asset" (func $miden_base_sys::bindings::account::extern_account_remove_asset (;4;) (type 3)))
    (import "miden:core-import/tx@1.0.0" "create-note" (func $miden_base_sys::bindings::tx::extern_tx_create_note (;5;) (type 4)))
    (func $__wasm_call_ctors (;6;) (type 5))
    (func $basic_wallet::bindings::__link_custom_section_describing_imports (;7;) (type 5))
    (func $__rust_alloc (;8;) (type 6) (param i32 i32) (result i32)
      i32.const 1048632
      local.get 1
      local.get 0
      call $<miden_sdk_alloc::BumpAlloc as core::alloc::global::GlobalAlloc>::alloc
    )
    (func $__rust_dealloc (;9;) (type 7) (param i32 i32 i32))
    (func $__rust_realloc (;10;) (type 8) (param i32 i32 i32 i32) (result i32)
      block ;; label = @1
        i32.const 1048632
        local.get 2
        local.get 3
        call $<miden_sdk_alloc::BumpAlloc as core::alloc::global::GlobalAlloc>::alloc
        local.tee 2
        i32.eqz
        br_if 0 (;@1;)
        local.get 2
        local.get 0
        local.get 1
        local.get 3
        local.get 1
        local.get 3
        i32.lt_u
        select
        memory.copy
      end
      local.get 2
    )
    (func $__rust_alloc_zeroed (;11;) (type 6) (param i32 i32) (result i32)
      block ;; label = @1
        i32.const 1048632
        local.get 1
        local.get 0
        call $<miden_sdk_alloc::BumpAlloc as core::alloc::global::GlobalAlloc>::alloc
        local.tee 1
        i32.eqz
        br_if 0 (;@1;)
        local.get 1
        i32.const 0
        local.get 0
        memory.fill
      end
      local.get 1
    )
    (func $miden:basic-wallet/basic-wallet@1.0.0#receive-asset (;12;) (type 9) (param f32 f32 f32 f32)
      (local i32 i32)
      global.get $__stack_pointer
      local.tee 4
      i32.const 64
      i32.sub
      i32.const -32
      i32.and
      local.tee 5
      global.set $__stack_pointer
      call $wit_bindgen_rt::run_ctors_once
      local.get 5
      local.get 3
      f32.store offset=12
      local.get 5
      local.get 2
      f32.store offset=8
      local.get 5
      local.get 1
      f32.store offset=4
      local.get 5
      local.get 0
      f32.store
      local.get 5
      i32.const 32
      i32.add
      local.get 5
      call $miden_base_sys::bindings::account::add_asset
      local.get 4
      global.set $__stack_pointer
    )
    (func $miden:basic-wallet/basic-wallet@1.0.0#send-asset (;13;) (type 10) (param f32 f32 f32 f32 f32 f32 f32 f32 f32 f32)
      (local i32 i32)
      global.get $__stack_pointer
      local.tee 10
      i32.const 96
      i32.sub
      i32.const -32
      i32.and
      local.tee 11
      global.set $__stack_pointer
      call $wit_bindgen_rt::run_ctors_once
      local.get 11
      local.get 3
      f32.store offset=12
      local.get 11
      local.get 2
      f32.store offset=8
      local.get 11
      local.get 1
      f32.store offset=4
      local.get 11
      local.get 0
      f32.store
      local.get 11
      local.get 9
      f32.store offset=44
      local.get 11
      local.get 8
      f32.store offset=40
      local.get 11
      local.get 7
      f32.store offset=36
      local.get 11
      local.get 6
      f32.store offset=32
      local.get 11
      i32.const 64
      i32.add
      local.get 11
      call $miden_base_sys::bindings::account::remove_asset
      local.get 11
      i32.const 64
      i32.add
      local.get 4
      local.get 5
      local.get 11
      i32.const 32
      i32.add
      call $miden_base_sys::bindings::tx::create_note
      drop
      local.get 10
      global.set $__stack_pointer
    )
    (func $miden:basic-wallet/aux@1.0.0#test-felt-intrinsics (;14;) (type 0) (param f32 f32) (result f32)
      call $wit_bindgen_rt::run_ctors_once
      local.get 0
      local.get 1
      call $miden_stdlib_sys::intrinsics::felt::extern_add
    )
    (func $miden:basic-wallet/aux@1.0.0#test-stdlib (;15;) (type 6) (param i32 i32) (result i32)
      (local i32 i32 i32 i32 i32 i32 i32 i32)
      global.get $__stack_pointer
      local.tee 2
      local.set 3
      local.get 2
      i32.const 96
      i32.sub
      i32.const -32
      i32.and
      local.tee 2
      global.set $__stack_pointer
      call $wit_bindgen_rt::run_ctors_once
      local.get 2
      local.get 0
      i32.store offset=24
      local.get 2
      local.get 1
      i32.store offset=20
      block ;; label = @1
        block ;; label = @2
          local.get 1
          i32.const 32
          i32.ne
          br_if 0 (;@2;)
          local.get 2
          i32.const 0
          i32.store offset=28
          local.get 0
          i32.load align=1
          local.set 1
          local.get 0
          i32.load offset=4 align=1
          local.set 4
          local.get 0
          i32.load offset=8 align=1
          local.set 5
          local.get 0
          i32.load offset=12 align=1
          local.set 6
          local.get 0
          i32.load offset=16 align=1
          local.set 7
          local.get 0
          i32.load offset=20 align=1
          local.set 8
          local.get 0
          i32.load offset=24 align=1
          local.set 9
          local.get 0
          i32.load offset=28 align=1
          local.set 0
          local.get 2
          i32.const 20
          i32.add
          call $<alloc::vec::Vec<T,A> as core::ops::drop::Drop>::drop
          local.get 2
          i32.const 20
          i32.add
          call $<alloc::raw_vec::RawVec<T,A> as core::ops::drop::Drop>::drop
          local.get 1
          local.get 4
          local.get 5
          local.get 6
          local.get 7
          local.get 8
          local.get 9
          local.get 0
          local.get 2
          i32.const 32
          i32.add
          call $miden_stdlib_sys::stdlib::crypto::hashes::extern_blake3_hash_1to1
          local.get 2
          i32.const 84
          i32.add
          i32.const 32
          i32.const 0
          call $alloc::raw_vec::RawVec<T,A>::try_allocate_in
          local.get 2
          i32.load offset=88
          local.set 1
          local.get 2
          i32.load offset=84
          i32.const 1
          i32.eq
          br_if 1 (;@1;)
          local.get 2
          i32.load offset=92
          local.tee 0
          i32.const 24
          i32.add
          local.get 2
          i64.load offset=56
          i64.store align=1
          local.get 0
          i32.const 16
          i32.add
          local.get 2
          i64.load offset=48
          i64.store align=1
          local.get 0
          i32.const 8
          i32.add
          local.get 2
          i64.load offset=40
          i64.store align=1
          local.get 0
          local.get 2
          i64.load offset=32
          i64.store align=1
          local.get 2
          i32.const 32
          i32.store offset=92
          local.get 2
          local.get 0
          i32.store offset=88
          local.get 2
          local.get 1
          i32.store offset=84
          local.get 2
          i32.const 8
          i32.add
          local.get 2
          i32.const 84
          i32.add
          call $alloc::vec::Vec<T,A>::into_boxed_slice
          i32.const 0
          local.get 2
          i64.load offset=8
          i64.store offset=1048616 align=4
          local.get 3
          global.set $__stack_pointer
          i32.const 1048616
          return
        end
        unreachable
      end
      local.get 1
      local.get 2
      i32.load offset=92
      call $alloc::raw_vec::handle_error
      unreachable
    )
    (func $miden:basic-wallet/aux@1.0.0#process-list-felt (;16;) (type 6) (param i32 i32) (result i32)
      call $wit_bindgen_rt::run_ctors_once
      unreachable
    )
    (func $cabi_post_miden:basic-wallet/aux@1.0.0#process-list-felt (;17;) (type 11) (param i32))
    (func $miden:basic-wallet/aux@1.0.0#process-core-asset (;18;) (type 12) (param f32 f32 f32 f32) (result i32)
      call $wit_bindgen_rt::run_ctors_once
      unreachable
    )
    (func $cabi_realloc_wit_bindgen_0_28_0 (;19;) (type 8) (param i32 i32 i32 i32) (result i32)
      local.get 0
      local.get 1
      local.get 2
      local.get 3
      call $wit_bindgen_rt::cabi_realloc
    )
    (func $wit_bindgen_rt::cabi_realloc (;20;) (type 8) (param i32 i32 i32 i32) (result i32)
      block ;; label = @1
        block ;; label = @2
          block ;; label = @3
            local.get 1
            br_if 0 (;@3;)
            local.get 3
            i32.eqz
            br_if 2 (;@1;)
            i32.const 0
            i32.load8_u offset=1048636
            drop
            local.get 3
            local.get 2
            call $__rust_alloc
            local.set 2
            br 1 (;@2;)
          end
          local.get 0
          local.get 1
          local.get 2
          local.get 3
          call $__rust_realloc
          local.set 2
        end
        local.get 2
        br_if 0 (;@1;)
        unreachable
      end
      local.get 2
    )
    (func $wit_bindgen_rt::run_ctors_once (;21;) (type 5)
      block ;; label = @1
        i32.const 0
        i32.load8_u offset=1048637
        br_if 0 (;@1;)
        call $__wasm_call_ctors
        i32.const 0
        i32.const 1
        i32.store8 offset=1048637
      end
    )
    (func $<miden_sdk_alloc::BumpAlloc as core::alloc::global::GlobalAlloc>::alloc (;22;) (type 13) (param i32 i32 i32) (result i32)
      (local i32 i32)
      block ;; label = @1
        local.get 1
        i32.const 32
        local.get 1
        i32.const 32
        i32.gt_u
        select
        local.tee 1
        i32.popcnt
        i32.const 1
        i32.ne
        br_if 0 (;@1;)
        i32.const -2147483648
        local.get 1
        i32.sub
        local.get 2
        i32.lt_u
        br_if 0 (;@1;)
        i32.const 0
        local.set 3
        local.get 1
        local.get 2
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
    (func $miden_base_sys::bindings::account::add_asset (;23;) (type 14) (param i32 i32)
      local.get 1
      f32.load
      local.get 1
      f32.load offset=4
      local.get 1
      f32.load offset=8
      local.get 1
      f32.load offset=12
      local.get 0
      call $miden_base_sys::bindings::account::extern_account_add_asset
    )
    (func $miden_base_sys::bindings::account::remove_asset (;24;) (type 14) (param i32 i32)
      local.get 1
      f32.load
      local.get 1
      f32.load offset=4
      local.get 1
      f32.load offset=8
      local.get 1
      f32.load offset=12
      local.get 0
      call $miden_base_sys::bindings::account::extern_account_remove_asset
    )
    (func $miden_base_sys::bindings::tx::create_note (;25;) (type 15) (param i32 f32 f32 i32) (result f32)
      local.get 0
      f32.load
      local.get 0
      f32.load offset=4
      local.get 0
      f32.load offset=8
      local.get 0
      f32.load offset=12
      local.get 1
      local.get 2
      local.get 3
      f32.load
      local.get 3
      f32.load offset=4
      local.get 3
      f32.load offset=8
      local.get 3
      f32.load offset=12
      call $miden_base_sys::bindings::tx::extern_tx_create_note
    )
    (func $alloc::vec::Vec<T,A>::into_boxed_slice (;26;) (type 14) (param i32 i32)
      (local i32 i32)
      global.get $__stack_pointer
      i32.const 16
      i32.sub
      local.tee 2
      global.set $__stack_pointer
      block ;; label = @1
        block ;; label = @2
          local.get 1
          i32.load
          local.get 1
          i32.load offset=8
          local.tee 3
          i32.le_u
          br_if 0 (;@2;)
          local.get 2
          i32.const 8
          i32.add
          local.get 1
          local.get 3
          call $alloc::raw_vec::RawVec<T,A>::shrink_unchecked
          local.get 2
          i32.load offset=8
          i32.const -2147483647
          i32.ne
          br_if 1 (;@1;)
          local.get 1
          i32.load offset=8
          local.set 3
        end
        local.get 0
        local.get 3
        i32.store offset=4
        local.get 0
        local.get 1
        i32.load offset=4
        i32.store
        local.get 2
        i32.const 16
        i32.add
        global.set $__stack_pointer
        return
      end
      unreachable
    )
    (func $<alloc::vec::Vec<T,A> as core::ops::drop::Drop>::drop (;27;) (type 11) (param i32))
    (func $<alloc::raw_vec::RawVec<T,A> as core::ops::drop::Drop>::drop (;28;) (type 11) (param i32)
      (local i32)
      block ;; label = @1
        local.get 0
        i32.load
        local.tee 1
        i32.eqz
        br_if 0 (;@1;)
        local.get 0
        i32.load offset=4
        i32.const 1
        local.get 1
        call $<alloc::alloc::Global as core::alloc::Allocator>::deallocate
      end
    )
    (func $alloc::raw_vec::RawVec<T,A>::try_allocate_in (;29;) (type 7) (param i32 i32 i32)
      (local i32 i32)
      global.get $__stack_pointer
      i32.const 16
      i32.sub
      local.tee 3
      global.set $__stack_pointer
      block ;; label = @1
        block ;; label = @2
          local.get 1
          br_if 0 (;@2;)
          local.get 0
          i64.const 4294967296
          i64.store offset=4 align=4
          i32.const 0
          local.set 1
          br 1 (;@1;)
        end
        block ;; label = @2
          block ;; label = @3
            local.get 1
            i32.const -1
            i32.gt_s
            local.tee 4
            br_if 0 (;@3;)
            local.get 0
            i32.const 0
            i32.store offset=4
            br 1 (;@2;)
          end
          block ;; label = @3
            block ;; label = @4
              local.get 2
              br_if 0 (;@4;)
              local.get 3
              i32.const 8
              i32.add
              local.get 4
              local.get 1
              call $<alloc::alloc::Global as core::alloc::Allocator>::allocate
              local.get 3
              i32.load offset=8
              local.set 2
              br 1 (;@3;)
            end
            local.get 3
            local.get 4
            local.get 1
            i32.const 1
            call $alloc::alloc::Global::alloc_impl
            local.get 3
            i32.load
            local.set 2
          end
          block ;; label = @3
            local.get 2
            i32.eqz
            br_if 0 (;@3;)
            local.get 0
            local.get 2
            i32.store offset=8
            local.get 0
            local.get 1
            i32.store offset=4
            i32.const 0
            local.set 1
            br 2 (;@1;)
          end
          local.get 0
          local.get 1
          i32.store offset=8
          local.get 0
          local.get 4
          i32.store offset=4
        end
        i32.const 1
        local.set 1
      end
      local.get 0
      local.get 1
      i32.store
      local.get 3
      i32.const 16
      i32.add
      global.set $__stack_pointer
    )
    (func $<alloc::alloc::Global as core::alloc::Allocator>::allocate (;30;) (type 7) (param i32 i32 i32)
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
    (func $alloc::alloc::Global::alloc_impl (;31;) (type 16) (param i32 i32 i32 i32)
      block ;; label = @1
        local.get 2
        i32.eqz
        br_if 0 (;@1;)
        block ;; label = @2
          local.get 3
          br_if 0 (;@2;)
          i32.const 0
          i32.load8_u offset=1048636
          drop
          local.get 2
          local.get 1
          call $__rust_alloc
          local.set 1
          br 1 (;@1;)
        end
        local.get 2
        local.get 1
        call $__rust_alloc_zeroed
        local.set 1
      end
      local.get 0
      local.get 2
      i32.store offset=4
      local.get 0
      local.get 1
      i32.store
    )
    (func $alloc::raw_vec::RawVec<T,A>::shrink_unchecked (;32;) (type 7) (param i32 i32 i32)
      (local i32 i32 i32 i32)
      i32.const -2147483647
      local.set 3
      block ;; label = @1
        local.get 1
        i32.load
        local.tee 4
        i32.eqz
        br_if 0 (;@1;)
        local.get 1
        i32.load offset=4
        local.set 5
        block ;; label = @2
          block ;; label = @3
            local.get 2
            br_if 0 (;@3;)
            i32.const 1
            local.set 6
            local.get 5
            i32.const 1
            local.get 4
            call $<alloc::alloc::Global as core::alloc::Allocator>::deallocate
            br 1 (;@2;)
          end
          i32.const 1
          local.set 3
          local.get 5
          local.get 4
          i32.const 1
          local.get 2
          call $__rust_realloc
          local.tee 6
          i32.eqz
          br_if 1 (;@1;)
        end
        local.get 1
        local.get 2
        i32.store
        local.get 1
        local.get 6
        i32.store offset=4
        i32.const -2147483647
        local.set 3
      end
      local.get 0
      local.get 2
      i32.store offset=4
      local.get 0
      local.get 3
      i32.store
    )
    (func $<alloc::alloc::Global as core::alloc::Allocator>::deallocate (;33;) (type 7) (param i32 i32 i32)
      block ;; label = @1
        local.get 2
        i32.eqz
        br_if 0 (;@1;)
        local.get 0
        local.get 2
        local.get 1
        call $__rust_dealloc
      end
    )
    (func $alloc::raw_vec::handle_error (;34;) (type 14) (param i32 i32)
      unreachable
    )
    (func $cabi_realloc (;35;) (type 8) (param i32 i32 i32 i32) (result i32)
      local.get 0
      local.get 1
      local.get 2
      local.get 3
      call $cabi_realloc_wit_bindgen_0_28_0
    )
    (table (;0;) 3 3 funcref)
    (memory (;0;) 17)
    (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
    (export "memory" (memory 0))
    (export "miden:basic-wallet/basic-wallet@1.0.0#receive-asset" (func $miden:basic-wallet/basic-wallet@1.0.0#receive-asset))
    (export "miden:basic-wallet/basic-wallet@1.0.0#send-asset" (func $miden:basic-wallet/basic-wallet@1.0.0#send-asset))
    (export "miden:basic-wallet/aux@1.0.0#test-felt-intrinsics" (func $miden:basic-wallet/aux@1.0.0#test-felt-intrinsics))
    (export "miden:basic-wallet/aux@1.0.0#test-stdlib" (func $miden:basic-wallet/aux@1.0.0#test-stdlib))
    (export "miden:basic-wallet/aux@1.0.0#process-list-felt" (func $miden:basic-wallet/aux@1.0.0#process-list-felt))
    (export "cabi_post_miden:basic-wallet/aux@1.0.0#process-list-felt" (func $cabi_post_miden:basic-wallet/aux@1.0.0#process-list-felt))
    (export "miden:basic-wallet/aux@1.0.0#process-core-asset" (func $miden:basic-wallet/aux@1.0.0#process-core-asset))
    (export "cabi_post_miden:basic-wallet/aux@1.0.0#test-stdlib" (func $cabi_post_miden:basic-wallet/aux@1.0.0#process-list-felt))
    (export "cabi_realloc_wit_bindgen_0_28_0" (func $cabi_realloc_wit_bindgen_0_28_0))
    (export "cabi_realloc" (func $cabi_realloc))
    (elem (;0;) (i32.const 1) func $basic_wallet::bindings::__link_custom_section_describing_imports $cabi_realloc)
    (data $.rodata (;0;) (i32.const 1048576) "\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00\02\00\00\00")
  )
  (alias export 2 "add" (func (;0;)))
  (core func (;0;) (canon lower (func 0)))
  (core instance (;0;)
    (export "add" (func 0))
  )
  (alias export 3 "hash-one-to-one" (func (;1;)))
  (core func (;1;) (canon lower (func 1)))
  (core instance (;1;)
    (export "hash-one-to-one" (func 1))
  )
  (alias export 1 "heap-base" (func (;2;)))
  (core func (;2;) (canon lower (func 2)))
  (core instance (;2;)
    (export "heap-base" (func 2))
  )
  (alias export 4 "add-asset" (func (;3;)))
  (core func (;3;) (canon lower (func 3)))
  (alias export 4 "remove-asset" (func (;4;)))
  (core func (;4;) (canon lower (func 4)))
  (core instance (;3;)
    (export "add-asset" (func 3))
    (export "remove-asset" (func 4))
  )
  (alias export 5 "create-note" (func (;5;)))
  (core func (;5;) (canon lower (func 5)))
  (core instance (;4;)
    (export "create-note" (func 5))
  )
  (core instance (;5;) (instantiate 0
      (with "miden:core-import/intrinsics-felt@1.0.0" (instance 0))
      (with "miden:core-import/stdlib-crypto-hashes-blake3@1.0.0" (instance 1))
      (with "miden:core-import/intrinsics-mem@1.0.0" (instance 2))
      (with "miden:core-import/account@1.0.0" (instance 3))
      (with "miden:core-import/tx@1.0.0" (instance 4))
    )
  )
  (alias core export 5 "memory" (core memory (;0;)))
  (alias core export 5 "cabi_realloc" (core func (;6;)))
  (alias export 0 "core-asset" (type (;6;)))
  (type (;7;) (func (param "core-asset" 6)))
  (alias core export 5 "miden:basic-wallet/basic-wallet@1.0.0#receive-asset" (core func (;7;)))
  (func (;6;) (type 7) (canon lift (core func 7)))
  (alias export 0 "tag" (type (;8;)))
  (alias export 0 "note-type" (type (;9;)))
  (alias export 0 "recipient" (type (;10;)))
  (type (;11;) (func (param "core-asset" 6) (param "tag" 8) (param "note-type" 9) (param "recipient" 10)))
  (alias core export 5 "miden:basic-wallet/basic-wallet@1.0.0#send-asset" (core func (;8;)))
  (func (;7;) (type 11) (canon lift (core func 8)))
  (alias export 0 "felt" (type (;12;)))
  (alias export 0 "word" (type (;13;)))
  (alias export 0 "core-asset" (type (;14;)))
  (alias export 0 "tag" (type (;15;)))
  (alias export 0 "recipient" (type (;16;)))
  (alias export 0 "note-type" (type (;17;)))
  (component (;0;)
    (type (;0;) (record (field "inner" float32)))
    (import "import-type-felt" (type (;1;) (eq 0)))
    (type (;2;) (tuple 1 1 1 1))
    (type (;3;) (record (field "inner" 2)))
    (import "import-type-word" (type (;4;) (eq 3)))
    (type (;5;) (record (field "inner" 4)))
    (import "import-type-core-asset" (type (;6;) (eq 5)))
    (type (;7;) (record (field "inner" 1)))
    (import "import-type-tag" (type (;8;) (eq 7)))
    (type (;9;) (record (field "inner" 4)))
    (import "import-type-recipient" (type (;10;) (eq 9)))
    (type (;11;) (record (field "inner" 1)))
    (import "import-type-note-type" (type (;12;) (eq 11)))
    (import "import-type-core-asset0" (type (;13;) (eq 6)))
    (type (;14;) (func (param "core-asset" 13)))
    (import "import-func-receive-asset" (func (;0;) (type 14)))
    (import "import-type-tag0" (type (;15;) (eq 8)))
    (import "import-type-note-type0" (type (;16;) (eq 12)))
    (import "import-type-recipient0" (type (;17;) (eq 10)))
    (type (;18;) (func (param "core-asset" 13) (param "tag" 15) (param "note-type" 16) (param "recipient" 17)))
    (import "import-func-send-asset" (func (;1;) (type 18)))
    (export (;19;) "core-asset" (type 6))
    (export (;20;) "tag" (type 8))
    (export (;21;) "recipient" (type 10))
    (export (;22;) "note-type" (type 12))
    (export (;23;) "felt" (type 1))
    (type (;24;) (func (param "core-asset" 19)))
    (export (;2;) "receive-asset" (func 0) (func (type 24)))
    (type (;25;) (func (param "core-asset" 19) (param "tag" 20) (param "note-type" 22) (param "recipient" 21)))
    (export (;3;) "send-asset" (func 1) (func (type 25)))
  )
  (instance (;6;) (instantiate 0
      (with "import-func-receive-asset" (func 6))
      (with "import-func-send-asset" (func 7))
      (with "import-type-felt" (type 12))
      (with "import-type-word" (type 13))
      (with "import-type-core-asset" (type 14))
      (with "import-type-tag" (type 15))
      (with "import-type-recipient" (type 16))
      (with "import-type-note-type" (type 17))
      (with "import-type-core-asset0" (type 6))
      (with "import-type-tag0" (type 8))
      (with "import-type-note-type0" (type 9))
      (with "import-type-recipient0" (type 10))
    )
  )
  (export (;7;) "miden:basic-wallet/basic-wallet@1.0.0" (instance 6))
  (alias export 0 "felt" (type (;18;)))
  (type (;19;) (func (param "a" 18) (param "b" 18) (result 18)))
  (alias core export 5 "miden:basic-wallet/aux@1.0.0#test-felt-intrinsics" (core func (;9;)))
  (func (;8;) (type 19) (canon lift (core func 9)))
  (type (;20;) (list u8))
  (type (;21;) (func (param "input" 20) (result 20)))
  (alias core export 5 "miden:basic-wallet/aux@1.0.0#test-stdlib" (core func (;10;)))
  (alias core export 5 "cabi_post_miden:basic-wallet/aux@1.0.0#test-stdlib" (core func (;11;)))
  (func (;9;) (type 21) (canon lift (core func 10) (memory 0) (realloc 6) (post-return 11)))
  (type (;22;) (list 18))
  (type (;23;) (func (param "input" 22) (result 22)))
  (alias core export 5 "miden:basic-wallet/aux@1.0.0#process-list-felt" (core func (;12;)))
  (alias core export 5 "cabi_post_miden:basic-wallet/aux@1.0.0#process-list-felt" (core func (;13;)))
  (func (;10;) (type 23) (canon lift (core func 12) (memory 0) (realloc 6) (post-return 13)))
  (type (;24;) (func (param "input" 6) (result 6)))
  (alias core export 5 "miden:basic-wallet/aux@1.0.0#process-core-asset" (core func (;14;)))
  (func (;11;) (type 24) (canon lift (core func 14) (memory 0)))
  (component (;1;)
    (type (;0;) (record (field "inner" float32)))
    (import "import-type-felt" (type (;1;) (eq 0)))
    (type (;2;) (tuple 1 1 1 1))
    (type (;3;) (record (field "inner" 2)))
    (import "import-type-word" (type (;4;) (eq 3)))
    (type (;5;) (record (field "inner" 4)))
    (import "import-type-core-asset" (type (;6;) (eq 5)))
    (type (;7;) (record (field "inner" 1)))
    (import "import-type-tag" (type (;8;) (eq 7)))
    (type (;9;) (record (field "inner" 4)))
    (import "import-type-recipient" (type (;10;) (eq 9)))
    (type (;11;) (record (field "inner" 1)))
    (import "import-type-note-type" (type (;12;) (eq 11)))
    (import "import-type-felt0" (type (;13;) (eq 1)))
    (type (;14;) (func (param "a" 13) (param "b" 13) (result 13)))
    (import "import-func-test-felt-intrinsics" (func (;0;) (type 14)))
    (type (;15;) (list u8))
    (type (;16;) (func (param "input" 15) (result 15)))
    (import "import-func-test-stdlib" (func (;1;) (type 16)))
    (type (;17;) (list 13))
    (type (;18;) (func (param "input" 17) (result 17)))
    (import "import-func-process-list-felt" (func (;2;) (type 18)))
    (import "import-type-core-asset0" (type (;19;) (eq 6)))
    (type (;20;) (func (param "input" 19) (result 19)))
    (import "import-func-process-core-asset" (func (;3;) (type 20)))
    (export (;21;) "core-asset" (type 6))
    (export (;22;) "tag" (type 8))
    (export (;23;) "recipient" (type 10))
    (export (;24;) "note-type" (type 12))
    (export (;25;) "felt" (type 1))
    (type (;26;) (func (param "a" 25) (param "b" 25) (result 25)))
    (export (;4;) "test-felt-intrinsics" (func 0) (func (type 26)))
    (type (;27;) (list u8))
    (type (;28;) (func (param "input" 27) (result 27)))
    (export (;5;) "test-stdlib" (func 1) (func (type 28)))
    (type (;29;) (list 25))
    (type (;30;) (func (param "input" 29) (result 29)))
    (export (;6;) "process-list-felt" (func 2) (func (type 30)))
    (type (;31;) (func (param "input" 21) (result 21)))
    (export (;7;) "process-core-asset" (func 3) (func (type 31)))
  )
  (instance (;8;) (instantiate 1
      (with "import-func-test-felt-intrinsics" (func 8))
      (with "import-func-test-stdlib" (func 9))
      (with "import-func-process-list-felt" (func 10))
      (with "import-func-process-core-asset" (func 11))
      (with "import-type-felt" (type 12))
      (with "import-type-word" (type 13))
      (with "import-type-core-asset" (type 14))
      (with "import-type-tag" (type 15))
      (with "import-type-recipient" (type 16))
      (with "import-type-note-type" (type 17))
      (with "import-type-felt0" (type 18))
      (with "import-type-core-asset0" (type 6))
    )
  )
  (export (;9;) "miden:basic-wallet/aux@1.0.0" (instance 8))
)