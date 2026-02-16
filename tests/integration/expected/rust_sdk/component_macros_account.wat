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
    (type (;1;) (func (param f32 f32 f32 f32 f32 f32 f32 f32 f32 f32 f32 f32) (result i32)))
    (type (;2;) (func (param i32 i32)))
    (table (;0;) 2 2 funcref)
    (memory (;0;) 17)
    (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
    (global $GOT.data.internal.__memory_base (;1;) i32 i32.const 0)
    (export "memory" (memory 0))
    (export "miden:component-macros-account/component-macros-account@0.1.0#test-custom-types" (func $miden:component-macros-account/component-macros-account@0.1.0#test-custom-types))
    (export "miden:component-macros-account/component-macros-account@0.1.0#test-custom-types2" (func $miden:component-macros-account/component-macros-account@0.1.0#test-custom-types2))
    (elem (;0;) (i32.const 1) func $component_macros_account::bindings::__link_custom_section_describing_imports)
    (func $__wasm_call_ctors (;0;) (type 0))
    (func $component_macros_account::bindings::__link_custom_section_describing_imports (;1;) (type 0))
    (func $miden:component-macros-account/component-macros-account@0.1.0#test-custom-types (;2;) (type 1) (param f32 f32 f32 f32 f32 f32 f32 f32 f32 f32 f32 f32) (result i32)
      (local i32 i32)
      global.get $__stack_pointer
      i32.const 32
      i32.sub
      local.tee 12
      global.set $__stack_pointer
      call $wit_bindgen::rt::run_ctors_once
      local.get 12
      local.get 2
      f32.store offset=28
      local.get 12
      local.get 1
      f32.store offset=24
      local.get 12
      local.get 8
      f32.store offset=20
      local.get 12
      local.get 0
      f32.store offset=16
      global.get $GOT.data.internal.__memory_base
      local.set 13
      local.get 12
      local.get 12
      i32.const 16
      i32.add
      call $<miden_field::word::Word as core::convert::From<[miden_field::wasm_miden::Felt; 4]>>::from
      local.get 13
      i32.const 1048584
      i32.add
      local.tee 13
      local.get 12
      i64.load
      i64.store align=4
      local.get 12
      i32.const 32
      i32.add
      global.set $__stack_pointer
      local.get 13
    )
    (func $miden:component-macros-account/component-macros-account@0.1.0#test-custom-types2 (;3;) (type 1) (param f32 f32 f32 f32 f32 f32 f32 f32 f32 f32 f32 f32) (result i32)
      (local i32)
      global.get $GOT.data.internal.__memory_base
      local.set 12
      call $wit_bindgen::rt::run_ctors_once
      local.get 12
      i32.const 1048584
      i32.add
      local.tee 12
      local.get 1
      f32.store offset=4
      local.get 12
      local.get 0
      f32.store
      local.get 12
    )
    (func $wit_bindgen::rt::run_ctors_once (;4;) (type 0)
      (local i32)
      block ;; label = @1
        global.get $GOT.data.internal.__memory_base
        i32.const 1048592
        i32.add
        i32.load8_u
        br_if 0 (;@1;)
        global.get $GOT.data.internal.__memory_base
        local.set 0
        call $__wasm_call_ctors
        local.get 0
        i32.const 1048592
        i32.add
        i32.const 1
        i32.store8
      end
    )
    (func $<miden_field::word::Word as core::convert::From<[miden_field::wasm_miden::Felt; 4]>>::from (;5;) (type 2) (param i32 i32)
      local.get 0
      local.get 1
      i64.load offset=8 align=4
      i64.store offset=8
      local.get 0
      local.get 1
      i64.load align=4
      i64.store
    )
    (data $.data (;0;) (i32.const 1048576) "\01\00\00\00\01\00\00\00")
    (@custom "rodata,miden_account" (after data) "1component_macros_account\01\0b0.1.0\03\01\00\00\00\00\00\00\00\00\00\00\00\00\00\00")
  )
  (alias export $miden:base/core-types@1.0.0 "asset" (type $asset (;1;)))
  (alias export $miden:base/core-types@1.0.0 "felt" (type $felt (;2;)))
  (alias export $miden:base/core-types@1.0.0 "word" (type $word (;3;)))
  (type (;4;) (variant (case "variant-a") (case "variant-b")))
  (type (;5;) (record (field "value" $felt)))
  (type (;6;) (record (field "nested" 5)))
  (type (;7;) (record (field "foo" $word) (field "asset" $asset)))
  (type (;8;) (record (field "bar" $felt) (field "baz" $felt)))
  (type (;9;) (record (field "inner1" $felt) (field "inner2" $felt)))
  (type (;10;) (record (field "bar" $felt) (field "baz" $felt)))
  (core instance $main (;0;) (instantiate $main))
  (alias core export $main "memory" (core memory $memory (;0;)))
  (type (;11;) (func (param "a" 7) (param "asset" $asset) (result 8)))
  (alias core export $main "miden:component-macros-account/component-macros-account@0.1.0#test-custom-types" (core func $miden:component-macros-account/component-macros-account@0.1.0#test-custom-types (;0;)))
  (func $test-custom-types (;0;) (type 11) (canon lift (core func $miden:component-macros-account/component-macros-account@0.1.0#test-custom-types) (memory $memory)))
  (type (;12;) (func (param "a" 7) (param "asset" $asset) (result 9)))
  (alias core export $main "miden:component-macros-account/component-macros-account@0.1.0#test-custom-types2" (core func $miden:component-macros-account/component-macros-account@0.1.0#test-custom-types2 (;1;)))
  (func $test-custom-types2 (;1;) (type 12) (canon lift (core func $miden:component-macros-account/component-macros-account@0.1.0#test-custom-types2) (memory $memory)))
  (alias export $miden:base/core-types@1.0.0 "felt" (type $"#type13 felt" (@name "felt") (;13;)))
  (alias export $miden:base/core-types@1.0.0 "word" (type $"#type14 word" (@name "word") (;14;)))
  (alias export $miden:base/core-types@1.0.0 "asset" (type $"#type15 asset" (@name "asset") (;15;)))
  (component $miden:component-macros-account/component-macros-account@0.1.0-shim-component (;0;)
    (type (;0;) (record (field "inner" f32)))
    (import "import-type-felt" (type (;1;) (eq 0)))
    (type (;2;) (record (field "a" 1) (field "b" 1) (field "c" 1) (field "d" 1)))
    (import "import-type-word" (type (;3;) (eq 2)))
    (type (;4;) (record (field "inner" 3)))
    (import "import-type-asset" (type (;5;) (eq 4)))
    (import "import-type-word0" (type (;6;) (eq 3)))
    (import "import-type-asset0" (type (;7;) (eq 5)))
    (type (;8;) (record (field "foo" 6) (field "asset" 7)))
    (import "import-type-struct-a" (type (;9;) (eq 8)))
    (import "import-type-felt0" (type (;10;) (eq 1)))
    (type (;11;) (record (field "bar" 10) (field "baz" 10)))
    (import "import-type-struct-b" (type (;12;) (eq 11)))
    (type (;13;) (func (param "a" 9) (param "asset" 7) (result 12)))
    (import "import-func-test-custom-types" (func (;0;) (type 13)))
    (type (;14;) (record (field "inner1" 10) (field "inner2" 10)))
    (import "import-type-struct-c" (type (;15;) (eq 14)))
    (type (;16;) (func (param "a" 9) (param "asset" 7) (result 15)))
    (import "import-func-test-custom-types2" (func (;1;) (type 16)))
    (export (;17;) "asset" (type 5))
    (export (;18;) "felt" (type 1))
    (export (;19;) "word" (type 3))
    (type (;20;) (variant (case "variant-a") (case "variant-b")))
    (export (;21;) "enum-a" (type 20))
    (type (;22;) (record (field "value" 18)))
    (export (;23;) "later-defined" (type 22))
    (type (;24;) (record (field "nested" 23)))
    (export (;25;) "forward-holder" (type 24))
    (type (;26;) (record (field "foo" 19) (field "asset" 17)))
    (export (;27;) "struct-a" (type 26))
    (type (;28;) (record (field "bar" 18) (field "baz" 18)))
    (export (;29;) "struct-b" (type 28))
    (type (;30;) (record (field "inner1" 18) (field "inner2" 18)))
    (export (;31;) "struct-c" (type 30))
    (type (;32;) (record (field "bar" 18) (field "baz" 18)))
    (export (;33;) "struct-d" (type 32))
    (type (;34;) (func (param "a" 27) (param "asset" 17) (result 29)))
    (export (;2;) "test-custom-types" (func 0) (func (type 34)))
    (type (;35;) (func (param "a" 27) (param "asset" 17) (result 31)))
    (export (;3;) "test-custom-types2" (func 1) (func (type 35)))
  )
  (instance $miden:component-macros-account/component-macros-account@0.1.0-shim-instance (;1;) (instantiate $miden:component-macros-account/component-macros-account@0.1.0-shim-component
      (with "import-func-test-custom-types" (func $test-custom-types))
      (with "import-func-test-custom-types2" (func $test-custom-types2))
      (with "import-type-felt" (type $"#type13 felt"))
      (with "import-type-word" (type $"#type14 word"))
      (with "import-type-asset" (type $"#type15 asset"))
      (with "import-type-word0" (type $word))
      (with "import-type-asset0" (type $asset))
      (with "import-type-struct-a" (type 7))
      (with "import-type-felt0" (type $felt))
      (with "import-type-struct-b" (type 8))
      (with "import-type-struct-c" (type 9))
    )
  )
  (export $miden:component-macros-account/component-macros-account@0.1.0 (;2;) "miden:component-macros-account/component-macros-account@0.1.0" (instance $miden:component-macros-account/component-macros-account@0.1.0-shim-instance))
)
