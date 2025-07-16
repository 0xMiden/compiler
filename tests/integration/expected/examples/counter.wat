(component
  (type (;0;)
    (instance
      (type (;0;) (func (param "a" u32) (result f32)))
      (export (;0;) "from-u32" (func (type 0)))
      (type (;1;) (func (param "a" f32) (param "b" f32) (result f32)))
      (export (;1;) "add" (func (type 1)))
    )
  )
  (import "miden:core-intrinsics/intrinsics-felt@1.0.0" (instance (;0;) (type 0)))
  (type (;1;)
    (instance
      (type (;0;) (func (param "index" f32) (param "key0" f32) (param "key1" f32) (param "key2" f32) (param "key3" f32) (param "result-ptr" s32)))
      (export (;0;) "get-map-item" (func (type 0)))
      (type (;1;) (func (param "index" f32) (param "key0" f32) (param "key1" f32) (param "key2" f32) (param "key3" f32) (param "value0" f32) (param "value1" f32) (param "value2" f32) (param "value3" f32) (param "result-ptr" s32)))
      (export (;1;) "set-map-item" (func (type 1)))
      (type (;2;) (func (param "value" u32)))
      (export (;2;) "incr-nonce" (func (type 2)))
    )
  )
  (import "miden:core-base/account@1.0.0" (instance (;1;) (type 1)))
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
    (type (;2;) (func (param i32)))
    (type (;3;) (func (param f32 f32 f32 f32 f32 i32)))
    (type (;4;) (func (param f32 f32 f32 f32 f32 f32 f32 f32 f32 i32)))
    (type (;5;) (func))
    (type (;6;) (func (result f32)))
    (type (;7;) (func (param i32 i32 i32)))
    (type (;8;) (func (param i32 i32 i32 i32)))
    (type (;9;) (func (param i32 f32)))
    (import "miden:core-intrinsics/intrinsics-felt@1.0.0" "from-u32" (func $miden_stdlib_sys::intrinsics::felt::extern_from_u32 (;0;) (type 0)))
    (import "miden:core-intrinsics/intrinsics-felt@1.0.0" "add" (func $miden_stdlib_sys::intrinsics::felt::extern_add (;1;) (type 1)))
    (import "miden:core-base/account@1.0.0" "incr-nonce" (func $miden_base_sys::bindings::account::extern_account_incr_nonce (;2;) (type 2)))
    (import "miden:core-base/account@1.0.0" "get-map-item" (func $miden_base_sys::bindings::storage::extern_get_storage_map_item (;3;) (type 3)))
    (import "miden:core-base/account@1.0.0" "set-map-item" (func $miden_base_sys::bindings::storage::extern_set_storage_map_item (;4;) (type 4)))
    (table (;0;) 2 2 funcref)
    (memory (;0;) 17)
    (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
    (global $GOT.data.internal.__memory_base (;1;) i32 i32.const 0)
    (export "memory" (memory 0))
    (export "miden:counter-contract/counter@0.1.0#get-count" (func $miden:counter-contract/counter@0.1.0#get-count))
    (export "miden:counter-contract/counter@0.1.0#increment-count" (func $miden:counter-contract/counter@0.1.0#increment-count))
    (elem (;0;) (i32.const 1) func $counter_contract::bindings::__link_custom_section_describing_imports)
    (func $__wasm_call_ctors (;5;) (type 5))
    (func $counter_contract::bindings::__link_custom_section_describing_imports (;6;) (type 5))
    (func $miden:counter-contract/counter@0.1.0#get-count (;7;) (type 6) (result f32)
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
    (func $miden:counter-contract/counter@0.1.0#increment-count (;8;) (type 6) (result f32)
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
    (func $wit_bindgen_rt::run_ctors_once (;9;) (type 5)
      (local i32)
      block ;; label = @1
        global.get $GOT.data.internal.__memory_base
        i32.const 1048616
        i32.add
        i32.load8_u
        br_if 0 (;@1;)
        global.get $GOT.data.internal.__memory_base
        local.set 0
        call $__wasm_call_ctors
        local.get 0
        i32.const 1048616
        i32.add
        i32.const 1
        i32.store8
      end
    )
    (func $miden_base_sys::bindings::account::incr_nonce (;10;) (type 2) (param i32)
      local.get 0
      call $miden_base_sys::bindings::account::extern_account_incr_nonce
    )
    (func $miden_base_sys::bindings::storage::get_map_item (;11;) (type 7) (param i32 i32 i32)
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
    (func $miden_base_sys::bindings::storage::set_map_item (;12;) (type 8) (param i32 i32 i32 i32)
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
    (func $<miden_stdlib_sys::intrinsics::word::Word as core::convert::From<miden_stdlib_sys::intrinsics::felt::Felt>>::from (;13;) (type 9) (param i32 f32)
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
    (data $.data (;0;) (i32.const 1048576) "\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00\01\00\00\00")
    (@custom "rodata,miden_account" (after data) "!counter-contract\95A simple example of a Miden counter contract using the Account Storage API\0b0.1.0\03\01\03\01\00\01\13count_map\019counter contract storage map")
  )
  (alias export 0 "from-u32" (func (;0;)))
  (core func (;0;) (canon lower (func 0)))
  (alias export 0 "add" (func (;1;)))
  (core func (;1;) (canon lower (func 1)))
  (core instance (;0;)
    (export "from-u32" (func 0))
    (export "add" (func 1))
  )
  (alias export 1 "incr-nonce" (func (;2;)))
  (core func (;2;) (canon lower (func 2)))
  (alias export 1 "get-map-item" (func (;3;)))
  (core func (;3;) (canon lower (func 3)))
  (alias export 1 "set-map-item" (func (;4;)))
  (core func (;4;) (canon lower (func 4)))
  (core instance (;1;)
    (export "incr-nonce" (func 2))
    (export "get-map-item" (func 3))
    (export "set-map-item" (func 4))
  )
  (core instance (;2;) (instantiate 0
      (with "miden:core-intrinsics/intrinsics-felt@1.0.0" (instance 0))
      (with "miden:core-base/account@1.0.0" (instance 1))
    )
  )
  (alias core export 2 "memory" (core memory (;0;)))
  (alias export 2 "felt" (type (;3;)))
  (type (;4;) (func (result 3)))
  (alias core export 2 "miden:counter-contract/counter@0.1.0#get-count" (core func (;5;)))
  (func (;5;) (type 4) (canon lift (core func 5)))
  (alias core export 2 "miden:counter-contract/counter@0.1.0#increment-count" (core func (;6;)))
  (func (;6;) (type 4) (canon lift (core func 6)))
  (alias export 2 "felt" (type (;5;)))
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
  (instance (;3;) (instantiate 0
      (with "import-func-get-count" (func 5))
      (with "import-func-increment-count" (func 6))
      (with "import-type-felt" (type 5))
      (with "import-type-felt0" (type 3))
    )
  )
  (export (;4;) "miden:counter-contract/counter@0.1.0" (instance 3))
)
