(component
  (type (;0;)
    (instance
      (type (;0;) (record (field "inner" float32)))
      (export (;1;) "felt" (type (eq 0)))
    )
  )
  (import "miden:base/core-types@1.0.0" (instance (;0;) (type 0)))
  (alias export 0 "felt" (type (;1;)))
  (type (;2;)
    (instance
      (alias outer 1 1 (type (;0;)))
      (export (;1;) "felt" (type (eq 0)))
      (type (;2;) (func (param "input" 1) (result 1)))
      (export (;0;) "process-felt" (func (type 2)))
    )
  )
  (import "miden:cross-ctx-account/foo@1.0.0" (instance (;1;) (type 2)))
  (type (;3;)
    (instance
      (type (;0;) (func (result s32)))
      (export (;0;) "heap-base" (func (type 0)))
    )
  )
  (import "miden:core-import/intrinsics-mem@1.0.0" (instance (;2;) (type 3)))
  (type (;4;)
    (instance
      (type (;0;) (func (param "a" u32) (result float32)))
      (export (;0;) "from-u32" (func (type 0)))
      (type (;1;) (func (param "a" float32) (param "b" float32)))
      (export (;1;) "assert-eq" (func (type 1)))
    )
  )
  (import "miden:core-import/intrinsics-felt@1.0.0" (instance (;3;) (type 4)))
  (core module (;0;)
    (type (;0;) (func (param i32) (result f32)))
    (type (;1;) (func (param f32) (result f32)))
    (type (;2;) (func (param f32 f32)))
    (type (;3;) (func (result i32)))
    (type (;4;) (func))
    (type (;5;) (func (param i32 i32) (result i32)))
    (type (;6;) (func (param i32 i32 i32 i32) (result i32)))
    (type (;7;) (func (param i32 i32 i32) (result i32)))
    (import "miden:core-import/intrinsics-felt@1.0.0" "from-u32" (func $miden_stdlib_sys::intrinsics::felt::extern_from_u32 (;0;) (type 0)))
    (import "miden:cross-ctx-account/foo@1.0.0" "process-felt" (func $cross_ctx_note::bindings::miden::cross_ctx_account::foo::process_felt::wit_import1 (;1;) (type 1)))
    (import "miden:core-import/intrinsics-felt@1.0.0" "assert-eq" (func $miden_stdlib_sys::intrinsics::felt::extern_assert_eq (;2;) (type 2)))
    (import "miden:core-import/intrinsics-mem@1.0.0" "heap-base" (func $miden_sdk_alloc::heap_base (;3;) (type 3)))
    (func $__wasm_call_ctors (;4;) (type 4))
    (func $cross_ctx_note::bindings::__link_custom_section_describing_imports (;5;) (type 4))
    (func $__rust_alloc (;6;) (type 5) (param i32 i32) (result i32)
      i32.const 1048616
      local.get 1
      local.get 0
      call $<miden_sdk_alloc::BumpAlloc as core::alloc::global::GlobalAlloc>::alloc
    )
    (func $__rust_realloc (;7;) (type 6) (param i32 i32 i32 i32) (result i32)
      block ;; label = @1
        i32.const 1048616
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
    (func $miden:base/note-script@1.0.0#note-script (;8;) (type 4)
      call $wit_bindgen_rt::run_ctors_once
      i32.const 7
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      call $cross_ctx_note::bindings::miden::cross_ctx_account::foo::process_felt::wit_import1
      i32.const 10
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      call $miden_stdlib_sys::intrinsics::felt::extern_assert_eq
    )
    (func $cabi_realloc_wit_bindgen_0_28_0 (;9;) (type 6) (param i32 i32 i32 i32) (result i32)
      local.get 0
      local.get 1
      local.get 2
      local.get 3
      call $wit_bindgen_rt::cabi_realloc
    )
    (func $wit_bindgen_rt::cabi_realloc (;10;) (type 6) (param i32 i32 i32 i32) (result i32)
      block ;; label = @1
        block ;; label = @2
          block ;; label = @3
            local.get 1
            br_if 0 (;@3;)
            local.get 3
            i32.eqz
            br_if 2 (;@1;)
            i32.const 0
            i32.load8_u offset=1048620
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
    (func $wit_bindgen_rt::run_ctors_once (;11;) (type 4)
      block ;; label = @1
        i32.const 0
        i32.load8_u offset=1048621
        br_if 0 (;@1;)
        call $__wasm_call_ctors
        i32.const 0
        i32.const 1
        i32.store8 offset=1048621
      end
    )
    (func $<miden_sdk_alloc::BumpAlloc as core::alloc::global::GlobalAlloc>::alloc (;12;) (type 7) (param i32 i32 i32) (result i32)
      (local i32 i32)
      block ;; label = @1
        local.get 1
        i32.const 32
        local.get 1
        i32.const 32
        i32.gt_u
        select
        local.tee 3
        i32.popcnt
        i32.const 1
        i32.ne
        br_if 0 (;@1;)
        i32.const -2147483648
        local.get 1
        local.get 3
        call $core::ptr::alignment::Alignment::max
        local.tee 1
        i32.sub
        local.get 2
        i32.lt_u
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
    (func $core::ptr::alignment::Alignment::max (;13;) (type 5) (param i32 i32) (result i32)
      local.get 0
      local.get 1
      local.get 0
      local.get 1
      i32.gt_u
      select
    )
    (func $cabi_realloc (;14;) (type 6) (param i32 i32 i32 i32) (result i32)
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
    (export "miden:base/note-script@1.0.0#note-script" (func $miden:base/note-script@1.0.0#note-script))
    (export "cabi_realloc_wit_bindgen_0_28_0" (func $cabi_realloc_wit_bindgen_0_28_0))
    (export "cabi_realloc" (func $cabi_realloc))
    (elem (;0;) (i32.const 1) func $cross_ctx_note::bindings::__link_custom_section_describing_imports $cabi_realloc)
    (data $.rodata (;0;) (i32.const 1048576) "\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00\02\00\00\00")
  )
  (alias export 3 "from-u32" (func (;0;)))
  (core func (;0;) (canon lower (func 0)))
  (alias export 3 "assert-eq" (func (;1;)))
  (core func (;1;) (canon lower (func 1)))
  (core instance (;0;)
    (export "from-u32" (func 0))
    (export "assert-eq" (func 1))
  )
  (alias export 1 "process-felt" (func (;2;)))
  (core func (;2;) (canon lower (func 2)))
  (core instance (;1;)
    (export "process-felt" (func 2))
  )
  (alias export 2 "heap-base" (func (;3;)))
  (core func (;3;) (canon lower (func 3)))
  (core instance (;2;)
    (export "heap-base" (func 3))
  )
  (core instance (;3;) (instantiate 0
      (with "miden:core-import/intrinsics-felt@1.0.0" (instance 0))
      (with "miden:cross-ctx-account/foo@1.0.0" (instance 1))
      (with "miden:core-import/intrinsics-mem@1.0.0" (instance 2))
    )
  )
  (alias core export 3 "memory" (core memory (;0;)))
  (type (;5;) (func))
  (alias core export 3 "miden:base/note-script@1.0.0#note-script" (core func (;4;)))
  (alias core export 3 "cabi_realloc" (core func (;5;)))
  (func (;4;) (type 5) (canon lift (core func 4)))
  (component (;0;)
    (type (;0;) (func))
    (import "import-func-note-script" (func (;0;) (type 0)))
    (type (;1;) (func))
    (export (;1;) "note-script" (func 0) (func (type 1)))
  )
  (instance (;4;) (instantiate 0
      (with "import-func-note-script" (func 4))
    )
  )
  (export (;5;) "miden:base/note-script@1.0.0" (instance 4))
)