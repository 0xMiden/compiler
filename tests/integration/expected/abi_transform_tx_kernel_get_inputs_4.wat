(module $abi_transform_tx_kernel_get_inputs_4.wasm
  (type (;0;) (func (result i32)))
  (type (;1;) (func (param i32) (result i32)))
  (type (;2;) (func (param i32)))
  (type (;3;) (func (param i32 i32) (result i32)))
  (type (;4;) (func (param i32 i32 i32) (result i32)))
  (type (;5;) (func (param i32 i32 i32 i32 i32)))
  (type (;6;) (func (param i32 i32 i32)))
  (type (;7;) (func (param i32 i32 i32 i32)))
  (import "miden:core-import/intrinsics-mem@1.0.0" "heap-base" (func $miden_sdk_alloc::heap_base (;0;) (type 0)))
  (import "miden:core-import/note@1.0.0" "get-inputs" (func $miden_base_sys::bindings::note::extern_note_get_inputs (;1;) (type 1)))
  (func $entrypoint (;2;) (type 2) (param i32)
    local.get 0
    call $miden_base_sys::bindings::note::get_inputs
  )
  (func $__rust_alloc (;3;) (type 3) (param i32 i32) (result i32)
    i32.const 1048632
    local.get 1
    local.get 0
    call $<miden_sdk_alloc::BumpAlloc as core::alloc::global::GlobalAlloc>::alloc
  )
  (func $__rust_alloc_zeroed (;4;) (type 3) (param i32 i32) (result i32)
    block ;; label = @1
      i32.const 1048632
      local.get 1
      local.get 0
      call $<miden_sdk_alloc::BumpAlloc as core::alloc::global::GlobalAlloc>::alloc
      local.tee 1
      i32.eqz
      br_if 0 (;@1;)
      local.get 1
      i32.const 0
      local.get 0
      memory.fill
    end
    local.get 1
  )
  (func $<miden_sdk_alloc::BumpAlloc as core::alloc::global::GlobalAlloc>::alloc (;5;) (type 4) (param i32 i32 i32) (result i32)
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
  (func $miden_base_sys::bindings::note::get_inputs (;6;) (type 2) (param i32)
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
  (func $alloc::raw_vec::RawVecInner<A>::try_allocate_in (;7;) (type 5) (param i32 i32 i32 i32 i32)
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
  (func $<alloc::alloc::Global as core::alloc::Allocator>::allocate (;8;) (type 6) (param i32 i32 i32)
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
  (func $alloc::alloc::Global::alloc_impl (;9;) (type 7) (param i32 i32 i32 i32)
    block ;; label = @1
      local.get 2
      i32.eqz
      br_if 0 (;@1;)
      i32.const 0
      i32.load8_u offset=1048636
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
  (func $alloc::raw_vec::handle_error (;10;) (type 6) (param i32 i32 i32)
    unreachable
  )
  (func $core::ptr::alignment::Alignment::max (;11;) (type 3) (param i32 i32) (result i32)
    local.get 0
    local.get 1
    local.get 0
    local.get 1
    i32.gt_u
    select
  )
  (table (;0;) 1 1 funcref)
  (memory (;0;) 17)
  (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
  (export "memory" (memory 0))
  (export "entrypoint" (func $entrypoint))
  (data $.rodata (;0;) (i32.const 1048576) "../../sdk/base-sys/src/bindings/note.rs\00\00\00\10\00'\00\00\00\11\00\00\00!\00\00\00")
)