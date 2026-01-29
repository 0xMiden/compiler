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
  (alias export $miden:base/core-types@1.0.0 "word" (type $word (;1;)))
  (alias export $miden:base/core-types@1.0.0 "asset" (type $asset (;2;)))
  (alias export $miden:base/core-types@1.0.0 "felt" (type $felt (;3;)))
  (type $ty-miden:component-macros-account/component-macros-account@0.1.0 (;4;)
    (instance
      (alias outer 1 $word (type (;0;)))
      (export (;1;) "word" (type (eq 0)))
      (alias outer 1 $asset (type (;2;)))
      (export (;3;) "asset" (type (eq 2)))
      (type (;4;) (record (field "foo" 1) (field "asset" 3)))
      (export (;5;) "struct-a" (type (eq 4)))
      (alias outer 1 $felt (type (;6;)))
      (export (;7;) "felt" (type (eq 6)))
      (type (;8;) (record (field "bar" 7) (field "baz" 7)))
      (export (;9;) "struct-b" (type (eq 8)))
      (type (;10;) (func (param "a" 5) (param "asset" 3) (result 9)))
      (export (;0;) "test-custom-types" (func (type 10)))
    )
  )
  (import "miden:component-macros-account/component-macros-account@0.1.0" (instance $miden:component-macros-account/component-macros-account@0.1.0 (;1;) (type $ty-miden:component-macros-account/component-macros-account@0.1.0)))
  (core module $main (;0;)
    (type (;0;) (func (param f32 f32 f32 f32 f32 f32 f32 f32 f32 f32 f32 f32 i32)))
    (type (;1;) (func))
    (type (;2;) (func (param f32 f32 f32 f32)))
    (type (;3;) (func (param f32 f32) (result i32)))
    (type (;4;) (func (param i32) (result f32)))
    (type (;5;) (func (param i64) (result f32)))
    (import "miden:component-macros-account/component-macros-account@0.1.0" "test-custom-types" (func $component_macros_note::bindings::miden::component_macros_account::component_macros_account::test_custom_types::wit_import22 (;0;) (type 0)))
    (table (;0;) 2 2 funcref)
    (memory (;0;) 17)
    (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
    (global $GOT.data.internal.__memory_base (;1;) i32 i32.const 0)
    (export "memory" (memory 0))
    (export "miden:base/note-script@1.0.0#run" (func $miden:base/note-script@1.0.0#run))
    (elem (;0;) (i32.const 1) func $component_macros_note::bindings::__link_custom_section_describing_imports)
    (func $__wasm_call_ctors (;1;) (type 1))
    (func $component_macros_note::bindings::__link_custom_section_describing_imports (;2;) (type 1))
    (func $miden:base/note-script@1.0.0#run (;3;) (type 2) (param f32 f32 f32 f32)
      (local i32 f32 f32 f32 f32 f32 f32 f32 f32)
      global.get $__stack_pointer
      i32.const 16
      i32.sub
      local.tee 4
      global.set $__stack_pointer
      call $wit_bindgen::rt::run_ctors_once
      i64.const 11
      call $intrinsics::felt::from_u64_unchecked
      local.set 5
      i64.const 22
      call $intrinsics::felt::from_u64_unchecked
      local.set 6
      i64.const 33
      call $intrinsics::felt::from_u64_unchecked
      local.set 7
      i64.const 44
      call $intrinsics::felt::from_u64_unchecked
      local.set 8
      i32.const 99
      call $intrinsics::felt::from_u32
      local.set 9
      i32.const 88
      call $intrinsics::felt::from_u32
      local.set 10
      i32.const 77
      call $intrinsics::felt::from_u32
      local.set 11
      i32.const 66
      call $intrinsics::felt::from_u32
      local.set 12
      local.get 4
      i64.const 0
      i64.store offset=8
      local.get 5
      local.get 6
      local.get 7
      local.get 8
      local.get 9
      local.get 10
      local.get 11
      local.get 12
      local.get 9
      local.get 10
      local.get 11
      local.get 12
      local.get 4
      i32.const 8
      i32.add
      call $component_macros_note::bindings::miden::component_macros_account::component_macros_account::test_custom_types::wit_import22
      local.get 4
      f32.load offset=12
      local.set 10
      block ;; label = @1
        local.get 4
        f32.load offset=8
        local.get 5
        call $intrinsics::felt::eq
        i32.const 1
        i32.ne
        br_if 0 (;@1;)
        local.get 10
        local.get 9
        call $intrinsics::felt::eq
        i32.const 1
        i32.ne
        br_if 0 (;@1;)
        local.get 4
        i32.const 16
        i32.add
        global.set $__stack_pointer
        return
      end
      unreachable
    )
    (func $wit_bindgen::rt::run_ctors_once (;4;) (type 1)
      (local i32)
      block ;; label = @1
        global.get $GOT.data.internal.__memory_base
        i32.const 1048588
        i32.add
        i32.load8_u
        br_if 0 (;@1;)
        global.get $GOT.data.internal.__memory_base
        local.set 0
        call $__wasm_call_ctors
        local.get 0
        i32.const 1048588
        i32.add
        i32.const 1
        i32.store8
      end
    )
    (func $intrinsics::felt::eq (;5;) (type 3) (param f32 f32) (result i32)
      unreachable
    )
    (func $intrinsics::felt::from_u32 (;6;) (type 4) (param i32) (result f32)
      unreachable
    )
    (func $intrinsics::felt::from_u64_unchecked (;7;) (type 5) (param i64) (result f32)
      unreachable
    )
    (data $.data (;0;) (i32.const 1048576) "\01\00\00\00\01\00\00\00\01\00\00\00")
  )
  (core module $wit-component-shim-module (;1;)
    (type (;0;) (func (param f32 f32 f32 f32 f32 f32 f32 f32 f32 f32 f32 f32 i32)))
    (table (;0;) 1 1 funcref)
    (export "0" (func 0))
    (export "$imports" (table 0))
    (func (;0;) (type 0) (param f32 f32 f32 f32 f32 f32 f32 f32 f32 f32 f32 f32 i32)
      local.get 0
      local.get 1
      local.get 2
      local.get 3
      local.get 4
      local.get 5
      local.get 6
      local.get 7
      local.get 8
      local.get 9
      local.get 10
      local.get 11
      local.get 12
      i32.const 0
      call_indirect (type 0)
    )
  )
  (core module $wit-component-fixup (;2;)
    (type (;0;) (func (param f32 f32 f32 f32 f32 f32 f32 f32 f32 f32 f32 f32 i32)))
    (import "" "0" (func (;0;) (type 0)))
    (import "" "$imports" (table (;0;) 1 1 funcref))
    (elem (;0;) (i32.const 0) func 0)
  )
  (core instance $wit-component-shim-instance (;0;) (instantiate $wit-component-shim-module))
  (alias export $miden:base/core-types@1.0.0 "word" (type $"#type5 word" (@name "word") (;5;)))
  (alias core export $wit-component-shim-instance "0" (core func $indirect-miden:component-macros-account/component-macros-account@0.1.0-test-custom-types (;0;)))
  (core instance $miden:component-macros-account/component-macros-account@0.1.0 (;1;)
    (export "test-custom-types" (func $indirect-miden:component-macros-account/component-macros-account@0.1.0-test-custom-types))
  )
  (core instance $main (;2;) (instantiate $main
      (with "miden:component-macros-account/component-macros-account@0.1.0" (instance $miden:component-macros-account/component-macros-account@0.1.0))
    )
  )
  (alias core export $main "memory" (core memory $memory (;0;)))
  (alias core export $wit-component-shim-instance "$imports" (core table $"shim table" (;0;)))
  (alias export $miden:component-macros-account/component-macros-account@0.1.0 "test-custom-types" (func $test-custom-types (;0;)))
  (core func $"#core-func1 indirect-miden:component-macros-account/component-macros-account@0.1.0-test-custom-types" (@name "indirect-miden:component-macros-account/component-macros-account@0.1.0-test-custom-types") (;1;) (canon lower (func $test-custom-types) (memory $memory)))
  (core instance $fixup-args (;3;)
    (export "$imports" (table $"shim table"))
    (export "0" (func $"#core-func1 indirect-miden:component-macros-account/component-macros-account@0.1.0-test-custom-types"))
  )
  (core instance $fixup (;4;) (instantiate $wit-component-fixup
      (with "" (instance $fixup-args))
    )
  )
  (type (;6;) (func (param "arg" $"#type5 word")))
  (alias core export $main "miden:base/note-script@1.0.0#run" (core func $miden:base/note-script@1.0.0#run (;2;)))
  (func $run (;1;) (type 6) (canon lift (core func $miden:base/note-script@1.0.0#run)))
  (alias export $miden:base/core-types@1.0.0 "felt" (type $"#type7 felt" (@name "felt") (;7;)))
  (alias export $miden:base/core-types@1.0.0 "word" (type $"#type8 word" (@name "word") (;8;)))
  (component $miden:base/note-script@1.0.0-shim-component (;0;)
    (type (;0;) (record (field "inner" f32)))
    (import "import-type-felt" (type (;1;) (eq 0)))
    (type (;2;) (tuple 1 1 1 1))
    (type (;3;) (record (field "inner" 2)))
    (import "import-type-word" (type (;4;) (eq 3)))
    (import "import-type-word0" (type (;5;) (eq 4)))
    (type (;6;) (func (param "arg" 5)))
    (import "import-func-run" (func (;0;) (type 6)))
    (export (;7;) "word" (type 4))
    (type (;8;) (func (param "arg" 7)))
    (export (;1;) "run" (func 0) (func (type 8)))
  )
  (instance $miden:base/note-script@1.0.0-shim-instance (;2;) (instantiate $miden:base/note-script@1.0.0-shim-component
      (with "import-func-run" (func $run))
      (with "import-type-felt" (type $"#type7 felt"))
      (with "import-type-word" (type $"#type8 word"))
      (with "import-type-word0" (type $"#type5 word"))
    )
  )
  (export $miden:base/note-script@1.0.0 (;3;) "miden:base/note-script@1.0.0" (instance $miden:base/note-script@1.0.0-shim-instance))
)
