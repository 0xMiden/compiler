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
    (type (;1;) (func (param i32 i32)))
    (type (;2;) (func (param i32 i32) (result i32)))
    (type (;3;) (func (param i32 i32 i32)))
    (type (;4;) (func (param i32 i32 i32 i32) (result i32)))
    (type (;5;) (func (param f32 f32 f32 f32)))
    (type (;6;) (func (param i32 i32 i32) (result i32)))
    (type (;7;) (func (result i32)))
    (type (;8;) (func (result f32)))
    (type (;9;) (func (param i32)))
    (type (;10;) (func (param f32 i32)))
    (type (;11;) (func (param i32) (result f32)))
    (type (;12;) (func (param f32 f32)))
    (type (;13;) (func (param f32 f32 f32 f32 f32 f32 f32 f32)))
    (type (;14;) (func (param f32 f32 f32 f32 i32 i32)))
    (type (;15;) (func (param i32 i32 i32 i32 i32 i32)))
    (type (;16;) (func (param i32 i32 i32 i32 i32)))
    (type (;17;) (func (param i32 i32 i32 i32)))
    (table (;0;) 2 2 funcref)
    (memory (;0;) 17)
    (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
    (global $GOT.data.internal.__memory_base (;1;) i32 i32.const 0)
    (export "memory" (memory 0))
    (export "miden:base/authentication-component@1.0.0#auth-procedure" (func $miden:base/authentication-component@1.0.0#auth-procedure))
    (elem (;0;) (i32.const 1) func $auth_component_rpo_falcon512::bindings::__link_custom_section_describing_imports)
    (func $__wasm_call_ctors (;0;) (type 0))
    (func $alloc::vec::Vec<T,A>::extend_from_slice (;1;) (type 1) (param i32 i32)
      (local i32)
      block ;; label = @1
        local.get 0
        i32.load
        local.get 0
        i32.load offset=8
        local.tee 2
        i32.sub
        i32.const 3
        i32.gt_u
        br_if 0 (;@1;)
        local.get 0
        local.get 2
        i32.const 4
        i32.const 4
        i32.const 4
        call $alloc::raw_vec::RawVecInner<A>::reserve::do_reserve_and_handle
        local.get 0
        i32.load offset=8
        local.set 2
      end
      local.get 0
      i32.load offset=4
      local.get 2
      i32.const 2
      i32.shl
      i32.add
      local.tee 2
      local.get 1
      i64.load align=4
      i64.store align=4
      local.get 2
      i32.const 8
      i32.add
      local.get 1
      i32.const 8
      i32.add
      i64.load align=4
      i64.store align=4
      local.get 0
      local.get 0
      i32.load offset=8
      i32.const 4
      i32.add
      i32.store offset=8
    )
    (func $auth_component_rpo_falcon512::bindings::__link_custom_section_describing_imports (;2;) (type 0))
    (func $__rustc::__rust_alloc (;3;) (type 2) (param i32 i32) (result i32)
      global.get $GOT.data.internal.__memory_base
      i32.const 1048612
      i32.add
      local.get 1
      local.get 0
      call $<miden_sdk_alloc::BumpAlloc as core::alloc::global::GlobalAlloc>::alloc
    )
    (func $__rustc::__rust_dealloc (;4;) (type 3) (param i32 i32 i32))
    (func $__rustc::__rust_realloc (;5;) (type 4) (param i32 i32 i32 i32) (result i32)
      block ;; label = @1
        global.get $GOT.data.internal.__memory_base
        i32.const 1048612
        i32.add
        local.get 2
        local.get 3
        call $<miden_sdk_alloc::BumpAlloc as core::alloc::global::GlobalAlloc>::alloc
        local.tee 2
        i32.eqz
        br_if 0 (;@1;)
        local.get 3
        local.get 1
        local.get 3
        local.get 1
        i32.lt_u
        select
        local.tee 3
        i32.eqz
        br_if 0 (;@1;)
        local.get 2
        local.get 0
        local.get 3
        memory.copy
      end
      local.get 2
    )
    (func $__rustc::__rust_alloc_zeroed (;6;) (type 2) (param i32 i32) (result i32)
      block ;; label = @1
        global.get $GOT.data.internal.__memory_base
        i32.const 1048612
        i32.add
        local.get 1
        local.get 0
        call $<miden_sdk_alloc::BumpAlloc as core::alloc::global::GlobalAlloc>::alloc
        local.tee 1
        i32.eqz
        br_if 0 (;@1;)
        local.get 0
        i32.eqz
        br_if 0 (;@1;)
        local.get 1
        i32.const 0
        local.get 0
        memory.fill
      end
      local.get 1
    )
    (func $miden:base/authentication-component@1.0.0#auth-procedure (;7;) (type 5) (param f32 f32 f32 f32)
      (local i32 i32 f32 f32 f32 f32 i32 i32 f32 f32 f32 f32 f32 f32 f32 f32 f32 f32 f32 f32 f32 f32 f32 f32)
      global.get $__stack_pointer
      local.tee 4
      local.set 5
      local.get 4
      i32.const 256
      i32.sub
      i32.const -32
      i32.and
      local.tee 4
      global.set $__stack_pointer
      call $wit_bindgen_rt::run_ctors_once
      call $miden_base_sys::bindings::tx::get_block_number
      local.set 6
      call $miden::account::incr_nonce
      local.set 7
      local.get 4
      i32.const 240
      i32.add
      call $miden::account::compute_delta_commitment
      local.get 4
      local.get 4
      i64.load offset=248
      i64.store offset=136
      local.get 4
      local.get 4
      i64.load offset=240
      i64.store offset=128
      local.get 4
      local.get 4
      i32.const 128
      i32.add
      call $miden_stdlib_sys::intrinsics::word::Word::reverse
      local.get 4
      i32.const 16
      i32.add
      call $miden_base_sys::bindings::tx::get_input_notes_commitment
      local.get 4
      i32.const 32
      i32.add
      call $miden_base_sys::bindings::tx::get_output_notes_commitment
      i32.const 0
      call $intrinsics::felt::from_u32
      local.set 8
      i32.const 0
      call $intrinsics::felt::from_u32
      local.set 9
      local.get 4
      i32.const 128
      i32.add
      i32.const 16
      i32.const 0
      i32.const 4
      i32.const 4
      call $alloc::raw_vec::RawVecInner<A>::try_allocate_in
      local.get 4
      i32.load offset=132
      local.set 10
      block ;; label = @1
        local.get 4
        i32.load offset=128
        i32.const 1
        i32.ne
        br_if 0 (;@1;)
        global.get $GOT.data.internal.__memory_base
        local.set 5
        local.get 10
        local.get 4
        i32.load offset=136
        local.get 5
        i32.const 1048596
        i32.add
        call $alloc::raw_vec::handle_error
        unreachable
      end
      local.get 4
      i32.const 52
      i32.add
      i32.const 8
      i32.add
      local.tee 11
      i32.const 0
      i32.store
      local.get 4
      local.get 4
      i32.load offset=136
      i32.store offset=56
      local.get 4
      local.get 10
      i32.store offset=52
      local.get 4
      local.get 4
      f32.load offset=12
      local.tee 12
      f32.store offset=76
      local.get 4
      local.get 4
      f32.load offset=8
      local.tee 13
      f32.store offset=72
      local.get 4
      local.get 4
      f32.load offset=4
      local.tee 14
      f32.store offset=68
      local.get 4
      local.get 4
      f32.load
      local.tee 15
      f32.store offset=64
      local.get 4
      local.get 4
      f32.load offset=28
      local.tee 16
      f32.store offset=92
      local.get 4
      local.get 4
      f32.load offset=24
      local.tee 17
      f32.store offset=88
      local.get 4
      local.get 4
      f32.load offset=20
      local.tee 18
      f32.store offset=84
      local.get 4
      local.get 4
      f32.load offset=16
      local.tee 19
      f32.store offset=80
      local.get 4
      local.get 4
      f32.load offset=44
      local.tee 20
      f32.store offset=108
      local.get 4
      local.get 4
      f32.load offset=40
      local.tee 21
      f32.store offset=104
      local.get 4
      local.get 4
      f32.load offset=36
      local.tee 22
      f32.store offset=100
      local.get 4
      local.get 4
      f32.load offset=32
      local.tee 23
      f32.store offset=96
      local.get 4
      local.get 7
      f32.store offset=124
      local.get 4
      local.get 6
      f32.store offset=120
      local.get 4
      local.get 9
      f32.store offset=116
      local.get 4
      local.get 8
      f32.store offset=112
      local.get 4
      i32.const 52
      i32.add
      local.get 4
      i32.const 64
      i32.add
      call $alloc::vec::Vec<T,A>::extend_from_slice
      local.get 4
      i32.const 52
      i32.add
      local.get 4
      i32.const 80
      i32.add
      call $alloc::vec::Vec<T,A>::extend_from_slice
      local.get 4
      i32.const 52
      i32.add
      local.get 4
      i32.const 96
      i32.add
      call $alloc::vec::Vec<T,A>::extend_from_slice
      local.get 4
      i32.const 52
      i32.add
      local.get 4
      i32.const 112
      i32.add
      call $alloc::vec::Vec<T,A>::extend_from_slice
      local.get 4
      i32.const 208
      i32.add
      i32.const 8
      i32.add
      local.tee 10
      local.get 11
      i32.load
      i32.store
      local.get 4
      local.get 4
      i64.load offset=52 align=4
      i64.store offset=208
      local.get 4
      i32.load offset=212
      i32.const 2
      i32.shr_u
      local.tee 11
      i32.const 3
      i32.and
      call $intrinsics::felt::from_u32
      i32.const 0
      call $intrinsics::felt::from_u32
      call $intrinsics::felt::assert_eq
      local.get 11
      local.get 10
      i32.load
      local.get 4
      i32.const 240
      i32.add
      call $std::crypto::hashes::rpo::hash_memory
      local.get 4
      local.get 4
      i64.load offset=248
      i64.store offset=136
      local.get 4
      local.get 4
      i64.load offset=240
      i64.store offset=128
      local.get 4
      i32.const 224
      i32.add
      local.get 4
      i32.const 128
      i32.add
      call $miden_stdlib_sys::intrinsics::word::Word::reverse
      local.get 4
      i32.const 208
      i32.add
      i32.const 4
      i32.const 4
      call $alloc::raw_vec::RawVecInner<A>::deallocate
      local.get 4
      f32.load offset=224
      local.set 24
      local.get 4
      f32.load offset=228
      local.set 25
      local.get 4
      f32.load offset=232
      local.set 26
      local.get 4
      f32.load offset=236
      local.set 27
      local.get 4
      local.get 12
      f32.store offset=188
      local.get 4
      local.get 13
      f32.store offset=184
      local.get 4
      local.get 14
      f32.store offset=180
      local.get 4
      local.get 15
      f32.store offset=176
      local.get 4
      local.get 16
      f32.store offset=172
      local.get 4
      local.get 17
      f32.store offset=168
      local.get 4
      local.get 18
      f32.store offset=164
      local.get 4
      local.get 19
      f32.store offset=160
      local.get 4
      local.get 20
      f32.store offset=156
      local.get 4
      local.get 21
      f32.store offset=152
      local.get 4
      local.get 22
      f32.store offset=148
      local.get 4
      local.get 23
      f32.store offset=144
      local.get 4
      local.get 7
      f32.store offset=140
      local.get 4
      local.get 6
      f32.store offset=136
      local.get 4
      local.get 9
      f32.store offset=132
      local.get 4
      local.get 8
      f32.store offset=128
      local.get 27
      local.get 26
      local.get 25
      local.get 24
      local.get 4
      i32.const 128
      i32.add
      i32.const 2
      i32.shr_u
      local.tee 10
      local.get 10
      i32.const 16
      i32.add
      call $intrinsics::advice::adv_insert_mem
      i32.const 0
      call $<miden_stdlib_sys::intrinsics::felt::Felt as core::convert::From<u8>>::from
      local.get 4
      i32.const 224
      i32.add
      call $miden::account::get_item
      local.get 4
      local.get 4
      i64.load offset=232
      i64.store offset=248
      local.get 4
      local.get 4
      i64.load offset=224
      i64.store offset=240
      local.get 4
      i32.const 208
      i32.add
      local.get 4
      i32.const 240
      i32.add
      call $miden_stdlib_sys::intrinsics::word::Word::reverse
      local.get 27
      local.get 26
      local.get 25
      local.get 24
      local.get 4
      f32.load offset=220
      local.tee 6
      local.get 4
      f32.load offset=216
      local.tee 7
      local.get 4
      f32.load offset=212
      local.tee 8
      local.get 4
      f32.load offset=208
      local.tee 9
      call $intrinsics::advice::emit_falcon_sig_to_stack
      local.get 6
      local.get 7
      local.get 8
      local.get 9
      local.get 27
      local.get 26
      local.get 25
      local.get 24
      call $std::crypto::dsa::rpo_falcon512::verify
      local.get 5
      global.set $__stack_pointer
    )
    (func $__rustc::__rust_no_alloc_shim_is_unstable_v2 (;8;) (type 0)
      return
    )
    (func $wit_bindgen_rt::run_ctors_once (;9;) (type 0)
      (local i32)
      block ;; label = @1
        global.get $GOT.data.internal.__memory_base
        i32.const 1048616
        i32.add
        i32.load8_u
        br_if 0 (;@1;)
        global.get $GOT.data.internal.__memory_base
        local.set 0
        call $__wasm_call_ctors
        local.get 0
        i32.const 1048616
        i32.add
        i32.const 1
        i32.store8
      end
    )
    (func $<miden_sdk_alloc::BumpAlloc as core::alloc::global::GlobalAlloc>::alloc (;10;) (type 6) (param i32 i32 i32) (result i32)
      (local i32 i32)
      block ;; label = @1
        local.get 1
        i32.const 16
        local.get 1
        i32.const 16
        i32.gt_u
        select
        local.tee 3
        local.get 3
        i32.const -1
        i32.add
        i32.and
        br_if 0 (;@1;)
        local.get 2
        i32.const -2147483648
        local.get 1
        local.get 3
        call $core::ptr::alignment::Alignment::max
        local.tee 1
        i32.sub
        i32.gt_u
        br_if 0 (;@1;)
        i32.const 0
        local.set 3
        local.get 2
        local.get 1
        i32.add
        i32.const -1
        i32.add
        i32.const 0
        local.get 1
        i32.sub
        i32.and
        local.set 2
        block ;; label = @2
          local.get 0
          i32.load
          br_if 0 (;@2;)
          local.get 0
          call $intrinsics::mem::heap_base
          memory.size
          i32.const 16
          i32.shl
          i32.add
          i32.store
        end
        block ;; label = @2
          local.get 2
          local.get 0
          i32.load
          local.tee 4
          i32.const -1
          i32.xor
          i32.gt_u
          br_if 0 (;@2;)
          local.get 0
          local.get 4
          local.get 2
          i32.add
          i32.store
          local.get 4
          local.get 1
          i32.add
          local.set 3
        end
        local.get 3
        return
      end
      unreachable
    )
    (func $intrinsics::mem::heap_base (;11;) (type 7) (result i32)
      unreachable
    )
    (func $miden_base_sys::bindings::tx::get_block_number (;12;) (type 8) (result f32)
      call $miden::tx::get_block_number
    )
    (func $miden_base_sys::bindings::tx::get_input_notes_commitment (;13;) (type 9) (param i32)
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
    (func $miden_base_sys::bindings::tx::get_output_notes_commitment (;14;) (type 9) (param i32)
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
    (func $miden::account::compute_delta_commitment (;15;) (type 9) (param i32)
      unreachable
    )
    (func $miden::account::get_item (;16;) (type 10) (param f32 i32)
      unreachable
    )
    (func $miden::account::incr_nonce (;17;) (type 8) (result f32)
      unreachable
    )
    (func $miden::tx::get_block_number (;18;) (type 8) (result f32)
      unreachable
    )
    (func $miden::tx::get_input_notes_commitment (;19;) (type 9) (param i32)
      unreachable
    )
    (func $miden::tx::get_output_notes_commitment (;20;) (type 9) (param i32)
      unreachable
    )
    (func $<miden_stdlib_sys::intrinsics::felt::Felt as core::convert::From<u8>>::from (;21;) (type 11) (param i32) (result f32)
      local.get 0
      i32.const 255
      i32.and
      f32.reinterpret_i32
    )
    (func $miden_stdlib_sys::intrinsics::word::Word::reverse (;22;) (type 1) (param i32 i32)
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
    (func $intrinsics::felt::from_u32 (;23;) (type 11) (param i32) (result f32)
      unreachable
    )
    (func $intrinsics::felt::assert_eq (;24;) (type 12) (param f32 f32)
      unreachable
    )
    (func $intrinsics::advice::emit_falcon_sig_to_stack (;25;) (type 13) (param f32 f32 f32 f32 f32 f32 f32 f32)
      unreachable
    )
    (func $intrinsics::advice::adv_insert_mem (;26;) (type 14) (param f32 f32 f32 f32 i32 i32)
      unreachable
    )
    (func $std::crypto::hashes::rpo::hash_memory (;27;) (type 3) (param i32 i32 i32)
      unreachable
    )
    (func $std::crypto::dsa::rpo_falcon512::verify (;28;) (type 13) (param f32 f32 f32 f32 f32 f32 f32 f32)
      unreachable
    )
    (func $alloc::raw_vec::RawVecInner<A>::grow_amortized (;29;) (type 15) (param i32 i32 i32 i32 i32 i32)
      (local i32 i32 i32 i64)
      global.get $__stack_pointer
      i32.const 32
      i32.sub
      local.tee 6
      global.set $__stack_pointer
      i32.const 0
      local.set 7
      block ;; label = @1
        block ;; label = @2
          local.get 5
          i32.eqz
          br_if 0 (;@2;)
          local.get 2
          local.get 3
          i32.add
          local.tee 3
          local.get 2
          i32.lt_u
          br_if 1 (;@1;)
          i32.const 0
          local.set 7
          local.get 4
          local.get 5
          i32.add
          i32.const -1
          i32.add
          i32.const 0
          local.get 4
          i32.sub
          i32.and
          i64.extend_i32_u
          local.get 3
          local.get 1
          i32.load
          i32.const 1
          i32.shl
          local.tee 8
          local.get 3
          local.get 8
          i32.gt_u
          select
          local.tee 8
          i32.const 8
          i32.const 4
          i32.const 1
          local.get 5
          i32.const 1025
          i32.lt_u
          select
          local.get 5
          i32.const 1
          i32.eq
          select
          local.tee 2
          local.get 8
          local.get 2
          i32.gt_u
          select
          local.tee 2
          i64.extend_i32_u
          i64.mul
          local.tee 9
          i64.const 32
          i64.shr_u
          i32.wrap_i64
          br_if 0 (;@2;)
          local.get 9
          i32.wrap_i64
          local.tee 3
          i32.const -2147483648
          local.get 4
          i32.sub
          i32.gt_u
          br_if 1 (;@1;)
          local.get 6
          i32.const 20
          i32.add
          local.get 1
          local.get 4
          local.get 5
          call $alloc::raw_vec::RawVecInner<A>::current_memory
          local.get 6
          i32.const 8
          i32.add
          local.get 4
          local.get 3
          local.get 6
          i32.const 20
          i32.add
          local.get 0
          call $alloc::raw_vec::finish_grow
          local.get 6
          i32.load offset=12
          local.set 7
          block ;; label = @3
            local.get 6
            i32.load offset=8
            i32.eqz
            br_if 0 (;@3;)
            local.get 6
            i32.load offset=16
            local.set 8
            br 2 (;@1;)
          end
          local.get 1
          local.get 2
          i32.store
          local.get 1
          local.get 7
          i32.store offset=4
          i32.const -2147483647
          local.set 7
          br 1 (;@1;)
        end
      end
      local.get 0
      local.get 8
      i32.store offset=4
      local.get 0
      local.get 7
      i32.store
      local.get 6
      i32.const 32
      i32.add
      global.set $__stack_pointer
    )
    (func $alloc::raw_vec::RawVecInner<A>::deallocate (;30;) (type 3) (param i32 i32 i32)
      (local i32)
      global.get $__stack_pointer
      i32.const 16
      i32.sub
      local.tee 3
      global.set $__stack_pointer
      local.get 3
      i32.const 4
      i32.add
      local.get 0
      local.get 1
      local.get 2
      call $alloc::raw_vec::RawVecInner<A>::current_memory
      block ;; label = @1
        local.get 3
        i32.load offset=8
        local.tee 2
        i32.eqz
        br_if 0 (;@1;)
        local.get 3
        i32.load offset=4
        local.get 2
        local.get 3
        i32.load offset=12
        call $<alloc::alloc::Global as core::alloc::Allocator>::deallocate
      end
      local.get 3
      i32.const 16
      i32.add
      global.set $__stack_pointer
    )
    (func $alloc::raw_vec::RawVecInner<A>::try_allocate_in (;31;) (type 16) (param i32 i32 i32 i32 i32)
      (local i32 i64)
      global.get $__stack_pointer
      i32.const 16
      i32.sub
      local.tee 5
      global.set $__stack_pointer
      block ;; label = @1
        block ;; label = @2
          block ;; label = @3
            local.get 3
            local.get 4
            i32.add
            i32.const -1
            i32.add
            i32.const 0
            local.get 3
            i32.sub
            i32.and
            i64.extend_i32_u
            local.get 1
            i64.extend_i32_u
            i64.mul
            local.tee 6
            i64.const 32
            i64.shr_u
            i32.wrap_i64
            br_if 0 (;@3;)
            local.get 6
            i32.wrap_i64
            local.tee 4
            i32.const -2147483648
            local.get 3
            i32.sub
            i32.le_u
            br_if 1 (;@2;)
          end
          local.get 0
          i32.const 0
          i32.store offset=4
          i32.const 1
          local.set 3
          br 1 (;@1;)
        end
        block ;; label = @2
          local.get 4
          br_if 0 (;@2;)
          local.get 0
          local.get 3
          i32.store offset=8
          i32.const 0
          local.set 3
          local.get 0
          i32.const 0
          i32.store offset=4
          br 1 (;@1;)
        end
        block ;; label = @2
          block ;; label = @3
            local.get 2
            br_if 0 (;@3;)
            local.get 5
            i32.const 8
            i32.add
            local.get 3
            local.get 4
            call $<alloc::alloc::Global as core::alloc::Allocator>::allocate
            local.get 5
            i32.load offset=8
            local.set 2
            br 1 (;@2;)
          end
          local.get 5
          local.get 3
          local.get 4
          i32.const 1
          call $alloc::alloc::Global::alloc_impl
          local.get 5
          i32.load
          local.set 2
        end
        block ;; label = @2
          local.get 2
          i32.eqz
          br_if 0 (;@2;)
          local.get 0
          local.get 2
          i32.store offset=8
          local.get 0
          local.get 1
          i32.store offset=4
          i32.const 0
          local.set 3
          br 1 (;@1;)
        end
        local.get 0
        local.get 4
        i32.store offset=8
        local.get 0
        local.get 3
        i32.store offset=4
        i32.const 1
        local.set 3
      end
      local.get 0
      local.get 3
      i32.store
      local.get 5
      i32.const 16
      i32.add
      global.set $__stack_pointer
    )
    (func $<alloc::alloc::Global as core::alloc::Allocator>::allocate (;32;) (type 3) (param i32 i32 i32)
      (local i32)
      global.get $__stack_pointer
      i32.const 16
      i32.sub
      local.tee 3
      global.set $__stack_pointer
      local.get 3
      i32.const 8
      i32.add
      local.get 1
      local.get 2
      i32.const 0
      call $alloc::alloc::Global::alloc_impl
      local.get 3
      i32.load offset=12
      local.set 2
      local.get 0
      local.get 3
      i32.load offset=8
      i32.store
      local.get 0
      local.get 2
      i32.store offset=4
      local.get 3
      i32.const 16
      i32.add
      global.set $__stack_pointer
    )
    (func $alloc::alloc::Global::alloc_impl (;33;) (type 17) (param i32 i32 i32 i32)
      block ;; label = @1
        local.get 2
        i32.eqz
        br_if 0 (;@1;)
        call $__rustc::__rust_no_alloc_shim_is_unstable_v2
        block ;; label = @2
          local.get 3
          br_if 0 (;@2;)
          local.get 2
          local.get 1
          call $__rustc::__rust_alloc
          local.set 1
          br 1 (;@1;)
        end
        local.get 2
        local.get 1
        call $__rustc::__rust_alloc_zeroed
        local.set 1
      end
      local.get 0
      local.get 2
      i32.store offset=4
      local.get 0
      local.get 1
      i32.store
    )
    (func $alloc::raw_vec::RawVecInner<A>::current_memory (;34;) (type 17) (param i32 i32 i32 i32)
      (local i32 i32 i32)
      i32.const 0
      local.set 4
      i32.const 4
      local.set 5
      block ;; label = @1
        local.get 3
        i32.eqz
        br_if 0 (;@1;)
        local.get 1
        i32.load
        local.tee 6
        i32.eqz
        br_if 0 (;@1;)
        local.get 0
        local.get 2
        i32.store offset=4
        local.get 0
        local.get 1
        i32.load offset=4
        i32.store
        local.get 6
        local.get 3
        i32.mul
        local.set 4
        i32.const 8
        local.set 5
      end
      local.get 0
      local.get 5
      i32.add
      local.get 4
      i32.store
    )
    (func $alloc::raw_vec::RawVecInner<A>::reserve::do_reserve_and_handle (;35;) (type 16) (param i32 i32 i32 i32 i32)
      (local i32)
      global.get $__stack_pointer
      i32.const 16
      i32.sub
      local.tee 5
      global.set $__stack_pointer
      local.get 5
      i32.const 8
      i32.add
      local.get 0
      local.get 1
      local.get 2
      local.get 3
      local.get 4
      call $alloc::raw_vec::RawVecInner<A>::grow_amortized
      block ;; label = @1
        local.get 5
        i32.load offset=8
        i32.const -2147483647
        i32.eq
        br_if 0 (;@1;)
        unreachable
      end
      local.get 5
      i32.const 16
      i32.add
      global.set $__stack_pointer
    )
    (func $alloc::raw_vec::finish_grow (;36;) (type 16) (param i32 i32 i32 i32 i32)
      (local i32 i32)
      global.get $__stack_pointer
      i32.const 16
      i32.sub
      local.tee 5
      global.set $__stack_pointer
      i32.const 0
      local.set 6
      block ;; label = @1
        block ;; label = @2
          local.get 2
          i32.const 0
          i32.ge_s
          br_if 0 (;@2;)
          i32.const 1
          local.set 2
          i32.const 4
          local.set 3
          br 1 (;@1;)
        end
        block ;; label = @2
          block ;; label = @3
            local.get 3
            i32.load offset=4
            i32.eqz
            br_if 0 (;@3;)
            block ;; label = @4
              local.get 3
              i32.load offset=8
              local.tee 6
              br_if 0 (;@4;)
              local.get 5
              i32.const 8
              i32.add
              local.get 1
              local.get 2
              i32.const 0
              call $alloc::alloc::Global::alloc_impl
              local.get 5
              i32.load offset=12
              local.set 6
              local.get 5
              i32.load offset=8
              local.set 3
              br 2 (;@2;)
            end
            local.get 3
            i32.load
            local.get 6
            local.get 1
            local.get 2
            call $__rustc::__rust_realloc
            local.set 3
            local.get 2
            local.set 6
            br 1 (;@2;)
          end
          local.get 5
          local.get 1
          local.get 2
          call $<alloc::alloc::Global as core::alloc::Allocator>::allocate
          local.get 5
          i32.load offset=4
          local.set 6
          local.get 5
          i32.load
          local.set 3
        end
        local.get 0
        local.get 3
        local.get 1
        local.get 3
        select
        i32.store offset=4
        local.get 6
        local.get 2
        local.get 3
        select
        local.set 6
        local.get 3
        i32.eqz
        local.set 2
        i32.const 8
        local.set 3
      end
      local.get 0
      local.get 3
      i32.add
      local.get 6
      i32.store
      local.get 0
      local.get 2
      i32.store
      local.get 5
      i32.const 16
      i32.add
      global.set $__stack_pointer
    )
    (func $<alloc::alloc::Global as core::alloc::Allocator>::deallocate (;37;) (type 3) (param i32 i32 i32)
      block ;; label = @1
        local.get 2
        i32.eqz
        br_if 0 (;@1;)
        local.get 0
        local.get 2
        local.get 1
        call $__rustc::__rust_dealloc
      end
    )
    (func $alloc::raw_vec::handle_error (;38;) (type 3) (param i32 i32 i32)
      unreachable
    )
    (func $core::ptr::alignment::Alignment::max (;39;) (type 2) (param i32 i32) (result i32)
      local.get 0
      local.get 1
      local.get 0
      local.get 1
      i32.gt_u
      select
    )
    (data $.rodata (;0;) (i32.const 1048576) "src/lib.rs\00")
    (data $.data (;1;) (i32.const 1048588) "\01\00\00\00\01\00\00\00\00\00\10\00\0a\00\00\004\00\00\000\00\00\00")
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
