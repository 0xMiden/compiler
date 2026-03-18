use miden_core::Felt;
use midenc_expect_test::expect_file;
use midenc_frontend_wasm::WasmTranslationConfig;

use crate::{
    CompilerTest,
    testing::{eval_package, setup},
};

#[test]
fn test_func_arg_same() {
    // This test reproduces the https://github.com/0xMiden/compiler/issues/606
    let main_fn = r#"
        (x: &mut Felt, y: &mut Felt) -> i32 {
            intrinsic(x, y)
        }

        #[unsafe(no_mangle)]
        #[inline(never)]
        fn intrinsic(a: &mut Felt, b: &mut Felt) -> i32 {
            unsafe { (a as *mut Felt) as i32 }
        }

    "#;

    setup::enable_compiler_instrumentation();
    let config = WasmTranslationConfig::default();
    let mut test = CompilerTest::rust_fn_body_with_stdlib_sys("func_arg_same", main_fn, config, []);

    let package = test.compile_package();

    let addr1: u32 = 10 * 65536;
    let addr2: u32 = 11 * 65536;

    // Test 1: addr1 is passed as x and should be returned
    let args1 = [Felt::from(addr1), Felt::from(addr2)];
    eval_package::<i32, _, _>(&package, [], &args1, &test.session, |trace| {
        let result: u32 = trace.parse_result().unwrap();
        assert_eq!(result, addr1);
        Ok(())
    })
    .unwrap();

    // Test 1: addr2 is passed as x and should be returned
    let args2 = [Felt::from(addr2), Felt::from(addr1)];
    eval_package::<i32, _, _>(&package, [], &args2, &test.session, |trace| {
        let result: u32 = trace.parse_result().unwrap();
        assert_eq!(result, addr2);
        Ok(())
    })
    .unwrap();
}

/// Regression test for https://github.com/0xMiden/compiler/issues/872
///
/// Previously, compilation could panic during stack manipulation with:
/// `invalid stack index: only the first 16 elements on the stack are directly accessible, got 16`.
#[test]
fn test_invalid_stack_index_16_issue_872() {
    let main_fn = r#"
        (a0: Felt, a1: Felt, a2: Felt, a3: Felt, a4: Felt, a5: Felt, a6: Felt, a7: Felt,
         a8: Felt, a9: Felt, a10: Felt, a11: Felt, a12: Felt, a13: Felt, a14: Felt, a15: Felt) -> Felt {
            // Keep locals live across the call which are used only after the call, so that the 16
            // call arguments are not at the top of the operand stack at call time.
            let post = a0 + miden_stdlib_sys::felt!(1);

            let res = callee_16(a0, a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11, a12, a13, a14, a15);

            // Use all post-call locals to prevent DCE.
            res + post
        }

        #[inline(never)]
        fn callee_16(
            a0: Felt, a1: Felt, a2: Felt, a3: Felt, a4: Felt, a5: Felt, a6: Felt, a7: Felt,
            a8: Felt, a9: Felt, a10: Felt, a11: Felt, a12: Felt, a13: Felt, a14: Felt, a15: Felt,
        ) -> Felt {
            a0 + a1 + a2 + a3 + a4 + a5 + a6 + a7 + a8 + a9 + a10 + a11 + a12 + a13 + a14 + a15
        }
    "#;

    setup::enable_compiler_instrumentation();
    let config = WasmTranslationConfig::default();
    let mut test =
        CompilerTest::rust_fn_body_with_stdlib_sys("movup_16_issue_831", main_fn, config, []);

    let package = test.compile_package();

    // This should execute and return the expected value.
    let args: [Felt; 16] = [
        Felt::from(1u32),
        Felt::from(2u32),
        Felt::from(3u32),
        Felt::from(4u32),
        Felt::from(5u32),
        Felt::from(6u32),
        Felt::from(7u32),
        Felt::from(8u32),
        Felt::from(9u32),
        Felt::from(10u32),
        Felt::from(11u32),
        Felt::from(12u32),
        Felt::from(13u32),
        Felt::from(14u32),
        Felt::from(15u32),
        Felt::from(16u32),
    ];

    let expected = (1u32..=16u32).fold(Felt::ZERO, |acc, x| acc + Felt::from(x)) + Felt::from(2u32);

    eval_package::<Felt, _, _>(&package, [], &args, &test.session, |trace| {
        let res: Felt = trace.parse_result().unwrap();
        assert_eq!(res, expected);
        Ok(())
    })
    .unwrap();
}

/// Regression test for the case of 4 words + 1 felt (17 felts) function args the words are passed by pointer.
#[test]
fn test_invalid_stack_index_4_word_1_felt_args() {
    let main_fn = r#"
        (
            w0_0: Felt,
            w0_1: Felt,
            w0_2: Felt,
            w0_3: Felt,
            w1_0: Felt,
            w1_1: Felt,
            w1_2: Felt,
            w1_3: Felt,
            w2_0: Felt,
            w2_1: Felt,
            w2_2: Felt,
            w2_3: Felt,
            w3_0: Felt,
            w3_1: Felt,
            w3_2: Felt,
            w3_3: Felt,
        ) -> Felt {
            let w0 = Word::new([w0_0, w0_1, w0_2, w0_3]);
            let w1 = Word::new([w1_0, w1_1, w1_2, w1_3]);
            let w2 = Word::new([w2_0, w2_1, w2_2, w2_3]);
            let w3 = Word::new([w3_0, w3_1, w3_2, w3_3]);

            // Keep locals live across the call which are used only after the call, so that the
            // call arguments are not at the top of the operand stack at call time.
            let post = w0[0] + miden_stdlib_sys::felt!(1);

            let extra = w0[1];
            let res = callee_5(w0, w1, w2, w3, extra);

            // Use all post-call locals to prevent DCE.
            res + post
        }

        #[inline(never)]
        fn callee_5(w0: Word, w1: Word, w2: Word, w3: Word, extra: Felt) -> Felt {
            w0[0] + w0[1] + w0[2] + w0[3] +
            w1[0] + w1[1] + w1[2] + w1[3] +
            w2[0] + w2[1] + w2[2] + w2[3] +
            w3[0] + w3[1] + w3[2] + w3[3] +
            extra
        }
    "#;

    setup::enable_compiler_instrumentation();
    let config = WasmTranslationConfig::default();
    let mut test = CompilerTest::rust_fn_body_with_stdlib_sys(
        "movup_4_word_1_felt_args_invalid_stack_index",
        main_fn,
        config,
        [],
    );

    let package = test.compile_package();

    // This should execute and return the expected value.
    let args: [Felt; 16] = [
        // w0
        Felt::from(1u32),
        Felt::from(2u32),
        Felt::from(3u32),
        Felt::from(4u32),
        // w1
        Felt::from(5u32),
        Felt::from(6u32),
        Felt::from(7u32),
        Felt::from(8u32),
        // w2
        Felt::from(9u32),
        Felt::from(10u32),
        Felt::from(11u32),
        Felt::from(12u32),
        // w3
        Felt::from(13u32),
        Felt::from(14u32),
        Felt::from(15u32),
        Felt::from(16u32),
    ];

    // Expected:
    // - callee_5 sums 1..=16 and adds `extra` (w0[1] == 2)
    // - main adds `post` (w0[0] + 1 == 2)
    let expected = (1u32..=16u32).fold(Felt::ZERO, |acc, x| acc + Felt::from(x)) + Felt::from(4u32);

    eval_package::<Felt, _, _>(&package, [], &args, &test.session, |trace| {
        let res: Felt = trace.parse_result().unwrap();
        assert_eq!(res, expected);
        Ok(())
    })
    .unwrap();
}

/// Regression test for https://github.com/0xMiden/compiler/issues/811
///
/// This reproduces a bug (byte-by-byte copying) in `memory.copy` lowering used by `Vec` reallocation.
#[test]
fn test_vec_realloc_copies_data_issue_811() {
    let main_fn = r#"() -> Felt {
        extern crate alloc;
        use alloc::vec::Vec;

        // Create a Vec with a tiny capacity to make growth (and thus reallocation) likely.
        let mut v: Vec<Felt> = Vec::with_capacity(1);

        v.push(felt!(11111));
        let mut last_ptr = v.as_ptr() as u32;
        let mut moves: u32 = 0;

        v.push(felt!(22222));
        let ptr = v.as_ptr() as u32;
        if ptr != last_ptr {
            moves += 1;
            last_ptr = ptr;
        }

        v.push(felt!(33333));
        let ptr = v.as_ptr() as u32;
        if ptr != last_ptr {
            moves += 1;
            last_ptr = ptr;
        }

        v.push(felt!(44444));
        let ptr = v.as_ptr() as u32;
        if ptr != last_ptr {
            moves += 1;
            last_ptr = ptr;
        }

        v.push(felt!(55555));
        let ptr = v.as_ptr() as u32;
        if ptr != last_ptr {
            moves += 1;
            last_ptr = ptr;
        }

        // Sum all elements - if realloc doesn't copy, the first 4 elements will be garbage.
        let sum = v[0] + v[1] + v[2] + v[3] + v[4];
        if moves >= 2 { sum } else { felt!(0) }
    }"#;

    setup::enable_compiler_instrumentation();
    let config = WasmTranslationConfig::default();
    let mut test =
        CompilerTest::rust_fn_body_with_stdlib_sys("vec_realloc_copies_data", main_fn, config, []);

    let package = test.compile_package();
    let args: [Felt; 0] = [];

    eval_package::<Felt, _, _>(&package, [], &args, &test.session, |trace| {
        let result: u64 = trace.parse_result::<Felt>().unwrap().as_canonical_u64();
        assert_eq!(result, 166_665, "Vec reallocation failed to copy existing elements");
        Ok(())
    })
    .unwrap();
}

#[ignore = "too fragile (depends on mem addrs), this bug is also covered by the test_hmerge test"]
#[test]
fn test_func_arg_order() {
    // This test reproduces the "swapped and frozen" function arguments issue
    // https://github.com/0xMiden/compiler/pull/576 discovered while working on hmerge VM op
    // The issue manifests in "intrinsic" function parameters being in the wrong order
    // (see assert_eq before the call and inside the function)
    // on the stack AND the their order is not changing when the parameters are
    // swapped at the call site (see expect_masm with the same file name, i.e. the MASM
    // do not change when she parameters are swapped).
    fn main_fn_template(digest_ptr_name: &str, result_ptr_name: &str) -> String {
        format!(
            r#"
    (f0: miden_stdlib_sys::Felt, f1: miden_stdlib_sys::Felt, f2: miden_stdlib_sys::Felt, f3: miden_stdlib_sys::Felt, f4: miden_stdlib_sys::Felt, f5: miden_stdlib_sys::Felt, f6: miden_stdlib_sys::Felt, f7: miden_stdlib_sys::Felt) -> miden_stdlib_sys::Felt {{
        let digest1 = miden_stdlib_sys::Digest::new([f0, f1, f2, f3]);
        let digest2 = miden_stdlib_sys::Digest::new([f4, f5, f6, f7]);
        let digests = [digest1, digest2];
        let res = merge(digests);
        res.inner[0]
    }}

    #[inline]
    pub fn merge(digests: [Digest; 2]) -> Digest {{
        unsafe {{
            let digests_ptr = digests.as_ptr().addr() as u32;
            assert_eq(Felt::from_u32(digests_ptr as u32), Felt::from_u32(1048528));

            let mut ret_area = ::core::mem::MaybeUninit::<Word>::uninit();
            let result_ptr = ret_area.as_mut_ptr().addr() as u32;
            assert_eq(Felt::from_u32(result_ptr as u32), Felt::from_u32(1048560));

            intrinsic({digest_ptr_name} as *const Felt, {result_ptr_name} as *mut Felt);

            Digest::from_word(ret_area.assume_init())
        }}
    }}

    #[unsafe(no_mangle)]
    fn intrinsic(digests_ptr: *const Felt, result_ptr: *mut Felt) {{
        // see assert_eq above, before the call
        assert_eq(Felt::from_u32(digests_ptr as u32), Felt::from_u32(1048528));
        // see assert_eq above, before the call
        assert_eq(Felt::from_u32(result_ptr as u32), Felt::from_u32(1048560));
    }}
        "#
        )
    }

    let config = WasmTranslationConfig::default();
    let main_fn_correct = main_fn_template("digests_ptr", "result_ptr");
    let mut test = CompilerTest::rust_fn_body_with_stdlib_sys(
        "func_arg_order",
        &main_fn_correct,
        config.clone(),
        [],
    );

    let args = [
        Felt::ZERO,
        Felt::ZERO,
        Felt::ZERO,
        Felt::ZERO,
        Felt::ZERO,
        Felt::ZERO,
        Felt::ZERO,
        Felt::ZERO,
    ];

    eval_package::<Felt, _, _>(&test.compile_package(), [], &args, &test.session, |trace| Ok(()))
        .unwrap();
}

/// Minimized regression test for the `resolve_turn` reproduction.
///
/// This keeps the relevant shape:
/// - `TurnAction { champion_id: u8, ability_index: u8 }`
/// - `Champion { abilities: [Ability; 2] }` with `abilities[action.ability_index]`
/// - A large `TurnResult` return-by-value
/// - A helper `execute_action` called from `resolve_turn`
#[test]
fn test_resolve_turn_minimal_large_return() {
    let main_fn = r#"() -> Felt {
        /// Player-selected action for a turn.
        #[derive(Clone, Copy)]
        struct TurnAction {
            champion_id: u8,
            ability_index: u8,
        }

        /// Ability category used to determine the emitted event.
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        #[repr(u8)]
        enum AbilityType {
            Damage = 0,
            Heal = 1,
            StatMod = 2,
        }

        /// Champion ability definition.
        #[derive(Clone, Copy)]
        struct Ability {
            ability_type: AbilityType,
        }

        /// Base champion definition.
        #[derive(Clone, Copy)]
        struct Champion {
            id: u8,
            abilities: [Ability; 2],
        }

        const MAX_EVENTS: usize = 16;

        /// Event stream emitted during turn resolution.
        #[derive(Clone, Copy)]
        enum TurnEvent {
            Attack {
                attacker_id: u8,
                ability_index: u8,
            },
            Heal {
                champion_id: u8,
                ability_index: u8,
            },
            Debuff {
                target_id: u8,
                ability_index: u8,
            },
            None,
        }

        /// Return value of `resolve_turn`.
        #[derive(Clone, Copy)]
        struct TurnResult {
            events: [TurnEvent; MAX_EVENTS],
            event_count: u8,
        }

        /// Append an event to the fixed-size event buffer.
        fn push_event(events: &mut [TurnEvent; MAX_EVENTS], count: &mut u8, event: TurnEvent) {
            if (*count as usize) < MAX_EVENTS {
                events[*count as usize] = event;
                *count += 1;
            } else {
                core::arch::wasm32::unreachable();
            }
        }

        /// Execute a single action by indexing into `abilities` via `action.ability_index`.
        #[inline(never)]
        fn execute_action(
            actor_champ: &Champion,
            action: &TurnAction,
            events: &mut [TurnEvent; MAX_EVENTS],
            event_count: &mut u8,
        ) {
            let ability = &actor_champ.abilities[action.ability_index as usize];
            match ability.ability_type {
                AbilityType::Damage => {
                    push_event(
                        events,
                        event_count,
                        TurnEvent::Attack {
                            attacker_id: actor_champ.id,
                            ability_index: action.ability_index,
                        },
                    );
                }
                AbilityType::Heal => {
                    push_event(
                        events,
                        event_count,
                        TurnEvent::Heal {
                            champion_id: actor_champ.id,
                            ability_index: action.ability_index,
                        },
                    );
                }
                AbilityType::StatMod => {
                    push_event(
                        events,
                        event_count,
                        TurnEvent::Debuff {
                            target_id: actor_champ.id,
                            ability_index: action.ability_index,
                        },
                    );
                }
            }
        }

        /// Resolve a single combat round between two champions.
        #[inline(never)]
        fn resolve_turn(
            action_a: &TurnAction,
            action_b: &TurnAction,
        ) -> TurnResult {
            let champ_a = Champion {
                id: 1,
                abilities: [
                    Ability {
                        ability_type: AbilityType::Damage,
                    },
                    Ability {
                        ability_type: AbilityType::Heal,
                    },
                ],
            };
            let champ_b = Champion {
                id: 2,
                abilities: [
                    Ability {
                        ability_type: AbilityType::Damage,
                    },
                    Ability {
                        ability_type: AbilityType::StatMod,
                    },
                ],
            };

            let mut events = [TurnEvent::None; MAX_EVENTS];
            let mut event_count: u8 = 0;

            execute_action(&champ_a, action_a, &mut events, &mut event_count);
            execute_action(&champ_b, action_b, &mut events, &mut event_count);

            TurnResult {
                events,
                event_count,
            }
        }

        let actions = [
            TurnAction {
                champion_id: 1,
                ability_index: 0,
            },
            TurnAction {
                champion_id: 0,
                ability_index: 1,
            },
        ];

        let result = resolve_turn(&actions[0], &actions[1]);

        fn tag(ev: TurnEvent) -> u8 {
            match ev {
                TurnEvent::None => 0,
                TurnEvent::Attack { .. } => 1,
                TurnEvent::Heal { .. } => 2,
                TurnEvent::Debuff { .. } => 3,
            }
        }

        let mut ok: u32 = 0;
        if result.event_count == 2 { ok |= 1 << 0; }
        if tag(result.events[0]) == 1 { ok |= 1 << 1; }
        if tag(result.events[1]) == 3 { ok |= 1 << 2; }

        // Pack diagnostics into the return value:
        // - low 8 bits: ok-bitmask
        // - bits 8..=15: `event_count`
        // - bits 16..=23: tag(events[0])
        // - bits 24..=31: tag(events[1])
        let diag = (ok & 0xff)
            | ((result.event_count as u32) << 8)
            | ((tag(result.events[0]) as u32) << 16)
            | ((tag(result.events[1]) as u32) << 24);
        Felt::from_u32(diag)
    }"#;

    setup::enable_compiler_instrumentation();
    let config = WasmTranslationConfig::default();
    let mut test = CompilerTest::rust_fn_body_with_stdlib_sys(
        "resolve_turn_minimal_large_return",
        main_fn,
        config,
        [],
    );

    let package = test.compile_package();
    let args: [Felt; 0] = [];

    eval_package::<Felt, _, _>(&package, [], &args, &test.session, |trace| {
        let res: Felt = trace.parse_result().unwrap();
        let diag: u32 = res.as_int() as u32;
        let ok = diag & 0xff;
        let event_count = (diag >> 8) & 0xff;
        let tag0 = (diag >> 16) & 0xff;
        let tag1 = (diag >> 24) & 0xff;
        if ok != 0b111 {
            panic!(
                "resolve_turn_minimal_large_return unexpected result: ok=0b{ok:08b} \
                 (event_count={event_count}, tag0={tag0}, tag1={tag1})"
            );
        }
        Ok(())
    })
    .unwrap();
}
