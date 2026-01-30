(component
  (type $ty-miden:base/core-types@1.0.0 (;0;)
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
  (import "miden:base/core-types@1.0.0" (instance $miden:base/core-types@1.0.0 (;0;) (type $ty-miden:base/core-types@1.0.0)))
  (core module $main (;0;)
    (type (;0;) (func))
    (type (;1;) (func (param i32)))
    (type (;2;) (func (param f32 f32 f32 f32) (result f32)))
    (type (;3;) (func (param f32 f32 f32 f32 f32 f32 f32 f32 f32)))
    (type (;4;) (func (param i32 f32)))
    (type (;5;) (func (param f32 f32) (result i32)))
    (type (;6;) (func (param i32) (result f32)))
    (type (;7;) (func (param i64) (result f32)))
    (type (;8;) (func (param f32 f32 i32)))
    (type (;9;) (func (param f32 f32 f32 f32 f32 f32 i32)))
    (type (;10;) (func (param f32 f32 f32 f32 f32 f32 f32 f32 f32 f32 i32)))
    (table (;0;) 2 2 funcref)
    (memory (;0;) 17)
    (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
    (global $GOT.data.internal.__memory_base (;1;) i32 i32.const 0)
    (export "memory" (memory 0))
    (export "miden:storage-example/foo@1.0.0#get-asset-qty" (func $miden:storage-example/foo@1.0.0#get-asset-qty))
    (export "miden:storage-example/foo@1.0.0#set-asset-qty" (func $miden:storage-example/foo@1.0.0#set-asset-qty))
    (elem (;0;) (i32.const 1) func $storage_example::bindings::__link_custom_section_describing_imports)
    (func $__wasm_call_ctors (;0;) (type 0))
    (func $storage_example::bindings::__link_custom_section_describing_imports (;1;) (type 0))
    (func $<storage_example::MyAccount as core::default::Default>::default (;2;) (type 1) (param i32)
      (local f32 f32 f32)
      i64.const 3042002143573699788
      call $intrinsics::felt::from_u64_unchecked
      local.set 1
      i64.const -637820373289844411
      call $intrinsics::felt::from_u64_unchecked
      local.set 2
      i64.const -6475110084634595035
      call $intrinsics::felt::from_u64_unchecked
      local.set 3
      local.get 0
      i64.const -6348547769199644661
      call $intrinsics::felt::from_u64_unchecked
      f32.store offset=12
      local.get 0
      local.get 3
      f32.store offset=8
      local.get 0
      local.get 2
      f32.store offset=4
      local.get 0
      local.get 1
      f32.store
    )
    (func $miden:storage-example/foo@1.0.0#get-asset-qty (;3;) (type 2) (param f32 f32 f32 f32) (result f32)
      (local i32)
      global.get $__stack_pointer
      i32.const 32
      i32.sub
      local.tee 4
      global.set $__stack_pointer
      call $wit_bindgen::rt::run_ctors_once
      local.get 4
      call $<storage_example::MyAccount as core::default::Default>::default
      local.get 4
      f32.load offset=12
      local.get 4
      f32.load offset=8
      local.get 3
      local.get 2
      local.get 1
      local.get 0
      local.get 4
      i32.const 16
      i32.add
      call $miden::protocol::active_account::get_map_item
      local.get 4
      f32.load offset=16
      local.set 0
      local.get 4
      i32.const 32
      i32.add
      global.set $__stack_pointer
      local.get 0
    )
    (func $miden:storage-example/foo@1.0.0#set-asset-qty (;4;) (type 3) (param f32 f32 f32 f32 f32 f32 f32 f32 f32)
      (local i32 f32 f32 f32)
      global.get $__stack_pointer
      i32.const 64
      i32.sub
      local.tee 9
      global.set $__stack_pointer
      call $wit_bindgen::rt::run_ctors_once
      local.get 9
      call $<storage_example::MyAccount as core::default::Default>::default
      local.get 9
      f32.load offset=4
      local.get 9
      f32.load
      local.get 9
      i32.const 32
      i32.add
      call $miden::protocol::active_account::get_item
      local.get 9
      f32.load offset=40
      local.set 10
      local.get 9
      f32.load offset=36
      local.set 11
      local.get 9
      f32.load offset=32
      local.set 12
      block ;; label = @1
        local.get 0
        local.get 9
        f32.load offset=44
        call $intrinsics::felt::eq
        i32.const 1
        i32.ne
        br_if 0 (;@1;)
        local.get 1
        local.get 10
        call $intrinsics::felt::eq
        i32.const 1
        i32.ne
        br_if 0 (;@1;)
        local.get 2
        local.get 11
        call $intrinsics::felt::eq
        i32.const 1
        i32.ne
        br_if 0 (;@1;)
        local.get 3
        local.get 12
        call $intrinsics::felt::eq
        i32.const 1
        i32.ne
        br_if 0 (;@1;)
        local.get 9
        f32.load offset=12
        local.set 0
        local.get 9
        f32.load offset=8
        local.set 1
        local.get 9
        i32.const 16
        i32.add
        local.get 8
        call $<miden_stdlib_sys::intrinsics::word::Word as core::convert::From<miden_field::wasm32::Felt>>::from
        local.get 0
        local.get 1
        local.get 7
        local.get 6
        local.get 5
        local.get 4
        local.get 9
        f32.load offset=28
        local.get 9
        f32.load offset=24
        local.get 9
        f32.load offset=20
        local.get 9
        f32.load offset=16
        local.get 9
        i32.const 32
        i32.add
        call $miden::protocol::native_account::set_map_item
      end
      local.get 9
      i32.const 64
      i32.add
      global.set $__stack_pointer
    )
    (func $wit_bindgen::rt::run_ctors_once (;5;) (type 0)
      (local i32)
      block ;; label = @1
        global.get $GOT.data.internal.__memory_base
        i32.const 1048584
        i32.add
        i32.load8_u
        br_if 0 (;@1;)
        global.get $GOT.data.internal.__memory_base
        local.set 0
        call $__wasm_call_ctors
        local.get 0
        i32.const 1048584
        i32.add
        i32.const 1
        i32.store8
      end
    )
    (func $<miden_stdlib_sys::intrinsics::word::Word as core::convert::From<miden_field::wasm32::Felt>>::from (;6;) (type 4) (param i32 f32)
      (local f32 f32 f32)
      i32.const 0
      call $intrinsics::felt::from_u32
      local.set 2
      i32.const 0
      call $intrinsics::felt::from_u32
      local.set 3
      i32.const 0
      call $intrinsics::felt::from_u32
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
    (func $intrinsics::felt::eq (;7;) (type 5) (param f32 f32) (result i32)
      unreachable
    )
    (func $intrinsics::felt::from_u32 (;8;) (type 6) (param i32) (result f32)
      unreachable
    )
    (func $intrinsics::felt::from_u64_unchecked (;9;) (type 7) (param i64) (result f32)
      unreachable
    )
    (func $miden::protocol::active_account::get_item (;10;) (type 8) (param f32 f32 i32)
      unreachable
    )
    (func $miden::protocol::active_account::get_map_item (;11;) (type 9) (param f32 f32 f32 f32 f32 f32 i32)
      unreachable
    )
    (func $miden::protocol::native_account::set_map_item (;12;) (type 10) (param f32 f32 f32 f32 f32 f32 f32 f32 f32 f32 i32)
      unreachable
    )
    (data $.data (;0;) (i32.const 1048576) "\01\00\00\00\01\00\00\00")
    (@custom "rodata,miden_account" (after data) "\1fstorage-example_A simple example of a Miden account storage API\0b0.1.0\03\01\02\006miden::component::miden_storage_example::asset_qty_map\01\01%asset quantity map\00\00\09word\00\00\09word\009miden::component::miden_storage_example::owner_public_key\00\01!owner public key\00\09word\00\00\00\00\00\00\00\00")
  )
  (alias export $miden:base/core-types@1.0.0 "felt" (type $felt (;1;)))
  (alias export $miden:base/core-types@1.0.0 "word" (type $word (;2;)))
  (alias export $miden:base/core-types@1.0.0 "asset" (type $asset (;3;)))
  (core instance $main (;0;) (instantiate $main))
  (alias core export $main "memory" (core memory $memory (;0;)))
  (type (;4;) (func (param "pub-key" $word) (param "asset" $asset) (param "qty" $felt)))
  (alias core export $main "miden:storage-example/foo@1.0.0#set-asset-qty" (core func $miden:storage-example/foo@1.0.0#set-asset-qty (;0;)))
  (func $set-asset-qty (;0;) (type 4) (canon lift (core func $miden:storage-example/foo@1.0.0#set-asset-qty)))
  (type (;5;) (func (param "asset" $asset) (result $felt)))
  (alias core export $main "miden:storage-example/foo@1.0.0#get-asset-qty" (core func $miden:storage-example/foo@1.0.0#get-asset-qty (;1;)))
  (func $get-asset-qty (;1;) (type 5) (canon lift (core func $miden:storage-example/foo@1.0.0#get-asset-qty)))
  (alias export $miden:base/core-types@1.0.0 "felt" (type $"#type6 felt" (@name "felt") (;6;)))
  (alias export $miden:base/core-types@1.0.0 "word" (type $"#type7 word" (@name "word") (;7;)))
  (alias export $miden:base/core-types@1.0.0 "asset" (type $"#type8 asset" (@name "asset") (;8;)))
  (component $miden:storage-example/foo@1.0.0-shim-component (;0;)
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
  (instance $miden:storage-example/foo@1.0.0-shim-instance (;1;) (instantiate $miden:storage-example/foo@1.0.0-shim-component
      (with "import-func-set-asset-qty" (func $set-asset-qty))
      (with "import-func-get-asset-qty" (func $get-asset-qty))
      (with "import-type-felt" (type $"#type6 felt"))
      (with "import-type-word" (type $"#type7 word"))
      (with "import-type-asset" (type $"#type8 asset"))
      (with "import-type-word0" (type $word))
      (with "import-type-asset0" (type $asset))
      (with "import-type-felt0" (type $felt))
    )
  )
  (export $miden:storage-example/foo@1.0.0 (;2;) "miden:storage-example/foo@1.0.0" (instance $miden:storage-example/foo@1.0.0-shim-instance))
)
