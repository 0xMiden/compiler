(component $storage-example
  (type (;0;)
    (instance
      (type (;0;) (func (result s32)))
      (export (;0;) "heap-base" (func (type 0)))
    )
  )
  (import "miden:core-import/intrinsics-mem@1.0.0" (instance (;0;) (type 0)))
  (type (;1;)
    (instance
      (type (;0;) (func (param "a" f32) (param "b" f32) (result bool)))
      (export (;0;) "eq" (func (type 0)))
    )
  )
  (import "miden:core-import/intrinsics-felt@1.0.0" (instance (;1;) (type 1)))
  (type (;2;)
    (instance
      (type (;0;) (func (param "index" f32) (param "result-ptr" s32)))
      (export (;0;) "get-storage-item" (func (type 0)))
      (type (;1;) (func (param "index" f32) (param "value0" f32) (param "value1" f32) (param "value2" f32) (param "value3" f32) (param "result-ptr" s32)))
      (export (;1;) "set-storage-item" (func (type 1)))
      (type (;2;) (func (param "index" f32) (param "key0" f32) (param "key1" f32) (param "key2" f32) (param "key3" f32) (param "result-ptr" s32)))
      (export (;2;) "get-storage-map-item" (func (type 2)))
      (type (;3;) (func (param "index" f32) (param "key0" f32) (param "key1" f32) (param "key2" f32) (param "key3" f32) (param "value0" f32) (param "value1" f32) (param "value2" f32) (param "value3" f32) (param "result-ptr" s32)))
      (export (;3;) "set-storage-map-item" (func (type 3)))
    )
  )
  (import "miden:core-import/account@1.0.0" (instance (;2;) (type 2)))
  (type (;3;)
    (instance
      (type (;0;) (record (field "inner" f32)))
      (export (;1;) "felt" (type (eq 0)))
      (type (;2;) (tuple 1 1 1 1))
      (type (;3;) (record (field "inner" 2)))
      (export (;4;) "word" (type (eq 3)))
    )
  )
  (import "miden:base/core-types@1.0.0" (instance (;3;) (type 3)))
  (core module (;0;)
    (type (;0;) (func (param f32 f32) (result i32)))
    (type (;1;) (func (result i32)))
    (type (;2;) (func (param f32 i32)))
    (type (;3;) (func (param f32 f32 f32 f32 f32 i32)))
    (type (;4;) (func (param f32 f32 f32 f32 f32 f32 f32 f32 f32 i32)))
    (type (;5;) (func))
    (type (;6;) (func (param i32 i32) (result i32)))
    (type (;7;) (func (param i32 i32 i32 i32) (result i32)))
    (type (;8;) (func (param f32 f32 f32 f32 f32) (result f32)))
    (type (;9;) (func (param f32 f32 f32 f32 f32 f32 f32 f32 f32) (result f32)))
    (type (;10;) (func (param i32 i32 i32) (result i32)))
    (type (;11;) (func (param i32 f32)))
    (type (;12;) (func (param i32 f32 i32)))
    (type (;13;) (func (param i32 f32 i32 i32)))
    (import "miden:core-import/intrinsics-felt@1.0.0" "eq" (func $miden_stdlib_sys::intrinsics::felt::extern_eq (;0;) (type 0)))
    (import "miden:core-import/intrinsics-mem@1.0.0" "heap-base" (func $miden_sdk_alloc::heap_base (;1;) (type 1)))
    (import "miden:core-import/account@1.0.0" "get-storage-item" (func $miden_base_sys::bindings::storage::extern_get_storage_item (;2;) (type 2)))
    (import "miden:core-import/account@1.0.0" "set-storage-item" (func $miden_base_sys::bindings::storage::extern_set_storage_item (;3;) (type 3)))
    (import "miden:core-import/account@1.0.0" "get-storage-map-item" (func $miden_base_sys::bindings::storage::extern_get_storage_map_item (;4;) (type 3)))
    (import "miden:core-import/account@1.0.0" "set-storage-map-item" (func $miden_base_sys::bindings::storage::extern_set_storage_map_item (;5;) (type 4)))
    (table (;0;) 3 3 funcref)
    (memory (;0;) 17)
    (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
    (export "memory" (memory 0))
    (export "miden:storage-example/foo@1.0.0#test-storage-item" (func $miden:storage-example/foo@1.0.0#test-storage-item))
    (export "miden:storage-example/foo@1.0.0#test-storage-map-item" (func $miden:storage-example/foo@1.0.0#test-storage-map-item))
    (export "cabi_realloc_wit_bindgen_0_28_0" (func $cabi_realloc_wit_bindgen_0_28_0))
    (export "cabi_realloc" (func $cabi_realloc))
    (elem (;0;) (i32.const 1) func $storage_example::bindings::__link_custom_section_describing_imports $cabi_realloc)
    (func $__wasm_call_ctors (;6;) (type 5))
    (func $<miden_stdlib_sys::intrinsics::word::Word as core::cmp::PartialEq>::eq (;7;) (type 6) (param i32 i32) (result i32)
      (local i32)
      i32.const 0
      local.set 2
      block ;; label = @1
        local.get 0
        f32.load
        local.get 1
        f32.load
        call $miden_stdlib_sys::intrinsics::felt::extern_eq
        i32.const 1
        i32.ne
        br_if 0 (;@1;)
        local.get 0
        f32.load offset=4
        local.get 1
        f32.load offset=4
        call $miden_stdlib_sys::intrinsics::felt::extern_eq
        i32.const 1
        i32.ne
        br_if 0 (;@1;)
        local.get 0
        f32.load offset=8
        local.get 1
        f32.load offset=8
        call $miden_stdlib_sys::intrinsics::felt::extern_eq
        i32.const 1
        i32.ne
        br_if 0 (;@1;)
        local.get 0
        f32.load offset=12
        local.get 1
        f32.load offset=12
        call $miden_stdlib_sys::intrinsics::felt::extern_eq
        i32.const 1
        i32.eq
        local.set 2
      end
      local.get 2
    )
    (func $storage_example::bindings::__link_custom_section_describing_imports (;8;) (type 5))
    (func $__rustc::__rust_alloc (;9;) (type 6) (param i32 i32) (result i32)
      i32.const 1048612
      local.get 1
      local.get 0
      call $<miden_sdk_alloc::BumpAlloc as core::alloc::global::GlobalAlloc>::alloc
    )
    (func $__rustc::__rust_realloc (;10;) (type 7) (param i32 i32 i32 i32) (result i32)
      block ;; label = @1
        i32.const 1048612
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
    (func $miden:storage-example/foo@1.0.0#test-storage-item (;11;) (type 8) (param f32 f32 f32 f32 f32) (result f32)
      (local i32 i32)
      global.get $__stack_pointer
      local.tee 5
      local.set 6
      local.get 5
      i32.const 96
      i32.sub
      i32.const -32
      i32.and
      local.tee 5
      global.set $__stack_pointer
      call $wit_bindgen_rt::run_ctors_once
      local.get 5
      local.get 4
      f32.store offset=12
      local.get 5
      local.get 3
      f32.store offset=8
      local.get 5
      local.get 2
      f32.store offset=4
      local.get 5
      local.get 1
      f32.store
      local.get 5
      i32.const 32
      i32.add
      local.get 0
      local.get 5
      call $miden_base_sys::bindings::storage::set_item
      local.get 5
      i32.const 32
      i32.add
      local.get 0
      call $miden_base_sys::bindings::storage::get_item
      block ;; label = @1
        local.get 5
        local.get 5
        i32.const 32
        i32.add
        call $<miden_stdlib_sys::intrinsics::word::Word as core::cmp::PartialEq>::eq
        br_if 0 (;@1;)
        unreachable
      end
      local.get 5
      f32.load offset=32
      local.set 0
      local.get 6
      global.set $__stack_pointer
      local.get 0
    )
    (func $miden:storage-example/foo@1.0.0#test-storage-map-item (;12;) (type 9) (param f32 f32 f32 f32 f32 f32 f32 f32 f32) (result f32)
      (local i32 i32)
      global.get $__stack_pointer
      local.tee 9
      local.set 10
      local.get 9
      i32.const 128
      i32.sub
      i32.const -32
      i32.and
      local.tee 9
      global.set $__stack_pointer
      call $wit_bindgen_rt::run_ctors_once
      local.get 9
      local.get 4
      f32.store offset=12
      local.get 9
      local.get 3
      f32.store offset=8
      local.get 9
      local.get 2
      f32.store offset=4
      local.get 9
      local.get 1
      f32.store
      local.get 9
      local.get 8
      f32.store offset=44
      local.get 9
      local.get 7
      f32.store offset=40
      local.get 9
      local.get 6
      f32.store offset=36
      local.get 9
      local.get 5
      f32.store offset=32
      local.get 9
      i32.const 64
      i32.add
      local.get 0
      local.get 9
      local.get 9
      i32.const 32
      i32.add
      call $miden_base_sys::bindings::storage::set_map_item
      local.get 9
      i32.const 64
      i32.add
      local.get 0
      local.get 9
      call $miden_base_sys::bindings::storage::get_map_item
      block ;; label = @1
        local.get 9
        i32.const 32
        i32.add
        local.get 9
        i32.const 64
        i32.add
        call $<miden_stdlib_sys::intrinsics::word::Word as core::cmp::PartialEq>::eq
        br_if 0 (;@1;)
        unreachable
      end
      local.get 9
      f32.load offset=64
      local.set 0
      local.get 10
      global.set $__stack_pointer
      local.get 0
    )
    (func $cabi_realloc_wit_bindgen_0_28_0 (;13;) (type 7) (param i32 i32 i32 i32) (result i32)
      local.get 0
      local.get 1
      local.get 2
      local.get 3
      call $wit_bindgen_rt::cabi_realloc
    )
    (func $wit_bindgen_rt::cabi_realloc (;14;) (type 7) (param i32 i32 i32 i32) (result i32)
      block ;; label = @1
        block ;; label = @2
          block ;; label = @3
            local.get 1
            br_if 0 (;@3;)
            local.get 3
            i32.eqz
            br_if 2 (;@1;)
            i32.const 0
            i32.load8_u offset=1048616
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
    (func $wit_bindgen_rt::run_ctors_once (;15;) (type 5)
      block ;; label = @1
        i32.const 0
        i32.load8_u offset=1048617
        br_if 0 (;@1;)
        call $__wasm_call_ctors
        i32.const 0
        i32.const 1
        i32.store8 offset=1048617
      end
    )
    (func $<miden_sdk_alloc::BumpAlloc as core::alloc::global::GlobalAlloc>::alloc (;16;) (type 10) (param i32 i32 i32) (result i32)
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
    (func $miden_base_sys::bindings::storage::get_item (;17;) (type 11) (param i32 f32)
      local.get 1
      local.get 0
      call $miden_base_sys::bindings::storage::extern_get_storage_item
    )
    (func $miden_base_sys::bindings::storage::set_item (;18;) (type 12) (param i32 f32 i32)
      local.get 1
      local.get 2
      f32.load
      local.get 2
      f32.load offset=4
      local.get 2
      f32.load offset=8
      local.get 2
      f32.load offset=12
      local.get 0
      call $miden_base_sys::bindings::storage::extern_set_storage_item
    )
    (func $miden_base_sys::bindings::storage::get_map_item (;19;) (type 12) (param i32 f32 i32)
      local.get 1
      local.get 2
      f32.load
      local.get 2
      f32.load offset=4
      local.get 2
      f32.load offset=8
      local.get 2
      f32.load offset=12
      local.get 0
      call $miden_base_sys::bindings::storage::extern_get_storage_map_item
    )
    (func $miden_base_sys::bindings::storage::set_map_item (;20;) (type 13) (param i32 f32 i32 i32)
      local.get 1
      local.get 2
      f32.load
      local.get 2
      f32.load offset=4
      local.get 2
      f32.load offset=8
      local.get 2
      f32.load offset=12
      local.get 3
      f32.load
      local.get 3
      f32.load offset=4
      local.get 3
      f32.load offset=8
      local.get 3
      f32.load offset=12
      local.get 0
      call $miden_base_sys::bindings::storage::extern_set_storage_map_item
    )
    (func $core::ptr::alignment::Alignment::max (;21;) (type 6) (param i32 i32) (result i32)
      local.get 0
      local.get 1
      local.get 0
      local.get 1
      i32.gt_u
      select
    )
    (func $cabi_realloc (;22;) (type 7) (param i32 i32 i32 i32) (result i32)
      local.get 0
      local.get 1
      local.get 2
      local.get 3
      call $cabi_realloc_wit_bindgen_0_28_0
    )
    (data $.rodata (;0;) (i32.const 1048576) "\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00\02\00\00\00")
  )
  (alias export 1 "eq" (func (;0;)))
  (core func (;0;) (canon lower (func 0)))
  (core instance (;0;)
    (export "eq" (func 0))
  )
  (alias export 0 "heap-base" (func (;1;)))
  (core func (;1;) (canon lower (func 1)))
  (core instance (;1;)
    (export "heap-base" (func 1))
  )
  (alias export 2 "get-storage-item" (func (;2;)))
  (core func (;2;) (canon lower (func 2)))
  (alias export 2 "set-storage-item" (func (;3;)))
  (core func (;3;) (canon lower (func 3)))
  (alias export 2 "get-storage-map-item" (func (;4;)))
  (core func (;4;) (canon lower (func 4)))
  (alias export 2 "set-storage-map-item" (func (;5;)))
  (core func (;5;) (canon lower (func 5)))
  (core instance (;2;)
    (export "get-storage-item" (func 2))
    (export "set-storage-item" (func 3))
    (export "get-storage-map-item" (func 4))
    (export "set-storage-map-item" (func 5))
  )
  (core instance (;3;) (instantiate 0
      (with "miden:core-import/intrinsics-felt@1.0.0" (instance 0))
      (with "miden:core-import/intrinsics-mem@1.0.0" (instance 1))
      (with "miden:core-import/account@1.0.0" (instance 2))
    )
  )
  (alias core export 3 "memory" (core memory (;0;)))
  (alias export 3 "felt" (type (;4;)))
  (alias export 3 "word" (type (;5;)))
  (type (;6;) (func (param "index" 4) (param "value" 5) (result 4)))
  (alias core export 3 "miden:storage-example/foo@1.0.0#test-storage-item" (core func (;6;)))
  (alias core export 3 "cabi_realloc" (core func (;7;)))
  (func (;6;) (type 6) (canon lift (core func 6)))
  (type (;7;) (func (param "index" 4) (param "key" 5) (param "value" 5) (result 4)))
  (alias core export 3 "miden:storage-example/foo@1.0.0#test-storage-map-item" (core func (;8;)))
  (func (;7;) (type 7) (canon lift (core func 8)))
  (alias export 3 "felt" (type (;8;)))
  (alias export 3 "word" (type (;9;)))
  (component (;0;)
    (type (;0;) (record (field "inner" f32)))
    (import "import-type-felt" (type (;1;) (eq 0)))
    (type (;2;) (tuple 1 1 1 1))
    (type (;3;) (record (field "inner" 2)))
    (import "import-type-word" (type (;4;) (eq 3)))
    (import "import-type-felt0" (type (;5;) (eq 1)))
    (import "import-type-word0" (type (;6;) (eq 4)))
    (type (;7;) (func (param "index" 5) (param "value" 6) (result 5)))
    (import "import-func-test-storage-item" (func (;0;) (type 7)))
    (type (;8;) (func (param "index" 5) (param "key" 6) (param "value" 6) (result 5)))
    (import "import-func-test-storage-map-item" (func (;1;) (type 8)))
    (export (;9;) "felt" (type 1))
    (export (;10;) "word" (type 4))
    (type (;11;) (func (param "index" 9) (param "value" 10) (result 9)))
    (export (;2;) "test-storage-item" (func 0) (func (type 11)))
    (type (;12;) (func (param "index" 9) (param "key" 10) (param "value" 10) (result 9)))
    (export (;3;) "test-storage-map-item" (func 1) (func (type 12)))
  )
  (instance (;4;) (instantiate 0
      (with "import-func-test-storage-item" (func 6))
      (with "import-func-test-storage-map-item" (func 7))
      (with "import-type-felt" (type 8))
      (with "import-type-word" (type 9))
      (with "import-type-felt0" (type 4))
      (with "import-type-word0" (type 5))
    )
  )
  (export (;5;) "miden:storage-example/foo@1.0.0" (instance 4))
  (@custom "version" "0.1.0")
)
