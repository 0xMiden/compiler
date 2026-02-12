(module $test_rust_ac1835b1091ba32949add35009f37731365cda4a0ad759cd80e29c4bfe780395.wasm
  (type (;0;) (func (param i32)))
  (type (;1;) (func (param i32 i32 i32)))
  (table (;0;) 1 1 funcref)
  (memory (;0;) 17)
  (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
  (global (;1;) i32 i32.const 1048777)
  (global (;2;) i32 i32.const 1048784)
  (export "memory" (memory 0))
  (export "entrypoint" (func $entrypoint))
  (export "__data_end" (global 1))
  (export "__heap_base" (global 2))
  (func $__rustc::rust_begin_unwind (;0;) (type 0) (param i32)
    unreachable
  )
  (func $entrypoint (;1;) (type 1) (param i32 i32 i32)
    block ;; label = @1
      local.get 2
      br_if 0 (;@1;)
      i32.const 1048736
      call $core::panicking::panic_const::panic_const_div_by_zero
      unreachable
    end
    local.get 0
    i32.const 0
    i32.store8 offset=2
    local.get 0
    local.get 1
    local.get 2
    i32.div_u
    i32.store16
  )
  (func $core::panicking::panic_fmt (;2;) (type 1) (param i32 i32 i32)
    (local i32)
    global.get $__stack_pointer
    i32.const 32
    i32.sub
    local.tee 3
    global.set $__stack_pointer
    local.get 3
    local.get 1
    i32.store offset=16
    local.get 3
    local.get 0
    i32.store offset=12
    local.get 3
    i32.const 1
    i32.store16 offset=28
    local.get 3
    local.get 2
    i32.store offset=24
    local.get 3
    local.get 3
    i32.const 12
    i32.add
    i32.store offset=20
    local.get 3
    i32.const 20
    i32.add
    call $__rustc::rust_begin_unwind
    unreachable
  )
  (func $core::panicking::panic_const::panic_const_div_by_zero (;3;) (type 0) (param i32)
    i32.const 1048752
    i32.const 51
    local.get 0
    call $core::panicking::panic_fmt
    unreachable
  )
  (data $.rodata (;0;) (i32.const 1048576) "/tmp/test_rust_ac1835b1091ba32949add35009f37731365cda4a0ad759cd80e29c4bfe780395/test_rust_ac1835b1091ba32949add35009f37731365cda4a0ad759cd80e29c4bfe780395.rs\00\00\00\00\00\10\00\9d\00\00\00\13\00\00\00\0b\00\00\00attempt to divide by zero")
)
