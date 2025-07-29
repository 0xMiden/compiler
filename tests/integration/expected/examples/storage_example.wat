(component
  (type (;0;)
    (instance
      (type (;0;) (func (param "a" u32) (result f32)))
      (export (;0;) "from-u32" (func (type 0)))
      (type (;1;) (func (param "a" f32) (param "b" f32) (result s32)))
      (export (;1;) "eq" (func (type 1)))
    )
  )
  (import "miden:core-intrinsics/intrinsics-felt@1.0.0" (instance (;0;) (type 0)))
  (type (;1;)
    (instance
      (type (;0;) (func (param "index" f32) (param "result-ptr" s32)))
      (export (;0;) "get-item" (func (type 0)))
      (type (;1;) (func (param "index" f32) (param "key0" f32) (param "key1" f32) (param "key2" f32) (param "key3" f32) (param "result-ptr" s32)))
      (export (;1;) "get-map-item" (func (type 1)))
      (type (;2;) (func (param "index" f32) (param "key0" f32) (param "key1" f32) (param "key2" f32) (param "key3" f32) (param "value0" f32) (param "value1" f32) (param "value2" f32) (param "value3" f32) (param "result-ptr" s32)))
      (export (;2;) "set-map-item" (func (type 2)))
    )
  )
  (import "miden:core-base/account@1.0.0" (instance (;1;) (type 1)))
  (type (;2;)
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
  (import "miden:base/core-types@1.0.0" (instance (;2;) (type 2)))
  (core module (;0;)
    (type (;0;) (func (param f32 f32) (result i32)))
    (type (;1;) (func (param f32 i32)))
    (type (;2;) (func (param f32 f32 f32 f32 f32 i32)))
    (type (;3;) (func (param f32 f32 f32 f32 f32 f32 f32 f32 f32 i32)))
    (type (;4;) (func (param i32) (result f32)))
    (type (;5;) (func))
    (type (;6;) (func (param f32 f32 f32 f32 f32 f32 f32 f32 f32)))
    (type (;7;) (func (param f32 f32 f32 f32) (result f32)))
    (type (;8;) (func (param i32 i32)))
    (type (;9;) (func (param i32 i32 i32)))
    (type (;10;) (func (param i32 i32 i32 i32)))
    (type (;11;) (func (param i32 f32)))
    (import "miden:core-intrinsics/intrinsics-felt@1.0.0" "eq" (func $miden_stdlib_sys::intrinsics::felt::extern_eq (;0;) (type 0)))
    (import "miden:core-base/account@1.0.0" "get-item" (func $miden_base_sys::bindings::storage::extern_get_storage_item (;1;) (type 1)))
    (import "miden:core-base/account@1.0.0" "get-map-item" (func $miden_base_sys::bindings::storage::extern_get_storage_map_item (;2;) (type 2)))
    (import "miden:core-base/account@1.0.0" "set-map-item" (func $miden_base_sys::bindings::storage::extern_set_storage_map_item (;3;) (type 3)))
    (import "miden:core-intrinsics/intrinsics-felt@1.0.0" "from-u32" (func $miden_stdlib_sys::intrinsics::felt::extern_from_u32 (;4;) (type 4)))
    (table (;0;) 2 2 funcref)
    (memory (;0;) 17)
    (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
    (global $GOT.data.internal.__memory_base (;1;) i32 i32.const 0)
    (export "memory" (memory 0))
    (export "miden:storage-example/foo@1.0.0#set-asset-qty" (func $miden:storage-example/foo@1.0.0#set-asset-qty))
    (export "miden:storage-example/foo@1.0.0#get-asset-qty" (func $miden:storage-example/foo@1.0.0#get-asset-qty))
    (elem (;0;) (i32.const 1) func $storage_example::bindings::__link_custom_section_describing_imports)
    (func $__wasm_call_ctors (;5;) (type 5))
    (func $storage_example::bindings::__link_custom_section_describing_imports (;6;) (type 5))
    (func $miden:storage-example/foo@1.0.0#set-asset-qty (;7;) (type 6) (param f32 f32 f32 f32 f32 f32 f32 f32 f32)
      (local i32)
      global.get $__stack_pointer
      i32.const 64
      i32.sub
      local.tee 9
      global.set $__stack_pointer
      call $wit_bindgen_rt::run_ctors_once
      local.get 9
      local.get 7
      f32.store offset=12
      local.get 9
      local.get 6
      f32.store offset=8
      local.get 9
      local.get 5
      f32.store offset=4
      local.get 9
      local.get 4
      f32.store
      local.get 9
      i32.const 16
      i32.add
      i32.const 0
      call $miden_base_sys::bindings::storage::get_item
      local.get 9
      f32.load offset=28
      local.set 5
      local.get 9
      f32.load offset=24
      local.set 6
      local.get 9
      f32.load offset=20
      local.set 7
      block ;; label = @1
        local.get 0
        local.get 9
        f32.load offset=16
        call $miden_stdlib_sys::intrinsics::felt::extern_eq
        i32.const 1
        i32.ne
        br_if 0 (;@1;)
        local.get 1
        local.get 7
        call $miden_stdlib_sys::intrinsics::felt::extern_eq
        i32.const 1
        i32.ne
        br_if 0 (;@1;)
        local.get 2
        local.get 6
        call $miden_stdlib_sys::intrinsics::felt::extern_eq
        i32.const 1
        i32.ne
        br_if 0 (;@1;)
        local.get 3
        local.get 5
        call $miden_stdlib_sys::intrinsics::felt::extern_eq
        i32.const 1
        i32.ne
        br_if 0 (;@1;)
        local.get 9
        i32.const 48
        i32.add
        local.get 8
        call $<miden_stdlib_sys::intrinsics::word::Word as core::convert::From<miden_stdlib_sys::intrinsics::felt::Felt>>::from
        local.get 9
        i32.const 16
        i32.add
        i32.const 1
        local.get 9
        local.get 9
        i32.const 48
        i32.add
        call $miden_base_sys::bindings::storage::set_map_item
      end
      local.get 9
      i32.const 64
      i32.add
      global.set $__stack_pointer
    )
    (func $miden:storage-example/foo@1.0.0#get-asset-qty (;8;) (type 7) (param f32 f32 f32 f32) (result f32)
      (local i32)
      global.get $__stack_pointer
      i32.const 32
      i32.sub
      local.tee 4
      global.set $__stack_pointer
      call $wit_bindgen_rt::run_ctors_once
      local.get 4
      local.get 3
      f32.store offset=12
      local.get 4
      local.get 2
      f32.store offset=8
      local.get 4
      local.get 1
      f32.store offset=4
      local.get 4
      local.get 0
      f32.store
      local.get 4
      i32.const 16
      i32.add
      i32.const 1
      local.get 4
      call $miden_base_sys::bindings::storage::get_map_item
      local.get 4
      f32.load offset=28
      local.set 3
      local.get 4
      i32.const 32
      i32.add
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
    (func $miden_base_sys::bindings::storage::get_item (;10;) (type 8) (param i32 i32)
      (local i32)
      global.get $__stack_pointer
      i32.const 16
      i32.sub
      local.tee 2
      global.set $__stack_pointer
      local.get 1
      call $<miden_stdlib_sys::intrinsics::felt::Felt as core::convert::From<u8>>::from
      local.get 2
      call $miden_base_sys::bindings::storage::extern_get_storage_item
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
    (func $miden_base_sys::bindings::storage::get_map_item (;11;) (type 9) (param i32 i32 i32)
      (local i32)
      global.get $__stack_pointer
      i32.const 16
      i32.sub
      local.tee 3
      global.set $__stack_pointer
      local.get 1
      call $<miden_stdlib_sys::intrinsics::felt::Felt as core::convert::From<u8>>::from
      local.get 2
      f32.load offset=12
      local.get 2
      f32.load offset=8
      local.get 2
      f32.load offset=4
      local.get 2
      f32.load
      local.get 3
      call $miden_base_sys::bindings::storage::extern_get_storage_map_item
      local.get 0
      local.get 3
      i64.load
      i64.const 32
      i64.rotl
      i64.store offset=8
      local.get 0
      local.get 3
      i64.load offset=8
      i64.const 32
      i64.rotl
      i64.store
      local.get 3
      i32.const 16
      i32.add
      global.set $__stack_pointer
    )
    (func $miden_base_sys::bindings::storage::set_map_item (;12;) (type 10) (param i32 i32 i32 i32)
      (local i32)
      global.get $__stack_pointer
      i32.const 32
      i32.sub
      local.tee 4
      global.set $__stack_pointer
      local.get 1
      call $<miden_stdlib_sys::intrinsics::felt::Felt as core::convert::From<u8>>::from
      local.get 2
      f32.load offset=12
      local.get 2
      f32.load offset=8
      local.get 2
      f32.load offset=4
      local.get 2
      f32.load
      local.get 3
      f32.load offset=12
      local.get 3
      f32.load offset=8
      local.get 3
      f32.load offset=4
      local.get 3
      f32.load
      local.get 4
      call $miden_base_sys::bindings::storage::extern_set_storage_map_item
      local.get 0
      local.get 4
      i64.load offset=16
      i64.const 32
      i64.rotl
      i64.store offset=24
      local.get 0
      local.get 4
      i64.load offset=24
      i64.const 32
      i64.rotl
      i64.store offset=16
      local.get 0
      local.get 4
      i64.load
      i64.const 32
      i64.rotl
      i64.store offset=8
      local.get 0
      local.get 4
      i64.load offset=8
      i64.const 32
      i64.rotl
      i64.store
      local.get 4
      i32.const 32
      i32.add
      global.set $__stack_pointer
    )
    (func $<miden_stdlib_sys::intrinsics::felt::Felt as core::convert::From<u8>>::from (;13;) (type 4) (param i32) (result f32)
      local.get 0
      i32.const 255
      i32.and
      f32.reinterpret_i32
    )
    (func $<miden_stdlib_sys::intrinsics::word::Word as core::convert::From<miden_stdlib_sys::intrinsics::felt::Felt>>::from (;14;) (type 11) (param i32 f32)
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
    (@custom "rodata,miden_account" (after data) "\1fstorage-example_A simple example of a Miden account storage API\0b0.1.0\03\01\05\00\00\00!owner_public_key\01\15test value9auth::rpo_falcon512::pub_key\01\01\01\1basset_qty_map\01\11test map")
  )
  (alias export 2 "felt" (type (;3;)))
  (alias export 2 "word" (type (;4;)))
  (alias export 2 "asset" (type (;5;)))
  (alias export 0 "eq" (func (;0;)))
  (core func (;0;) (canon lower (func 0)))
  (alias export 0 "from-u32" (func (;1;)))
  (core func (;1;) (canon lower (func 1)))
  (core instance (;0;)
    (export "eq" (func 0))
    (export "from-u32" (func 1))
  )
  (alias export 1 "get-item" (func (;2;)))
  (core func (;2;) (canon lower (func 2)))
  (alias export 1 "get-map-item" (func (;3;)))
  (core func (;3;) (canon lower (func 3)))
  (alias export 1 "set-map-item" (func (;4;)))
  (core func (;4;) (canon lower (func 4)))
  (core instance (;1;)
    (export "get-item" (func 2))
    (export "get-map-item" (func 3))
    (export "set-map-item" (func 4))
  )
  (core instance (;2;) (instantiate 0
      (with "miden:core-intrinsics/intrinsics-felt@1.0.0" (instance 0))
      (with "miden:core-base/account@1.0.0" (instance 1))
    )
  )
  (alias core export 2 "memory" (core memory (;0;)))
  (type (;6;) (func (param "pub-key" 4) (param "asset" 5) (param "qty" 3)))
  (alias core export 2 "miden:storage-example/foo@1.0.0#set-asset-qty" (core func (;5;)))
  (func (;5;) (type 6) (canon lift (core func 5)))
  (type (;7;) (func (param "asset" 5) (result 3)))
  (alias core export 2 "miden:storage-example/foo@1.0.0#get-asset-qty" (core func (;6;)))
  (func (;6;) (type 7) (canon lift (core func 6)))
  (alias export 2 "felt" (type (;8;)))
  (alias export 2 "word" (type (;9;)))
  (alias export 2 "asset" (type (;10;)))
  (component (;0;)
    (type (;0;) (record (field "inner" f32)))
    (import "import-type-felt" (type (;1;) (eq 0)))
    (type (;2;) (tuple 1 1 1 1))
    (type (;3;) (record (field "inner" 2)))
    (import "import-type-word" (type (;4;) (eq 3)))
    (type (;5;) (record (field "inner" 4)))
    (import "import-type-asset" (type (;6;) (eq 5)))
    (import "import-type-word0" (type (;7;) (eq 4)))
    (import "import-type-asset0" (type (;8;) (eq 6)))
    (import "import-type-felt0" (type (;9;) (eq 1)))
    (type (;10;) (func (param "pub-key" 7) (param "asset" 8) (param "qty" 9)))
    (import "import-func-set-asset-qty" (func (;0;) (type 10)))
    (type (;11;) (func (param "asset" 8) (result 9)))
    (import "import-func-get-asset-qty" (func (;1;) (type 11)))
    (export (;12;) "felt" (type 1))
    (export (;13;) "word" (type 4))
    (export (;14;) "asset" (type 6))
    (type (;15;) (func (param "pub-key" 13) (param "asset" 14) (param "qty" 12)))
    (export (;2;) "set-asset-qty" (func 0) (func (type 15)))
    (type (;16;) (func (param "asset" 14) (result 12)))
    (export (;3;) "get-asset-qty" (func 1) (func (type 16)))
  )
  (instance (;3;) (instantiate 0
      (with "import-func-set-asset-qty" (func 5))
      (with "import-func-get-asset-qty" (func 6))
      (with "import-type-felt" (type 8))
      (with "import-type-word" (type 9))
      (with "import-type-asset" (type 10))
      (with "import-type-word0" (type 4))
      (with "import-type-asset0" (type 5))
      (with "import-type-felt0" (type 3))
    )
  )
  (export (;4;) "miden:storage-example/foo@1.0.0" (instance 3))
)
