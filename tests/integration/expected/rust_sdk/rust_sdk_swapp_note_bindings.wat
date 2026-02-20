(component
  (type $ty-miden:base/core-types@1.0.0 (;0;)
    (instance
      (type (;0;) (record (field "inner" f32)))
      (export (;1;) "felt" (type (eq 0)))
      (type (;2;) (record (field "a" 1) (field "b" 1) (field "c" 1) (field "d" 1)))
      (export (;3;) "word" (type (eq 2)))
    )
  )
  (import "miden:base/core-types@1.0.0" (instance $miden:base/core-types@1.0.0 (;0;) (type $ty-miden:base/core-types@1.0.0)))
  (core module $main (;0;)
    (type (;0;) (func))
    (type (;1;) (func (param i32 i32) (result i32)))
    (type (;2;) (func (param f32 f32 f32 f32)))
    (type (;3;) (func (param i32)))
    (type (;4;) (func (param f32 f32) (result f32)))
    (type (;5;) (func (param f32 f32) (result i32)))
    (table (;0;) 2 2 funcref)
    (memory (;0;) 17)
    (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
    (global $GOT.data.internal.__memory_base (;1;) i32 i32.const 0)
    (export "memory" (memory 0))
    (export "miden:base/note-script@1.0.0#run" (func $miden:base/note-script@1.0.0#run))
    (elem (;0;) (i32.const 1) func $rust_sdk_swapp_note_bindings::bindings::__link_custom_section_describing_imports)
    (func $__wasm_call_ctors (;0;) (type 0))
    (func $rust_sdk_swapp_note_bindings::bindings::__link_custom_section_describing_imports (;1;) (type 0))
    (func $<miden_field::word::Word as core::cmp::PartialEq>::eq (;2;) (type 1) (param i32 i32) (result i32)
      (local i32)
      i32.const 0
      local.set 2
      block ;; label = @1
        local.get 0
        f32.load
        local.get 1
        f32.load
        call $intrinsics::felt::eq
        i32.const 1
        i32.ne
        br_if 0 (;@1;)
        local.get 0
        f32.load offset=4
        local.get 1
        f32.load offset=4
        call $intrinsics::felt::eq
        i32.const 1
        i32.ne
        br_if 0 (;@1;)
        local.get 0
        f32.load offset=8
        local.get 1
        f32.load offset=8
        call $intrinsics::felt::eq
        i32.const 1
        i32.ne
        br_if 0 (;@1;)
        local.get 0
        f32.load offset=12
        local.get 1
        f32.load offset=12
        call $intrinsics::felt::eq
        i32.const 1
        i32.eq
        local.set 2
      end
      local.get 2
    )
    (func $miden:base/note-script@1.0.0#run (;3;) (type 2) (param f32 f32 f32 f32)
      (local i32 f32 f32 f32)
      global.get $__stack_pointer
      i32.const 48
      i32.sub
      local.tee 4
      global.set $__stack_pointer
      call $wit_bindgen::rt::run_ctors_once
      local.get 4
      i32.const 8
      i32.add
      call $miden_base_sys::bindings::active_note::get_sender
      local.get 4
      f32.load offset=12
      local.set 5
      local.get 4
      f32.load offset=8
      local.set 6
      local.get 4
      i32.const 16
      i32.add
      call $miden_base_sys::bindings::active_note::get_script_root
      local.get 4
      i32.const 32
      i32.add
      call $miden_base_sys::bindings::active_note::get_serial_number
      local.get 6
      local.get 5
      call $miden_base_sys::bindings::active_account::get_balance
      local.set 7
      block ;; label = @1
        local.get 6
        local.get 6
        call $intrinsics::felt::eq
        i32.const 1
        i32.ne
        br_if 0 (;@1;)
        local.get 5
        local.get 5
        call $intrinsics::felt::eq
        i32.const 1
        i32.ne
        br_if 0 (;@1;)
        local.get 4
        i32.const 16
        i32.add
        local.get 4
        i32.const 16
        i32.add
        call $<miden_field::word::Word as core::cmp::PartialEq>::eq
        i32.eqz
        br_if 0 (;@1;)
        local.get 4
        i32.const 32
        i32.add
        local.get 4
        i32.const 32
        i32.add
        call $<miden_field::word::Word as core::cmp::PartialEq>::eq
        i32.eqz
        br_if 0 (;@1;)
        local.get 7
        local.get 7
        call $intrinsics::felt::eq
        i32.const 1
        i32.ne
        br_if 0 (;@1;)
        local.get 4
        i32.const 48
        i32.add
        global.set $__stack_pointer
        return
      end
      unreachable
    )
    (func $wit_bindgen::rt::run_ctors_once (;4;) (type 0)
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
    (func $miden_base_sys::bindings::active_note::get_sender (;5;) (type 3) (param i32)
      (local i32)
      global.get $__stack_pointer
      i32.const 16
      i32.sub
      local.tee 1
      global.set $__stack_pointer
      local.get 1
      i32.const 8
      i32.add
      call $miden::protocol::active_note::get_sender
      local.get 0
      local.get 1
      i64.load offset=8 align=4
      i64.store
      local.get 1
      i32.const 16
      i32.add
      global.set $__stack_pointer
    )
    (func $miden_base_sys::bindings::active_note::get_script_root (;6;) (type 3) (param i32)
      (local i32)
      global.get $__stack_pointer
      i32.const 16
      i32.sub
      local.tee 1
      global.set $__stack_pointer
      local.get 1
      call $miden::protocol::active_note::get_script_root
      local.get 0
      local.get 1
      i64.load
      i64.const 32
      i64.rotl
      i64.store offset=8
      local.get 0
      local.get 1
      i64.load offset=8
      i64.const 32
      i64.rotl
      i64.store
      local.get 1
      i32.const 16
      i32.add
      global.set $__stack_pointer
    )
    (func $miden_base_sys::bindings::active_note::get_serial_number (;7;) (type 3) (param i32)
      (local i32)
      global.get $__stack_pointer
      i32.const 16
      i32.sub
      local.tee 1
      global.set $__stack_pointer
      local.get 1
      call $miden::protocol::active_note::get_serial_number
      local.get 0
      local.get 1
      i64.load
      i64.const 32
      i64.rotl
      i64.store offset=8
      local.get 0
      local.get 1
      i64.load offset=8
      i64.const 32
      i64.rotl
      i64.store
      local.get 1
      i32.const 16
      i32.add
      global.set $__stack_pointer
    )
    (func $miden_base_sys::bindings::active_account::get_balance (;8;) (type 4) (param f32 f32) (result f32)
      local.get 0
      local.get 1
      call $miden::protocol::active_account::get_balance
    )
    (func $intrinsics::felt::eq (;9;) (type 5) (param f32 f32) (result i32)
      unreachable
    )
    (func $miden::protocol::active_account::get_balance (;10;) (type 4) (param f32 f32) (result f32)
      unreachable
    )
    (func $miden::protocol::active_note::get_script_root (;11;) (type 3) (param i32)
      unreachable
    )
    (func $miden::protocol::active_note::get_sender (;12;) (type 3) (param i32)
      unreachable
    )
    (func $miden::protocol::active_note::get_serial_number (;13;) (type 3) (param i32)
      unreachable
    )
    (data $.data (;0;) (i32.const 1048576) "\01\00\00\00\01\00\00\00")
  )
  (alias export $miden:base/core-types@1.0.0 "word" (type $word (;1;)))
  (core instance $main (;0;) (instantiate $main))
  (alias core export $main "memory" (core memory $memory (;0;)))
  (type (;2;) (func (param "arg" $word)))
  (alias core export $main "miden:base/note-script@1.0.0#run" (core func $miden:base/note-script@1.0.0#run (;0;)))
  (func $run (;0;) (type 2) (canon lift (core func $miden:base/note-script@1.0.0#run)))
  (alias export $miden:base/core-types@1.0.0 "felt" (type $felt (;3;)))
  (alias export $miden:base/core-types@1.0.0 "word" (type $"#type4 word" (@name "word") (;4;)))
  (component $miden:base/note-script@1.0.0-shim-component (;0;)
    (type (;0;) (record (field "inner" f32)))
    (import "import-type-felt" (type (;1;) (eq 0)))
    (type (;2;) (record (field "a" 1) (field "b" 1) (field "c" 1) (field "d" 1)))
    (import "import-type-word" (type (;3;) (eq 2)))
    (import "import-type-word0" (type (;4;) (eq 3)))
    (type (;5;) (func (param "arg" 4)))
    (import "import-func-run" (func (;0;) (type 5)))
    (export (;6;) "word" (type 3))
    (type (;7;) (func (param "arg" 6)))
    (export (;1;) "run" (func 0) (func (type 7)))
  )
  (instance $miden:base/note-script@1.0.0-shim-instance (;1;) (instantiate $miden:base/note-script@1.0.0-shim-component
      (with "import-func-run" (func $run))
      (with "import-type-felt" (type $felt))
      (with "import-type-word" (type $"#type4 word"))
      (with "import-type-word0" (type $word))
    )
  )
  (export $miden:base/note-script@1.0.0 (;2;) "miden:base/note-script@1.0.0" (instance $miden:base/note-script@1.0.0-shim-instance))
)
