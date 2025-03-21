(module $collatz.wasm
  (type (;0;) (func (param i32) (result i32)))
  (memory (;0;) 16)
  (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
  (export "memory" (memory 0))
  (export "entrypoint" (func $entrypoint))
  (func $entrypoint (;0;) (type 0) (param i32) (result i32)
    (local i32)
    i32.const 0
    local.set 1
    loop (result i32) ;; label = @1
      block ;; label = @2
        local.get 0
        i32.const 1
        i32.ne
        br_if 0 (;@2;)
        local.get 1
        return
      end
      local.get 0
      i32.const 3
      i32.mul
      i32.const 1
      i32.add
      local.get 0
      i32.const 1
      i32.shr_u
      local.get 0
      i32.const 1
      i32.and
      select
      local.set 0
      local.get 1
      i32.const 1
      i32.add
      local.set 1
      br 0 (;@1;)
    end
  )
)
