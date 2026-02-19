(component
  (type $ty-miden:base/core-types@1.0.0 (;0;)
    (instance
      (type (;0;) (record (field "inner" f32)))
      (export (;1;) "felt" (type (eq 0)))
      (type (;2;) (record (field "a" 1) (field "b" 1) (field "c" 1) (field "d" 1)))
      (export (;3;) "word" (type (eq 2)))
      (type (;4;) (record (field "inner" 3)))
      (export (;5;) "asset" (type (eq 4)))
    )
  )
  (import "miden:base/core-types@1.0.0" (instance $miden:base/core-types@1.0.0 (;0;) (type $ty-miden:base/core-types@1.0.0)))
  (alias export $miden:base/core-types@1.0.0 "asset" (type $asset (;1;)))
  (type $ty-miden:basic-wallet/basic-wallet@0.1.0 (;2;)
    (instance
      (alias outer 1 $asset (type (;0;)))
      (export (;1;) "asset" (type (eq 0)))
      (type (;2;) (func (param "asset" 1)))
      (export (;0;) "receive-asset" (func (type 2)))
    )
  )
  (import "miden:basic-wallet/basic-wallet@0.1.0" (instance $miden:basic-wallet/basic-wallet@0.1.0 (;1;) (type $ty-miden:basic-wallet/basic-wallet@0.1.0)))
  (core module $main (;0;)
    (type (;0;) (func (param f32 f32 f32 f32)))
    (type (;1;) (func))
    (type (;2;) (func (param i32 i32) (result i32)))
    (type (;3;) (func (param i32 i32 i32)))
    (type (;4;) (func (param i32 i32 i32) (result i32)))
    (type (;5;) (func (result i32)))
    (type (;6;) (func (param i32)))
    (type (;7;) (func (param i32 i32 i32 i32)))
    (type (;8;) (func (param f32 f32) (result i32)))
    (type (;9;) (func (param i32 i32 i32 i32 i32)))
    (type (;10;) (func (param i32 i32)))
    (type (;11;) (func (param i32) (result i32)))
    (import "miden:basic-wallet/basic-wallet@0.1.0" "receive-asset" (func $p2id::bindings::miden::basic_wallet::basic_wallet::receive_asset::wit_import6 (;0;) (type 0)))
    (table (;0;) 2 2 funcref)
    (memory (;0;) 17)
    (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
    (global $GOT.data.internal.__memory_base (;1;) i32 i32.const 0)
    (export "memory" (memory 0))
    (export "miden:base/note-script@1.0.0#run" (func $miden:base/note-script@1.0.0#run))
    (elem (;0;) (i32.const 1) func $p2id::bindings::__link_custom_section_describing_imports)
    (func $__wasm_call_ctors (;1;) (type 1))
    (func $__rustc::__rust_alloc (;2;) (type 2) (param i32 i32) (result i32)
      global.get $GOT.data.internal.__memory_base
      i32.const 1048588
      i32.add
      local.get 1
      local.get 0
      call $<miden_sdk_alloc::BumpAlloc as core::alloc::global::GlobalAlloc>::alloc
    )
    (func $__rustc::__rust_dealloc (;3;) (type 3) (param i32 i32 i32))
    (func $__rustc::__rust_alloc_zeroed (;4;) (type 2) (param i32 i32) (result i32)
      block ;; label = @1
        global.get $GOT.data.internal.__memory_base
        i32.const 1048588
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
    (func $p2id::bindings::__link_custom_section_describing_imports (;5;) (type 1))
    (func $miden:base/note-script@1.0.0#run (;6;) (type 0) (param f32 f32 f32 f32)
      (local i32 i32 f32 f32 f32 i32 i32 i32)
      global.get $__stack_pointer
      i32.const 32
      i32.sub
      local.tee 4
      global.set $__stack_pointer
      call $wit_bindgen::rt::run_ctors_once
      local.get 4
      i32.const 12
      i32.add
      call $miden_base_sys::bindings::active_note::get_inputs
      block ;; label = @1
        block ;; label = @2
          local.get 4
          i32.load offset=20
          br_table 1 (;@1;) 1 (;@1;) 0 (;@2;)
        end
        local.get 4
        i32.load offset=16
        local.tee 5
        f32.load offset=4
        local.set 6
        local.get 5
        f32.load
        local.set 7
        local.get 4
        i32.const 12
        i32.add
        call $<alloc::vec::Vec<miden_field::wasm_miden::Felt> as core::ops::drop::Drop>::drop
        local.get 4
        i32.const 12
        i32.add
        call $<alloc::raw_vec::RawVec<miden_field::wasm_miden::Felt> as core::ops::drop::Drop>::drop
        local.get 4
        call $miden_base_sys::bindings::active_account::get_id
        local.get 4
        f32.load offset=4
        local.set 8
        local.get 4
        f32.load
        local.get 7
        call $intrinsics::felt::eq
        i32.const 1
        i32.ne
        br_if 0 (;@1;)
        local.get 8
        local.get 6
        call $intrinsics::felt::eq
        i32.const 1
        i32.ne
        br_if 0 (;@1;)
        local.get 4
        i32.const 12
        i32.add
        call $miden_base_sys::bindings::active_note::get_assets
        local.get 4
        i32.load offset=20
        i32.const 4
        i32.shl
        local.set 9
        local.get 4
        i32.load offset=12
        local.set 10
        local.get 4
        i32.load offset=16
        local.tee 11
        local.set 5
        block ;; label = @2
          loop ;; label = @3
            local.get 9
            i32.eqz
            br_if 1 (;@2;)
            local.get 5
            f32.load
            local.get 5
            f32.load offset=4
            local.get 5
            f32.load offset=8
            local.get 5
            f32.load offset=12
            call $p2id::bindings::miden::basic_wallet::basic_wallet::receive_asset::wit_import6
            local.get 9
            i32.const -16
            i32.add
            local.set 9
            local.get 5
            i32.const 16
            i32.add
            local.set 5
            br 0 (;@3;)
          end
        end
        local.get 4
        local.get 11
        i32.store offset=28
        local.get 4
        local.get 10
        i32.store offset=24
        local.get 4
        i32.const 24
        i32.add
        i32.const 16
        i32.const 16
        call $<alloc::raw_vec::RawVecInner>::deallocate
        local.get 4
        i32.const 32
        i32.add
        global.set $__stack_pointer
        return
      end
      unreachable
    )
    (func $__rustc::__rust_no_alloc_shim_is_unstable_v2 (;7;) (type 1)
      return
    )
    (func $wit_bindgen::rt::run_ctors_once (;8;) (type 1)
      (local i32)
      block ;; label = @1
        global.get $GOT.data.internal.__memory_base
        i32.const 1048592
        i32.add
        i32.load8_u
        br_if 0 (;@1;)
        global.get $GOT.data.internal.__memory_base
        local.set 0
        call $__wasm_call_ctors
        local.get 0
        i32.const 1048592
        i32.add
        i32.const 1
        i32.store8
      end
    )
    (func $<miden_sdk_alloc::BumpAlloc as core::alloc::global::GlobalAlloc>::alloc (;9;) (type 4) (param i32 i32 i32) (result i32)
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
    (func $intrinsics::mem::heap_base (;10;) (type 5) (result i32)
      unreachable
    )
    (func $<alloc::vec::Vec<miden_base_sys::bindings::types::Asset>>::with_capacity (;11;) (type 6) (param i32)
      (local i32 i64)
      global.get $__stack_pointer
      i32.const 16
      i32.sub
      local.tee 1
      global.set $__stack_pointer
      local.get 1
      i32.const 8
      i32.add
      i32.const 256
      i32.const 16
      i32.const 16
      call $<alloc::raw_vec::RawVecInner>::with_capacity_in
      local.get 1
      i64.load offset=8
      local.set 2
      local.get 0
      i32.const 0
      i32.store offset=8
      local.get 0
      local.get 2
      i64.store align=4
      local.get 1
      i32.const 16
      i32.add
      global.set $__stack_pointer
    )
    (func $<alloc::raw_vec::RawVecInner>::with_capacity_in (;12;) (type 7) (param i32 i32 i32 i32)
      (local i32)
      global.get $__stack_pointer
      i32.const 16
      i32.sub
      local.tee 4
      global.set $__stack_pointer
      local.get 4
      i32.const 4
      i32.add
      local.get 1
      i32.const 0
      local.get 2
      local.get 3
      call $<alloc::raw_vec::RawVecInner>::try_allocate_in
      local.get 4
      i32.load offset=8
      local.set 3
      block ;; label = @1
        local.get 4
        i32.load offset=4
        i32.const 1
        i32.ne
        br_if 0 (;@1;)
        local.get 3
        local.get 4
        i32.load offset=12
        call $alloc::raw_vec::handle_error
        unreachable
      end
      local.get 0
      local.get 4
      i32.load offset=12
      i32.store offset=4
      local.get 0
      local.get 3
      i32.store
      local.get 4
      i32.const 16
      i32.add
      global.set $__stack_pointer
    )
    (func $<alloc::raw_vec::RawVec<miden_field::wasm_miden::Felt> as core::ops::drop::Drop>::drop (;13;) (type 6) (param i32)
      local.get 0
      i32.const 4
      i32.const 4
      call $<alloc::raw_vec::RawVecInner>::deallocate
    )
    (func $miden_base_sys::bindings::active_note::get_assets (;14;) (type 6) (param i32)
      (local i32)
      global.get $__stack_pointer
      i32.const 16
      i32.sub
      local.tee 1
      global.set $__stack_pointer
      local.get 1
      i32.const 4
      i32.add
      call $<alloc::vec::Vec<miden_base_sys::bindings::types::Asset>>::with_capacity
      local.get 0
      i32.const 8
      i32.add
      local.get 1
      i32.load offset=8
      i32.const 2
      i32.shr_u
      call $miden::protocol::active_note::get_assets
      i32.store
      local.get 0
      local.get 1
      i64.load offset=4 align=4
      i64.store align=4
      local.get 1
      i32.const 16
      i32.add
      global.set $__stack_pointer
    )
    (func $miden_base_sys::bindings::active_note::get_inputs (;15;) (type 6) (param i32)
      (local i32 i32 i32)
      global.get $__stack_pointer
      i32.const 16
      i32.sub
      local.tee 1
      global.set $__stack_pointer
      local.get 1
      i32.const 8
      i32.add
      i32.const 1024
      i32.const 4
      i32.const 4
      call $<alloc::raw_vec::RawVecInner>::with_capacity_in
      local.get 1
      i32.load offset=8
      local.set 2
      local.get 0
      local.get 1
      i32.load offset=12
      local.tee 3
      i32.const 2
      i32.shr_u
      call $miden::protocol::active_note::get_inputs
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
    (func $miden_base_sys::bindings::active_account::get_id (;16;) (type 6) (param i32)
      (local i32)
      global.get $__stack_pointer
      i32.const 16
      i32.sub
      local.tee 1
      global.set $__stack_pointer
      local.get 1
      i32.const 8
      i32.add
      call $miden::protocol::active_account::get_id
      local.get 0
      local.get 1
      i64.load offset=8 align=4
      i64.store
      local.get 1
      i32.const 16
      i32.add
      global.set $__stack_pointer
    )
    (func $<alloc::vec::Vec<miden_field::wasm_miden::Felt> as core::ops::drop::Drop>::drop (;17;) (type 6) (param i32))
    (func $intrinsics::felt::eq (;18;) (type 8) (param f32 f32) (result i32)
      unreachable
    )
    (func $<alloc::alloc::Global>::alloc_impl (;19;) (type 7) (param i32 i32 i32 i32)
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
    (func $<alloc::raw_vec::RawVecInner>::deallocate (;20;) (type 3) (param i32 i32 i32)
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
    (func $<alloc::raw_vec::RawVecInner>::current_memory (;21;) (type 7) (param i32 i32 i32 i32)
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
    (func $<alloc::alloc::Global as core::alloc::Allocator>::deallocate (;22;) (type 3) (param i32 i32 i32)
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
    (func $<alloc::alloc::Global as core::alloc::Allocator>::allocate (;23;) (type 3) (param i32 i32 i32)
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
    (func $<alloc::raw_vec::RawVecInner>::try_allocate_in (;24;) (type 9) (param i32 i32 i32 i32 i32)
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
    (func $alloc::raw_vec::handle_error (;25;) (type 10) (param i32 i32)
      unreachable
    )
    (func $<core::ptr::alignment::Alignment>::max (;26;) (type 2) (param i32 i32) (result i32)
      local.get 0
      local.get 1
      local.get 0
      local.get 1
      i32.gt_u
      select
    )
    (func $miden::protocol::active_account::get_id (;27;) (type 6) (param i32)
      unreachable
    )
    (func $miden::protocol::active_note::get_assets (;28;) (type 11) (param i32) (result i32)
      unreachable
    )
    (func $miden::protocol::active_note::get_inputs (;29;) (type 11) (param i32) (result i32)
      unreachable
    )
    (data $.data (;0;) (i32.const 1048576) "\01\00\00\00\01\00\00\00\01\00\00\00")
  )
  (alias export $miden:base/core-types@1.0.0 "word" (type $word (;3;)))
  (alias export $miden:basic-wallet/basic-wallet@0.1.0 "receive-asset" (func $receive-asset (;0;)))
  (core func $receive-asset (;0;) (canon lower (func $receive-asset)))
  (core instance $miden:basic-wallet/basic-wallet@0.1.0 (;0;)
    (export "receive-asset" (func $receive-asset))
  )
  (core instance $main (;1;) (instantiate $main
      (with "miden:basic-wallet/basic-wallet@0.1.0" (instance $miden:basic-wallet/basic-wallet@0.1.0))
    )
  )
  (alias core export $main "memory" (core memory $memory (;0;)))
  (type (;4;) (func (param "arg" $word)))
  (alias core export $main "miden:base/note-script@1.0.0#run" (core func $miden:base/note-script@1.0.0#run (;1;)))
  (func $run (;1;) (type 4) (canon lift (core func $miden:base/note-script@1.0.0#run)))
  (alias export $miden:base/core-types@1.0.0 "felt" (type $felt (;5;)))
  (alias export $miden:base/core-types@1.0.0 "word" (type $"#type6 word" (@name "word") (;6;)))
  (component $miden:base/note-script@1.0.0-shim-component (;0;)
    (type (;0;) (record (field "inner" f32)))
    (import "import-type-felt" (type (;1;) (eq 0)))
    (type (;2;) (record (field "a" 1) (field "b" 1) (field "c" 1) (field "d" 1)))
    (import "import-type-word" (type (;3;) (eq 2)))
    (import "import-type-word0" (type (;4;) (eq 3)))
    (type (;5;) (func (param "arg" 4)))
    (import "import-func-run" (func (;0;) (type 5)))
    (export (;6;) "word" (type 3))
    (type (;7;) (func (param "arg" 6)))
    (export (;1;) "run" (func 0) (func (type 7)))
  )
  (instance $miden:base/note-script@1.0.0-shim-instance (;2;) (instantiate $miden:base/note-script@1.0.0-shim-component
      (with "import-func-run" (func $run))
      (with "import-type-felt" (type $felt))
      (with "import-type-word" (type $"#type6 word"))
      (with "import-type-word0" (type $word))
    )
  )
  (export $miden:base/note-script@1.0.0 (;3;) "miden:base/note-script@1.0.0" (instance $miden:base/note-script@1.0.0-shim-instance))
)
