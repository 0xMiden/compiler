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
    (type (;1;) (func (param f32 f32 f32 f32)))
    (type (;2;) (func (result f32)))
    (type (;3;) (func (param i32)))
    (type (;4;) (func (param i32 i32 i32)))
    (type (;5;) (func (param f32 f32 f32 f32 i32 i32)))
    (type (;6;) (func (param f32 f32 f32 f32 f32 f32 f32 f32)))
    (type (;7;) (func (param f32 f32)))
    (type (;8;) (func (param i32) (result f32)))
    (type (;9;) (func (param f32 i32)))
    (table (;0;) 2 2 funcref)
    (memory (;0;) 17)
    (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
    (global $GOT.data.internal.__memory_base (;1;) i32 i32.const 0)
    (export "memory" (memory 0))
    (export "miden:auth-component-rpo-falcon512/auth-component-rpo-falcon512@0.1.0#auth-procedure" (func $miden:auth-component-rpo-falcon512/auth-component-rpo-falcon512@0.1.0#auth-procedure))
    (elem (;0;) (i32.const 1) func $auth_component_rpo_falcon512::bindings::__link_custom_section_describing_imports)
    (func $__wasm_call_ctors (;0;) (type 0))
    (func $auth_component_rpo_falcon512::bindings::__link_custom_section_describing_imports (;1;) (type 0))
    (func $miden:auth-component-rpo-falcon512/auth-component-rpo-falcon512@0.1.0#auth-procedure (;2;) (type 1) (param f32 f32 f32 f32)
      (local i32 f32 f32 i64 i64 i32 f32 f32 i32 i32 i32 i32 i32 i32 f32 f32 f32 f32)
      global.get $__stack_pointer
      i32.const 112
      i32.sub
      local.tee 4
      global.set $__stack_pointer
      call $wit_bindgen::rt::run_ctors_once
      call $miden_base_sys::bindings::tx::get_block_number
      local.set 5
      call $miden::protocol::native_account::incr_nonce
      local.set 6
      local.get 4
      i32.const 32
      i32.add
      call $miden::protocol::native_account::compute_delta_commitment
      local.get 4
      i64.load offset=40
      local.set 7
      local.get 4
      i64.load offset=32
      local.set 8
      local.get 4
      call $miden_base_sys::bindings::tx::get_input_notes_commitment
      local.get 4
      i32.const 16
      i32.add
      call $miden_base_sys::bindings::tx::get_output_notes_commitment
      i32.const 0
      local.set 9
      i32.const 0
      call $intrinsics::felt::from_u32
      local.set 10
      i32.const 0
      call $intrinsics::felt::from_u32
      local.set 11
      local.get 4
      i32.const 56
      i32.add
      local.get 4
      i64.load offset=8
      i64.store
      local.get 4
      i32.const 72
      i32.add
      local.get 4
      i64.load offset=24
      i64.store
      local.get 4
      local.get 8
      i64.const 32
      i64.rotl
      i64.store offset=40
      local.get 4
      local.get 7
      i64.const 32
      i64.rotl
      i64.store offset=32
      local.get 4
      local.get 4
      i64.load
      i64.store offset=48
      local.get 4
      local.get 4
      i64.load offset=16
      i64.store offset=64
      local.get 4
      local.get 6
      f32.store offset=92
      local.get 4
      local.get 5
      f32.store offset=88
      local.get 4
      local.get 11
      f32.store offset=84
      local.get 4
      local.get 10
      f32.store offset=80
      i32.const 0
      call $intrinsics::felt::from_u32
      i32.const 0
      call $intrinsics::felt::from_u32
      call $intrinsics::felt::assert_eq
      local.get 4
      i32.const 32
      i32.add
      i32.const 2
      i32.shr_u
      local.tee 12
      local.get 12
      i32.const 16
      i32.add
      local.get 4
      i32.const 96
      i32.add
      call $std::crypto::hashes::rpo::hash_memory_words
      local.get 4
      f32.load offset=108
      local.set 5
      local.get 4
      f32.load offset=104
      local.set 6
      local.get 4
      f32.load offset=100
      local.set 10
      local.get 4
      f32.load offset=96
      local.set 11
      local.get 4
      i32.const 80
      i32.add
      local.set 13
      local.get 4
      i32.const 32
      i32.add
      local.set 14
      block ;; label = @1
        loop ;; label = @2
          local.get 9
          i32.const 2
          i32.eq
          br_if 1 (;@1;)
          i32.const 0
          local.set 12
          block ;; label = @3
            loop ;; label = @4
              local.get 12
              i32.const 16
              i32.eq
              br_if 1 (;@3;)
              local.get 14
              local.get 12
              i32.add
              local.tee 15
              i32.load
              local.set 16
              local.get 15
              local.get 13
              local.get 12
              i32.add
              local.tee 17
              i32.load
              i32.store
              local.get 17
              local.get 16
              i32.store
              local.get 12
              i32.const 4
              i32.add
              local.set 12
              br 0 (;@4;)
            end
          end
          local.get 13
          i32.const -16
          i32.add
          local.set 13
          local.get 14
          i32.const 16
          i32.add
          local.set 14
          local.get 9
          i32.const 1
          i32.add
          local.set 9
          br 0 (;@2;)
        end
      end
      local.get 4
      local.get 11
      f32.store offset=108
      local.get 4
      local.get 10
      f32.store offset=104
      local.get 4
      local.get 6
      f32.store offset=100
      local.get 4
      local.get 5
      f32.store offset=96
      local.get 4
      i32.const 96
      i32.add
      local.get 4
      i32.const 32
      i32.add
      i32.const 4
      call $miden_stdlib_sys::intrinsics::advice::adv_insert
      i32.const 0
      call $<miden_field::wasm32::Felt as core::convert::From<u8>>::from
      local.get 4
      i32.const 96
      i32.add
      call $miden::protocol::active_account::get_item
      local.get 11
      local.get 10
      local.get 6
      local.get 5
      local.get 4
      f32.load offset=96
      local.tee 18
      local.get 4
      f32.load offset=100
      local.tee 19
      local.get 4
      f32.load offset=104
      local.tee 20
      local.get 4
      f32.load offset=108
      local.tee 21
      call $intrinsics::advice::emit_falcon_sig_to_stack
      local.get 18
      local.get 19
      local.get 20
      local.get 21
      local.get 11
      local.get 10
      local.get 6
      local.get 5
      call $std::crypto::dsa::rpo_falcon512::verify
      local.get 4
      i32.const 112
      i32.add
      global.set $__stack_pointer
    )
    (func $wit_bindgen::rt::run_ctors_once (;3;) (type 0)
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
    (func $miden_base_sys::bindings::tx::get_block_number (;4;) (type 2) (result f32)
      call $miden::protocol::tx::get_block_number
    )
    (func $miden_base_sys::bindings::tx::get_input_notes_commitment (;5;) (type 3) (param i32)
      (local i32)
      global.get $__stack_pointer
      i32.const 16
      i32.sub
      local.tee 1
      global.set $__stack_pointer
      local.get 1
      call $miden::protocol::tx::get_input_notes_commitment
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
    (func $miden_base_sys::bindings::tx::get_output_notes_commitment (;6;) (type 3) (param i32)
      (local i32)
      global.get $__stack_pointer
      i32.const 16
      i32.sub
      local.tee 1
      global.set $__stack_pointer
      local.get 1
      call $miden::protocol::tx::get_output_notes_commitment
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
    (func $miden_stdlib_sys::intrinsics::advice::adv_insert (;7;) (type 4) (param i32 i32 i32)
      local.get 0
      f32.load offset=12
      local.get 0
      f32.load offset=8
      local.get 0
      f32.load offset=4
      local.get 0
      f32.load
      local.get 1
      i32.const 2
      i32.shr_u
      local.tee 0
      local.get 0
      local.get 2
      i32.const 2
      i32.shl
      i32.add
      call $intrinsics::advice::adv_insert_mem
    )
    (func $intrinsics::advice::adv_insert_mem (;8;) (type 5) (param f32 f32 f32 f32 i32 i32)
      unreachable
    )
    (func $intrinsics::advice::emit_falcon_sig_to_stack (;9;) (type 6) (param f32 f32 f32 f32 f32 f32 f32 f32)
      unreachable
    )
    (func $intrinsics::felt::assert_eq (;10;) (type 7) (param f32 f32)
      unreachable
    )
    (func $intrinsics::felt::from_u32 (;11;) (type 8) (param i32) (result f32)
      unreachable
    )
    (func $std::crypto::dsa::rpo_falcon512::verify (;12;) (type 6) (param f32 f32 f32 f32 f32 f32 f32 f32)
      unreachable
    )
    (func $std::crypto::hashes::rpo::hash_memory_words (;13;) (type 4) (param i32 i32 i32)
      unreachable
    )
    (func $<miden_field::wasm32::Felt as core::convert::From<u8>>::from (;14;) (type 8) (param i32) (result f32)
      local.get 0
      i32.const 255
      i32.and
      f32.reinterpret_i32
    )
    (func $miden::protocol::active_account::get_item (;15;) (type 9) (param f32 i32)
      unreachable
    )
    (func $miden::protocol::native_account::compute_delta_commitment (;16;) (type 3) (param i32)
      unreachable
    )
    (func $miden::protocol::native_account::incr_nonce (;17;) (type 2) (result f32)
      unreachable
    )
    (func $miden::protocol::tx::get_block_number (;18;) (type 2) (result f32)
      unreachable
    )
    (func $miden::protocol::tx::get_input_notes_commitment (;19;) (type 3) (param i32)
      unreachable
    )
    (func $miden::protocol::tx::get_output_notes_commitment (;20;) (type 3) (param i32)
      unreachable
    )
    (data $.data (;0;) (i32.const 1048576) "\01\00\00\00\01\00\00\00")
    (@custom "rodata,miden_account" (after data) "9auth-component-rpo-falcon512\01\0b0.1.0\01\01\00Fmiden::component::miden_auth_component_rpo_falcon512::owner_public_key\00\01!owner public key\00]miden::standards::auth::falcon512_rpo::pub_key\00\00\00\00\00\00\00\00\00\00\00\00\00\00\00")
  )
  (alias export $miden:base/core-types@1.0.0 "word" (type $word (;1;)))
  (core instance $main (;0;) (instantiate $main))
  (alias core export $main "memory" (core memory $memory (;0;)))
  (type (;2;) (func (param "arg" $word)))
  (alias core export $main "miden:auth-component-rpo-falcon512/auth-component-rpo-falcon512@0.1.0#auth-procedure" (core func $miden:auth-component-rpo-falcon512/auth-component-rpo-falcon512@0.1.0#auth-procedure (;0;)))
  (func $auth-procedure (;0;) (type 2) (canon lift (core func $miden:auth-component-rpo-falcon512/auth-component-rpo-falcon512@0.1.0#auth-procedure)))
  (alias export $miden:base/core-types@1.0.0 "felt" (type $felt (;3;)))
  (alias export $miden:base/core-types@1.0.0 "word" (type $"#type4 word" (@name "word") (;4;)))
  (component $miden:auth-component-rpo-falcon512/auth-component-rpo-falcon512@0.1.0-shim-component (;0;)
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
  (instance $miden:auth-component-rpo-falcon512/auth-component-rpo-falcon512@0.1.0-shim-instance (;1;) (instantiate $miden:auth-component-rpo-falcon512/auth-component-rpo-falcon512@0.1.0-shim-component
      (with "import-func-auth-procedure" (func $auth-procedure))
      (with "import-type-felt" (type $felt))
      (with "import-type-word" (type $"#type4 word"))
      (with "import-type-word0" (type $word))
    )
  )
  (export $miden:auth-component-rpo-falcon512/auth-component-rpo-falcon512@0.1.0 (;2;) "miden:auth-component-rpo-falcon512/auth-component-rpo-falcon512@0.1.0" (instance $miden:auth-component-rpo-falcon512/auth-component-rpo-falcon512@0.1.0-shim-instance))
)
