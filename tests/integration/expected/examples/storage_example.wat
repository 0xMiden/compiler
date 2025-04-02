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
      (type (;0;) (func (param "index" f32) (param "result-ptr" s32)))
      (export (;0;) "get-item" (func (type 0)))
      (type (;1;) (func (param "index" f32) (param "value0" f32) (param "value1" f32) (param "value2" f32) (param "value3" f32) (param "result-ptr" s32)))
      (export (;1;) "set-item" (func (type 1)))
      (type (;2;) (func (param "index" f32) (param "key0" f32) (param "key1" f32) (param "key2" f32) (param "key3" f32) (param "result-ptr" s32)))
      (export (;2;) "get-map-item" (func (type 2)))
      (type (;3;) (func (param "index" f32) (param "key0" f32) (param "key1" f32) (param "key2" f32) (param "key3" f32) (param "value0" f32) (param "value1" f32) (param "value2" f32) (param "value3" f32) (param "result-ptr" s32)))
      (export (;3;) "set-map-item" (func (type 3)))
    )
  )
  (import "miden:core-import/account@1.0.0" (instance (;1;) (type 1)))
  (type (;2;)
    (instance
      (type (;0;) (record (field "inner" f32)))
      (export (;1;) "felt" (type (eq 0)))
      (type (;2;) (tuple 1 1 1 1))
      (type (;3;) (record (field "inner" 2)))
      (export (;4;) "word" (type (eq 3)))
    )
  )
  (import "miden:base/core-types@1.0.0" (instance (;2;) (type 2)))
  (core module (;0;)
    (type (;0;) (func (result i32)))
    (type (;1;) (func (param f32 i32)))
    (type (;2;) (func (param f32 f32 f32 f32 f32 i32)))
    (type (;3;) (func (param f32 f32 f32 f32 f32 f32 f32 f32 f32 i32)))
    (type (;4;) (func))
    (type (;5;) (func (param i32 i32) (result i32)))
    (type (;6;) (func (param i32 i32 i32 i32) (result i32)))
    (type (;7;) (func (param f32 f32 f32 f32) (result f32)))
    (type (;8;) (func (param f32 f32 f32 f32 f32 f32 f32 f32) (result f32)))
    (type (;9;) (func (param i32 i32 i32) (result i32)))
    (type (;10;) (func (param i32 i32)))
    (type (;11;) (func (param i32 i32 i32)))
    (type (;12;) (func (param i32 i32 i32 i32)))
    (import "miden:core-import/intrinsics-mem@1.0.0" "heap-base" (func $miden_sdk_alloc::heap_base (;0;) (type 0)))
    (import "miden:core-import/account@1.0.0" "get-item" (func $miden_base_sys::bindings::storage::extern_get_storage_item (;1;) (type 1)))
    (import "miden:core-import/account@1.0.0" "set-item" (func $miden_base_sys::bindings::storage::extern_set_storage_item (;2;) (type 2)))
    (import "miden:core-import/account@1.0.0" "get-map-item" (func $miden_base_sys::bindings::storage::extern_get_storage_map_item (;3;) (type 2)))
    (import "miden:core-import/account@1.0.0" "set-map-item" (func $miden_base_sys::bindings::storage::extern_set_storage_map_item (;4;) (type 3)))
    (table (;0;) 3 3 funcref)
    (memory (;0;) 17)
    (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
    (export "memory" (memory 0))
    (export "miden:storage-example/foo@1.0.0#test-storage-item-low" (func $miden:storage-example/foo@1.0.0#test-storage-item-low))
    (export "miden:storage-example/foo@1.0.0#test-storage-map-item-low" (func $miden:storage-example/foo@1.0.0#test-storage-map-item-low))
    (export "miden:storage-example/foo@1.0.0#test-storage-item-high" (func $miden:storage-example/foo@1.0.0#test-storage-item-high))
    (export "miden:storage-example/foo@1.0.0#test-storage-map-item-high" (func $miden:storage-example/foo@1.0.0#test-storage-map-item-high))
    (export "cabi_realloc_wit_bindgen_0_28_0" (func $cabi_realloc_wit_bindgen_0_28_0))
    (export "cabi_realloc" (func $cabi_realloc))
    (elem (;0;) (i32.const 1) func $storage_example::bindings::__link_custom_section_describing_imports $cabi_realloc)
    (func $__wasm_call_ctors (;5;) (type 4))
    (func $storage_example::bindings::__link_custom_section_describing_imports (;6;) (type 4))
    (func $__rustc::__rust_alloc (;7;) (type 5) (param i32 i32) (result i32)
      i32.const 1048612
      local.get 1
      local.get 0
      call $<miden_sdk_alloc::BumpAlloc as core::alloc::global::GlobalAlloc>::alloc
    )
    (func $__rustc::__rust_realloc (;8;) (type 6) (param i32 i32 i32 i32) (result i32)
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
    (func $miden:storage-example/foo@1.0.0#test-storage-item-low (;9;) (type 7) (param f32 f32 f32 f32) (result f32)
      (local i32 i32)
      global.get $__stack_pointer
      local.tee 4
      i32.const 96
      i32.sub
      i32.const -32
      i32.and
      local.tee 5
      global.set $__stack_pointer
      call $wit_bindgen_rt::run_ctors_once
      local.get 5
      local.get 3
      f32.store offset=76
      local.get 5
      local.get 2
      f32.store offset=72
      local.get 5
      local.get 1
      f32.store offset=68
      local.get 5
      local.get 0
      f32.store offset=64
      local.get 5
      i32.const 0
      local.get 5
      i32.const 64
      i32.add
      call $miden_base_sys::bindings::storage::set_item
      local.get 5
      i32.const 0
      call $miden_base_sys::bindings::storage::get_item
      local.get 5
      f32.load
      local.set 3
      local.get 4
      global.set $__stack_pointer
      local.get 3
    )
    (func $miden:storage-example/foo@1.0.0#test-storage-map-item-low (;10;) (type 8) (param f32 f32 f32 f32 f32 f32 f32 f32) (result f32)
      (local i32 i32)
      global.get $__stack_pointer
      local.tee 8
      i32.const 160
      i32.sub
      i32.const -32
      i32.and
      local.tee 9
      global.set $__stack_pointer
      call $wit_bindgen_rt::run_ctors_once
      local.get 9
      local.get 3
      f32.store offset=12
      local.get 9
      local.get 2
      f32.store offset=8
      local.get 9
      local.get 1
      f32.store offset=4
      local.get 9
      local.get 0
      f32.store
      local.get 9
      local.get 3
      f32.store offset=108
      local.get 9
      local.get 2
      f32.store offset=104
      local.get 9
      local.get 1
      f32.store offset=100
      local.get 9
      local.get 0
      f32.store offset=96
      local.get 9
      local.get 7
      f32.store offset=140
      local.get 9
      local.get 6
      f32.store offset=136
      local.get 9
      local.get 5
      f32.store offset=132
      local.get 9
      local.get 4
      f32.store offset=128
      local.get 9
      i32.const 32
      i32.add
      i32.const 1
      local.get 9
      i32.const 96
      i32.add
      local.get 9
      i32.const 128
      i32.add
      call $miden_base_sys::bindings::storage::set_map_item
      local.get 9
      i32.const 32
      i32.add
      i32.const 1
      local.get 9
      call $miden_base_sys::bindings::storage::get_map_item
      local.get 9
      f32.load offset=32
      local.set 3
      local.get 8
      global.set $__stack_pointer
      local.get 3
    )
    (func $miden:storage-example/foo@1.0.0#test-storage-item-high (;11;) (type 7) (param f32 f32 f32 f32) (result f32)
      (local i32 i32)
      global.get $__stack_pointer
      local.tee 4
      i32.const 96
      i32.sub
      i32.const -32
      i32.and
      local.tee 5
      global.set $__stack_pointer
      call $wit_bindgen_rt::run_ctors_once
      local.get 5
      local.get 3
      f32.store offset=76
      local.get 5
      local.get 2
      f32.store offset=72
      local.get 5
      local.get 1
      f32.store offset=68
      local.get 5
      local.get 0
      f32.store offset=64
      local.get 5
      i32.const 0
      local.get 5
      i32.const 64
      i32.add
      call $miden_base_sys::bindings::storage::set_item
      local.get 5
      i32.const 0
      call $miden_base_sys::bindings::storage::get_item
      local.get 5
      f32.load
      local.set 3
      local.get 4
      global.set $__stack_pointer
      local.get 3
    )
    (func $miden:storage-example/foo@1.0.0#test-storage-map-item-high (;12;) (type 8) (param f32 f32 f32 f32 f32 f32 f32 f32) (result f32)
      (local i32 i32)
      global.get $__stack_pointer
      local.tee 8
      i32.const 160
      i32.sub
      i32.const -32
      i32.and
      local.tee 9
      global.set $__stack_pointer
      call $wit_bindgen_rt::run_ctors_once
      local.get 9
      local.get 3
      f32.store offset=12
      local.get 9
      local.get 2
      f32.store offset=8
      local.get 9
      local.get 1
      f32.store offset=4
      local.get 9
      local.get 0
      f32.store
      local.get 9
      local.get 3
      f32.store offset=108
      local.get 9
      local.get 2
      f32.store offset=104
      local.get 9
      local.get 1
      f32.store offset=100
      local.get 9
      local.get 0
      f32.store offset=96
      local.get 9
      local.get 7
      f32.store offset=140
      local.get 9
      local.get 6
      f32.store offset=136
      local.get 9
      local.get 5
      f32.store offset=132
      local.get 9
      local.get 4
      f32.store offset=128
      local.get 9
      i32.const 32
      i32.add
      i32.const 1
      local.get 9
      i32.const 96
      i32.add
      local.get 9
      i32.const 128
      i32.add
      call $miden_base_sys::bindings::storage::set_map_item
      local.get 9
      i32.const 32
      i32.add
      i32.const 1
      local.get 9
      call $miden_base_sys::bindings::storage::get_map_item
      local.get 9
      f32.load offset=32
      local.set 3
      local.get 8
      global.set $__stack_pointer
      local.get 3
    )
    (func $cabi_realloc_wit_bindgen_0_28_0 (;13;) (type 6) (param i32 i32 i32 i32) (result i32)
      local.get 0
      local.get 1
      local.get 2
      local.get 3
      call $wit_bindgen_rt::cabi_realloc
    )
    (func $wit_bindgen_rt::cabi_realloc (;14;) (type 6) (param i32 i32 i32 i32) (result i32)
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
    (func $wit_bindgen_rt::run_ctors_once (;15;) (type 4)
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
    (func $<miden_sdk_alloc::BumpAlloc as core::alloc::global::GlobalAlloc>::alloc (;16;) (type 9) (param i32 i32 i32) (result i32)
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
    (func $miden_base_sys::bindings::storage::get_item (;17;) (type 10) (param i32 i32)
      local.get 1
      i32.const 255
      i32.and
      f32.reinterpret_i32
      local.get 0
      call $miden_base_sys::bindings::storage::extern_get_storage_item
    )
    (func $miden_base_sys::bindings::storage::set_item (;18;) (type 11) (param i32 i32 i32)
      local.get 1
      i32.const 255
      i32.and
      f32.reinterpret_i32
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
    (func $miden_base_sys::bindings::storage::get_map_item (;19;) (type 11) (param i32 i32 i32)
      local.get 1
      i32.const 255
      i32.and
      f32.reinterpret_i32
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
    (func $miden_base_sys::bindings::storage::set_map_item (;20;) (type 12) (param i32 i32 i32 i32)
      local.get 1
      i32.const 255
      i32.and
      f32.reinterpret_i32
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
    (func $core::ptr::alignment::Alignment::max (;21;) (type 5) (param i32 i32) (result i32)
      local.get 0
      local.get 1
      local.get 0
      local.get 1
      i32.gt_u
      select
    )
    (func $cabi_realloc (;22;) (type 6) (param i32 i32 i32 i32) (result i32)
      local.get 0
      local.get 1
      local.get 2
      local.get 3
      call $cabi_realloc_wit_bindgen_0_28_0
    )
    (data $.rodata (;0;) (i32.const 1048576) "\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00\02\00\00\00")
    (@custom "rodata,miden_account" (after data) "\1fstorage-example_A simple example of a Miden account storage API\0b0.1.0\01\05\00\00\00!owner_public_key\01\15test value9auth::rpo_falcon512::pub_key\01\01\01\0ffoo_map\01\11test map")
  )
  (alias export 0 "heap-base" (func (;0;)))
  (core func (;0;) (canon lower (func 0)))
  (core instance (;0;)
    (export "heap-base" (func 0))
  )
  (alias export 1 "get-item" (func (;1;)))
  (core func (;1;) (canon lower (func 1)))
  (alias export 1 "set-item" (func (;2;)))
  (core func (;2;) (canon lower (func 2)))
  (alias export 1 "get-map-item" (func (;3;)))
  (core func (;3;) (canon lower (func 3)))
  (alias export 1 "set-map-item" (func (;4;)))
  (core func (;4;) (canon lower (func 4)))
  (core instance (;1;)
    (export "get-item" (func 1))
    (export "set-item" (func 2))
    (export "get-map-item" (func 3))
    (export "set-map-item" (func 4))
  )
  (core instance (;2;) (instantiate 0
      (with "miden:core-import/intrinsics-mem@1.0.0" (instance 0))
      (with "miden:core-import/account@1.0.0" (instance 1))
    )
  )
  (alias core export 2 "memory" (core memory (;0;)))
  (alias export 2 "word" (type (;3;)))
  (alias export 2 "felt" (type (;4;)))
  (type (;5;) (func (param "value" 3) (result 4)))
  (alias core export 2 "miden:storage-example/foo@1.0.0#test-storage-item-low" (core func (;5;)))
  (alias core export 2 "cabi_realloc" (core func (;6;)))
  (func (;5;) (type 5) (canon lift (core func 5)))
  (type (;6;) (func (param "key" 3) (param "value" 3) (result 4)))
  (alias core export 2 "miden:storage-example/foo@1.0.0#test-storage-map-item-low" (core func (;7;)))
  (func (;6;) (type 6) (canon lift (core func 7)))
  (alias core export 2 "miden:storage-example/foo@1.0.0#test-storage-item-high" (core func (;8;)))
  (func (;7;) (type 5) (canon lift (core func 8)))
  (alias core export 2 "miden:storage-example/foo@1.0.0#test-storage-map-item-high" (core func (;9;)))
  (func (;8;) (type 6) (canon lift (core func 9)))
  (alias export 2 "felt" (type (;7;)))
  (alias export 2 "word" (type (;8;)))
  (component (;0;)
    (type (;0;) (record (field "inner" f32)))
    (import "import-type-felt" (type (;1;) (eq 0)))
    (type (;2;) (tuple 1 1 1 1))
    (type (;3;) (record (field "inner" 2)))
    (import "import-type-word" (type (;4;) (eq 3)))
    (import "import-type-word0" (type (;5;) (eq 4)))
    (import "import-type-felt0" (type (;6;) (eq 1)))
    (type (;7;) (func (param "value" 5) (result 6)))
    (import "import-func-test-storage-item-low" (func (;0;) (type 7)))
    (type (;8;) (func (param "key" 5) (param "value" 5) (result 6)))
    (import "import-func-test-storage-map-item-low" (func (;1;) (type 8)))
    (import "import-func-test-storage-item-high" (func (;2;) (type 7)))
    (import "import-func-test-storage-map-item-high" (func (;3;) (type 8)))
    (export (;9;) "felt" (type 1))
    (export (;10;) "word" (type 4))
    (type (;11;) (func (param "value" 10) (result 9)))
    (export (;4;) "test-storage-item-low" (func 0) (func (type 11)))
    (type (;12;) (func (param "key" 10) (param "value" 10) (result 9)))
    (export (;5;) "test-storage-map-item-low" (func 1) (func (type 12)))
    (export (;6;) "test-storage-item-high" (func 2) (func (type 11)))
    (export (;7;) "test-storage-map-item-high" (func 3) (func (type 12)))
  )
  (instance (;3;) (instantiate 0
      (with "import-func-test-storage-item-low" (func 5))
      (with "import-func-test-storage-map-item-low" (func 6))
      (with "import-func-test-storage-item-high" (func 7))
      (with "import-func-test-storage-map-item-high" (func 8))
      (with "import-type-felt" (type 7))
      (with "import-type-word" (type 8))
      (with "import-type-word0" (type 3))
      (with "import-type-felt0" (type 4))
    )
  )
  (export (;4;) "miden:storage-example/foo@1.0.0" (instance 3))
  (@custom "description" "A simple example of a Miden account storage API")
  (@custom "version" "0.1.0")
)
