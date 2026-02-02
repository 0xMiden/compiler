(component
  (type $ty-miden:base/core-types@1.0.0 (;0;)
    (instance
      (type (;0;) (record (field "inner" f32)))
      (export (;1;) "felt" (type (eq 0)))
    )
  )
  (import "miden:base/core-types@1.0.0" (instance $miden:base/core-types@1.0.0 (;0;) (type $ty-miden:base/core-types@1.0.0)))
  (core module $main (;0;)
    (type (;0;) (func))
    (type (;1;) (func (param i32 f32 f32 i32)))
    (type (;2;) (func (param i32)))
    (type (;3;) (func (result f32)))
    (type (;4;) (func (param i32 f32)))
    (type (;5;) (func (param f32 f32) (result f32)))
    (type (;6;) (func (param i32) (result f32)))
    (type (;7;) (func (param i64) (result f32)))
    (type (;8;) (func (param f32 f32 f32 f32 f32 f32 i32)))
    (type (;9;) (func (param f32 f32 f32 f32 f32 f32 f32 f32 f32 f32 i32)))
    (table (;0;) 2 2 funcref)
    (memory (;0;) 17)
    (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
    (global $GOT.data.internal.__memory_base (;1;) i32 i32.const 0)
    (export "memory" (memory 0))
    (export "miden:counter-contract/counter-contract@0.1.0#get-count" (func $miden:counter-contract/counter-contract@0.1.0#get-count))
    (export "miden:counter-contract/counter-contract@0.1.0#increment-count" (func $miden:counter-contract/counter-contract@0.1.0#increment-count))
    (elem (;0;) (i32.const 1) func $counter_contract::bindings::__link_custom_section_describing_imports)
    (func $__wasm_call_ctors (;0;) (type 0))
    (func $counter_contract::bindings::__link_custom_section_describing_imports (;1;) (type 0))
    (func $miden_base_sys::bindings::storage::get_map_item (;2;) (type 1) (param i32 f32 f32 i32)
      (local i32)
      global.get $__stack_pointer
      i32.const 16
      i32.sub
      local.tee 4
      global.set $__stack_pointer
      local.get 2
      local.get 1
      local.get 3
      f32.load offset=12
      local.get 3
      f32.load offset=8
      local.get 3
      f32.load offset=4
      local.get 3
      f32.load
      local.get 4
      call $miden::protocol::active_account::get_map_item
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
      i32.const 16
      i32.add
      global.set $__stack_pointer
    )
    (func $<counter_contract::CounterContract as core::default::Default>::default (;3;) (type 2) (param i32)
      (local f32)
      i64.const -6649237899119803499
      call $intrinsics::felt::from_u64_unchecked
      local.set 1
      local.get 0
      i64.const 1466204381252459361
      call $intrinsics::felt::from_u64_unchecked
      f32.store offset=4
      local.get 0
      local.get 1
      f32.store
    )
    (func $miden:counter-contract/counter-contract@0.1.0#get-count (;4;) (type 3) (result f32)
      (local i32 f32 f32 f32 f32 f32)
      global.get $__stack_pointer
      i32.const 48
      i32.sub
      local.tee 0
      global.set $__stack_pointer
      call $wit_bindgen::rt::run_ctors_once
      local.get 0
      i32.const 8
      i32.add
      call $<counter_contract::CounterContract as core::default::Default>::default
      local.get 0
      f32.load offset=12
      local.set 1
      local.get 0
      f32.load offset=8
      local.set 2
      i64.const 0
      call $intrinsics::felt::from_u64_unchecked
      local.set 3
      i64.const 0
      call $intrinsics::felt::from_u64_unchecked
      local.set 4
      i64.const 0
      call $intrinsics::felt::from_u64_unchecked
      local.set 5
      local.get 0
      i64.const 1
      call $intrinsics::felt::from_u64_unchecked
      f32.store offset=28
      local.get 0
      local.get 5
      f32.store offset=24
      local.get 0
      local.get 4
      f32.store offset=20
      local.get 0
      local.get 3
      f32.store offset=16
      local.get 0
      i32.const 32
      i32.add
      local.get 2
      local.get 1
      local.get 0
      i32.const 16
      i32.add
      call $miden_base_sys::bindings::storage::get_map_item
      local.get 0
      f32.load offset=44
      local.set 1
      local.get 0
      i32.const 48
      i32.add
      global.set $__stack_pointer
      local.get 1
    )
    (func $miden:counter-contract/counter-contract@0.1.0#increment-count (;5;) (type 3) (result f32)
      (local i32 f32 f32 f32 f32 f32 f32 f32)
      global.get $__stack_pointer
      i32.const 80
      i32.sub
      local.tee 0
      global.set $__stack_pointer
      call $wit_bindgen::rt::run_ctors_once
      local.get 0
      i32.const 8
      i32.add
      call $<counter_contract::CounterContract as core::default::Default>::default
      local.get 0
      f32.load offset=12
      local.set 1
      local.get 0
      f32.load offset=8
      local.set 2
      i64.const 0
      call $intrinsics::felt::from_u64_unchecked
      local.set 3
      i64.const 0
      call $intrinsics::felt::from_u64_unchecked
      local.set 4
      i64.const 0
      call $intrinsics::felt::from_u64_unchecked
      local.set 5
      local.get 0
      i64.const 1
      call $intrinsics::felt::from_u64_unchecked
      local.tee 6
      f32.store offset=28
      local.get 0
      local.get 5
      f32.store offset=24
      local.get 0
      local.get 4
      f32.store offset=20
      local.get 0
      local.get 3
      f32.store offset=16
      local.get 0
      i32.const 48
      i32.add
      local.get 2
      local.get 1
      local.get 0
      i32.const 16
      i32.add
      call $miden_base_sys::bindings::storage::get_map_item
      local.get 0
      i32.const 32
      i32.add
      local.get 0
      f32.load offset=60
      i32.const 1
      call $intrinsics::felt::from_u32
      call $intrinsics::felt::add
      local.tee 7
      call $<miden_stdlib_sys::intrinsics::word::Word as core::convert::From<miden_field::wasm32::Felt>>::from
      local.get 1
      local.get 2
      local.get 6
      local.get 5
      local.get 4
      local.get 3
      local.get 0
      f32.load offset=44
      local.get 0
      f32.load offset=40
      local.get 0
      f32.load offset=36
      local.get 0
      f32.load offset=32
      local.get 0
      i32.const 48
      i32.add
      call $miden::protocol::native_account::set_map_item
      local.get 0
      i32.const 80
      i32.add
      global.set $__stack_pointer
      local.get 7
    )
    (func $wit_bindgen::rt::run_ctors_once (;6;) (type 0)
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
    (func $<miden_stdlib_sys::intrinsics::word::Word as core::convert::From<miden_field::wasm32::Felt>>::from (;7;) (type 4) (param i32 f32)
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
    (func $intrinsics::felt::add (;8;) (type 5) (param f32 f32) (result f32)
      unreachable
    )
    (func $intrinsics::felt::from_u32 (;9;) (type 6) (param i32) (result f32)
      unreachable
    )
    (func $intrinsics::felt::from_u64_unchecked (;10;) (type 7) (param i64) (result f32)
      unreachable
    )
    (func $miden::protocol::active_account::get_map_item (;11;) (type 8) (param f32 f32 f32 f32 f32 f32 i32)
      unreachable
    )
    (func $miden::protocol::native_account::set_map_item (;12;) (type 9) (param f32 f32 f32 f32 f32 f32 f32 f32 f32 f32 i32)
      unreachable
    )
    (data $.data (;0;) (i32.const 1048576) "\01\00\00\00\01\00\00\00")
    (@custom "rodata,miden_account" (after data) "!counter-contract\95A simple example of a Miden counter contract using the Account Storage API\0b0.1.0\03\01\01\003miden::component::miden_counter_contract::count_map\01\019counter contract storage map\00\00\09word\00\00\09word\00\00\00\00\00\00\00\00\00")
  )
  (alias export $miden:base/core-types@1.0.0 "felt" (type $felt (;1;)))
  (core instance $main (;0;) (instantiate $main))
  (alias core export $main "memory" (core memory $memory (;0;)))
  (type (;2;) (func (result $felt)))
  (alias core export $main "miden:counter-contract/counter-contract@0.1.0#get-count" (core func $miden:counter-contract/counter-contract@0.1.0#get-count (;0;)))
  (func $get-count (;0;) (type 2) (canon lift (core func $miden:counter-contract/counter-contract@0.1.0#get-count)))
  (alias core export $main "miden:counter-contract/counter-contract@0.1.0#increment-count" (core func $miden:counter-contract/counter-contract@0.1.0#increment-count (;1;)))
  (func $increment-count (;1;) (type 2) (canon lift (core func $miden:counter-contract/counter-contract@0.1.0#increment-count)))
  (alias export $miden:base/core-types@1.0.0 "felt" (type $"#type3 felt" (@name "felt") (;3;)))
  (component $miden:counter-contract/counter-contract@0.1.0-shim-component (;0;)
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
  (instance $miden:counter-contract/counter-contract@0.1.0-shim-instance (;1;) (instantiate $miden:counter-contract/counter-contract@0.1.0-shim-component
      (with "import-func-get-count" (func $get-count))
      (with "import-func-increment-count" (func $increment-count))
      (with "import-type-felt" (type $"#type3 felt"))
      (with "import-type-felt0" (type $felt))
    )
  )
  (export $miden:counter-contract/counter-contract@0.1.0 (;2;) "miden:counter-contract/counter-contract@0.1.0" (instance $miden:counter-contract/counter-contract@0.1.0-shim-instance))
)
