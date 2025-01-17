(module $miden_sdk_account_test.wasm
  (type (;0;) (func (param i32) (result f32)))
  (type (;1;) (func (param f32 f32) (result f32)))
  (type (;2;) (func (param f32) (result i64)))
  (type (;3;) (func (param f32 f32) (result i32)))
  (type (;4;) (func (param f32) (result i32)))
  (type (;5;) (func (param f32)))
  (type (;6;) (func (param f32) (result f32)))
  (type (;7;) (func (param f32 f32)))
  (type (;8;) (func (param i64) (result f32)))
  (type (;9;) (func (param i32 i32 i32 i32 i32 i32 i32 i32 i32)))
  (type (;10;) (func (param i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32)))
  (type (;11;) (func (param f32 f32 f32 f32 f32 f32 f32 f32)))
  (type (;12;) (func (result i32)))
  (type (;13;) (func (result f32)))
  (type (;14;) (func (param f32 f32 f32 f32 i32)))
  (type (;15;) (func (param i32) (result i32)))
  (type (;16;) (func (param f32 f32 f32 f32 f32 f32 f32 f32 f32 f32) (result f32)))
  (type (;17;) (func (param f32 i32 i32)))
  (type (;18;) (func (param f32 f32 f32 f32 f32 f32 f32 f32 f32 f32 f32 f32 i32 i32 i32)))
  (type (;19;) (func (param i32)))
  (type (;20;) (func (param i32 i32 i32) (result i32)))
  (type (;21;) (func (param i32 i32)))
  (type (;22;) (func (param i32 f32)))
  (type (;23;) (func (param i32 f32 f32 i32) (result f32)))
  (type (;24;) (func (param i32 i32) (result i32)))
  (type (;25;) (func (param i32 i32 i32)))
  (type (;26;) (func))
  (type (;27;) (func (param i32 i32 i32 i32 i32)))
  (type (;28;) (func (param i32 i32 i32 i32)))
  (import "miden:core-import/intrinsics-felt@1.0.0" "from-u32" (func $miden_stdlib_sys::intrinsics::felt::extern_from_u32 (;0;) (type 0)))
  (import "miden:core-import/intrinsics-felt@1.0.0" "add" (func $miden_stdlib_sys::intrinsics::felt::extern_add (;1;) (type 1)))
  (import "miden:core-import/intrinsics-felt@1.0.0" "as_u64" (func $miden_stdlib_sys::intrinsics::felt::extern_as_u64 (;2;) (type 2)))
  (import "miden:core-import/intrinsics-felt@1.0.0" "gt" (func $miden_stdlib_sys::intrinsics::felt::extern_gt (;3;) (type 3)))
  (import "miden:core-import/intrinsics-felt@1.0.0" "lt" (func $miden_stdlib_sys::intrinsics::felt::extern_lt (;4;) (type 3)))
  (import "miden:core-import/intrinsics-felt@1.0.0" "le" (func $miden_stdlib_sys::intrinsics::felt::extern_le (;5;) (type 3)))
  (import "miden:core-import/intrinsics-felt@1.0.0" "ge" (func $miden_stdlib_sys::intrinsics::felt::extern_ge (;6;) (type 3)))
  (import "miden:core-import/intrinsics-felt@1.0.0" "eq" (func $miden_stdlib_sys::intrinsics::felt::extern_eq (;7;) (type 3)))
  (import "miden:core-import/intrinsics-felt@1.0.0" "is-odd" (func $miden_stdlib_sys::intrinsics::felt::extern_is_odd (;8;) (type 4)))
  (import "miden:core-import/intrinsics-felt@1.0.0" "assertz" (func $miden_stdlib_sys::intrinsics::felt::extern_assertz (;9;) (type 5)))
  (import "miden:core-import/intrinsics-felt@1.0.0" "assert" (func $miden_stdlib_sys::intrinsics::felt::extern_assert (;10;) (type 5)))
  (import "miden:core-import/intrinsics-felt@1.0.0" "inv" (func $miden_stdlib_sys::intrinsics::felt::extern_inv (;11;) (type 6)))
  (import "miden:core-import/intrinsics-felt@1.0.0" "exp" (func $miden_stdlib_sys::intrinsics::felt::extern_exp (;12;) (type 1)))
  (import "miden:core-import/intrinsics-felt@1.0.0" "sub" (func $miden_stdlib_sys::intrinsics::felt::extern_sub (;13;) (type 1)))
  (import "miden:core-import/intrinsics-felt@1.0.0" "pow2" (func $miden_stdlib_sys::intrinsics::felt::extern_pow2 (;14;) (type 6)))
  (import "miden:core-import/intrinsics-felt@1.0.0" "mul" (func $miden_stdlib_sys::intrinsics::felt::extern_mul (;15;) (type 1)))
  (import "miden:core-import/intrinsics-felt@1.0.0" "div" (func $miden_stdlib_sys::intrinsics::felt::extern_div (;16;) (type 1)))
  (import "miden:core-import/intrinsics-felt@1.0.0" "assert-eq" (func $miden_stdlib_sys::intrinsics::felt::extern_assert_eq (;17;) (type 7)))
  (import "miden:core-import/intrinsics-felt@1.0.0" "from-u64-unchecked" (func $miden_stdlib_sys::intrinsics::felt::extern_from_u64_unchecked (;18;) (type 8)))
  (import "miden:core-import/intrinsics-felt@1.0.0" "neg" (func $miden_stdlib_sys::intrinsics::felt::extern_neg (;19;) (type 6)))
  (import "miden:core-import/stdlib-crypto-hashes-blake3@1.0.0" "hash-one-to-one" (func $miden_stdlib_sys::stdlib::crypto::hashes::extern_blake3_hash_1to1 (;20;) (type 9)))
  (import "miden:core-import/stdlib-crypto-hashes-blake3@1.0.0" "hash-two-to-one" (func $miden_stdlib_sys::stdlib::crypto::hashes::extern_blake3_hash_2to1 (;21;) (type 10)))
  (import "miden:core-import/stdlib-crypto-dsa-rpo-falcon@1.0.0" "rpo-falcon512-verify" (func $miden_stdlib_sys::stdlib::crypto::dsa::extern_rpo_falcon512_verify (;22;) (type 11)))
  (import "miden:core-import/intrinsics-mem@1.0.0" "heap-base" (func $miden_sdk_alloc::heap_base (;23;) (type 12)))
  (import "miden:core-import/account@1.0.0" "get-id" (func $miden_base_sys::bindings::account::extern_account_get_id (;24;) (type 13)))
  (import "miden:core-import/account@1.0.0" "add-asset" (func $miden_base_sys::bindings::account::extern_account_add_asset (;25;) (type 14)))
  (import "miden:core-import/account@1.0.0" "remove-asset" (func $miden_base_sys::bindings::account::extern_account_remove_asset (;26;) (type 14)))
  (import "miden:core-import/note@1.0.0" "get-inputs" (func $miden_base_sys::bindings::note::extern_note_get_inputs (;27;) (type 15)))
  (import "miden:core-import/tx@1.0.0" "create-note" (func $miden_base_sys::bindings::tx::extern_tx_create_note (;28;) (type 16)))
  (import "miden:core-import/stdlib-mem@1.0.0" "pipe-words-to-memory" (func $miden_stdlib_sys::stdlib::mem::extern_pipe_words_to_memory (;29;) (type 17)))
  (import "miden:core-import/stdlib-mem@1.0.0" "pipe-double-words-to-memory" (func $miden_stdlib_sys::stdlib::mem::extern_pipe_double_words_to_memory (;30;) (type 18)))
  (func $<<alloc::vec::into_iter::IntoIter<T,A> as core::ops::drop::Drop>::drop::DropGuard<T,A> as core::ops::drop::Drop>::drop (;31;) (type 19) (param i32)
    (local i32)
    global.get $__stack_pointer
    i32.const 16
    i32.sub
    local.tee 1
    global.set $__stack_pointer
    local.get 1
    local.get 0
    i32.load
    local.tee 0
    i32.load
    i32.store offset=12
    local.get 1
    local.get 0
    i32.load offset=8
    i32.store offset=8
    local.get 1
    i32.const 8
    i32.add
    call $<alloc::raw_vec::RawVec<T,A> as core::ops::drop::Drop>::drop
    local.get 1
    i32.const 16
    i32.add
    global.set $__stack_pointer
  )
  (func $<alloc::raw_vec::RawVec<T,A> as core::ops::drop::Drop>::drop (;32;) (type 19) (param i32)
    local.get 0
    i32.const 4
    i32.const 4
    call $alloc::raw_vec::RawVecInner<A>::deallocate
  )
  (func $core::alloc::global::GlobalAlloc::alloc_zeroed (;33;) (type 20) (param i32 i32 i32) (result i32)
    block ;; label = @1
      local.get 0
      local.get 1
      local.get 2
      call $<miden_sdk_alloc::BumpAlloc as core::alloc::global::GlobalAlloc>::alloc
      local.tee 1
      i32.eqz
      br_if 0 (;@1;)
      local.get 1
      i32.const 0
      local.get 2
      memory.fill
    end
    local.get 1
  )
  (func $<alloc::vec::into_iter::IntoIter<T,A> as core::ops::drop::Drop>::drop (;34;) (type 19) (param i32)
    (local i32)
    global.get $__stack_pointer
    i32.const 16
    i32.sub
    local.tee 1
    global.set $__stack_pointer
    local.get 1
    local.get 0
    i32.store offset=12
    local.get 1
    i32.const 12
    i32.add
    call $<<alloc::vec::into_iter::IntoIter<T,A> as core::ops::drop::Drop>::drop::DropGuard<T,A> as core::ops::drop::Drop>::drop
    local.get 1
    i32.const 16
    i32.add
    global.set $__stack_pointer
  )
  (func $get_wallet_magic_number (;35;) (type 13) (result f32)
    (local f32)
    call $miden_base_sys::bindings::account::get_id
    local.set 0
    i32.const 42
    call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
    local.get 0
    call $miden_stdlib_sys::intrinsics::felt::extern_add
  )
  (func $test_add_asset (;36;) (type 13) (result f32)
    (local i32 i32 f32 f32 f32)
    global.get $__stack_pointer
    local.tee 0
    i32.const 64
    i32.sub
    i32.const -32
    i32.and
    local.tee 1
    global.set $__stack_pointer
    i32.const 1
    call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
    local.set 2
    i32.const 2
    call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
    local.set 3
    i32.const 3
    call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
    local.set 4
    local.get 1
    i32.const 4
    call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
    f32.store offset=12
    local.get 1
    local.get 4
    f32.store offset=8
    local.get 1
    local.get 3
    f32.store offset=4
    local.get 1
    local.get 2
    f32.store
    local.get 1
    i32.const 32
    i32.add
    local.get 1
    call $miden_base_sys::bindings::account::add_asset
    local.get 1
    f32.load offset=32
    local.set 2
    local.get 0
    global.set $__stack_pointer
    local.get 2
  )
  (func $test_felt_ops_smoke (;37;) (type 1) (param f32 f32) (result f32)
    (local i64)
    local.get 0
    call $miden_stdlib_sys::intrinsics::felt::extern_as_u64
    local.set 2
    block ;; label = @1
      block ;; label = @2
        block ;; label = @3
          block ;; label = @4
            block ;; label = @5
              block ;; label = @6
                local.get 0
                local.get 1
                call $miden_stdlib_sys::intrinsics::felt::extern_gt
                br_if 0 (;@6;)
                local.get 1
                local.get 0
                call $miden_stdlib_sys::intrinsics::felt::extern_lt
                br_if 1 (;@5;)
                local.get 1
                local.get 0
                call $miden_stdlib_sys::intrinsics::felt::extern_le
                br_if 2 (;@4;)
                local.get 0
                local.get 1
                call $miden_stdlib_sys::intrinsics::felt::extern_ge
                br_if 3 (;@3;)
                local.get 0
                local.get 1
                call $miden_stdlib_sys::intrinsics::felt::extern_eq
                i32.const 1
                i32.eq
                br_if 4 (;@2;)
                local.get 0
                local.get 1
                call $miden_stdlib_sys::intrinsics::felt::extern_eq
                i32.const 1
                i32.ne
                br_if 5 (;@1;)
                block ;; label = @7
                  local.get 1
                  call $miden_stdlib_sys::intrinsics::felt::extern_is_odd
                  br_if 0 (;@7;)
                  local.get 1
                  call $miden_stdlib_sys::intrinsics::felt::extern_assertz
                  local.get 0
                  return
                end
                local.get 0
                call $miden_stdlib_sys::intrinsics::felt::extern_assert
                local.get 1
                return
              end
              local.get 0
              call $miden_stdlib_sys::intrinsics::felt::extern_inv
              local.get 1
              call $miden_stdlib_sys::intrinsics::felt::extern_add
              return
            end
            local.get 0
            local.get 1
            call $miden_stdlib_sys::intrinsics::felt::extern_exp
            local.get 1
            call $miden_stdlib_sys::intrinsics::felt::extern_sub
            return
          end
          local.get 0
          call $miden_stdlib_sys::intrinsics::felt::extern_pow2
          local.get 1
          call $miden_stdlib_sys::intrinsics::felt::extern_mul
          return
        end
        local.get 1
        local.get 0
        call $miden_stdlib_sys::intrinsics::felt::extern_div
        return
      end
      local.get 0
      local.get 1
      call $miden_stdlib_sys::intrinsics::felt::extern_assert_eq
      local.get 0
      local.get 2
      call $miden_stdlib_sys::intrinsics::felt::extern_from_u64_unchecked
      call $miden_stdlib_sys::intrinsics::felt::extern_add
      return
    end
    local.get 0
    call $miden_stdlib_sys::intrinsics::felt::extern_neg
  )
  (func $note_script (;38;) (type 13) (result f32)
    (local i32 f32 i32 i32 i32 i32)
    global.get $__stack_pointer
    i32.const 16
    i32.sub
    local.tee 0
    global.set $__stack_pointer
    i64.const 0
    call $miden_stdlib_sys::intrinsics::felt::extern_from_u64_unchecked
    local.set 1
    local.get 0
    call $miden_base_sys::bindings::note::get_inputs
    local.get 0
    i32.load
    local.set 2
    local.get 0
    local.get 0
    i32.load offset=4
    local.tee 3
    local.get 0
    i32.load offset=8
    i32.const 2
    i32.shl
    local.tee 4
    i32.add
    local.tee 5
    i32.store offset=12
    local.get 0
    local.get 2
    i32.store offset=8
    local.get 0
    local.get 3
    i32.store
    loop (result f32) ;; label = @1
      block ;; label = @2
        local.get 4
        br_if 0 (;@2;)
        local.get 0
        local.get 5
        i32.store offset=4
        local.get 0
        call $<alloc::vec::into_iter::IntoIter<T,A> as core::ops::drop::Drop>::drop
        local.get 0
        i32.const 16
        i32.add
        global.set $__stack_pointer
        local.get 1
        return
      end
      local.get 4
      i32.const -4
      i32.add
      local.set 4
      local.get 1
      local.get 3
      f32.load
      call $miden_stdlib_sys::intrinsics::felt::extern_add
      local.set 1
      local.get 3
      i32.const 4
      i32.add
      local.set 3
      br 0 (;@1;)
    end
  )
  (func $test_blake3_hash_1to1 (;39;) (type 21) (param i32 i32)
    (local i32 i32)
    global.get $__stack_pointer
    local.tee 2
    i32.const 32
    i32.sub
    i32.const -32
    i32.and
    local.tee 3
    global.set $__stack_pointer
    local.get 1
    i32.load align=1
    local.get 1
    i32.load offset=4 align=1
    local.get 1
    i32.load offset=8 align=1
    local.get 1
    i32.load offset=12 align=1
    local.get 1
    i32.load offset=16 align=1
    local.get 1
    i32.load offset=20 align=1
    local.get 1
    i32.load offset=24 align=1
    local.get 1
    i32.load offset=28 align=1
    local.get 3
    call $miden_stdlib_sys::stdlib::crypto::hashes::extern_blake3_hash_1to1
    local.get 0
    i32.const 24
    i32.add
    local.get 3
    i64.load offset=24
    i64.store align=1
    local.get 0
    i32.const 16
    i32.add
    local.get 3
    i64.load offset=16
    i64.store align=1
    local.get 0
    i32.const 8
    i32.add
    local.get 3
    i64.load offset=8
    i64.store align=1
    local.get 0
    local.get 3
    i64.load
    i64.store align=1
    local.get 2
    global.set $__stack_pointer
  )
  (func $test_blake3_hash_2to1 (;40;) (type 21) (param i32 i32)
    local.get 1
    i32.load align=1
    local.get 1
    i32.load offset=4 align=1
    local.get 1
    i32.load offset=8 align=1
    local.get 1
    i32.load offset=12 align=1
    local.get 1
    i32.load offset=16 align=1
    local.get 1
    i32.load offset=20 align=1
    local.get 1
    i32.load offset=24 align=1
    local.get 1
    i32.load offset=28 align=1
    local.get 1
    i32.load offset=32 align=1
    local.get 1
    i32.load offset=36 align=1
    local.get 1
    i32.load offset=40 align=1
    local.get 1
    i32.load offset=44 align=1
    local.get 1
    i32.load offset=48 align=1
    local.get 1
    i32.load offset=52 align=1
    local.get 1
    i32.load offset=56 align=1
    local.get 1
    i32.load offset=60 align=1
    local.get 0
    call $miden_stdlib_sys::stdlib::crypto::hashes::extern_blake3_hash_2to1
  )
  (func $test_rpo_falcon512_verify (;41;) (type 21) (param i32 i32)
    local.get 0
    f32.load
    local.get 0
    f32.load offset=4
    local.get 0
    f32.load offset=8
    local.get 0
    f32.load offset=12
    local.get 1
    f32.load
    local.get 1
    f32.load offset=4
    local.get 1
    f32.load offset=8
    local.get 1
    f32.load offset=12
    call $miden_stdlib_sys::stdlib::crypto::dsa::extern_rpo_falcon512_verify
  )
  (func $test_pipe_words_to_memory (;42;) (type 22) (param i32 f32)
    local.get 0
    local.get 1
    call $miden_stdlib_sys::stdlib::mem::pipe_words_to_memory
  )
  (func $test_pipe_double_words_to_memory (;43;) (type 22) (param i32 f32)
    local.get 0
    local.get 1
    call $miden_stdlib_sys::stdlib::mem::pipe_double_words_to_memory
  )
  (func $test_remove_asset (;44;) (type 0) (param i32) (result f32)
    (local i32 i32 f32)
    global.get $__stack_pointer
    local.tee 1
    i32.const 32
    i32.sub
    i32.const -32
    i32.and
    local.tee 2
    global.set $__stack_pointer
    local.get 2
    local.get 0
    call $miden_base_sys::bindings::account::remove_asset
    local.get 2
    f32.load
    local.set 3
    local.get 1
    global.set $__stack_pointer
    local.get 3
  )
  (func $test_create_note (;45;) (type 23) (param i32 f32 f32 i32) (result f32)
    local.get 0
    local.get 1
    local.get 2
    local.get 3
    call $miden_base_sys::bindings::tx::create_note
  )
  (func $__rust_alloc (;46;) (type 24) (param i32 i32) (result i32)
    i32.const 1048704
    local.get 1
    local.get 0
    call $<miden_sdk_alloc::BumpAlloc as core::alloc::global::GlobalAlloc>::alloc
  )
  (func $__rust_dealloc (;47;) (type 25) (param i32 i32 i32))
  (func $__rust_alloc_zeroed (;48;) (type 24) (param i32 i32) (result i32)
    i32.const 1048704
    local.get 1
    local.get 0
    call $core::alloc::global::GlobalAlloc::alloc_zeroed
  )
  (func $<miden_sdk_alloc::BumpAlloc as core::alloc::global::GlobalAlloc>::alloc (;49;) (type 20) (param i32 i32 i32) (result i32)
    (local i32 i32)
    block ;; label = @1
      local.get 1
      i32.const 32
      local.get 1
      i32.const 32
      i32.gt_u
      select
      local.tee 3
      i32.popcnt
      i32.const 1
      i32.ne
      br_if 0 (;@1;)
      i32.const -2147483648
      local.get 1
      local.get 3
      call $core::ptr::alignment::Alignment::max
      local.tee 1
      i32.sub
      local.get 2
      i32.lt_u
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
        call $miden_sdk_alloc::heap_base
        memory.size
        i32.const 16
        i32.shl
        i32.add
        i32.store
      end
      block ;; label = @2
        i32.const 268435456
        local.get 0
        i32.load
        local.tee 4
        i32.sub
        local.get 2
        i32.lt_u
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
  (func $miden_base_sys::bindings::account::get_id (;50;) (type 13) (result f32)
    call $miden_base_sys::bindings::account::extern_account_get_id
  )
  (func $miden_base_sys::bindings::account::add_asset (;51;) (type 21) (param i32 i32)
    local.get 1
    f32.load
    local.get 1
    f32.load offset=4
    local.get 1
    f32.load offset=8
    local.get 1
    f32.load offset=12
    local.get 0
    call $miden_base_sys::bindings::account::extern_account_add_asset
  )
  (func $miden_base_sys::bindings::account::remove_asset (;52;) (type 21) (param i32 i32)
    local.get 1
    f32.load
    local.get 1
    f32.load offset=4
    local.get 1
    f32.load offset=8
    local.get 1
    f32.load offset=12
    local.get 0
    call $miden_base_sys::bindings::account::extern_account_remove_asset
  )
  (func $miden_base_sys::bindings::note::get_inputs (;53;) (type 19) (param i32)
    (local i32 i32 i32)
    global.get $__stack_pointer
    i32.const 16
    i32.sub
    local.tee 1
    global.set $__stack_pointer
    local.get 1
    i32.const 4
    i32.add
    i32.const 256
    i32.const 0
    i32.const 4
    i32.const 4
    call $alloc::raw_vec::RawVecInner<A>::try_allocate_in
    local.get 1
    i32.load offset=8
    local.set 2
    block ;; label = @1
      local.get 1
      i32.load offset=4
      i32.const 1
      i32.ne
      br_if 0 (;@1;)
      local.get 2
      local.get 1
      i32.load offset=12
      i32.const 1048616
      call $alloc::raw_vec::handle_error
      unreachable
    end
    local.get 0
    local.get 1
    i32.load offset=12
    local.tee 3
    i32.const 4
    i32.shr_u
    call $miden_base_sys::bindings::note::extern_note_get_inputs
    i32.store offset=8
    local.get 0
    local.get 3
    i32.store offset=4
    local.get 0
    local.get 2
    i32.store
    local.get 1
    i32.const 16
    i32.add
    global.set $__stack_pointer
  )
  (func $miden_base_sys::bindings::tx::create_note (;54;) (type 23) (param i32 f32 f32 i32) (result f32)
    local.get 0
    f32.load
    local.get 0
    f32.load offset=4
    local.get 0
    f32.load offset=8
    local.get 0
    f32.load offset=12
    local.get 1
    local.get 2
    local.get 3
    f32.load
    local.get 3
    f32.load offset=4
    local.get 3
    f32.load offset=8
    local.get 3
    f32.load offset=12
    call $miden_base_sys::bindings::tx::extern_tx_create_note
  )
  (func $alloc::vec::Vec<T>::with_capacity (;55;) (type 25) (param i32 i32 i32)
    (local i32)
    global.get $__stack_pointer
    i32.const 16
    i32.sub
    local.tee 3
    global.set $__stack_pointer
    local.get 3
    i32.const 4
    i32.add
    local.get 1
    i32.const 0
    i32.const 4
    i32.const 4
    call $alloc::raw_vec::RawVecInner<A>::try_allocate_in
    local.get 3
    i32.load offset=8
    local.set 1
    block ;; label = @1
      local.get 3
      i32.load offset=4
      i32.const 1
      i32.ne
      br_if 0 (;@1;)
      local.get 1
      local.get 3
      i32.load offset=12
      local.get 2
      call $alloc::raw_vec::handle_error
      unreachable
    end
    local.get 3
    i32.load offset=12
    local.set 2
    local.get 0
    i32.const 0
    i32.store offset=8
    local.get 0
    local.get 2
    i32.store offset=4
    local.get 0
    local.get 1
    i32.store
    local.get 3
    i32.const 16
    i32.add
    global.set $__stack_pointer
  )
  (func $miden_stdlib_sys::stdlib::mem::pipe_words_to_memory (;56;) (type 22) (param i32 f32)
    (local i32 i32)
    global.get $__stack_pointer
    local.tee 2
    i32.const 96
    i32.sub
    i32.const -32
    i32.and
    local.tee 3
    global.set $__stack_pointer
    local.get 3
    i32.const 20
    i32.add
    local.get 1
    call $miden_stdlib_sys::intrinsics::felt::extern_as_u64
    i32.wrap_i64
    i32.const 2
    i32.shl
    i32.const 1048672
    call $alloc::vec::Vec<T>::with_capacity
    local.get 1
    local.get 3
    i32.load offset=24
    local.get 3
    i32.const 32
    i32.add
    call $miden_stdlib_sys::stdlib::mem::extern_pipe_words_to_memory
    local.get 0
    i32.const 24
    i32.add
    local.get 3
    i64.load offset=56
    i64.store
    local.get 0
    i32.const 16
    i32.add
    local.get 3
    i64.load offset=48
    i64.store
    local.get 0
    i32.const 8
    i32.add
    local.get 3
    i64.load offset=40
    i64.store
    local.get 0
    local.get 3
    i64.load offset=32
    i64.store
    local.get 0
    i32.const 40
    i32.add
    local.get 3
    i32.const 20
    i32.add
    i32.const 8
    i32.add
    i32.load
    i32.store
    local.get 0
    local.get 3
    i64.load offset=20 align=4
    i64.store offset=32 align=4
    local.get 2
    global.set $__stack_pointer
  )
  (func $miden_stdlib_sys::stdlib::mem::pipe_double_words_to_memory (;57;) (type 22) (param i32 f32)
    (local i32 i32 i32 i32)
    global.get $__stack_pointer
    local.tee 2
    i32.const 160
    i32.sub
    i32.const -32
    i32.and
    local.tee 3
    global.set $__stack_pointer
    local.get 3
    i32.const 20
    i32.add
    local.get 1
    call $miden_stdlib_sys::intrinsics::felt::extern_as_u64
    i32.wrap_i64
    local.tee 4
    i32.const 2
    i32.shl
    i32.const 1048688
    call $alloc::vec::Vec<T>::with_capacity
    local.get 3
    i32.load offset=24
    local.set 5
    i32.const 0
    call $miden_stdlib_sys::intrinsics::felt::extern_from_u32
    local.tee 1
    local.get 1
    local.get 1
    local.get 1
    local.get 1
    local.get 1
    local.get 1
    local.get 1
    local.get 1
    local.get 1
    local.get 1
    local.get 1
    local.get 5
    local.get 5
    local.get 4
    i32.const 4
    i32.shl
    i32.add
    local.get 3
    i32.const 32
    i32.add
    call $miden_stdlib_sys::stdlib::mem::extern_pipe_double_words_to_memory
    local.get 0
    i32.const 24
    i32.add
    local.get 3
    i32.const 88
    i32.add
    i64.load
    i64.store
    local.get 0
    i32.const 16
    i32.add
    local.get 3
    i32.const 80
    i32.add
    i64.load
    i64.store
    local.get 0
    i32.const 8
    i32.add
    local.get 3
    i32.const 32
    i32.add
    i32.const 40
    i32.add
    i64.load
    i64.store
    local.get 0
    local.get 3
    i64.load offset=64
    i64.store
    local.get 0
    i32.const 40
    i32.add
    local.get 3
    i32.const 20
    i32.add
    i32.const 8
    i32.add
    i32.load
    i32.store
    local.get 0
    local.get 3
    i64.load offset=20 align=4
    i64.store offset=32 align=4
    local.get 2
    global.set $__stack_pointer
  )
  (func $dummy (;58;) (type 26))
  (func $__wasm_call_dtors (;59;) (type 26)
    call $dummy
    call $dummy
  )
  (func $alloc::raw_vec::RawVecInner<A>::deallocate (;60;) (type 25) (param i32 i32 i32)
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
  (func $alloc::raw_vec::RawVecInner<A>::try_allocate_in (;61;) (type 27) (param i32 i32 i32 i32 i32)
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
          i32.const -2147483648
          local.get 3
          i32.sub
          local.get 6
          i32.wrap_i64
          local.tee 4
          i32.lt_u
          br_if 0 (;@3;)
          block ;; label = @4
            local.get 4
            br_if 0 (;@4;)
            local.get 0
            local.get 3
            i32.store offset=8
            i32.const 0
            local.set 3
            local.get 0
            i32.const 0
            i32.store offset=4
            br 3 (;@1;)
          end
          block ;; label = @4
            block ;; label = @5
              local.get 2
              br_if 0 (;@5;)
              local.get 5
              i32.const 8
              i32.add
              local.get 3
              local.get 4
              call $<alloc::alloc::Global as core::alloc::Allocator>::allocate
              local.get 5
              i32.load offset=8
              local.set 2
              br 1 (;@4;)
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
          local.get 2
          i32.eqz
          br_if 1 (;@2;)
          local.get 0
          local.get 2
          i32.store offset=8
          local.get 0
          local.get 1
          i32.store offset=4
          i32.const 0
          local.set 3
          br 2 (;@1;)
        end
        local.get 0
        i32.const 0
        i32.store offset=4
        i32.const 1
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
  (func $<alloc::alloc::Global as core::alloc::Allocator>::allocate (;62;) (type 25) (param i32 i32 i32)
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
  (func $alloc::alloc::Global::alloc_impl (;63;) (type 28) (param i32 i32 i32 i32)
    block ;; label = @1
      local.get 2
      i32.eqz
      br_if 0 (;@1;)
      i32.const 0
      i32.load8_u offset=1048708
      drop
      block ;; label = @2
        local.get 3
        br_if 0 (;@2;)
        local.get 2
        local.get 1
        call $__rust_alloc
        local.set 1
        br 1 (;@1;)
      end
      local.get 2
      local.get 1
      call $__rust_alloc_zeroed
      local.set 1
    end
    local.get 0
    local.get 2
    i32.store offset=4
    local.get 0
    local.get 1
    i32.store
  )
  (func $alloc::raw_vec::RawVecInner<A>::current_memory (;64;) (type 28) (param i32 i32 i32 i32)
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
  (func $<alloc::alloc::Global as core::alloc::Allocator>::deallocate (;65;) (type 25) (param i32 i32 i32)
    block ;; label = @1
      local.get 2
      i32.eqz
      br_if 0 (;@1;)
      local.get 0
      local.get 2
      local.get 1
      call $__rust_dealloc
    end
  )
  (func $alloc::raw_vec::handle_error (;66;) (type 25) (param i32 i32 i32)
    unreachable
  )
  (func $core::ptr::alignment::Alignment::max (;67;) (type 24) (param i32 i32) (result i32)
    local.get 0
    local.get 1
    local.get 0
    local.get 1
    i32.gt_u
    select
  )
  (func $get_wallet_magic_number.command_export (;68;) (type 13) (result f32)
    call $get_wallet_magic_number
    call $__wasm_call_dtors
  )
  (func $test_add_asset.command_export (;69;) (type 13) (result f32)
    call $test_add_asset
    call $__wasm_call_dtors
  )
  (func $test_felt_ops_smoke.command_export (;70;) (type 1) (param f32 f32) (result f32)
    local.get 0
    local.get 1
    call $test_felt_ops_smoke
    call $__wasm_call_dtors
  )
  (func $note_script.command_export (;71;) (type 13) (result f32)
    call $note_script
    call $__wasm_call_dtors
  )
  (func $test_blake3_hash_1to1.command_export (;72;) (type 21) (param i32 i32)
    local.get 0
    local.get 1
    call $test_blake3_hash_1to1
    call $__wasm_call_dtors
  )
  (func $test_blake3_hash_2to1.command_export (;73;) (type 21) (param i32 i32)
    local.get 0
    local.get 1
    call $test_blake3_hash_2to1
    call $__wasm_call_dtors
  )
  (func $test_rpo_falcon512_verify.command_export (;74;) (type 21) (param i32 i32)
    local.get 0
    local.get 1
    call $test_rpo_falcon512_verify
    call $__wasm_call_dtors
  )
  (func $test_pipe_words_to_memory.command_export (;75;) (type 22) (param i32 f32)
    local.get 0
    local.get 1
    call $test_pipe_words_to_memory
    call $__wasm_call_dtors
  )
  (func $test_pipe_double_words_to_memory.command_export (;76;) (type 22) (param i32 f32)
    local.get 0
    local.get 1
    call $test_pipe_double_words_to_memory
    call $__wasm_call_dtors
  )
  (func $test_remove_asset.command_export (;77;) (type 0) (param i32) (result f32)
    local.get 0
    call $test_remove_asset
    call $__wasm_call_dtors
  )
  (func $test_create_note.command_export (;78;) (type 23) (param i32 f32 f32 i32) (result f32)
    local.get 0
    local.get 1
    local.get 2
    local.get 3
    call $test_create_note
    call $__wasm_call_dtors
  )
  (table (;0;) 1 1 funcref)
  (memory (;0;) 17)
  (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
  (export "memory" (memory 0))
  (export "get_wallet_magic_number" (func $get_wallet_magic_number.command_export))
  (export "test_add_asset" (func $test_add_asset.command_export))
  (export "test_felt_ops_smoke" (func $test_felt_ops_smoke.command_export))
  (export "note_script" (func $note_script.command_export))
  (export "test_blake3_hash_1to1" (func $test_blake3_hash_1to1.command_export))
  (export "test_blake3_hash_2to1" (func $test_blake3_hash_2to1.command_export))
  (export "test_rpo_falcon512_verify" (func $test_rpo_falcon512_verify.command_export))
  (export "test_pipe_words_to_memory" (func $test_pipe_words_to_memory.command_export))
  (export "test_pipe_double_words_to_memory" (func $test_pipe_double_words_to_memory.command_export))
  (export "test_remove_asset" (func $test_remove_asset.command_export))
  (export "test_create_note" (func $test_create_note.command_export))
  (data $.rodata (;0;) (i32.const 1048576) "../../sdk/base-sys/src/bindings/note.rs\00\00\00\10\00'\00\00\00\11\00\00\00!\00\00\00../../sdk/stdlib-sys/src/stdlib/mem.rs\00\008\00\10\00&\00\00\00J\00\00\00\22\00\00\008\00\10\00&\00\00\00a\00\00\00\1e\00\00\00")
)