(component
  (type (;0;)
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
  (import "miden:base/core-types@1.0.0" (instance (;0;) (type 0)))
  (core module (;0;)
    (type (;0;) (func))
    (type (;1;) (func (param f32 f32 f32 f32 f32 f32 f32 f32 f32 f32 f32 f32) (result i32)))
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
      (local i32)
      global.get $GOT.data.internal.__memory_base
      local.set 12
      call $wit_bindgen::rt::run_ctors_once
      local.get 12
      i32.const 1048584
      i32.add
      local.tee 12
      local.get 8
      f32.store offset=4
      local.get 12
      local.get 0
      f32.store
      local.get 12
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
    (data $.data (;0;) (i32.const 1048576) "\01\00\00\00\01\00\00\00")
    (@custom "rodata,miden_account" (after data) "1component_macros_account\01\0b0.1.0\03\01\01\00\00\00\00\00\00\00\00\00\00\00\00\00")
  )
  (alias export 0 "asset" (type (;1;)))
  (alias export 0 "felt" (type (;2;)))
  (alias export 0 "word" (type (;3;)))
  (type (;4;) (variant (case "variant-a") (case "variant-b")))
  (type (;5;) (record (field "foo" 3) (field "asset" 1)))
  (type (;6;) (record (field "bar" 2) (field "baz" 2)))
  (type (;7;) (record (field "inner1" 2) (field "inner2" 2)))
  (type (;8;) (record (field "bar" 2) (field "baz" 2)))
  (core instance (;0;) (instantiate 0))
  (alias core export 0 "memory" (core memory (;0;)))
  (type (;9;) (func (param "a" 5) (param "asset" 1) (result 6)))
  (alias core export 0 "miden:component-macros-account/component-macros-account@0.1.0#test-custom-types" (core func (;0;)))
  (func (;0;) (type 9) (canon lift (core func 0) (memory 0)))
  (type (;10;) (func (param "a" 5) (param "asset" 1) (result 7)))
  (alias core export 0 "miden:component-macros-account/component-macros-account@0.1.0#test-custom-types2" (core func (;1;)))
  (func (;1;) (type 10) (canon lift (core func 1) (memory 0)))
  (alias export 0 "felt" (type (;11;)))
  (alias export 0 "word" (type (;12;)))
  (alias export 0 "asset" (type (;13;)))
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
    (type (;9;) (record (field "foo" 7) (field "asset" 8)))
    (import "import-type-struct-a" (type (;10;) (eq 9)))
    (import "import-type-felt0" (type (;11;) (eq 1)))
    (type (;12;) (record (field "bar" 11) (field "baz" 11)))
    (import "import-type-struct-b" (type (;13;) (eq 12)))
    (type (;14;) (func (param "a" 10) (param "asset" 8) (result 13)))
    (import "import-func-test-custom-types" (func (;0;) (type 14)))
    (type (;15;) (record (field "inner1" 11) (field "inner2" 11)))
    (import "import-type-struct-c" (type (;16;) (eq 15)))
    (type (;17;) (func (param "a" 10) (param "asset" 8) (result 16)))
    (import "import-func-test-custom-types2" (func (;1;) (type 17)))
    (export (;18;) "asset" (type 6))
    (export (;19;) "felt" (type 1))
    (export (;20;) "word" (type 4))
    (type (;21;) (variant (case "variant-a") (case "variant-b")))
    (export (;22;) "enum-a" (type 21))
    (type (;23;) (record (field "foo" 20) (field "asset" 18)))
    (export (;24;) "struct-a" (type 23))
    (type (;25;) (record (field "bar" 19) (field "baz" 19)))
    (export (;26;) "struct-b" (type 25))
    (type (;27;) (record (field "inner1" 19) (field "inner2" 19)))
    (export (;28;) "struct-c" (type 27))
    (type (;29;) (record (field "bar" 19) (field "baz" 19)))
    (export (;30;) "struct-d" (type 29))
    (type (;31;) (func (param "a" 24) (param "asset" 18) (result 26)))
    (export (;2;) "test-custom-types" (func 0) (func (type 31)))
    (type (;32;) (func (param "a" 24) (param "asset" 18) (result 28)))
    (export (;3;) "test-custom-types2" (func 1) (func (type 32)))
  )
  (instance (;1;) (instantiate 0
      (with "import-func-test-custom-types" (func 0))
      (with "import-func-test-custom-types2" (func 1))
      (with "import-type-felt" (type 11))
      (with "import-type-word" (type 12))
      (with "import-type-asset" (type 13))
      (with "import-type-word0" (type 3))
      (with "import-type-asset0" (type 1))
      (with "import-type-struct-a" (type 5))
      (with "import-type-felt0" (type 2))
      (with "import-type-struct-b" (type 6))
      (with "import-type-struct-c" (type 7))
    )
  )
  (export (;2;) "miden:component-macros-account/component-macros-account@0.1.0" (instance 1))
)
