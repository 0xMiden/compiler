(module $felt_intrinsics.wasm
  (type (;0;) (func (param i32 i32) (result i32)))
  (type (;1;) (func (param i32 i32 i32 i32) (result i32)))
  (type (;2;) (func (param f32 f32) (result f32)))
  (type (;3;) (func))
  (type (;4;) (func (param i32 i32 i32) (result i32)))
  (type (;5;) (func (result i32)))
  (table (;0;) 2 2 funcref)
  (memory (;0;) 17)
  (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
  (export "memory" (memory 0))
  (export "entrypoint" (func $entrypoint))
  (export "cabi_realloc_wit_bindgen_0_46_0" (func $cabi_realloc_wit_bindgen_0_46_0))
  (export "cabi_realloc" (func $cabi_realloc))
  (elem (;0;) (i32.const 1) func $cabi_realloc)
  (func $__rustc::__rust_alloc (;0;) (type 0) (param i32 i32) (result i32)
    i32.const 1048580
    local.get 1
    local.get 0
    call $<miden_sdk_alloc::BumpAlloc as core::alloc::global::GlobalAlloc>::alloc
  )
  (func $__rustc::__rust_realloc (;1;) (type 1) (param i32 i32 i32 i32) (result i32)
    block ;; label = @1
      i32.const 1048580
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
  (func $entrypoint (;2;) (type 2) (param f32 f32) (result f32)
    local.get 0
    local.get 0
    local.get 1
    call $intrinsics::felt::mul
    local.get 0
    call $intrinsics::felt::sub
    local.get 1
    call $intrinsics::felt::add
    call $intrinsics::felt::div
  )
  (func $__rustc::__rust_no_alloc_shim_is_unstable_v2 (;3;) (type 3)
    return
  )
  (func $<miden_sdk_alloc::BumpAlloc as core::alloc::global::GlobalAlloc>::alloc (;4;) (type 4) (param i32 i32 i32) (result i32)
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
      call $<core::ptr::alignment::Alignment>::max
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
  (func $intrinsics::mem::heap_base (;5;) (type 5) (result i32)
    unreachable
  )
  (func $intrinsics::felt::add (;6;) (type 2) (param f32 f32) (result f32)
    unreachable
  )
  (func $intrinsics::felt::div (;7;) (type 2) (param f32 f32) (result f32)
    unreachable
  )
  (func $intrinsics::felt::mul (;8;) (type 2) (param f32 f32) (result f32)
    unreachable
  )
  (func $intrinsics::felt::sub (;9;) (type 2) (param f32 f32) (result f32)
    unreachable
  )
  (func $<core::ptr::alignment::Alignment>::max (;10;) (type 0) (param i32 i32) (result i32)
    local.get 0
    local.get 1
    local.get 0
    local.get 1
    i32.gt_u
    select
  )
  (func $cabi_realloc (;11;) (type 1) (param i32 i32 i32 i32) (result i32)
    local.get 0
    local.get 1
    local.get 2
    local.get 3
    call $cabi_realloc_wit_bindgen_0_46_0
  )
  (func $alloc::alloc::alloc (;12;) (type 0) (param i32 i32) (result i32)
    call $__rustc::__rust_no_alloc_shim_is_unstable_v2
    local.get 1
    local.get 0
    call $__rustc::__rust_alloc
  )
  (func $wit_bindgen::rt::cabi_realloc (;13;) (type 1) (param i32 i32 i32 i32) (result i32)
    block ;; label = @1
      block ;; label = @2
        block ;; label = @3
          local.get 1
          br_if 0 (;@3;)
          local.get 3
          i32.eqz
          br_if 2 (;@1;)
          local.get 2
          local.get 3
          call $alloc::alloc::alloc
          local.set 2
          br 1 (;@2;)
        end
        local.get 0
        local.get 1
        local.get 2
        local.get 3
        call $__rustc::__rust_realloc
        local.set 2
      end
      local.get 2
      br_if 0 (;@1;)
      unreachable
    end
    local.get 2
  )
  (func $cabi_realloc_wit_bindgen_0_46_0 (;14;) (type 1) (param i32 i32 i32 i32) (result i32)
    local.get 0
    local.get 1
    local.get 2
    local.get 3
    call $wit_bindgen::rt::cabi_realloc
  )
  (data $.rodata (;0;) (i32.const 1048576) "\01\00\00\00")
)
