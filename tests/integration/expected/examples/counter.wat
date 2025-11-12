(component
  (type (;0;)
    (instance
      (type (;0;) (record (field "inner" f32)))
      (export (;1;) "felt" (type (eq 0)))
    )
  )
  (import "miden:base/core-types@1.0.0" (instance (;0;) (type 0)))
  (core module (;0;)
    (type (;0;) (func))
    (type (;1;) (func (param i32 i32)))
    (type (;2;) (func (result f32)))
    (type (;3;) (func (param i32) (result f32)))
    (type (;4;) (func (param i32 f32)))
    (type (;5;) (func (param f32 f32) (result f32)))
    (type (;6;) (func (param f32 f32 f32 f32 f32 i32)))
    (type (;7;) (func (param f32 f32 f32 f32 f32 f32 f32 f32 f32 i32)))
    (table (;0;) 2 2 funcref)
    (memory (;0;) 17)
    (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
    (global $GOT.data.internal.__memory_base (;1;) i32 i32.const 0)
    (export "memory" (memory 0))
    (export "miden:counter-contract/counter@0.1.0#get-count" (func $miden:counter-contract/counter@0.1.0#get-count))
    (export "miden:counter-contract/counter@0.1.0#increment-count" (func $miden:counter-contract/counter@0.1.0#increment-count))
    (elem (;0;) (i32.const 1) func $counter_contract::bindings::__link_custom_section_describing_imports)
    (func $__wasm_call_ctors (;0;) (type 0))
    (func $miden_base_sys::bindings::storage::get_map_item (;1;) (type 1) (param i32 i32)
      (local i32)
      global.get $__stack_pointer
      i32.const 32
      i32.sub
      local.tee 2
      global.set $__stack_pointer
      i32.const 0
      call $<miden_stdlib_sys::intrinsics::felt::Felt as core::convert::From<u8>>::from
      local.get 1
      f32.load offset=12
      local.get 1
      f32.load offset=8
      local.get 1
      f32.load offset=4
      local.get 1
      f32.load
      local.get 2
      call $miden::active_account::get_map_item
      local.get 2
      local.get 2
      i64.load offset=8
      i64.store offset=24
      local.get 2
      local.get 2
      i64.load
      i64.store offset=16
      local.get 0
      local.get 2
      i32.const 16
      i32.add
      call $miden_stdlib_sys::intrinsics::word::Word::reverse
      local.get 2
      i32.const 32
      i32.add
      global.set $__stack_pointer
    )
    (func $counter_contract::bindings::__link_custom_section_describing_imports (;2;) (type 0))
    (func $miden:counter-contract/counter@0.1.0#get-count (;3;) (type 2) (result f32)
      (local i32 f32 f32 f32)
      global.get $__stack_pointer
      i32.const 32
      i32.sub
      local.tee 0
      global.set $__stack_pointer
      call $wit_bindgen::rt::run_ctors_once
      i32.const 0
      call $intrinsics::felt::from_u32
      local.set 1
      i32.const 0
      call $intrinsics::felt::from_u32
      local.set 2
      i32.const 0
      call $intrinsics::felt::from_u32
      local.set 3
      local.get 0
      i32.const 1
      call $intrinsics::felt::from_u32
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
      local.get 0
      i32.const 16
      i32.add
      local.get 0
      call $miden_base_sys::bindings::storage::get_map_item
      local.get 0
      f32.load offset=28
      local.set 1
      local.get 0
      i32.const 32
      i32.add
      global.set $__stack_pointer
      local.get 1
    )
    (func $miden:counter-contract/counter@0.1.0#increment-count (;4;) (type 2) (result f32)
      (local i32 f32 f32 f32 f32 f32)
      global.get $__stack_pointer
      i32.const 128
      i32.sub
      local.tee 0
      global.set $__stack_pointer
      call $wit_bindgen::rt::run_ctors_once
      i32.const 0
      call $intrinsics::felt::from_u32
      local.set 1
      i32.const 0
      call $intrinsics::felt::from_u32
      local.set 2
      i32.const 0
      call $intrinsics::felt::from_u32
      local.set 3
      local.get 0
      i32.const 1
      call $intrinsics::felt::from_u32
      local.tee 4
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
      local.get 0
      i32.const 64
      i32.add
      local.get 0
      call $miden_base_sys::bindings::storage::get_map_item
      local.get 0
      i32.const 48
      i32.add
      local.get 0
      f32.load offset=76
      i32.const 1
      call $intrinsics::felt::from_u32
      call $intrinsics::felt::add
      local.tee 5
      call $<miden_stdlib_sys::intrinsics::word::Word as core::convert::From<miden_stdlib_sys::intrinsics::felt::Felt>>::from
      i32.const 0
      call $<miden_stdlib_sys::intrinsics::felt::Felt as core::convert::From<u8>>::from
      local.get 4
      local.get 3
      local.get 2
      local.get 1
      local.get 0
      f32.load offset=60
      local.get 0
      f32.load offset=56
      local.get 0
      f32.load offset=52
      local.get 0
      f32.load offset=48
      local.get 0
      i32.const 64
      i32.add
      call $miden::native_account::set_map_item
      local.get 0
      local.get 0
      i64.load offset=72
      i64.store offset=104
      local.get 0
      local.get 0
      i64.load offset=64
      i64.store offset=96
      local.get 0
      local.get 0
      i32.const 88
      i32.add
      i64.load
      i64.store offset=120
      local.get 0
      local.get 0
      i64.load offset=80
      i64.store offset=112
      local.get 0
      i32.const 16
      i32.add
      local.get 0
      i32.const 96
      i32.add
      call $miden_stdlib_sys::intrinsics::word::Word::reverse
      local.get 0
      i32.const 32
      i32.add
      local.get 0
      i32.const 112
      i32.add
      call $miden_stdlib_sys::intrinsics::word::Word::reverse
      local.get 0
      i32.const 128
      i32.add
      global.set $__stack_pointer
      local.get 5
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
    (func $<miden_stdlib_sys::intrinsics::felt::Felt as core::convert::From<u8>>::from (;6;) (type 3) (param i32) (result f32)
      local.get 0
      i32.const 255
      i32.and
      f32.reinterpret_i32
    )
    (func $miden_stdlib_sys::intrinsics::word::Word::reverse (;7;) (type 1) (param i32 i32)
      (local i32 i32 i32 f32)
      global.get $__stack_pointer
      i32.const 16
      i32.sub
      local.tee 2
      local.get 1
      i64.load offset=8
      i64.store offset=8 align=4
      local.get 2
      local.get 1
      i64.load
      i64.store align=4
      local.get 2
      i32.const 12
      i32.add
      local.set 3
      i32.const 0
      local.set 1
      block ;; label = @1
        loop ;; label = @2
          local.get 1
          i32.const 8
          i32.eq
          br_if 1 (;@1;)
          local.get 2
          local.get 1
          i32.add
          local.tee 4
          f32.load
          local.set 5
          local.get 4
          local.get 3
          i32.load
          i32.store
          local.get 3
          local.get 5
          f32.store
          local.get 1
          i32.const 4
          i32.add
          local.set 1
          local.get 3
          i32.const -4
          i32.add
          local.set 3
          br 0 (;@2;)
        end
      end
      local.get 0
      local.get 2
      i64.load offset=8 align=4
      i64.store offset=8
      local.get 0
      local.get 2
      i64.load align=4
      i64.store
    )
    (func $<miden_stdlib_sys::intrinsics::word::Word as core::convert::From<miden_stdlib_sys::intrinsics::felt::Felt>>::from (;8;) (type 4) (param i32 f32)
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
    (func $intrinsics::felt::add (;9;) (type 5) (param f32 f32) (result f32)
      unreachable
    )
    (func $intrinsics::felt::from_u32 (;10;) (type 3) (param i32) (result f32)
      unreachable
    )
    (func $miden::active_account::get_map_item (;11;) (type 6) (param f32 f32 f32 f32 f32 i32)
      unreachable
    )
    (func $miden::native_account::set_map_item (;12;) (type 7) (param f32 f32 f32 f32 f32 f32 f32 f32 f32 i32)
      unreachable
    )
    (data $.data (;0;) (i32.const 1048576) "\01\00\00\00\01\00\00\00")
    (@custom "rodata,miden_account" (after data) "!counter-contract\95A simple example of a Miden counter contract using the Account Storage API\0b0.1.0\03\01\03\01\00\00\13count_map\019counter contract storage map\01\00\00\00\00\00\00\00\00\00\00\00\00\00\00\00")
  )
  (alias export 0 "felt" (type (;1;)))
  (core instance (;0;) (instantiate 0))
  (alias core export 0 "memory" (core memory (;0;)))
  (type (;2;) (func (result 1)))
  (alias core export 0 "miden:counter-contract/counter@0.1.0#get-count" (core func (;0;)))
  (func (;0;) (type 2) (canon lift (core func 0)))
  (alias core export 0 "miden:counter-contract/counter@0.1.0#increment-count" (core func (;1;)))
  (func (;1;) (type 2) (canon lift (core func 1)))
  (alias export 0 "felt" (type (;3;)))
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
  (instance (;1;) (instantiate 0
      (with "import-func-get-count" (func 0))
      (with "import-func-increment-count" (func 1))
      (with "import-type-felt" (type 3))
      (with "import-type-felt0" (type 1))
    )
  )
  (export (;2;) "miden:counter-contract/counter@0.1.0" (instance 1))
)
