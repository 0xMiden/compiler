(component
  (type $ty-miden:base/core-types@1.0.0 (;0;)
    (instance
      (type (;0;) (record (field "inner" f32)))
      (export (;1;) "felt" (type (eq 0)))
      (type (;2;) (record (field "a" 1) (field "b" 1) (field "c" 1) (field "d" 1)))
      (export (;3;) "word" (type (eq 2)))
      (type (;4;) (record (field "inner" 3)))
      (export (;5;) "asset" (type (eq 4)))
    )
  )
  (import "miden:base/core-types@1.0.0" (instance $miden:base/core-types@1.0.0 (;0;) (type $ty-miden:base/core-types@1.0.0)))
  (core module $main (;0;)
    (type (;0;) (func))
    (type (;1;) (func (param i32)))
    (type (;2;) (func (param f32 f32 f32 f32) (result f32)))
    (type (;3;) (func (param f32 f32 f32 f32 f32 f32 f32 f32 f32)))
    (type (;4;) (func (param f32 f32) (result i32)))
    (type (;5;) (func (param i64) (result f32)))
    (type (;6;) (func (param i32 i32 i32) (result i32)))
    (type (;7;) (func (param f32 f32 i32)))
    (type (;8;) (func (param f32 f32 f32 f32 f32 f32 i32)))
    (type (;9;) (func (param f32 f32 f32 f32 f32 f32 f32 f32 f32 f32 i32)))
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
      (local i32 i32)
      global.get $__stack_pointer
      i32.const 64
      i32.sub
      local.tee 4
      global.set $__stack_pointer
      call $wit_bindgen::rt::run_ctors_once
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
      global.get $GOT.data.internal.__memory_base
      local.set 5
      local.get 4
      i32.const 16
      i32.add
      call $<storage_example::MyAccount as core::default::Default>::default
      local.get 4
      f32.load offset=28
      local.get 4
      f32.load offset=24
      local.get 4
      i32.const 3
      local.get 5
      i32.const 1048596
      i32.add
      local.tee 5
      call $<miden_field::word::Word as core::ops::index::Index<usize>>::index
      f32.load
      local.get 4
      i32.const 2
      local.get 5
      call $<miden_field::word::Word as core::ops::index::Index<usize>>::index
      f32.load
      local.get 4
      i32.const 1
      local.get 5
      call $<miden_field::word::Word as core::ops::index::Index<usize>>::index
      f32.load
      local.get 4
      i32.const 0
      local.get 5
      call $<miden_field::word::Word as core::ops::index::Index<usize>>::index
      f32.load
      local.get 4
      i32.const 48
      i32.add
      call $miden::protocol::active_account::get_map_item
      local.get 4
      local.get 4
      i64.load offset=48
      i64.const 32
      i64.rotl
      i64.store offset=40
      local.get 4
      local.get 4
      i64.load offset=56
      i64.const 32
      i64.rotl
      i64.store offset=32
      local.get 4
      i32.const 32
      i32.add
      i32.const 3
      local.get 5
      call $<miden_field::word::Word as core::ops::index::Index<usize>>::index
      f32.load
      local.set 3
      local.get 4
      i32.const 64
      i32.add
      global.set $__stack_pointer
      local.get 3
    )
    (func $miden:storage-example/foo@1.0.0#set-asset-qty (;4;) (type 3) (param f32 f32 f32 f32 f32 f32 f32 f32 f32)
      (local i32 f32 f32 f32 i32)
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
      i32.const 48
      i32.add
      call $miden::protocol::active_account::get_item
      local.get 9
      f32.load offset=56
      local.set 10
      local.get 9
      f32.load offset=52
      local.set 11
      local.get 9
      f32.load offset=48
      local.set 12
      block ;; label = @1
        local.get 0
        local.get 9
        f32.load offset=60
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
        local.get 7
        f32.store offset=28
        local.get 9
        local.get 6
        f32.store offset=24
        local.get 9
        local.get 5
        f32.store offset=20
        local.get 9
        local.get 4
        f32.store offset=16
        local.get 9
        f32.load offset=8
        local.set 0
        local.get 9
        f32.load offset=12
        local.set 1
        local.get 9
        i32.const 0
        i32.store offset=44
        local.get 9
        i64.const 0
        i64.store offset=36 align=4
        local.get 9
        local.get 8
        f32.store offset=32
        local.get 1
        local.get 0
        local.get 9
        i32.const 16
        i32.add
        i32.const 3
        global.get $GOT.data.internal.__memory_base
        i32.const 1048596
        i32.add
        local.tee 13
        call $<miden_field::word::Word as core::ops::index::Index<usize>>::index
        f32.load
        local.get 9
        i32.const 16
        i32.add
        i32.const 2
        local.get 13
        call $<miden_field::word::Word as core::ops::index::Index<usize>>::index
        f32.load
        local.get 9
        i32.const 16
        i32.add
        i32.const 1
        local.get 13
        call $<miden_field::word::Word as core::ops::index::Index<usize>>::index
        f32.load
        local.get 9
        i32.const 16
        i32.add
        i32.const 0
        local.get 13
        call $<miden_field::word::Word as core::ops::index::Index<usize>>::index
        f32.load
        local.get 9
        i32.const 32
        i32.add
        i32.const 3
        local.get 13
        call $<miden_field::word::Word as core::ops::index::Index<usize>>::index
        f32.load
        local.get 9
        i32.const 32
        i32.add
        i32.const 2
        local.get 13
        call $<miden_field::word::Word as core::ops::index::Index<usize>>::index
        f32.load
        local.get 9
        i32.const 32
        i32.add
        i32.const 1
        local.get 13
        call $<miden_field::word::Word as core::ops::index::Index<usize>>::index
        f32.load
        local.get 9
        i32.const 32
        i32.add
        i32.const 0
        local.get 13
        call $<miden_field::word::Word as core::ops::index::Index<usize>>::index
        f32.load
        local.get 9
        i32.const 48
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
        i32.const 1048612
        i32.add
        i32.load8_u
        br_if 0 (;@1;)
        global.get $GOT.data.internal.__memory_base
        local.set 0
        call $__wasm_call_ctors
        local.get 0
        i32.const 1048612
        i32.add
        i32.const 1
        i32.store8
      end
    )
    (func $intrinsics::felt::eq (;6;) (type 4) (param f32 f32) (result i32)
      unreachable
    )
    (func $intrinsics::felt::from_u64_unchecked (;7;) (type 5) (param i64) (result f32)
      unreachable
    )
    (func $<miden_field::word::Word as core::ops::index::Index<usize>>::index (;8;) (type 6) (param i32 i32 i32) (result i32)
      block ;; label = @1
        local.get 1
        i32.const 3
        i32.gt_u
        br_if 0 (;@1;)
        local.get 0
        local.get 1
        i32.const 2
        i32.shl
        i32.add
        return
      end
      unreachable
    )
    (func $miden::protocol::active_account::get_item (;9;) (type 7) (param f32 f32 i32)
      unreachable
    )
    (func $miden::protocol::active_account::get_map_item (;10;) (type 8) (param f32 f32 f32 f32 f32 f32 i32)
      unreachable
    )
    (func $miden::protocol::native_account::set_map_item (;11;) (type 9) (param f32 f32 f32 f32 f32 f32 f32 f32 f32 f32 i32)
      unreachable
    )
    (data $.rodata (;0;) (i32.const 1048576) "<redacted>\00")
    (data $.data (;1;) (i32.const 1048588) "\01\00\00\00\01\00\00\00\00\00\10\00\0a\00\00\00\00\00\00\00\00\00\00\00")
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
    (type (;2;) (record (field "a" 1) (field "b" 1) (field "c" 1) (field "d" 1)))
    (import "import-type-word" (type (;3;) (eq 2)))
    (type (;4;) (record (field "inner" 3)))
    (import "import-type-asset" (type (;5;) (eq 4)))
    (import "import-type-word0" (type (;6;) (eq 3)))
    (import "import-type-asset0" (type (;7;) (eq 5)))
    (import "import-type-felt0" (type (;8;) (eq 1)))
    (type (;9;) (func (param "pub-key" 6) (param "asset" 7) (param "qty" 8)))
    (import "import-func-set-asset-qty" (func (;0;) (type 9)))
    (type (;10;) (func (param "asset" 7) (result 8)))
    (import "import-func-get-asset-qty" (func (;1;) (type 10)))
    (export (;11;) "felt" (type 1))
    (export (;12;) "word" (type 3))
    (export (;13;) "asset" (type 5))
    (type (;14;) (func (param "pub-key" 12) (param "asset" 13) (param "qty" 11)))
    (export (;2;) "set-asset-qty" (func 0) (func (type 14)))
    (type (;15;) (func (param "asset" 13) (result 11)))
    (export (;3;) "get-asset-qty" (func 1) (func (type 15)))
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
