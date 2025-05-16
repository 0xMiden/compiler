(component $cross-ctx-account
  (type (;0;)
    (instance
      (type (;0;) (func (result s32)))
      (export (;0;) "heap-base" (func (type 0)))
    )
  )
  (import "miden:core-intrinsics/intrinsics-mem@1.0.0" (instance (;0;) (type 0)))
  (type (;1;)
    (instance
      (type (;0;) (func (param "a" f32) (param "b" f32) (result f32)))
      (export (;0;) "add" (func (type 0)))
      (type (;1;) (func (param "a" u32) (result f32)))
      (export (;1;) "from-u32" (func (type 1)))
    )
  )
  (import "miden:core-intrinsics/intrinsics-felt@1.0.0" (instance (;1;) (type 1)))
  (type (;2;)
    (instance
      (type (;0;) (record (field "inner" f32)))
      (export (;1;) "felt" (type (eq 0)))
    )
  )
  (import "miden:base/core-types@1.0.0" (instance (;2;) (type 2)))
  (core module (;0;)
    (type (;0;) (func (param i32) (result f32)))
    (type (;1;) (func (param f32 f32) (result f32)))
    (type (;2;) (func (result i32)))
    (type (;3;) (func))
    (type (;4;) (func (param i32 i32) (result i32)))
    (type (;5;) (func (param i32 i32 i32 i32) (result i32)))
    (type (;6;) (func (param f32) (result f32)))
    (type (;7;) (func (param i32 i32 i32) (result i32)))
    (import "miden:core-intrinsics/intrinsics-felt@1.0.0" "from-u32" (func $miden_stdlib_sys::intrinsics::felt::extern_from_u32 (;0;) (type 0)))
    (import "miden:core-intrinsics/intrinsics-felt@1.0.0" "add" (func $miden_stdlib_sys::intrinsics::felt::extern_add (;1;) (type 1)))
    (import "miden:core-intrinsics/intrinsics-mem@1.0.0" "heap-base" (func $miden_sdk_alloc::heap_base (;2;) (type 2)))
    (table (;0;) 3 3 funcref)
    (memory (;0;) 17)
    (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
    (export "memory" (memory 0))
    (export "miden:cross-ctx-account/foo@1.0.0#process-felt" (func $miden:cross-ctx-account/foo@1.0.0#process-felt))
    (export "cabi_realloc_wit_bindgen_0_28_0" (func $cabi_realloc_wit_bindgen_0_28_0))
    (export "cabi_realloc" (func $cabi_realloc))
    (elem (;0;) (i32.const 1) func $cross_ctx_account::bindings::__link_custom_section_describing_imports $cabi_realloc)
    (func $__wasm_call_ctors (;3;) (type 3))
    (func $cross_ctx_account::bindings::__link_custom_section_describing_imports (;4;) (type 3))
    (func $__rustc::__rust_alloc (;5;) (type 4) (param i32 i32) (result i32)
      i32.const 1048600
      local.get 1
      local.get 0
      call $<miden_sdk_alloc::BumpAlloc as core::alloc::global::GlobalAlloc>::alloc
    )
    (func $__rustc::__rust_realloc (;6;) (type 5) (param i32 i32 i32 i32) (result i32)
      block ;; label = @1
        i32.const 1048600
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
    (func $miden:cross-ctx-account/foo@1.0.0#process-felt (;7;) (type 6) (param f32) (result f32)
      call $wit_bindgen_rt::run_ctors_once
      local.get 0
      i32.const 3
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      call $miden_stdlib_sys::intrinsics::felt::extern_add
    )
    (func $cabi_realloc_wit_bindgen_0_28_0 (;8;) (type 5) (param i32 i32 i32 i32) (result i32)
      local.get 0
      local.get 1
      local.get 2
      local.get 3
      call $wit_bindgen_rt::cabi_realloc
    )
    (func $wit_bindgen_rt::cabi_realloc (;9;) (type 5) (param i32 i32 i32 i32) (result i32)
      block ;; label = @1
        block ;; label = @2
          block ;; label = @3
            local.get 1
            br_if 0 (;@3;)
            local.get 3
            i32.eqz
            br_if 2 (;@1;)
            i32.const 0
            i32.load8_u offset=1048604
            drop
            local.get 3
            local.get 2
            call $__rustc::__rust_alloc
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
    (func $wit_bindgen_rt::run_ctors_once (;10;) (type 3)
      block ;; label = @1
        i32.const 0
        i32.load8_u offset=1048605
        br_if 0 (;@1;)
        call $__wasm_call_ctors
        i32.const 0
        i32.const 1
        i32.store8 offset=1048605
      end
    )
    (func $<miden_sdk_alloc::BumpAlloc as core::alloc::global::GlobalAlloc>::alloc (;11;) (type 7) (param i32 i32 i32) (result i32)
      (local i32 i32)
      block ;; label = @1
        local.get 1
        i32.const 32
        local.get 1
        i32.const 32
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
    (func $core::ptr::alignment::Alignment::max (;12;) (type 4) (param i32 i32) (result i32)
      local.get 0
      local.get 1
      local.get 0
      local.get 1
      i32.gt_u
      select
    )
    (func $cabi_realloc (;13;) (type 5) (param i32 i32 i32 i32) (result i32)
      local.get 0
      local.get 1
      local.get 2
      local.get 3
      call $cabi_realloc_wit_bindgen_0_28_0
    )
    (data $.rodata (;0;) (i32.const 1048576) "\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00\02\00\00\00")
  )
  (alias export 1 "from-u32" (func (;0;)))
  (core func (;0;) (canon lower (func 0)))
  (alias export 1 "add" (func (;1;)))
  (core func (;1;) (canon lower (func 1)))
  (core instance (;0;)
    (export "from-u32" (func 0))
    (export "add" (func 1))
  )
  (alias export 0 "heap-base" (func (;2;)))
  (core func (;2;) (canon lower (func 2)))
  (core instance (;1;)
    (export "heap-base" (func 2))
  )
  (core instance (;2;) (instantiate 0
      (with "miden:core-intrinsics/intrinsics-felt@1.0.0" (instance 0))
      (with "miden:core-intrinsics/intrinsics-mem@1.0.0" (instance 1))
    )
  )
  (alias core export 2 "memory" (core memory (;0;)))
  (alias export 2 "felt" (type (;3;)))
  (type (;4;) (func (param "input" 3) (result 3)))
  (alias core export 2 "miden:cross-ctx-account/foo@1.0.0#process-felt" (core func (;3;)))
  (alias core export 2 "cabi_realloc" (core func (;4;)))
  (func (;3;) (type 4) (canon lift (core func 3)))
  (alias export 2 "felt" (type (;5;)))
  (component (;0;)
    (type (;0;) (record (field "inner" f32)))
    (import "import-type-felt" (type (;1;) (eq 0)))
    (import "import-type-felt0" (type (;2;) (eq 1)))
    (type (;3;) (func (param "input" 2) (result 2)))
    (import "import-func-process-felt" (func (;0;) (type 3)))
    (export (;4;) "felt" (type 1))
    (type (;5;) (func (param "input" 4) (result 4)))
    (export (;1;) "process-felt" (func 0) (func (type 5)))
  )
  (instance (;3;) (instantiate 0
      (with "import-func-process-felt" (func 3))
      (with "import-type-felt" (type 5))
      (with "import-type-felt0" (type 3))
    )
  )
  (export (;4;) "miden:cross-ctx-account/foo@1.0.0" (instance 3))
  (@custom "version" "0.1.0")
)
