(component
  (type $ty-miden:base/core-types@1.0.0 (;0;)
    (instance
      (type (;0;) (record (field "inner" f32)))
      (export (;1;) "felt" (type (eq 0)))
      (type (;2;) (tuple 1 1 1 1))
      (type (;3;) (record (field "inner" 2)))
      (export (;4;) "word" (type (eq 3)))
    )
  )
  (import "miden:base/core-types@1.0.0" (instance $miden:base/core-types@1.0.0 (;0;) (type $ty-miden:base/core-types@1.0.0)))
  (core module $main (;0;)
    (type (;0;) (func))
    (type (;1;) (func (result i32)))
    (type (;2;) (func (param i32 f32)))
    (type (;3;) (func (param i32 i32)))
    (type (;4;) (func (param i32) (result f32)))
    (type (;5;) (func (param f32 i32)))
    (table (;0;) 2 2 funcref)
    (memory (;0;) 17)
    (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
    (global $GOT.data.internal.__memory_base (;1;) i32 i32.const 0)
    (export "memory" (memory 0))
    (export "miden:rust-sdk-input-note-get-script-root-binding/rust-sdk-input-note-get-script-root-binding@0.0.1#binding" (func $miden:rust-sdk-input-note-get-script-root-binding/rust-sdk-input-note-get-script-root-binding@0.0.1#binding))
    (elem (;0;) (i32.const 1) func $rust_sdk_input_note_get_script_root_binding::bindings::__link_custom_section_describing_imports)
    (func $__wasm_call_ctors (;0;) (type 0))
    (func $rust_sdk_input_note_get_script_root_binding::bindings::__link_custom_section_describing_imports (;1;) (type 0))
    (func $miden:rust-sdk-input-note-get-script-root-binding/rust-sdk-input-note-get-script-root-binding@0.0.1#binding (;2;) (type 1) (result i32)
      (local i32 i32)
      global.get $__stack_pointer
      i32.const 16
      i32.sub
      local.tee 0
      global.set $__stack_pointer
      call $wit_bindgen::rt::run_ctors_once
      local.get 0
      i32.const 0
      call $intrinsics::felt::from_u32
      call $miden_base_sys::bindings::input_note::get_script_root
      global.get $GOT.data.internal.__memory_base
      i32.const 1048584
      i32.add
      local.tee 1
      local.get 0
      i64.load offset=8
      i64.store offset=8 align=4
      local.get 1
      local.get 0
      i64.load
      i64.store align=4
      local.get 0
      i32.const 16
      i32.add
      global.set $__stack_pointer
      local.get 1
    )
    (func $wit_bindgen::rt::run_ctors_once (;3;) (type 0)
      (local i32)
      block ;; label = @1
        global.get $GOT.data.internal.__memory_base
        i32.const 1048600
        i32.add
        i32.load8_u
        br_if 0 (;@1;)
        global.get $GOT.data.internal.__memory_base
        local.set 0
        call $__wasm_call_ctors
        local.get 0
        i32.const 1048600
        i32.add
        i32.const 1
        i32.store8
      end
    )
    (func $miden_base_sys::bindings::input_note::get_script_root (;4;) (type 2) (param i32 f32)
      (local i32)
      global.get $__stack_pointer
      i32.const 32
      i32.sub
      local.tee 2
      global.set $__stack_pointer
      local.get 1
      local.get 2
      call $miden::input_note::get_script_root
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
      call $<miden_stdlib_sys::intrinsics::word::Word>::reverse
      local.get 2
      i32.const 32
      i32.add
      global.set $__stack_pointer
    )
    (func $<miden_stdlib_sys::intrinsics::word::Word>::reverse (;5;) (type 3) (param i32 i32)
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
    (func $intrinsics::felt::from_u32 (;6;) (type 4) (param i32) (result f32)
      unreachable
    )
    (func $miden::input_note::get_script_root (;7;) (type 5) (param f32 i32)
      unreachable
    )
    (data $.data (;0;) (i32.const 1048576) "\01\00\00\00\01\00\00\00")
    (@custom "rodata,miden_account" (after data) "Wrust_sdk_input_note_get_script_root_binding\01\0b0.0.1\03\01\01\00\00\00\00\00\00\00\00\00\00")
  )
  (alias export $miden:base/core-types@1.0.0 "word" (type $word (;1;)))
  (core instance $main (;0;) (instantiate $main))
  (alias core export $main "memory" (core memory $memory (;0;)))
  (type (;2;) (func (result $word)))
  (alias core export $main "miden:rust-sdk-input-note-get-script-root-binding/rust-sdk-input-note-get-script-root-binding@0.0.1#binding" (core func $miden:rust-sdk-input-note-get-script-root-binding/rust-sdk-input-note-get-script-root-binding@0.0.1#binding (;0;)))
  (func $binding (;0;) (type 2) (canon lift (core func $miden:rust-sdk-input-note-get-script-root-binding/rust-sdk-input-note-get-script-root-binding@0.0.1#binding) (memory $memory)))
  (alias export $miden:base/core-types@1.0.0 "felt" (type $felt (;3;)))
  (alias export $miden:base/core-types@1.0.0 "word" (type $"#type4 word" (@name "word") (;4;)))
  (component $miden:rust-sdk-input-note-get-script-root-binding/rust-sdk-input-note-get-script-root-binding@0.0.1-shim-component (;0;)
    (type (;0;) (record (field "inner" f32)))
    (import "import-type-felt" (type (;1;) (eq 0)))
    (type (;2;) (tuple 1 1 1 1))
    (type (;3;) (record (field "inner" 2)))
    (import "import-type-word" (type (;4;) (eq 3)))
    (import "import-type-word0" (type (;5;) (eq 4)))
    (type (;6;) (func (result 5)))
    (import "import-func-binding" (func (;0;) (type 6)))
    (export (;7;) "word" (type 4))
    (type (;8;) (func (result 7)))
    (export (;1;) "binding" (func 0) (func (type 8)))
  )
  (instance $miden:rust-sdk-input-note-get-script-root-binding/rust-sdk-input-note-get-script-root-binding@0.0.1-shim-instance (;1;) (instantiate $miden:rust-sdk-input-note-get-script-root-binding/rust-sdk-input-note-get-script-root-binding@0.0.1-shim-component
      (with "import-func-binding" (func $binding))
      (with "import-type-felt" (type $felt))
      (with "import-type-word" (type $"#type4 word"))
      (with "import-type-word0" (type $word))
    )
  )
  (export $miden:rust-sdk-input-note-get-script-root-binding/rust-sdk-input-note-get-script-root-binding@0.0.1 (;2;) "miden:rust-sdk-input-note-get-script-root-binding/rust-sdk-input-note-get-script-root-binding@0.0.1" (instance $miden:rust-sdk-input-note-get-script-root-binding/rust-sdk-input-note-get-script-root-binding@0.0.1-shim-instance))
)
