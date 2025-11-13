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
  (core module (;0;)
    (type (;0;) (func))
    (type (;1;) (func (param i32 i32) (result i32)))
    (type (;2;) (func (param i32 i32 i32)))
    (type (;3;) (func (param i32 i32 i32 i32) (result i32)))
    (type (;4;) (func (param i32)))
    (type (;5;) (func (param i32 i32 i32) (result i32)))
    (type (;6;) (func (result i32)))
    (type (;7;) (func (param i32) (result i32)))
    (type (;8;) (func (param i32 i32 i32 i32 i32)))
    (type (;9;) (func (param i32 i32 i32 i32)))
    (table (;0;) 2 2 funcref)
    (memory (;0;) 17)
    (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
    (global $GOT.data.internal.__memory_base (;1;) i32 i32.const 0)
    (export "memory" (memory 0))
    (export "miden:cm-types/cm-types@0.1.0#func-list" (func $miden:cm-types/cm-types@0.1.0#func-list))
    (export "cabi_post_miden:cm-types/cm-types@0.1.0#func-list" (func $cabi_post_miden:cm-types/cm-types@0.1.0#func-list))
    (export "cabi_realloc" (func $cabi_realloc))
    (elem (;0;) (i32.const 1) func $cm_types::bindings::__link_custom_section_describing_imports)
    (func $__wasm_call_ctors (;0;) (type 0))
    (func $__rustc::__rust_alloc (;1;) (type 1) (param i32 i32) (result i32)
      global.get $GOT.data.internal.__memory_base
      i32.const 1048612
      i32.add
      local.get 1
      local.get 0
      call $<miden_sdk_alloc::BumpAlloc as core::alloc::global::GlobalAlloc>::alloc
    )
    (func $__rustc::__rust_dealloc (;2;) (type 2) (param i32 i32 i32))
    (func $__rustc::__rust_realloc (;3;) (type 3) (param i32 i32 i32 i32) (result i32)
      block ;; label = @1
        global.get $GOT.data.internal.__memory_base
        i32.const 1048612
        i32.add
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
        global.get $GOT.data.internal.__memory_base
        i32.const 1048612
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
    (func $cm_types::bindings::__link_custom_section_describing_imports (;5;) (type 0))
    (func $miden:cm-types/cm-types@0.1.0#func-list (;6;) (type 1) (param i32 i32) (result i32)
      (local i32 i32 i32 i32 i32 i32)
      global.get $__stack_pointer
      i32.const 32
      i32.sub
      local.tee 2
      global.set $__stack_pointer
      call $wit_bindgen::rt::run_ctors_once
      local.get 1
      call $alloc::raw_vec::new_cap
      local.set 3
      i32.const 0
      local.set 4
      local.get 2
      i32.const 12
      i32.add
      local.get 1
      i32.const 0
      i32.const 2
      i32.const 2
      call $alloc::raw_vec::RawVecInner<A>::try_allocate_in
      local.get 2
      i32.load offset=16
      local.set 5
      block ;; label = @1
        block ;; label = @2
          local.get 2
          i32.load offset=12
          i32.const 1
          i32.eq
          br_if 0 (;@2;)
          local.get 2
          i32.load offset=20
          local.tee 6
          local.set 7
          block ;; label = @3
            loop ;; label = @4
              local.get 1
              local.get 4
              i32.eq
              br_if 1 (;@3;)
              local.get 7
              local.get 0
              local.get 4
              i32.add
              i32.load8_u
              i32.const 1
              i32.shl
              i32.store16
              local.get 7
              i32.const 2
              i32.add
              local.set 7
              local.get 4
              i32.const 1
              i32.add
              local.set 4
              br 0 (;@4;)
            end
          end
          local.get 3
          call $alloc::raw_vec::new_cap
          local.set 4
          local.get 2
          local.get 0
          i32.store offset=28
          local.get 2
          local.get 4
          i32.store offset=24
          local.get 2
          i32.const 24
          i32.add
          call $<alloc::raw_vec::RawVec<T,A> as core::ops::drop::Drop>::drop
          local.get 2
          local.get 6
          i32.store offset=16
          local.get 2
          local.get 5
          i32.store offset=12
          local.get 2
          local.get 1
          i32.store offset=20
          block ;; label = @3
            local.get 5
            local.get 1
            i32.le_u
            br_if 0 (;@3;)
            local.get 2
            local.get 2
            i32.const 12
            i32.add
            local.get 1
            i32.const 2
            i32.const 2
            call $alloc::raw_vec::RawVecInner<A>::shrink_unchecked
            local.get 2
            i32.load
            local.tee 4
            i32.const -2147483647
            i32.ne
            br_if 2 (;@1;)
            local.get 2
            i32.load offset=16
            local.set 6
            local.get 2
            i32.load offset=20
            local.set 1
          end
          global.get $GOT.data.internal.__memory_base
          i32.const 1048616
          i32.add
          local.tee 4
          local.get 6
          i32.store
          local.get 4
          local.get 1
          i32.store offset=4
          local.get 2
          i32.const 32
          i32.add
          global.set $__stack_pointer
          local.get 4
          return
        end
        global.get $GOT.data.internal.__memory_base
        local.set 4
        local.get 5
        local.get 2
        i32.load offset=20
        local.get 4
        i32.const 1048596
        i32.add
        call $alloc::raw_vec::handle_error
        unreachable
      end
      local.get 4
      local.get 2
      i32.load offset=4
      global.get $GOT.data.internal.__memory_base
      i32.const 1048596
      i32.add
      call $alloc::raw_vec::handle_error
      unreachable
    )
    (func $cabi_post_miden:cm-types/cm-types@0.1.0#func-list (;7;) (type 4) (param i32))
    (func $__rustc::__rust_no_alloc_shim_is_unstable_v2 (;8;) (type 0)
      return
    )
    (func $wit_bindgen::rt::run_ctors_once (;9;) (type 0)
      (local i32)
      block ;; label = @1
        global.get $GOT.data.internal.__memory_base
        i32.const 1048624
        i32.add
        i32.load8_u
        br_if 0 (;@1;)
        global.get $GOT.data.internal.__memory_base
        local.set 0
        call $__wasm_call_ctors
        local.get 0
        i32.const 1048624
        i32.add
        i32.const 1
        i32.store8
      end
    )
    (func $<miden_sdk_alloc::BumpAlloc as core::alloc::global::GlobalAlloc>::alloc (;10;) (type 5) (param i32 i32 i32) (result i32)
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
    (func $cabi_realloc (;11;) (type 3) (param i32 i32 i32 i32) (result i32)
      unreachable
    )
    (func $intrinsics::mem::heap_base (;12;) (type 6) (result i32)
      unreachable
    )
    (func $alloc::raw_vec::new_cap (;13;) (type 7) (param i32) (result i32)
      local.get 0
    )
    (func $<alloc::raw_vec::RawVec<T,A> as core::ops::drop::Drop>::drop (;14;) (type 4) (param i32)
      local.get 0
      i32.const 1
      i32.const 1
      call $alloc::raw_vec::RawVecInner<A>::deallocate
    )
    (func $alloc::raw_vec::RawVecInner<A>::deallocate (;15;) (type 2) (param i32 i32 i32)
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
    (func $alloc::raw_vec::RawVecInner<A>::try_allocate_in (;16;) (type 8) (param i32 i32 i32 i32 i32)
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
    (func $<alloc::alloc::Global as core::alloc::Allocator>::allocate (;17;) (type 2) (param i32 i32 i32)
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
    (func $alloc::alloc::Global::alloc_impl (;18;) (type 9) (param i32 i32 i32 i32)
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
    (func $alloc::raw_vec::RawVecInner<A>::current_memory (;19;) (type 9) (param i32 i32 i32 i32)
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
    (func $alloc::raw_vec::RawVecInner<A>::shrink_unchecked (;20;) (type 8) (param i32 i32 i32 i32 i32)
      (local i32 i32 i32 i32)
      global.get $__stack_pointer
      i32.const 16
      i32.sub
      local.tee 5
      global.set $__stack_pointer
      local.get 5
      i32.const 4
      i32.add
      local.get 1
      local.get 3
      local.get 4
      call $alloc::raw_vec::RawVecInner<A>::current_memory
      block ;; label = @1
        block ;; label = @2
          local.get 5
          i32.load offset=8
          local.tee 6
          i32.eqz
          br_if 0 (;@2;)
          local.get 5
          i32.load offset=12
          local.set 7
          local.get 5
          i32.load offset=4
          local.set 8
          block ;; label = @3
            local.get 2
            br_if 0 (;@3;)
            local.get 8
            local.get 6
            local.get 7
            call $<alloc::alloc::Global as core::alloc::Allocator>::deallocate
            local.get 1
            i32.const 0
            i32.store
            local.get 1
            local.get 3
            i32.store offset=4
            br 1 (;@2;)
          end
          local.get 4
          local.get 2
          i32.mul
          local.set 3
          block ;; label = @3
            block ;; label = @4
              local.get 4
              br_if 0 (;@4;)
              local.get 8
              local.get 6
              local.get 7
              call $<alloc::alloc::Global as core::alloc::Allocator>::deallocate
              local.get 6
              local.set 4
              br 1 (;@3;)
            end
            local.get 8
            local.get 7
            local.get 6
            local.get 3
            call $__rustc::__rust_realloc
            local.set 4
          end
          local.get 4
          i32.eqz
          br_if 1 (;@1;)
          local.get 1
          local.get 2
          i32.store
          local.get 1
          local.get 4
          i32.store offset=4
        end
        i32.const -2147483647
        local.set 6
      end
      local.get 0
      local.get 3
      i32.store offset=4
      local.get 0
      local.get 6
      i32.store
      local.get 5
      i32.const 16
      i32.add
      global.set $__stack_pointer
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
    (func $alloc::raw_vec::handle_error (;22;) (type 2) (param i32 i32 i32)
      unreachable
    )
    (func $core::ptr::alignment::Alignment::max (;23;) (type 1) (param i32 i32) (result i32)
      local.get 0
      local.get 1
      local.get 0
      local.get 1
      i32.gt_u
      select
    )
    (data $.rodata (;0;) (i32.const 1048576) "<redacted>\00")
    (data $.data (;1;) (i32.const 1048588) "\01\00\00\00\01\00\00\00\00\00\10\00\0a\00\00\00\00\00\00\00\00\00\00\00")
  )
  (alias export 0 "felt" (type (;1;)))
  (alias export 0 "asset" (type (;2;)))
  (core instance (;0;) (instantiate 0))
  (alias core export 0 "memory" (core memory (;0;)))
  (type (;3;) (list u8))
  (type (;4;) (list u16))
  (type (;5;) (func (param "l" 3) (result 4)))
  (alias core export 0 "miden:cm-types/cm-types@0.1.0#func-list" (core func (;0;)))
  (alias core export 0 "cabi_realloc" (core func (;1;)))
  (alias core export 0 "cabi_post_miden:cm-types/cm-types@0.1.0#func-list" (core func (;2;)))
  (func (;0;) (type 5) (canon lift (core func 0) (memory 0) (realloc 1) (post-return 2)))
  (alias export 0 "felt" (type (;6;)))
  (alias export 0 "word" (type (;7;)))
  (alias export 0 "asset" (type (;8;)))
  (component (;0;)
    (type (;0;) (record (field "inner" f32)))
    (import "import-type-felt" (type (;1;) (eq 0)))
    (type (;2;) (tuple 1 1 1 1))
    (type (;3;) (record (field "inner" 2)))
    (import "import-type-word" (type (;4;) (eq 3)))
    (type (;5;) (record (field "inner" 4)))
    (import "import-type-asset" (type (;6;) (eq 5)))
    (type (;7;) (list u8))
    (type (;8;) (list u16))
    (type (;9;) (func (param "l" 7) (result 8)))
    (import "import-func-func-list" (func (;0;) (type 9)))
    (export (;10;) "felt" (type 1))
    (export (;11;) "asset" (type 6))
    (type (;12;) (list u8))
    (type (;13;) (list u16))
    (type (;14;) (func (param "l" 12) (result 13)))
    (export (;1;) "func-list" (func 0) (func (type 14)))
  )
  (instance (;1;) (instantiate 0
      (with "import-func-func-list" (func 0))
      (with "import-type-felt" (type 6))
      (with "import-type-word" (type 7))
      (with "import-type-asset" (type 8))
    )
  )
  (export (;2;) "miden:cm-types/cm-types@0.1.0" (instance 1))
)
