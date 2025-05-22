(component $counter-contract
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
      (type (;0;) (func (param "index" f32) (param "key0" f32) (param "key1" f32) (param "key2" f32) (param "key3" f32) (param "result-ptr" s32)))
      (export (;0;) "get-map-item" (func (type 0)))
      (type (;1;) (func (param "index" f32) (param "key0" f32) (param "key1" f32) (param "key2" f32) (param "key3" f32) (param "value0" f32) (param "value1" f32) (param "value2" f32) (param "value3" f32) (param "result-ptr" s32)))
      (export (;1;) "set-map-item" (func (type 1)))
      (type (;2;) (func (param "value" u32)))
      (export (;2;) "incr-nonce" (func (type 2)))
    )
  )
  (import "miden:core-base/account@1.0.0" (instance (;2;) (type 2)))
  (type (;3;)
    (instance
      (type (;0;) (record (field "inner" f32)))
      (export (;1;) "felt" (type (eq 0)))
    )
  )
  (import "miden:base/core-types@1.0.0" (instance (;3;) (type 3)))
  (core module (;0;)
    (type (;0;) (func (param i32) (result f32)))
    (type (;1;) (func (param f32 f32) (result f32)))
    (type (;2;) (func (result i32)))
    (type (;3;) (func (param i32)))
    (type (;4;) (func (param f32 f32 f32 f32 f32 i32)))
    (type (;5;) (func (param f32 f32 f32 f32 f32 f32 f32 f32 f32 i32)))
    (type (;6;) (func))
    (type (;7;) (func (param i32 i32) (result i32)))
    (type (;8;) (func (param i32 i32 i32 i32) (result i32)))
    (type (;9;) (func (result f32)))
    (type (;10;) (func (param i32 i32 i32) (result i32)))
    (type (;11;) (func (param i32 i32 i32)))
    (type (;12;) (func (param i32 i32 i32 i32)))
    (type (;13;) (func (param i32 f32)))
    (import "miden:core-intrinsics/intrinsics-felt@1.0.0" "from-u32" (func $miden_stdlib_sys::intrinsics::felt::extern_from_u32 (;0;) (type 0)))
    (import "miden:core-intrinsics/intrinsics-felt@1.0.0" "add" (func $miden_stdlib_sys::intrinsics::felt::extern_add (;1;) (type 1)))
    (import "miden:core-intrinsics/intrinsics-mem@1.0.0" "heap-base" (func $miden_sdk_alloc::heap_base (;2;) (type 2)))
    (import "miden:core-base/account@1.0.0" "incr-nonce" (func $miden_base_sys::bindings::account::extern_account_incr_nonce (;3;) (type 3)))
    (import "miden:core-base/account@1.0.0" "get-map-item" (func $miden_base_sys::bindings::storage::extern_get_storage_map_item (;4;) (type 4)))
    (import "miden:core-base/account@1.0.0" "set-map-item" (func $miden_base_sys::bindings::storage::extern_set_storage_map_item (;5;) (type 5)))
    (table (;0;) 3 3 funcref)
    (memory (;0;) 17)
    (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
    (export "memory" (memory 0))
    (export "miden:counter-contract/counter@0.1.0#get-count" (func $miden:counter-contract/counter@0.1.0#get-count))
    (export "miden:counter-contract/counter@0.1.0#increment-count" (func $miden:counter-contract/counter@0.1.0#increment-count))
    (export "cabi_realloc_wit_bindgen_0_28_0" (func $cabi_realloc_wit_bindgen_0_28_0))
    (export "cabi_realloc" (func $cabi_realloc))
    (elem (;0;) (i32.const 1) func $counter_contract::bindings::__link_custom_section_describing_imports $cabi_realloc)
    (func $__wasm_call_ctors (;6;) (type 6))
    (func $counter_contract::bindings::__link_custom_section_describing_imports (;7;) (type 6))
    (func $__rustc::__rust_alloc (;8;) (type 7) (param i32 i32) (result i32)
      i32.const 1048616
      local.get 1
      local.get 0
      call $<miden_sdk_alloc::BumpAlloc as core::alloc::global::GlobalAlloc>::alloc
    )
    (func $__rustc::__rust_realloc (;9;) (type 8) (param i32 i32 i32 i32) (result i32)
      block ;; label = @1
        i32.const 1048616
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
    (func $miden:counter-contract/counter@0.1.0#get-count (;10;) (type 9) (result f32)
      (local i32 i32 i32 f32)
      global.get $__stack_pointer
      local.tee 0
      local.set 1
      local.get 0
      i32.const 64
      i32.sub
      i32.const -32
      i32.and
      local.tee 2
      global.set $__stack_pointer
      call $wit_bindgen_rt::run_ctors_once
      i32.const 0
      local.set 0
      i32.const 0
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      local.set 3
      block ;; label = @1
        loop ;; label = @2
          local.get 0
          i32.const 16
          i32.eq
          br_if 1 (;@1;)
          local.get 2
          i32.const 32
          i32.add
          local.get 0
          i32.add
          local.get 3
          f32.store
          local.get 0
          i32.const 4
          i32.add
          local.set 0
          br 0 (;@2;)
        end
      end
      local.get 2
      local.get 2
      i64.load offset=40 align=4
      i64.store offset=8
      local.get 2
      local.get 2
      i64.load offset=32 align=4
      i64.store
      local.get 2
      i32.const 32
      i32.add
      i32.const 0
      local.get 2
      call $miden_base_sys::bindings::storage::get_map_item
      local.get 2
      f32.load offset=44
      local.set 3
      local.get 1
      global.set $__stack_pointer
      local.get 3
    )
    (func $miden:counter-contract/counter@0.1.0#increment-count (;11;) (type 9) (result f32)
      (local i32 i32 i32 f32)
      global.get $__stack_pointer
      local.tee 0
      local.set 1
      local.get 0
      i32.const 160
      i32.sub
      i32.const -32
      i32.and
      local.tee 2
      global.set $__stack_pointer
      call $wit_bindgen_rt::run_ctors_once
      i32.const 0
      local.set 0
      i32.const 0
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      local.set 3
      block ;; label = @1
        loop ;; label = @2
          local.get 0
          i32.const 16
          i32.eq
          br_if 1 (;@1;)
          local.get 2
          i32.const 32
          i32.add
          local.get 0
          i32.add
          local.get 3
          f32.store
          local.get 0
          i32.const 4
          i32.add
          local.set 0
          br 0 (;@2;)
        end
      end
      local.get 2
      local.get 2
      i64.load offset=40 align=4
      i64.store offset=8
      local.get 2
      local.get 2
      i64.load offset=32 align=4
      i64.store
      local.get 2
      i32.const 32
      i32.add
      i32.const 0
      local.get 2
      call $miden_base_sys::bindings::storage::get_map_item
      local.get 2
      f32.load offset=44
      i32.const 1
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      call $miden_stdlib_sys::intrinsics::felt::extern_add
      local.set 3
      local.get 2
      local.get 2
      i64.load offset=24
      i64.store offset=120
      local.get 2
      local.get 2
      i64.load offset=16
      i64.store offset=112
      local.get 2
      local.get 2
      i64.load offset=8
      i64.store offset=104
      local.get 2
      local.get 2
      i64.load
      i64.store offset=96
      local.get 2
      i32.const 128
      i32.add
      local.get 3
      call $<miden_stdlib_sys::intrinsics::word::Word as core::convert::From<miden_stdlib_sys::intrinsics::felt::Felt>>::from
      local.get 2
      i32.const 32
      i32.add
      i32.const 0
      local.get 2
      i32.const 96
      i32.add
      local.get 2
      i32.const 128
      i32.add
      call $miden_base_sys::bindings::storage::set_map_item
      i32.const 1
      call $miden_base_sys::bindings::account::incr_nonce
      local.get 1
      global.set $__stack_pointer
      local.get 3
    )
    (func $cabi_realloc_wit_bindgen_0_28_0 (;12;) (type 8) (param i32 i32 i32 i32) (result i32)
      local.get 0
      local.get 1
      local.get 2
      local.get 3
      call $wit_bindgen_rt::cabi_realloc
    )
    (func $wit_bindgen_rt::cabi_realloc (;13;) (type 8) (param i32 i32 i32 i32) (result i32)
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
    (func $wit_bindgen_rt::run_ctors_once (;14;) (type 6)
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
    (func $<miden_sdk_alloc::BumpAlloc as core::alloc::global::GlobalAlloc>::alloc (;15;) (type 10) (param i32 i32 i32) (result i32)
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
    (func $miden_base_sys::bindings::account::incr_nonce (;16;) (type 3) (param i32)
      local.get 0
      call $miden_base_sys::bindings::account::extern_account_incr_nonce
    )
    (func $miden_base_sys::bindings::storage::get_map_item (;17;) (type 11) (param i32 i32 i32)
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
    (func $miden_base_sys::bindings::storage::set_map_item (;18;) (type 12) (param i32 i32 i32 i32)
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
    (func $<miden_stdlib_sys::intrinsics::word::Word as core::convert::From<miden_stdlib_sys::intrinsics::felt::Felt>>::from (;19;) (type 13) (param i32 f32)
      (local f32 f32 f32)
      i32.const 0
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      local.set 2
      i32.const 0
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      local.set 3
      i32.const 0
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
      local.set 4
      local.get 0
      local.get 1
      f32.store offset=12
      local.get 0
      local.get 4
      f32.store offset=8
      local.get 0
      local.get 3
      f32.store offset=4
      local.get 0
      local.get 2
      f32.store
    )
    (func $core::ptr::alignment::Alignment::max (;20;) (type 7) (param i32 i32) (result i32)
      local.get 0
      local.get 1
      local.get 0
      local.get 1
      i32.gt_u
      select
    )
    (func $cabi_realloc (;21;) (type 8) (param i32 i32 i32 i32) (result i32)
      local.get 0
      local.get 1
      local.get 2
      local.get 3
      call $cabi_realloc_wit_bindgen_0_28_0
    )
    (data $.rodata (;0;) (i32.const 1048576) "\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00\02\00\00\00")
    (@custom "rodata,miden_account" (after data) "!counter-contract\95A simple example of a Miden counter contract using the Account Storage API\0b0.1.0\03\01\03\01\00\01\13count_map\019counter contract storage map")
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
  (alias export 2 "incr-nonce" (func (;3;)))
  (core func (;3;) (canon lower (func 3)))
  (alias export 2 "get-map-item" (func (;4;)))
  (core func (;4;) (canon lower (func 4)))
  (alias export 2 "set-map-item" (func (;5;)))
  (core func (;5;) (canon lower (func 5)))
  (core instance (;2;)
    (export "incr-nonce" (func 3))
    (export "get-map-item" (func 4))
    (export "set-map-item" (func 5))
  )
  (core instance (;3;) (instantiate 0
      (with "miden:core-intrinsics/intrinsics-felt@1.0.0" (instance 0))
      (with "miden:core-intrinsics/intrinsics-mem@1.0.0" (instance 1))
      (with "miden:core-base/account@1.0.0" (instance 2))
    )
  )
  (alias core export 3 "memory" (core memory (;0;)))
  (alias export 3 "felt" (type (;4;)))
  (type (;5;) (func (result 4)))
  (alias core export 3 "miden:counter-contract/counter@0.1.0#get-count" (core func (;6;)))
  (alias core export 3 "cabi_realloc" (core func (;7;)))
  (func (;6;) (type 5) (canon lift (core func 6)))
  (alias core export 3 "miden:counter-contract/counter@0.1.0#increment-count" (core func (;8;)))
  (func (;7;) (type 5) (canon lift (core func 8)))
  (alias export 3 "felt" (type (;6;)))
  (component (;0;)
    (type (;0;) (record (field "inner" f32)))
    (import "import-type-felt" (type (;1;) (eq 0)))
    (import "import-type-felt0" (type (;2;) (eq 1)))
    (type (;3;) (func (result 2)))
    (import "import-func-get-count" (func (;0;) (type 3)))
    (import "import-func-increment-count" (func (;1;) (type 3)))
    (export (;4;) "felt" (type 1))
    (type (;5;) (func (result 4)))
    (export (;2;) "get-count" (func 0) (func (type 5)))
    (export (;3;) "increment-count" (func 1) (func (type 5)))
  )
  (instance (;4;) (instantiate 0
      (with "import-func-get-count" (func 6))
      (with "import-func-increment-count" (func 7))
      (with "import-type-felt" (type 6))
      (with "import-type-felt0" (type 4))
    )
  )
  (export (;5;) "miden:counter-contract/counter@0.1.0" (instance 4))
  (@custom "description" "A simple example of a Miden counter contract using the Account Storage API")
  (@custom "version" "0.1.0")
)
