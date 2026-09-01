//! Typed calls to a procedure whose MAST root the account keeps in storage.
//!
//! An account can keep the MAST root of a procedure in a storage slot and call that procedure
//! later. [`ProcedureRoot`] holds such a root, but it does not say which arguments the procedure
//! takes. [`StoredProcedure`] holds the same root together with a signature, so the compiler
//! checks every call.
//!
//! # Dispatch stubs
//!
//! A call goes through a shape stub of the `miden-base-sys` stub archive. The Wasm frontend finds
//! the `intrinsics::exec_root` prefix, reads the call signature from the stub and replaces the
//! call with a dispatch through the root.
//!
//! The stubs are keyed by shape, not by call site: `a<N>_r<f|v>` takes the four root field
//! elements, then `N` argument field elements, and returns one field element (`rf`) or nothing
//! (`rv`). `N` runs from 0 to 12, because the operand stack window holds 16 field elements and the
//! root takes four of them.
//!
//! The declarations below name the stubs only. The stub archive holds the definitions, and
//! `miden-base-sys` links the whole archive into every guest, so the linker finds each stub that a
//! guest calls. The archive is a separate compilation unit, so the optimizer of the calling crate
//! sees a declaration and keeps the call.

use core::marker::PhantomData;

use miden_stdlib_sys::{Digest, Felt, Word};

use crate::{ProcedureRoot, WordValue};

/// Number of field elements a procedure root occupies on the operand stack.
const ROOT_FELTS: usize = 4;

/// Largest number of field elements the arguments of one call may occupy.
///
/// The backend must fit the procedure root and every argument into the 16-element operand window,
/// so the arguments get what the root leaves free.
const MAX_ARG_FELTS: usize = 16 - ROOT_FELTS;

/// A procedure root together with the signature of the procedure.
///
/// `F` is a bare function-pointer type that gives the signature, for example
/// `StoredProcedure<fn(Word, Felt) -> Felt>`. `F` carries no data and holds no function.
///
/// # Typed storage slots
///
/// A slot declared as `StorageValue<StoredProcedure<fn(Word, Felt) -> Felt>>` accepts a root of
/// that signature only. A value of another signature does not compile, and a call with other
/// argument types does not compile. The slot therefore fixes the signature for every reader of the
/// account.
///
/// # Trust boundary
///
/// [`ProcedureRoot::assume_signature`] is the one point where the signature enters the type. No
/// step after that point checks the signature again.
///
/// # Slot that holds no root
///
/// A slot that nobody set holds the zero word. A call through a zero root makes the transaction
/// fail with the message "stored procedure call: procedure root is zero (storage slot not
/// initialized)".
///
/// # Example
///
/// ```ignore
/// use miden::{Call2, Felt, StoredProcedure, StorageValue, Word};
///
/// // The slot fixes the signature of every root it holds.
/// type Handler = StoredProcedure<fn(Word, Felt) -> Felt>;
///
/// fn dispatch(slot: &StorageValue<Handler>, asset: Word, amount: Felt) -> Felt {
///     slot.get().call(asset, amount)
/// }
/// ```
pub struct StoredProcedure<F> {
    /// MAST root of the procedure.
    root: ProcedureRoot,
    /// Signature of the procedure.
    sig: PhantomData<F>,
}

// The derives put a bound on `F`, but `F` is a marker and never holds a value, so the impls are
// written by hand.

impl<F> Clone for StoredProcedure<F> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<F> Copy for StoredProcedure<F> {}

impl<F> core::fmt::Debug for StoredProcedure<F> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("StoredProcedure").field("root", &self.root).finish()
    }
}

impl<F> PartialEq for StoredProcedure<F> {
    fn eq(&self, other: &Self) -> bool {
        self.root == other.root
    }
}

impl<F> Eq for StoredProcedure<F> {}

impl ProcedureRoot {
    /// Adds a signature to this root without a check.
    ///
    /// # This is an assertion, not a check
    ///
    /// The caller asserts that this word is the MAST root of a procedure of this account's
    /// interface, and that the flat signature of that procedure is exactly `F`. Nothing tests the
    /// assertion: not this function, not the compiler, and not the VM.
    ///
    /// A wrong assertion breaks every call through the result. The VM takes the wrong number of
    /// field elements from the operand stack. The transaction then fails, or the call gives a
    /// wrong result.
    ///
    /// Use this function only where the root comes from a source you trust, for example from the
    /// package manifest of a known component.
    pub fn assume_signature<F>(self) -> StoredProcedure<F> {
        StoredProcedure {
            root: self,
            sig: PhantomData,
        }
    }
}

impl<F> StoredProcedure<F> {
    /// Returns the MAST root, without the signature.
    pub fn root(self) -> ProcedureRoot {
        self.root
    }
}

impl<F> From<StoredProcedure<F>> for ProcedureRoot {
    fn from(procedure: StoredProcedure<F>) -> Self {
        procedure.root
    }
}

impl<F> WordValue for StoredProcedure<F> {
    fn try_into_word(self) -> Result<Word, &'static str> {
        self.root.try_into_word()
    }

    fn try_from_word(word: Word) -> Result<Self, &'static str> {
        Ok(ProcedureRoot::try_from_word(word)?.assume_signature())
    }
}

/// A value that a stored-procedure call passes on the operand stack.
///
/// Everything crosses the stub boundary as a field element. A word-sized value takes four field
/// elements, `Felt` passes through, and `bool`, `u8`, `u16` and `u32` convert here — the same
/// conversions the `miden-base-sys` bindings make at their own extern boundaries.
pub trait StoredArg {
    /// Number of field elements the value occupies on the operand stack.
    const WIDTH: usize;

    /// Appends the field elements of the value to `buffer`.
    #[doc(hidden)]
    fn write_felts(self, buffer: &mut ArgBuffer);
}

/// A value that a stored-procedure call returns.
///
/// A call returns nothing or one field element. The implementation selects the dispatch stub and
/// converts the result.
pub trait StoredRet: Sized {
    /// Calls the shape stub that takes `width` argument field elements.
    #[doc(hidden)]
    fn dispatch(root: Word, args: &ArgBuffer, width: usize) -> Self;
}

/// The argument field elements of one call, in declaration order.
///
/// The buffer always holds room for the widest call. `len` counts the field elements the arguments
/// wrote, and every value it takes is a compile-time constant, so the optimizer removes the
/// cursor.
#[doc(hidden)]
#[derive(Clone, Copy)]
pub struct ArgBuffer {
    /// Argument field elements, followed by unused elements.
    felts: [Felt; MAX_ARG_FELTS],
    /// Number of field elements the arguments wrote.
    len: usize,
}

impl ArgBuffer {
    /// Returns an empty buffer.
    #[inline(always)]
    fn new() -> Self {
        Self {
            felts: [Felt::ZERO; MAX_ARG_FELTS],
            len: 0,
        }
    }

    /// Appends the field elements of `value` and returns the buffer.
    #[inline(always)]
    fn push<A: StoredArg>(mut self, value: A) -> Self {
        value.write_felts(&mut self);
        self
    }

    /// Appends one field element.
    #[inline(always)]
    fn push_felt(&mut self, felt: Felt) {
        self.felts[self.len] = felt;
        self.len += 1;
    }

    /// Appends the four field elements of a word.
    ///
    /// The four positions are written one by one, and not in a loop: a constant position lets the
    /// optimizer hold the buffer in registers.
    #[inline(always)]
    fn push_word(&mut self, word: Word) {
        self.push_felt(word[0]);
        self.push_felt(word[1]);
        self.push_felt(word[2]);
        self.push_felt(word[3]);
    }
}

impl StoredArg for Felt {
    const WIDTH: usize = 1;

    #[inline(always)]
    fn write_felts(self, buffer: &mut ArgBuffer) {
        buffer.push_felt(self);
    }
}

impl StoredArg for bool {
    const WIDTH: usize = 1;

    #[inline(always)]
    fn write_felts(self, buffer: &mut ArgBuffer) {
        buffer.push_felt(if self { Felt::ONE } else { Felt::ZERO });
    }
}

/// Implements [`StoredArg`] for an integer type that fits in one field element.
macro_rules! stored_arg_from_int {
    ($($ty:ty),* $(,)?) => {
        $(
            impl StoredArg for $ty {
                const WIDTH: usize = 1;

                #[inline(always)]
                fn write_felts(self, buffer: &mut ArgBuffer) {
                    buffer.push_felt(Felt::from(self));
                }
            }
        )*
    };
}

stored_arg_from_int!(u8, u16, u32);

/// Implements [`StoredArg`] for a type that a word holds.
macro_rules! stored_arg_from_word {
    ($($ty:ty),* $(,)?) => {
        $(
            impl StoredArg for $ty {
                const WIDTH: usize = ROOT_FELTS;

                #[inline(always)]
                fn write_felts(self, buffer: &mut ArgBuffer) {
                    buffer.push_word(Word::from(self));
                }
            }
        )*
    };
}

stored_arg_from_word!(Word, Digest, ProcedureRoot);

impl StoredRet for () {
    #[inline(always)]
    fn dispatch(root: Word, args: &ArgBuffer, width: usize) -> Self {
        dispatch_unit(root, args, width)
    }
}

impl StoredRet for Felt {
    #[inline(always)]
    fn dispatch(root: Word, args: &ArgBuffer, width: usize) -> Self {
        dispatch_felt(root, args, width)
    }
}

impl StoredRet for bool {
    #[inline(always)]
    fn dispatch(root: Word, args: &ArgBuffer, width: usize) -> Self {
        dispatch_felt(root, args, width) != Felt::ZERO
    }
}

/// Implements [`StoredRet`] for an integer type that fits in one field element.
macro_rules! stored_ret_to_int {
    ($($ty:ty),* $(,)?) => {
        $(
            impl StoredRet for $ty {
                #[inline(always)]
                fn dispatch(root: Word, args: &ArgBuffer, width: usize) -> Self {
                    dispatch_felt(root, args, width).as_canonical_u64() as _
                }
            }
        )*
    };
}

stored_ret_to_int!(u8, u16, u32);

/// Declares the dispatch stubs of every shape and the two dispatch functions.
///
/// Each entry supplies the number of argument field elements, the exported name and the local name
/// of the stub that returns a field element, the same pair for the stub that returns nothing, one
/// parameter name for each argument, and the position of each argument in the buffer.
macro_rules! exec_root_shapes {
    ($($width:literal, $felt_symbol:literal, $felt_stub:ident, $unit_symbol:literal,
       $unit_stub:ident, ($($param:ident),*), ($($index:literal),*);)*) => {
        // The stub archive holds the definitions. A declaration alone creates no reference, so a
        // guest keeps only the stubs it calls.
        unsafe extern "C" {
            $(
                #[link_name = $felt_symbol]
                fn $felt_stub(
                    root0: Felt,
                    root1: Felt,
                    root2: Felt,
                    root3: Felt
                    $(, $param: Felt)*
                ) -> Felt;

                #[link_name = $unit_symbol]
                fn $unit_stub(
                    root0: Felt,
                    root1: Felt,
                    root2: Felt,
                    root3: Felt
                    $(, $param: Felt)*
                );
            )*
        }

        /// Calls the stub of `width` argument field elements that returns a field element.
        ///
        /// `width` is a compile-time constant at every call site, so the optimizer keeps one arm.
        #[inline(always)]
        fn dispatch_felt(root: Word, args: &ArgBuffer, width: usize) -> Felt {
            match width {
                $(
                    $width => unsafe {
                        $felt_stub(root[0], root[1], root[2], root[3]
                            $(, args.felts[$index])*)
                    },
                )*
                // `Call0` to `Call12` check the width at compile time, so no other value arrives.
                _ => unreachable!(),
            }
        }

        /// Calls the stub of `width` argument field elements that returns nothing.
        ///
        /// `width` is a compile-time constant at every call site, so the optimizer keeps one arm.
        #[inline(always)]
        fn dispatch_unit(root: Word, args: &ArgBuffer, width: usize) {
            match width {
                $(
                    $width => unsafe {
                        $unit_stub(root[0], root[1], root[2], root[3]
                            $(, args.felts[$index])*)
                    },
                )*
                // `Call0` to `Call12` check the width at compile time, so no other value arrives.
                _ => unreachable!(),
            }
        }
    };
}

exec_root_shapes! {
    0, "intrinsics::exec_root::a0_rf", a0_rf, "intrinsics::exec_root::a0_rv", a0_rv, (), ();
    1, "intrinsics::exec_root::a1_rf", a1_rf, "intrinsics::exec_root::a1_rv", a1_rv, (p0), (0);
    2, "intrinsics::exec_root::a2_rf", a2_rf, "intrinsics::exec_root::a2_rv", a2_rv,
    (p0, p1), (0, 1);
    3, "intrinsics::exec_root::a3_rf", a3_rf, "intrinsics::exec_root::a3_rv", a3_rv,
    (p0, p1, p2), (0, 1, 2);
    4, "intrinsics::exec_root::a4_rf", a4_rf, "intrinsics::exec_root::a4_rv", a4_rv,
    (p0, p1, p2, p3), (0, 1, 2, 3);
    5, "intrinsics::exec_root::a5_rf", a5_rf, "intrinsics::exec_root::a5_rv", a5_rv,
    (p0, p1, p2, p3, p4), (0, 1, 2, 3, 4);
    6, "intrinsics::exec_root::a6_rf", a6_rf, "intrinsics::exec_root::a6_rv", a6_rv,
    (p0, p1, p2, p3, p4, p5), (0, 1, 2, 3, 4, 5);
    7, "intrinsics::exec_root::a7_rf", a7_rf, "intrinsics::exec_root::a7_rv", a7_rv,
    (p0, p1, p2, p3, p4, p5, p6), (0, 1, 2, 3, 4, 5, 6);
    8, "intrinsics::exec_root::a8_rf", a8_rf, "intrinsics::exec_root::a8_rv", a8_rv,
    (p0, p1, p2, p3, p4, p5, p6, p7), (0, 1, 2, 3, 4, 5, 6, 7);
    9, "intrinsics::exec_root::a9_rf", a9_rf, "intrinsics::exec_root::a9_rv", a9_rv,
    (p0, p1, p2, p3, p4, p5, p6, p7, p8), (0, 1, 2, 3, 4, 5, 6, 7, 8);
    10, "intrinsics::exec_root::a10_rf", a10_rf, "intrinsics::exec_root::a10_rv", a10_rv,
    (p0, p1, p2, p3, p4, p5, p6, p7, p8, p9), (0, 1, 2, 3, 4, 5, 6, 7, 8, 9);
    11, "intrinsics::exec_root::a11_rf", a11_rf, "intrinsics::exec_root::a11_rv", a11_rv,
    (p0, p1, p2, p3, p4, p5, p6, p7, p8, p9, p10), (0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10);
    12, "intrinsics::exec_root::a12_rf", a12_rf, "intrinsics::exec_root::a12_rv", a12_rv,
    (p0, p1, p2, p3, p4, p5, p6, p7, p8, p9, p10, p11),
    (0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11);
}

/// Declares one call trait for each number of arguments.
///
/// Each entry supplies the name of the trait, and one argument name with its type parameter for
/// each argument of the call.
macro_rules! call_traits {
    ($($trait_name:ident, ($($arg:ident: $ty:ident),*);)*) => {
        $(
            /// Calls a stored procedure through its MAST root.
            ///
            /// The signature of the [`StoredProcedure`] selects the trait, so a call with other
            /// argument types, or with another number of arguments, does not compile.
            pub trait $trait_name<$($ty,)* R> {
                /// Calls the procedure in the execution context of the caller.
                ///
                /// The transaction fails if the root is zero, and if the procedure does not
                /// belong to the interface of this account.
                // The number of arguments is the point of the trait.
                #[allow(clippy::too_many_arguments)]
                fn call(&self, $($arg: $ty),*) -> R;
            }

            impl<$($ty: StoredArg,)* R: StoredRet> $trait_name<$($ty,)* R>
                for StoredProcedure<fn($($ty),*) -> R>
            {
                #[inline(always)]
                // A call without an argument compares `0 <= 12`, which is always true.
                #[allow(unused_comparisons)]
                // The number of arguments is the point of the trait.
                #[allow(clippy::too_many_arguments)]
                fn call(&self, $($arg: $ty),*) -> R {
                    const {
                        assert!(
                            0 $(+ <$ty as StoredArg>::WIDTH)* <= MAX_ARG_FELTS,
                            "the arguments of this stored-procedure call need more than 12 field \
                             elements on the operand stack; the 16-element operand window must \
                             also hold the 4-element procedure root"
                        )
                    };

                    let args = ArgBuffer::new()$(.push($arg))*;
                    R::dispatch(
                        Word::from(self.root),
                        &args,
                        const { 0 $(+ <$ty as StoredArg>::WIDTH)* },
                    )
                }
            }
        )*
    };
}

call_traits! {
    Call0, ();
    Call1, (a1: A1);
    Call2, (a1: A1, a2: A2);
    Call3, (a1: A1, a2: A2, a3: A3);
    Call4, (a1: A1, a2: A2, a3: A3, a4: A4);
    Call5, (a1: A1, a2: A2, a3: A3, a4: A4, a5: A5);
    Call6, (a1: A1, a2: A2, a3: A3, a4: A4, a5: A5, a6: A6);
    Call7, (a1: A1, a2: A2, a3: A3, a4: A4, a5: A5, a6: A6, a7: A7);
    Call8, (a1: A1, a2: A2, a3: A3, a4: A4, a5: A5, a6: A6, a7: A7, a8: A8);
    Call9, (a1: A1, a2: A2, a3: A3, a4: A4, a5: A5, a6: A6, a7: A7, a8: A8, a9: A9);
    Call10, (a1: A1, a2: A2, a3: A3, a4: A4, a5: A5, a6: A6, a7: A7, a8: A8, a9: A9, a10: A10);
    Call11, (a1: A1, a2: A2, a3: A3, a4: A4, a5: A5, a6: A6, a7: A7, a8: A8, a9: A9, a10: A10,
             a11: A11);
    Call12, (a1: A1, a2: A2, a3: A3, a4: A4, a5: A5, a6: A6, a7: A7, a8: A8, a9: A9, a10: A10,
             a11: A11, a12: A12);
}
