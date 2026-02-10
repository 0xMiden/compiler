(component
  (type $ty-miden:base/core-types@1.0.0 (;0;)
    (instance
      (type (;0;) (record (field "inner" f32)))
      (export (;1;) "felt" (type (eq 0)))
      (type (;2;) (tuple 1 1 1 1))
      (type (;3;) (record (field "inner" 2)))
      (export (;4;) "word" (type (eq 3)))
    )
  )
  (import "miden:base/core-types@1.0.0" (instance $miden:base/core-types@1.0.0 (;0;) (type $ty-miden:base/core-types@1.0.0)))
  (core module $main (;0;)
    (type (;0;) (func))
    (type (;1;) (func (param i32 i32) (result i32)))
    (type (;2;) (func (param i32 i32 i32)))
    (type (;3;) (func (param f32 f32 f32 f32)))
    (type (;4;) (func (param i32 i32 i32) (result i32)))
    (type (;5;) (func (result i32)))
    (type (;6;) (func (param i32)))
    (type (;7;) (func (param i32 i32 i32 i32)))
    (type (;8;) (func (param i32 i32)))
    (type (;9;) (func (param i32 i32 i32 i32 i32)))
    (type (;10;) (func (param i32) (result i32)))
    (type (;11;) (func (param f32 f32 f32 f32 i32)))
    (table (;0;) 2 2 funcref)
    (memory (;0;) 17)
    (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
    (global $GOT.data.internal.__memory_base (;1;) i32 i32.const 0)
    (export "memory" (memory 0))
    (export "miden:base/note-script@1.0.0#run" (func $miden:base/note-script@1.0.0#run))
    (elem (;0;) (i32.const 1) func $rust_sdk_note_add_assets_to_account_binding::bindings::__link_custom_section_describing_imports)
    (func $__wasm_call_ctors (;0;) (type 0))
    (func $__rustc::__rust_alloc (;1;) (type 1) (param i32 i32) (result i32)
      global.get $GOT.data.internal.__memory_base
      i32.const 1048584
      i32.add
      local.get 1
      local.get 0
      call $<miden_sdk_alloc::BumpAlloc as core::alloc::global::GlobalAlloc>::alloc
    )
    (func $__rustc::__rust_dealloc (;2;) (type 2) (param i32 i32 i32))
    (func $__rustc::__rust_alloc_zeroed (;3;) (type 1) (param i32 i32) (result i32)
      block ;; label = @1
        global.get $GOT.data.internal.__memory_base
        i32.const 1048584
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
    (func $rust_sdk_note_add_assets_to_account_binding::bindings::__link_custom_section_describing_imports (;4;) (type 0))
    (func $miden:base/note-script@1.0.0#run (;5;) (type 3) (param f32 f32 f32 f32)
      call $wit_bindgen::rt::run_ctors_once
      call $miden_base_sys::bindings::active_note::add_assets_to_account
    )
    (func $__rustc::__rust_no_alloc_shim_is_unstable_v2 (;6;) (type 0)
      return
    )
    (func $wit_bindgen::rt::run_ctors_once (;7;) (type 0)
      (local i32)
      block ;; label = @1
        global.get $GOT.data.internal.__memory_base
        i32.const 1048588
        i32.add
        i32.load8_u
        br_if 0 (;@1;)
        global.get $GOT.data.internal.__memory_base
        local.set 0
        call $__wasm_call_ctors
        local.get 0
        i32.const 1048588
        i32.add
        i32.const 1
        i32.store8
      end
    )
    (func $<miden_sdk_alloc::BumpAlloc as core::alloc::global::GlobalAlloc>::alloc (;8;) (type 4) (param i32 i32 i32) (result i32)
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
    (func $intrinsics::mem::heap_base (;9;) (type 5) (result i32)
      unreachable
    )
    (func $<alloc::vec::Vec<miden_base_sys::bindings::types::Asset>>::with_capacity (;10;) (type 6) (param i32)
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
    (func $<alloc::raw_vec::RawVecInner>::with_capacity_in (;11;) (type 7) (param i32 i32 i32 i32)
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
    (func $miden_base_sys::bindings::active_note::get_assets (;12;) (type 6) (param i32)
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
    (func $miden_base_sys::bindings::active_note::add_assets_to_account (;13;) (type 0)
      (local i32 i32 i32 i32)
      global.get $__stack_pointer
      i32.const 48
      i32.sub
      local.tee 0
      global.set $__stack_pointer
      local.get 0
      i32.const 32
      i32.add
      call $miden_base_sys::bindings::active_note::get_assets
      local.get 0
      i32.load offset=32
      local.set 1
      local.get 0
      local.get 0
      i32.load offset=36
      local.tee 2
      local.get 0
      i32.load offset=40
      i32.const 4
      i32.shl
      local.tee 3
      i32.add
      i32.store offset=12
      local.get 0
      local.get 1
      i32.store offset=8
      local.get 0
      local.get 2
      i32.store offset=4
      local.get 0
      local.get 2
      i32.store
      block ;; label = @1
        loop ;; label = @2
          local.get 3
          i32.eqz
          br_if 1 (;@1;)
          local.get 0
          local.get 2
          i32.const 16
          i32.add
          local.tee 1
          i32.store offset=4
          local.get 0
          local.get 2
          i64.load
          i64.store offset=16
          local.get 0
          local.get 2
          i32.const 8
          i32.add
          i64.load
          i64.store offset=24
          local.get 3
          i32.const -16
          i32.add
          local.set 3
          local.get 0
          i32.const 32
          i32.add
          local.get 0
          i32.const 16
          i32.add
          call $miden_base_sys::bindings::native_account::add_asset
          local.get 1
          local.set 2
          br 0 (;@2;)
        end
      end
      local.get 0
      call $<alloc::vec::into_iter::IntoIter<miden_base_sys::bindings::types::Asset> as core::ops::drop::Drop>::drop
      local.get 0
      i32.const 48
      i32.add
      global.set $__stack_pointer
    )
    (func $miden_base_sys::bindings::native_account::add_asset (;14;) (type 8) (param i32 i32)
      (local i32)
      global.get $__stack_pointer
      i32.const 16
      i32.sub
      local.tee 2
      global.set $__stack_pointer
      local.get 1
      f32.load offset=12
      local.get 1
      f32.load offset=8
      local.get 1
      f32.load offset=4
      local.get 1
      f32.load
      local.get 2
      call $miden::protocol::native_account::add_asset
      local.get 0
      local.get 2
      i64.load
      i64.const 32
      i64.rotl
      i64.store offset=8
      local.get 0
      local.get 2
      i64.load offset=8
      i64.const 32
      i64.rotl
      i64.store
      local.get 2
      i32.const 16
      i32.add
      global.set $__stack_pointer
    )
    (func $<alloc::vec::into_iter::IntoIter<miden_base_sys::bindings::types::Asset> as core::ops::drop::Drop>::drop (;15;) (type 6) (param i32)
      (local i32)
      global.get $__stack_pointer
      i32.const 16
      i32.sub
      local.tee 1
      global.set $__stack_pointer
      local.get 1
      local.get 0
      i32.store offset=12
      local.get 1
      i32.const 12
      i32.add
      call $<<alloc::vec::into_iter::IntoIter<_, _> as core::ops::drop::Drop>::drop::DropGuard<miden_base_sys::bindings::types::Asset, alloc::alloc::Global> as core::ops::drop::Drop>::drop
      local.get 1
      i32.const 16
      i32.add
      global.set $__stack_pointer
    )
    (func $<<alloc::vec::into_iter::IntoIter<_, _> as core::ops::drop::Drop>::drop::DropGuard<miden_base_sys::bindings::types::Asset, alloc::alloc::Global> as core::ops::drop::Drop>::drop (;16;) (type 6) (param i32)
      (local i32)
      global.get $__stack_pointer
      i32.const 16
      i32.sub
      local.tee 1
      global.set $__stack_pointer
      local.get 1
      local.get 0
      i32.load
      local.tee 0
      i32.load
      i32.store offset=12
      local.get 1
      local.get 0
      i32.load offset=8
      i32.store offset=8
      local.get 1
      i32.const 8
      i32.add
      call $<alloc::raw_vec::RawVec<miden_base_sys::bindings::types::Asset> as core::ops::drop::Drop>::drop
      local.get 1
      i32.const 16
      i32.add
      global.set $__stack_pointer
    )
    (func $<alloc::raw_vec::RawVec<miden_base_sys::bindings::types::Asset> as core::ops::drop::Drop>::drop (;17;) (type 6) (param i32)
      local.get 0
      i32.const 16
      i32.const 16
      call $<alloc::raw_vec::RawVecInner>::deallocate
    )
    (func $<alloc::alloc::Global>::alloc_impl (;18;) (type 7) (param i32 i32 i32 i32)
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
    (func $<alloc::raw_vec::RawVecInner>::deallocate (;19;) (type 2) (param i32 i32 i32)
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
    (func $<alloc::raw_vec::RawVecInner>::current_memory (;20;) (type 7) (param i32 i32 i32 i32)
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
    (func $<alloc::alloc::Global as core::alloc::Allocator>::deallocate (;21;) (type 2) (param i32 i32 i32)
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
    (func $<alloc::alloc::Global as core::alloc::Allocator>::allocate (;22;) (type 2) (param i32 i32 i32)
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
    (func $<alloc::raw_vec::RawVecInner>::try_allocate_in (;23;) (type 9) (param i32 i32 i32 i32 i32)
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
    (func $alloc::raw_vec::handle_error (;24;) (type 8) (param i32 i32)
      unreachable
    )
    (func $<core::ptr::alignment::Alignment>::max (;25;) (type 1) (param i32 i32) (result i32)
      local.get 0
      local.get 1
      local.get 0
      local.get 1
      i32.gt_u
      select
    )
    (func $miden::protocol::active_note::get_assets (;26;) (type 10) (param i32) (result i32)
      unreachable
    )
    (func $miden::protocol::native_account::add_asset (;27;) (type 11) (param f32 f32 f32 f32 i32)
      unreachable
    )
    (data $.data (;0;) (i32.const 1048576) "\01\00\00\00\01\00\00\00")
  )
  (alias export $miden:base/core-types@1.0.0 "word" (type $word (;1;)))
  (core instance $main (;0;) (instantiate $main))
  (alias core export $main "memory" (core memory $memory (;0;)))
  (type (;2;) (func (param "arg" $word)))
  (alias core export $main "miden:base/note-script@1.0.0#run" (core func $miden:base/note-script@1.0.0#run (;0;)))
  (func $run (;0;) (type 2) (canon lift (core func $miden:base/note-script@1.0.0#run)))
  (alias export $miden:base/core-types@1.0.0 "felt" (type $felt (;3;)))
  (alias export $miden:base/core-types@1.0.0 "word" (type $"#type4 word" (@name "word") (;4;)))
  (component $miden:base/note-script@1.0.0-shim-component (;0;)
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
  (instance $miden:base/note-script@1.0.0-shim-instance (;1;) (instantiate $miden:base/note-script@1.0.0-shim-component
      (with "import-func-run" (func $run))
      (with "import-type-felt" (type $felt))
      (with "import-type-word" (type $"#type4 word"))
      (with "import-type-word0" (type $word))
    )
  )
  (export $miden:base/note-script@1.0.0 (;2;) "miden:base/note-script@1.0.0" (instance $miden:base/note-script@1.0.0-shim-instance))
)
