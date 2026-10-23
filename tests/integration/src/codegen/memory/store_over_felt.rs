//! Partial stores into memory elements that hold field elements.
//!
//! A store narrower than an element, or one that is not element-aligned, rewrites some bytes of an
//! element and keeps the others. When the element was last written as a felt above `2^32` (a stack
//! slot reused for a struct with a `u16` field), the kept bytes are the felt's low 32 bits. The
//! elements past the stored bytes keep their felts, and a load past an element's first byte does
//! not trap on a felt in the next element.

use proptest::array::uniform3;

use super::*;

/// The field modulus.
const P: u64 = 0xffff_ffff_0000_0001;

/// How a store gets its address.
#[derive(Clone, Copy, Debug)]
enum Pointer {
    /// A function argument, which the code generator cannot fold into the store.
    Dynamic,
    /// A constant, which the code generator folds into the store.
    Immediate,
}

/// Seeds three consecutive elements with felts above `2^32`, stores a `ty` value `offset` bytes
/// past the first of them through `pointer`, and checks that the elements the store touched hold
/// the stored bytes and the low 32 bits of the felts elsewhere, and that the others are unchanged.
fn store_over_felts(ty: Type, offset: u32, pointer: Pointer) {
    setup::enable_compiler_instrumentation();

    // The start of the 17th page, past the 16 pages reserved for the Rust stack.
    let base = 17 * 2u32.pow(16);
    let store_to = base + offset;
    let size = ty.size_in_bytes();
    let touched = (offset as usize + size).div_ceil(4);
    let span = SourceSpan::default();

    let params = match pointer {
        Pointer::Dynamic => vec![Type::U32, ty.clone()],
        Pointer::Immediate => vec![ty.clone()],
    };
    let (package, context) = compile_test_module(params, [Type::U32], |builder| {
        let block = builder.current_block();
        let args = block
            .borrow()
            .arguments()
            .iter()
            .map(|arg| *arg as ValueRef)
            .collect::<Vec<_>>();
        let (addr, value) = match pointer {
            Pointer::Dynamic => (args[0], args[1]),
            Pointer::Immediate => (builder.u32(store_to, span), args[0]),
        };
        let ptr = builder.inttoptr(addr, Type::from(PointerType::new(ty.clone())), span).unwrap();
        builder.store(ptr, value, span).unwrap();
        let result = builder.u32(1, span);
        builder.ret(Some(result), span).unwrap();
    });

    let value_mask = u64::MAX >> (64 - 8 * size);
    let felts = uniform3((1u64 << 32)..P);
    let config = proptest::test_runner::Config::with_cases(16);
    let res = TestRunner::new(config).run(&(any::<u64>(), felts), move |(value, felts)| {
        let value = value & value_mask;
        let felts = felts.map(Felt::new_unchecked);
        let initializers = [Initializer::MemoryFelts {
            addr: base / 4,
            felts: Cow::Borrowed(&felts),
        }];

        let mut args = Vec::new();
        if let Pointer::Dynamic = pointer {
            args.push(Felt::new_unchecked(store_to as u64));
        }
        if size == 8 {
            value.push_to_operand_stack(&mut args);
        } else {
            args.push(Felt::new_unchecked(value));
        }

        let mut expected = felts
            .iter()
            .flat_map(|felt| (felt.as_canonical_u64() as u32).to_le_bytes())
            .collect::<Vec<_>>();
        expected[offset as usize..][..size].copy_from_slice(&value.to_le_bytes()[..size]);

        let output = eval_package::<u32, _, _>(
            package.clone(),
            initializers,
            &args,
            context.session(),
            |trace| {
                for (index, felt) in felts.iter().enumerate() {
                    let addr = base + 4 * index as u32;
                    if index >= touched {
                        let element = trace.read_memory_element(addr / 4).unwrap_or_default();
                        prop_assert_eq!(element, *felt, "element at byte address {} changed", addr);
                        continue;
                    }
                    let observed = trace.read_from_rust_memory::<u32>(addr).ok_or_else(|| {
                        TestCaseError::fail(format!("failed to read from byte address {addr}"))
                    })?;
                    for (byte, (observed, expected)) in
                        observed.to_le_bytes().into_iter().zip(&expected[4 * index..]).enumerate()
                    {
                        prop_assert_eq!(
                            observed,
                            *expected,
                            "unexpected byte at address {}",
                            addr + byte as u32
                        );
                    }
                }
                Ok(())
            },
        )?;

        prop_assert_eq!(output, 1u32);
        Ok(())
    });

    match res {
        Err(TestError::Fail(reason, value)) => panic!(
            "FAILURE storing {ty} at offset {offset} through a {pointer:?} pointer: {}\nMinimal \
             failing case: {value:?}",
            reason.message()
        ),
        Ok(_) => (),
        _ => panic!("Unexpected test result: {res:?}"),
    }
}

/// A byte store keeps the low 32 bits of the felt in its element.
#[test]
fn store_u8_over_felt() {
    for pointer in [Pointer::Dynamic, Pointer::Immediate] {
        for offset in 0..4 {
            store_over_felts(Type::U8, offset, pointer);
        }
    }
}

/// A 16-bit store keeps the low 32 bits of the felts in its elements, within one element and
/// across two.
#[test]
fn store_u16_over_felt() {
    for pointer in [Pointer::Dynamic, Pointer::Immediate] {
        for offset in 0..4 {
            store_over_felts(Type::U16, offset, pointer);
        }
    }
}

/// An unaligned 32-bit store keeps the low 32 bits of the felts in the two elements it spans.
#[test]
fn store_unaligned_u32_over_felt() {
    for offset in 1..4 {
        store_over_felts(Type::U32, offset, Pointer::Dynamic);
    }
}

/// An unaligned 64-bit store keeps the low 32 bits of the felts in the outer two of the three
/// elements it spans.
#[test]
fn store_unaligned_u64_over_felt() {
    for offset in 1..4 {
        store_over_felts(Type::U64, offset, Pointer::Dynamic);
    }
}

/// Seeds an element with 32-bit bytes and the next one with a felt above `2^32`, and checks that a
/// `u16` load `offset` bytes into the first element through `pointer` returns those bytes.
fn load_u16_before_felt(offset: u32, pointer: Pointer) {
    setup::enable_compiler_instrumentation();

    // The start of the 17th page, past the 16 pages reserved for the Rust stack.
    let base = 17 * 2u32.pow(16);
    let load_from = base + offset;
    let span = SourceSpan::default();

    let params = match pointer {
        Pointer::Dynamic => vec![Type::U32],
        Pointer::Immediate => vec![],
    };
    let (package, context) = compile_test_module(params, [Type::U16], |builder| {
        let addr = match pointer {
            Pointer::Dynamic => builder.current_block().borrow().arguments()[0] as ValueRef,
            Pointer::Immediate => builder.u32(load_from, span),
        };
        let ptr = builder.inttoptr(addr, Type::from(PointerType::new(Type::U16)), span).unwrap();
        let loaded = builder.load(ptr, span).unwrap();
        builder.ret(Some(loaded), span).unwrap();
    });

    let config = proptest::test_runner::Config::with_cases(16);
    let res = TestRunner::new(config).run(&(any::<u32>(), (1u64 << 32)..P), move |(bits, felt)| {
        let felts = [Felt::new_unchecked(bits as u64), Felt::new_unchecked(felt)];
        let initializers = [Initializer::MemoryFelts {
            addr: base / 4,
            felts: Cow::Borrowed(&felts),
        }];
        let args = match pointer {
            Pointer::Dynamic => vec![Felt::new_unchecked(load_from as u64)],
            Pointer::Immediate => vec![],
        };

        let output = eval_package::<u16, _, _>(
            package.clone(),
            initializers,
            &args,
            context.session(),
            |_| Ok(()),
        )?;

        prop_assert_eq!(output, (bits >> (8 * offset)) as u16);
        Ok(())
    });

    match res {
        Err(TestError::Fail(reason, value)) => panic!(
            "FAILURE loading u16 at offset {offset} through a {pointer:?} pointer: {}\nMinimal \
             failing case: {value:?}",
            reason.message()
        ),
        Ok(_) => (),
        _ => panic!("Unexpected test result: {res:?}"),
    }
}

/// A 16-bit load within an element returns its bytes when the next element holds a felt.
#[test]
fn load_u16_before_felt_within_element() {
    for pointer in [Pointer::Dynamic, Pointer::Immediate] {
        for offset in 1..3 {
            load_u16_before_felt(offset, pointer);
        }
    }
}
