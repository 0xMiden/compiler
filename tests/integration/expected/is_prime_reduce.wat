(module $is_prime_reduce.wasm
  (type (;0;) (func (param i32) (result i32)))
  (func $entrypoint (;0;) (type 0) (param i32) (result i32)
    (local i32 i32 i32)
    i32.const 5
    local.set 1
    block ;; label = @1
      block ;; label = @2
        loop ;; label = @3
          local.get 1
          local.get 0
          i32.gt_u
          local.tee 2
          br_if 1 (;@2;)
          local.get 1
          i32.eqz
          br_if 2 (;@1;)
          local.get 0
          local.get 1
          i32.rem_u
          local.set 3
          local.get 1
          i32.const 6
          i32.add
          local.set 1
          local.get 3
          br_if 0 (;@3;)
        end
      end
      local.get 2
      return
    end
    unreachable
  )
  (memory (;0;) 16)
  (global $__stack_pointer (;0;) (mut i32) i32.const 1048576)
  (export "memory" (memory 0))
  (export "entrypoint" (func $entrypoint))
)