(component
  (type (;0;)
    (instance
      (type (;0;) (record (field "inner" f32)))
      (export (;1;) "felt" (type (eq 0)))
      (type (;2;) (tuple 1 1 1 1))
      (type (;3;) (record (field "inner" 2)))
      (export (;4;) "word" (type (eq 3)))
    )
  )
  (import "miden:base/core-types@1.0.0" (instance (;0;) (type 0)))
  (core module (;0;)
    (type (;0;) (func))
    (type (;1;) (func (param f32 f32 f32 f32)))
    (type (;2;) (func (param i32)))
    (type (;3;) (func (result f32)))
    (type (;4;) (func (param f32 i32)))
    (type (;5;) (func (param f32)))
    (type (;6;) (func (param i32) (result f32)))
    (type (;7;) (func (param i32 i32)))
    (type (;8;) (func (param f32 f32 f32 f32 f32 f32 f32 f32)))
    (table (;0;) 2 2 funcref)
    (memory (;0;) 17)
    (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
    (global $GOT.data.internal.__memory_base (;1;) i32 i32.const 0)
    (export "memory" (memory 0))
    (export "miden:base/authentication-component@1.0.0#auth-procedure" (func $miden:base/authentication-component@1.0.0#auth-procedure))
    (elem (;0;) (i32.const 1) func $auth_component_rpo_falcon512::bindings::__link_custom_section_describing_imports)
    (func $__wasm_call_ctors (;0;) (type 0))
    (func $auth_component_rpo_falcon512::bindings::__link_custom_section_describing_imports (;1;) (type 0))
    (func $miden:base/authentication-component@1.0.0#auth-procedure (;2;) (type 1) (param f32 f32 f32 f32)
      (local i32 f32 i64 f32 f32 f32 f32 f32 f32 f32)
      global.get $__stack_pointer
      i32.const 144
      i32.sub
      local.tee 4
      global.set $__stack_pointer
      call $wit_bindgen_rt::run_ctors_once
      local.get 4
      i32.const 112
      i32.add
      call $miden_base_sys::bindings::tx::get_output_notes_commitment
      local.get 4
      i32.const 80
      i32.add
      call $miden_base_sys::bindings::tx::get_input_notes_commitment
      call $miden::account::get_nonce
      local.set 5
      local.get 4
      i32.const 8
      i32.add
      call $miden_base_sys::bindings::account::get_id
      local.get 4
      i64.load offset=8
      local.set 6
      i32.const 0
      call $intrinsics::felt::from_u32
      local.set 7
      i32.const 0
      call $intrinsics::felt::from_u32
      local.set 8
      i32.const 0
      call $intrinsics::felt::from_u32
      local.set 9
      i32.const 0
      call $intrinsics::felt::from_u32
      local.set 10
      i32.const 0
      call $intrinsics::felt::from_u32
      local.set 11
      local.get 4
      local.get 5
      f32.store offset=60
      local.get 4
      local.get 11
      f32.store offset=56
      local.get 4
      local.get 10
      f32.store offset=52
      local.get 4
      local.get 9
      f32.store offset=48
      local.get 4
      local.get 6
      i64.store offset=40
      local.get 4
      local.get 8
      f32.store offset=36
      local.get 4
      local.get 7
      f32.store offset=32
      local.get 4
      i32.const 32
      i32.add
      local.get 4
      i32.const 16
      i32.add
      call $intrinsics::crypto::hmerge
      local.get 4
      i32.const 80
      i32.add
      i32.const 24
      i32.add
      local.get 4
      i64.load offset=24
      i64.store
      local.get 4
      local.get 4
      i64.load offset=16
      i64.store offset=96
      local.get 4
      i32.const 80
      i32.add
      local.get 4
      i32.const 64
      i32.add
      call $intrinsics::crypto::hmerge
      local.get 4
      i32.const 112
      i32.add
      i32.const 24
      i32.add
      local.get 4
      i64.load offset=72
      i64.store
      local.get 4
      local.get 4
      i64.load offset=64
      i64.store offset=128
      local.get 4
      i32.const 112
      i32.add
      local.get 4
      i32.const 80
      i32.add
      call $intrinsics::crypto::hmerge
      local.get 4
      f32.load offset=80
      local.set 5
      local.get 4
      f32.load offset=84
      local.set 7
      local.get 4
      f32.load offset=88
      local.set 8
      local.get 4
      f32.load offset=92
      local.set 9
      i32.const 0
      call $<miden_stdlib_sys::intrinsics::felt::Felt as core::convert::From<u8>>::from
      local.get 4
      i32.const 80
      i32.add
      call $miden::account::get_item
      local.get 4
      local.get 4
      i64.load offset=88
      i64.store offset=120
      local.get 4
      local.get 4
      i64.load offset=80
      i64.store offset=112
      local.get 4
      i32.const 32
      i32.add
      local.get 4
      i32.const 112
      i32.add
      call $miden_stdlib_sys::intrinsics::word::Word::reverse
      local.get 4
      f32.load offset=32
      local.set 10
      local.get 4
      f32.load offset=36
      local.set 11
      local.get 4
      f32.load offset=40
      local.set 12
      local.get 4
      f32.load offset=44
      local.set 13
      i32.const 1
      call $intrinsics::felt::from_u32
      call $miden::account::incr_nonce
      local.get 10
      local.get 11
      local.get 12
      local.get 13
      local.get 5
      local.get 7
      local.get 8
      local.get 9
      call $std::crypto::dsa::rpo_falcon512::verify
      local.get 4
      i32.const 144
      i32.add
      global.set $__stack_pointer
    )
    (func $wit_bindgen_rt::run_ctors_once (;3;) (type 0)
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
    (func $miden_base_sys::bindings::account::get_id (;4;) (type 2) (param i32)
      (local i32)
      global.get $__stack_pointer
      i32.const 16
      i32.sub
      local.tee 1
      global.set $__stack_pointer
      local.get 1
      i32.const 8
      i32.add
      call $miden::account::get_id
      local.get 0
      local.get 1
      i64.load offset=8 align=4
      i64.store
      local.get 1
      i32.const 16
      i32.add
      global.set $__stack_pointer
    )
    (func $miden_base_sys::bindings::tx::get_input_notes_commitment (;5;) (type 2) (param i32)
      (local i32)
      global.get $__stack_pointer
      i32.const 32
      i32.sub
      local.tee 1
      global.set $__stack_pointer
      local.get 1
      call $miden::tx::get_input_notes_commitment
      local.get 1
      local.get 1
      i64.load offset=8
      i64.store offset=24
      local.get 1
      local.get 1
      i64.load
      i64.store offset=16
      local.get 0
      local.get 1
      i32.const 16
      i32.add
      call $miden_stdlib_sys::intrinsics::word::Word::reverse
      local.get 1
      i32.const 32
      i32.add
      global.set $__stack_pointer
    )
    (func $miden_base_sys::bindings::tx::get_output_notes_commitment (;6;) (type 2) (param i32)
      (local i32)
      global.get $__stack_pointer
      i32.const 32
      i32.sub
      local.tee 1
      global.set $__stack_pointer
      local.get 1
      call $miden::tx::get_output_notes_commitment
      local.get 1
      local.get 1
      i64.load offset=8
      i64.store offset=24
      local.get 1
      local.get 1
      i64.load
      i64.store offset=16
      local.get 0
      local.get 1
      i32.const 16
      i32.add
      call $miden_stdlib_sys::intrinsics::word::Word::reverse
      local.get 1
      i32.const 32
      i32.add
      global.set $__stack_pointer
    )
    (func $miden::account::get_id (;7;) (type 2) (param i32)
      unreachable
    )
    (func $miden::account::get_nonce (;8;) (type 3) (result f32)
      unreachable
    )
    (func $miden::account::get_item (;9;) (type 4) (param f32 i32)
      unreachable
    )
    (func $miden::account::incr_nonce (;10;) (type 5) (param f32)
      unreachable
    )
    (func $miden::tx::get_input_notes_commitment (;11;) (type 2) (param i32)
      unreachable
    )
    (func $miden::tx::get_output_notes_commitment (;12;) (type 2) (param i32)
      unreachable
    )
    (func $<miden_stdlib_sys::intrinsics::felt::Felt as core::convert::From<u8>>::from (;13;) (type 6) (param i32) (result f32)
      local.get 0
      i32.const 255
      i32.and
      f32.reinterpret_i32
    )
    (func $miden_stdlib_sys::intrinsics::word::Word::reverse (;14;) (type 7) (param i32 i32)
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
    (func $intrinsics::felt::from_u32 (;15;) (type 6) (param i32) (result f32)
      unreachable
    )
    (func $intrinsics::crypto::hmerge (;16;) (type 7) (param i32 i32)
      unreachable
    )
    (func $std::crypto::dsa::rpo_falcon512::verify (;17;) (type 8) (param f32 f32 f32 f32 f32 f32 f32 f32)
      unreachable
    )
    (data $.data (;0;) (i32.const 1048576) "\01\00\00\00\01\00\00\00")
    (@custom "rodata,miden_account" (after data) "9auth-component-rpo-falcon512\01\0b0.1.0\01\03\00\00\00!owner_public_key\01!owner public key9auth::rpo_falcon512::pub_key")
  )
  (alias export 0 "word" (type (;1;)))
  (core instance (;0;) (instantiate 0))
  (alias core export 0 "memory" (core memory (;0;)))
  (type (;2;) (func (param "arg" 1)))
  (alias core export 0 "miden:base/authentication-component@1.0.0#auth-procedure" (core func (;0;)))
  (func (;0;) (type 2) (canon lift (core func 0)))
  (alias export 0 "felt" (type (;3;)))
  (alias export 0 "word" (type (;4;)))
  (component (;0;)
    (type (;0;) (record (field "inner" f32)))
    (import "import-type-felt" (type (;1;) (eq 0)))
    (type (;2;) (tuple 1 1 1 1))
    (type (;3;) (record (field "inner" 2)))
    (import "import-type-word" (type (;4;) (eq 3)))
    (import "import-type-word0" (type (;5;) (eq 4)))
    (type (;6;) (func (param "arg" 5)))
    (import "import-func-auth-procedure" (func (;0;) (type 6)))
    (export (;7;) "word" (type 4))
    (type (;8;) (func (param "arg" 7)))
    (export (;1;) "auth-procedure" (func 0) (func (type 8)))
  )
  (instance (;1;) (instantiate 0
      (with "import-func-auth-procedure" (func 0))
      (with "import-type-felt" (type 3))
      (with "import-type-word" (type 4))
      (with "import-type-word0" (type 1))
    )
  )
  (export (;2;) "miden:base/authentication-component@1.0.0" (instance 1))
)
