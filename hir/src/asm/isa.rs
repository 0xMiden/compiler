use std::{collections::BTreeSet, fmt};

use cranelift_entity::entity_impl;
pub use miden_assembly::ast::DebugOptions;
use miden_assembly::ast::SystemEventNode;
use smallvec::{smallvec, SmallVec};

use crate::{
    diagnostics::{SourceSpan, Span},
    Felt, FunctionIdent, Ident, LocalId,
};

/// A handle that refers to a MASM code block
#[derive(Default, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MasmBlockId(u32);
entity_impl!(MasmBlockId, "blk");

/// Represents a single code block in Miden Assembly
#[derive(Debug, Clone, PartialEq)]
pub struct MasmBlock {
    pub id: MasmBlockId,
    pub ops: SmallVec<[Span<MasmOp>; 4]>,
}
impl MasmBlock {
    /// Returns true if there are no instructions in this block
    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }

    /// Returns the instructions contained in this block as a slice
    #[inline(always)]
    pub fn ops(&self) -> &[Span<MasmOp>] {
        self.ops.as_slice()
    }

    /// Appends `op` to this code block
    #[inline(always)]
    pub fn push(&mut self, op: MasmOp, span: SourceSpan) {
        self.ops.push(Span::new(span, op));
    }

    /// Append `n` copies of `op` to the current block
    #[inline]
    pub fn push_n(&mut self, count: usize, op: MasmOp, span: SourceSpan) {
        let op = Span::new(span, op);
        for _ in 0..count {
            self.ops.push(op);
        }
    }

    /// Append `n` copies of the sequence `ops` to this block
    #[inline]
    pub fn push_repeat(&mut self, ops: &[Span<MasmOp>], count: usize) {
        for _ in 0..count {
            self.ops.extend_from_slice(ops);
        }
    }

    /// Append `n` copies of the sequence `ops` to this block
    #[inline]
    pub fn push_template<const N: usize, F>(&mut self, count: usize, template: F)
    where
        F: Fn(usize) -> [Span<MasmOp>; N],
    {
        for n in 0..count {
            self.ops.extend_from_slice(&template(n));
        }
    }

    /// Appends instructions from `slice` to the end of this block
    #[inline]
    pub fn extend_from_slice(&mut self, slice: &[Span<MasmOp>]) {
        self.ops.extend_from_slice(slice);
    }

    /// Appends instructions from `slice` to the end of this block
    #[inline]
    pub fn extend(&mut self, ops: impl IntoIterator<Item = Span<MasmOp>>) {
        self.ops.extend(ops);
    }

    /// Appends instructions from `other` to the end of this block
    #[inline]
    pub fn append<B>(&mut self, other: &mut SmallVec<B>)
    where
        B: smallvec::Array<Item = Span<MasmOp>>,
    {
        self.ops.append(other);
    }
}

/// This enum represents the Miden Assembly (MASM) instruction set.
///
/// Not all MASM instructions are necessarily represented here, only those we
/// actually use, or intend to use, when compiling from Miden IR.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum MasmOp {
    /// Pushes a null word on the stack, i.e. four 0 values
    Padw,
    /// Pushes the given field element constant on top of the stack
    Push(Felt),
    /// Pushes a pair of field elements on top of the stack
    Push2([Felt; 2]),
    /// Pushes the given word constant on top of the stack
    Pushw([Felt; 4]),
    /// Pushes the given 8-bit constant on top of the stack
    PushU8(u8),
    /// Pushes the given 16-bit constant on top of the stack
    PushU16(u16),
    /// Pushes the given 32-bit constant on top of the stack
    PushU32(u32),
    /// Removes the item on the top of the stack
    Drop,
    /// Removes the top 4 items on the stack
    Dropw,
    /// Copies the `n`th item on the stack to the top of stack
    ///
    /// * `Dup(0)` duplicates the item on top of the stack
    Dup(u8),
    /// Copies the `n`th word on the stack, to the top of the stack
    ///
    /// The only values of `n` which are valid, are 0, 1, 2, 3; or
    /// in other words, the 4 words which make up the top 16 elements
    /// of the stack.
    Dupw(u8),
    /// Swaps the 1st and `n`th items on the stack
    ///
    /// * `Swap(1)` swaps the top two elements of the stack
    Swap(u8),
    /// Swaps the 1st and `n`th words on the stack
    ///
    /// The only values of `n` which are valid, are 1, 2, 3; or
    /// in other words, the 3 words which make up the last 12 elements
    /// of the stack.
    Swapw(u8),
    // Swaps the two words on top of the stack, with the two words at the bottom of the stack
    Swapdw,
    /// Moves the `n`th stack item to top of stack
    ///
    /// * `Movup(1)` is equivalent to `Swap(1)`
    Movup(u8),
    /// Moves the `n`th stack word to the top of the stack
    ///
    /// The only values of `n` which are valid are 2 and 3. Use `Swapw(1)`
    /// if you want to move the second word to the top.
    Movupw(u8),
    /// Moves the top of stack to the `n`th index of the stack
    ///
    /// * `Movdn(1)` is equivalent to `Swap(1)`
    Movdn(u8),
    /// Moves the top word of the stack, into position as the `n`th word on the stack.
    ///
    /// The only values of `n` which are valid are 2 and 3. Use `Swapw(1)`
    /// if you want to make the top word the second word.
    Movdnw(u8),
    /// Pops `c, b, a` off the stack, and swaps `b` and `a` if `c` is 1, or leaves
    /// them as-is when 0.
    ///
    /// Traps if `c` is > 1.
    Cswap,
    /// Pops `c, B, A` off the stack, where `B` and `A` are words, and swaps `B` and `A`
    /// if `c` is 1, or leaves them as-is when 0.
    ///
    /// Traps if `c` is > 1.
    Cswapw,
    /// Pops `c, b, a` off the stack, and pushes back `b` if `c` is 1, and `a` if 0.
    ///
    /// Traps if `c` is > 1.
    Cdrop,
    /// Pops `c, B, A` off the stack, where `B` and `A` are words, and pushes back `B`
    /// if `c` is 1, and `A` if 0.
    ///
    /// Traps if `c` is > 1.
    Cdropw,
    /// Pops a value off the stack and asserts that it is equal to 1
    Assert,
    /// Pops a value off the stack and asserts that it is equal to 1, raising the given error code
    AssertWithError(u32),
    /// Pops a value off the stack and asserts that it is equal to 0
    Assertz,
    /// Pops a value off the stack and asserts that it is equal to 0, raising the given error code
    AssertzWithError(u32),
    /// Pops two values off the stack and asserts that they are equal
    AssertEq,
    /// Pops two values off the stack and asserts that they are equal, raising the given error code
    AssertEqWithError(u32),
    /// Pops two words off the stack and asserts that they are equal
    AssertEqw,
    /// Pops two words off the stack and asserts that they are equal, raising the given error code
    AssertEqwWithError(u32),
    /// Places the memory address of the given local index on top of the stack
    LocAddr(LocalId),
    /// Writes a value to the first element of the word at the address corresponding to the given
    /// local index
    LocStore(LocalId),
    /// Writes a word to the address corresponding to the given local index
    LocStorew(LocalId),
    /// Reads a value from the first element of the word at the address corresponding to the given
    /// local index
    LocLoad(LocalId),
    /// Reads a word from the address corresponding to the given local index
    LocLoadw(LocalId),
    /// Pops `a`, representing a memory address, from the top of the stack, then loads the
    /// first element of the word starting at that address, placing it on top of the stack.
    ///
    /// Traps if `a` >= 2^32
    MemLoad,
    /// Same as above, but the address is given as an immediate
    MemLoadImm(u32),
    /// Pops `a`, representing a memory address, from the top of the stack, then overwrites
    /// the top word of the stack with the word starting at that address.
    ///
    /// Traps if `a` >= 2^32
    MemLoadw,
    /// Same as above, but the address is given as an immediate
    MemLoadwImm(u32),
    /// Pops `a, v` from the stack, where `a` represents a memory address, and `v` the value
    /// to be stored, and stores `v` as the element as the first element of the word starting
    /// at that address. The remaining elements of the word are not modified.
    ///
    /// Traps if `a` >= 2^32
    MemStore,
    /// Same as above, but the address is given as an immediate
    MemStoreImm(u32),
    /// Pops `a, V` from the stack, where `a` represents a memory address, and `V` is a word to be
    /// stored at that location, and overwrites the word located at `a`.
    ///
    /// Traps if `a` >= 2^32
    MemStorew,
    /// Same as above, but the address is given as an immediate
    MemStorewImm(u32),
    /// Read two sequential words from memory starting at `a`, overwriting the first two words on
    /// the stack, and advancing `a` to the next address following the two that were loaded
    /// [C, B, A, a] <- [*a, *(a + 1), A, a + 2]
    MemStream,
    /// Pops the next two words from the advice stack, overwrites the
    /// top of the operand stack with them, and also writes these words
    /// into memory at `a` and `a + 1`
    ///
    /// [C, B, A, a] <- [*a, *(a + 1), A, a + 2]
    AdvPipe,
    /// Pops `n` elements from the advice stack, and pushes them on the operand stack
    ///
    /// Fails if less than `n` elements are available.
    ///
    /// Valid values of `n` fall in the range 1..=16
    AdvPush(u8),
    /// Pop the next word from the advice stack and overwrite the word on top of the operand stack
    /// with it.
    ///
    /// Fails if the advice stack does not have at least one word.
    AdvLoadw,
    /// Push the result of u64 division on the advice stack
    ///
    /// ```text,ignore
    /// [b_hi, b_lo, a_hi, a_lo]
    /// ```
    AdvInjectPushU64Div,
    /// Pushes a list of field elements on the advice stack.
    ///
    /// The list is looked up in the advice map using the word on top of the operand stack.
    ///
    /// ```text,ignore
    /// [K]
    /// ```
    AdvInjectPushMapVal,
    /// Pushes a list of field elements, along with the number of elements on the advice stack.
    ///
    /// The list is looked up in the advice map using the word on top of the operand stack.
    ///
    /// ```text,ignore
    /// [K]
    /// ```
    AdvInjectPushMapValN,
    /// Pushes a node of a Merkle tree with root `R` at depth `d` and index `i` from the Merkle
    /// store onto the advice stack
    ///
    /// ```text,ignore
    /// [d, i, R]
    /// ```
    AdvInjectPushMTreeNode,
    /// Reads words `mem[a]..mem[b]` from memory, and saves the data into the advice map under `K`
    ///
    /// ```text,ignore
    /// [K, a, b]
    /// ```
    AdvInjectInsertMem,
    /// Reads the top two words from the stack, and computes a key `K` as `hash(A || B, 0)`.
    ///
    /// The two words that were hashed are then saved into the advice map under `K`.
    ///
    /// ```text,ignore
    /// [B, A]
    /// ```
    AdvInjectInsertHdword,
    /// Reads the top three words from the stack, and computes a key `K` as `permute(C, A,
    /// B).digest`.
    ///
    /// The words `A` and `B` are saved into the advice map under `K` as `[A, B]`
    ///
    /// ```text,ignore
    /// [B, A, C]
    /// ```
    AdvInjectInsertHperm,
    /// TODO
    AdvInjectPushSignature(miden_assembly::ast::SignatureKind),
    /// Compute the Rescue Prime Optimized (RPO) hash of the word on top of the operand stack.
    ///
    /// The resulting hash of one word is placed on the operand stack.
    ///
    /// The input operand is consumed.
    Hash,
    /// Computes a 2-to-1 RPO hash of the two words on top of the operand stack.
    ///
    /// The resulting hash of one word is placed on the operand stack.
    ///
    /// The input operands are consumed.
    Hmerge,
    /// Compute an RPO permutation on the top 3 words of the operand stack, where the top 2 words
    /// (C and B) are the rate, and the last word (A) is the capacity.
    ///
    /// The digest output is the word E.
    ///
    /// ```text,ignore
    /// [C, B, A] => [F, E, D]
    /// ```
    Hperm,
    /// Fetches the value `V` of the Merkle tree with root `R`, at depth `d`, and index `i` from
    /// the advice provider, and runs a verification equivalent to `mtree_verify`, returning
    /// the value if successful.
    ///
    /// ```text,ignore
    /// [d, i, R] => [V, R]
    /// ```
    MtreeGet,
    /// Sets the value to `V'` of the Merkle tree with root `R`, at depth `d`, and index `i`.
    ///
    /// `R'` is the Merkle root of the new tree, and `V` is the old value of the node.
    ///
    /// Requires that a Merkle tree with root `R` is present in the advice provider, otherwise it
    /// fails.
    ///
    /// Both trees are in the advice provider upon return.
    ///
    /// ```text,ignore
    /// [d, i, R, V'] => [V, R']
    /// ```
    MtreeSet,
    /// Create a new Merkle tree root `M`, that joins two other Merkle trees, `R` and `L`.
    ///
    /// Both the new tree and the input trees are in the advice provider upon return.
    ///
    /// ```text,ignore
    /// [R, L] => [M]
    /// ```
    MtreeMerge,
    /// Verifies that a Merkle tree with root `R` opens to node `V` at depth `d` and index `i`.
    ///
    /// The Merkle tree with root `R` must be present in the advice provider or the operation
    /// fails.
    ///
    /// ```text,ignore
    /// [V, d, i, R] => [V, d, i, R]
    /// ```
    MtreeVerify,
    /// Verifies that a Merkle tree with root `R` opens to node `V` at depth `d` and index `i`.
    ///
    /// The Merkle tree with root `R` must be present in the advice provider or the operation
    /// fails.
    ///
    /// ```text,ignore
    /// [V, d, i, R] => [V, d, i, R]
    /// ```
    /// Raise the given error code if the verification fails
    MtreeVerifyWithError(u32),
    /// Performs FRI layer folding by a factor of 4 for FRI protocol executed in a degree 2
    /// extension of the base field. Additionally, performs several computations which simplify
    /// FRI verification procedure.
    ///
    /// * Folds 4 query values: `(v0, v1)`, `(v2, v3)`, `(v4, v5)`, and `(v6, v7)` into a single
    ///   value `(ne0, ne1)`
    /// * Computes new value of the domain generator power: `poe' = poe^4`
    /// * Increments layer pointer (`cptr`) by 2
    /// * Shifts the stack left to move an item from the overflow table to bottom of stack
    ///
    /// ```text,ignore
    /// [v7, v6, v5, v4, v3, v2, v1, v0, f_pos, d_seg, poe, pe1, pe0, a1, a0, cptr]
    /// => [t1, t0, s1, s0, df3, df2, df1, df0, poe^2, f_tau, cptr+2, poe^4, f_pos, ne1, ne0, eptr]
    /// ```
    ///
    /// Above, `eptr` is moved from the overflow table and is expected to be the address of the
    /// final FRI layer.
    FriExt2Fold4,
    /// Perform a single step of a random linear combination defining the DEEP composition
    /// polynomial, i.e. the input to the FRI protocol.
    ///
    /// ```text,ignore
    /// [t7, t6, t5, t4, t3, t2, t1, t0, p1, p0, r1, r0, x_addr, z_addr, a_addr]
    /// => [t0, t7, t6, t5, t4, t3, t2, t1, p1', p0', r1', r0', x_addr, z_addr+1, a_addr+1]
    /// ```
    ///
    /// Where:
    ///
    /// * `tN` stands for the value of the `N`th trace polynomial for the current query, i.e.
    ///   `tN(x)`
    /// * `p0` and `p1` stand for an extension field element accumulating the values for the
    ///   quotients with common denominator `x - z`
    /// * `r0` and `r1` stand for an extension field element accumulating the values for the
    ///   quotients with common denominator `x - gz`
    /// * `x_addr` is the memory address from which we are loading the `tN`s using the `mem_stream`
    ///   instruction
    /// * `z_addr` is the memory address to the `N`th OOD evaluations at `z` and `gz`
    /// * `a_addr` is the memory address of the `N`th random element `alpha_i` used in batching the
    ///   trace polynomial quotients
    RCombBase,
    /// [b1, b0, a1, a0] => [c1, c0]
    ///
    /// c1 = (a1 + b1) mod p
    /// c0 = (a0 + b0) mod p
    Ext2add,
    /// [b1, b0, a1, a0] => [c1, c0]
    ///
    /// c1 = (a1 - b1) mod p
    /// c0 = (a0 - b0) mod p
    Ext2sub,
    /// [b1, b0, a1, a0] => [c1, c0]
    ///
    /// c1 = ((a0 + a1) * (b0 + b1)) mod p
    /// c0 = ((a0 * b0) - 2(a1 * b1)) mod p
    Ext2mul,
    /// [a1, a0] => [a1', a0']
    ///
    /// a1' = -a1
    /// a0' = -a0
    Ext2neg,
    /// [a1, a0] => [a1', a0']
    ///
    /// a' = a^-1 mod q (where `q` is the extension field prime)
    ///
    /// Fails if `a` = 0.
    Ext2inv,
    /// [b1, b0, a1, a0] => [c1, c0]
    ///
    /// c = a * b^-1
    ///
    /// Fails if `b` is 0. Multiplication and inversion are defined by the ops above.
    Ext2div,
    /// Pops the top of the stack, and evaluates the ops in
    /// the block of code corresponding to the branch taken.
    ///
    /// If the value is `1`, corresponding to `true`, the first block
    /// is evaluated. Otherwise, the value must be `0`, corresponding to
    /// `false`, and the second block is evaluated.
    If(MasmBlockId, MasmBlockId),
    /// Pops the top of the stack, and evaluates the given block of
    /// code if the value is `1`, corresponding to `true`.
    ///
    /// Otherwise, the value must be `0`, corresponding to `false`,
    /// and the block is skipped.
    While(MasmBlockId),
    /// Repeatedly evaluates the given block, `n` times.
    Repeat(u16, MasmBlockId),
    /// Pops `N` args off the stack, executes the procedure, results will be placed on the stack
    Exec(FunctionIdent),
    /// Pops `N` args off the stack, executes the procedure in the root context, results will be
    /// placed on the stack
    Syscall(FunctionIdent),
    /// Pops `N` args off the stack, executes the procedure in a new context, results will be
    /// placed on the stack
    Call(FunctionIdent),
    /// Pops the address (MAST root hash) of a callee off the stack, and dynamically `exec` the
    /// function
    DynExec,
    /// TODO
    DynCall,
    /// Pushes the address (MAST root hash) of the given function on the stack, to be used by
    /// `dynexec` or `dyncall`
    ProcRef(FunctionIdent),
    /// Pops `b, a` off the stack, and places the result of `(a + b) mod p` on the stack
    Add,
    /// Same as above, but the immediate is used for `b`
    AddImm(Felt),
    /// Pops `b, a` off the stack, and places the result of `(a - b) mod p` on the stack
    Sub,
    /// Same as above, but the immediate is used for `b`
    SubImm(Felt),
    /// Pops `b, a` off the stack, and places the result of `(a * b) mod p` on the stack
    Mul,
    /// Same as above, but the immediate is used for `b`
    MulImm(Felt),
    /// Pops `b, a` off the stack, and places the result of `(a * b^-1) mod p` on the stack
    ///
    /// NOTE: `b` must not be 0
    Div,
    /// Same as above, but the immediate is used for `b`
    DivImm(Felt),
    /// Pops `a` off the stack, and places the result of `-a mod p` on the stack
    Neg,
    /// Pops `a` off the stack, and places the result of `a^-1 mod p` on the stack
    ///
    /// NOTE: `a` must not be equal to 0
    Inv,
    /// Pops `a` off the stack, and places the result of incrementing it by 1 back on the stack
    Incr,
    /// Computes the base 2 logarithm of `a`, rounded down, and places it on the advice stack.
    Ilog2,
    /// Pops `a` off the stack, and places the result of `2^a` on the stack
    ///
    /// NOTE: `a` must not be > 63
    Pow2,
    /// Pops `a` and `b` off the stack, and places the result of `a^b` on the stack
    ///
    /// NOTE: `b` must not be > 63
    Exp,
    /// Pops `a` off the stack, and places the result of `a^<imm>` on the stack
    ///
    /// NOTE: `imm` must not be > 63
    ExpImm(u8),
    ExpBitLength(u8),
    /// Pops `a` off the stack, and places the result of `1 - a` on the stack
    ///
    /// NOTE: `a` must be boolean
    Not,
    /// Pops `b, a` off the stack, and places the result of `a * b` on the stack
    ///
    /// NOTE: `a` must be boolean
    And,
    /// Same as above, but `a` is taken from the stack, and `b` is the immediate.
    ///
    /// NOTE: `a` must be boolean
    AndImm(bool),
    /// Pops `b, a` off the stack, and places the result of `a + b - a * b` on the stack
    ///
    /// NOTE: `a` must be boolean
    Or,
    /// Same as above, but `a` is taken from the stack, and `b` is the immediate.
    ///
    /// NOTE: `a` must be boolean
    OrImm(bool),
    /// Pops `b, a` off the stack, and places the result of `a + b - 2 * a * b` on the stack
    ///
    /// NOTE: `a` and `b` must be boolean
    Xor,
    /// Same as above, but `a` is taken from the stack, and `b` is the immediate.
    ///
    /// NOTE: `a` must be boolean
    XorImm(bool),
    /// Pops `b, a` off the stack, and places the result of `a == b` on the stack
    Eq,
    /// Same as above, but `b` is provided by the immediate
    EqImm(Felt),
    /// Pops `b, a` off the stack, and places the result of `a != b` on the stack
    Neq,
    /// Same as above, but `b` is provided by the immediate
    NeqImm(Felt),
    /// Pops `b, a` off the stack, and places the result of `a > b` on the stack
    Gt,
    /// Same as above, but `b` is provided by the immediate
    GtImm(Felt),
    /// Pops `b, a` off the stack, and places the result of `a >= b` on the stack
    Gte,
    /// Same as above, but `b` is provided by the immediate
    GteImm(Felt),
    /// Pops `b, a` off the stack, and places the result of `a < b` on the stack
    Lt,
    /// Same as above, but `b` is provided by the immediate
    LtImm(Felt),
    /// Pops `b, a` off the stack, and places the result of `a <= b` on the stack
    Lte,
    /// Same as above, but `b` is provided by the immediate
    LteImm(Felt),
    /// Pops `a` off the stack, and places the 1 on the stack if `a` is odd, else 0
    IsOdd,
    /// Pops `B, A` off the stack, and places the result of `A == B` on the stack,
    /// where the uppercase variables here represent words, rather than field elements.
    ///
    /// The comparison works by comparing pairs of elements from each word
    Eqw,
    /// Pushes the current depth of the operand stack, on the stack
    Sdepth,
    /// When the current procedure is called via `syscall`, this pushes the hash of the caller's
    /// MAST root on the stack
    Caller,
    /// Pushes the current value of the cycle counter (clock) on the stack
    Clk,
    /// Peeks `a` from the top of the stack, and places the 1 on the stack if `a < 2^32`, else 0
    U32Test,
    /// Peeks `A` from the top of the stack, and places the 1 on the stack if `forall a : A, a <
    /// 2^32`, else 0
    U32Testw,
    /// Peeks `a` from the top of the stack, and traps if `a >= 2^32`
    U32Assert,
    /// Peeks `a` from the top of the stack, and traps if `a >= 2^32`, raising the given error code
    U32AssertWithError(u32),
    /// Peeks `b, a` from the top of the stack, and traps if either `a` or `b` is >= 2^32
    U32Assert2,
    /// Peeks `b, a` from the top of the stack, and traps if either `a` or `b` is >= 2^32, raising
    /// the given error code
    U32Assert2WithError(u32),
    /// Peeks `A` from the top of the stack, and traps unless `forall a : A, a < 2^32`, else 0
    U32Assertw,
    /// Peeks `A` from the top of the stack, and traps unless `forall a : A, a < 2^32`, else 0,
    /// raising the given error code
    U32AssertwWithError(u32),
    /// Pops `a` from the top of the stack, and places the result of `a mod 2^32` on the stack
    ///
    /// This is used to cast a field element to the u32 range
    U32Cast,
    /// Pops `a` from the top of the stack, and splits it into upper and lower 32-bit values,
    /// placing them back on the stack. The lower part is calculated as `a mod 2^32`,
    /// and the higher part as `a / 2^32`. The higher part will be on top of the stack after.
    U32Split,
    /// Pops `b, a` from the stack, and places the result of `(a + b) mod 2^32` on the stack,
    /// followed by 1 if `(a + b) >= 2^32`, else 0. Thus the first item on the stack will be
    /// a boolean indicating whether the arithmetic overflowed, and the second will be the
    /// result of the addition.
    ///
    /// The behavior is undefined if either `b` or `a` are >= 2^32
    U32OverflowingAdd,
    /// Same as above, but with `b` provided by the immediate
    U32OverflowingAddImm(u32),
    /// Pops `b, a` from the stack, and places the result of `(a + b) mod 2^32` on the stack.
    ///
    /// The behavior is undefined if either `b` or `a` are >= 2^32
    U32WrappingAdd,
    /// Same as above, but with `b` provided by the immediate
    U32WrappingAddImm(u32),
    /// Pops `c, b, a` from the stack, adds them together, and splits the result into higher
    /// and lower parts. The lower part is calculated as `(a + b + c) mod 2^32`,
    /// the higher part as `(a + b + c) / 2^32`.
    ///
    /// The behavior is undefined if any of `c`, `b` or `a` are >= 2^32
    U32OverflowingAdd3,
    /// Pops `c, b, a` from the stack, adds them together, and splits the result into higher
    /// and lower parts. The lower part is calculated as `(a + b + c) mod 2^32`,
    /// the higher part as `(a + b + c) / 2^32`.
    ///
    /// The behavior is undefined if any of `c`, `b` or `a` are >= 2^32
    U32WrappingAdd3,
    /// Pops `b, a` from the stack, and places the result of `(a - b) mod 2^32` on the stack,
    /// followed by 1 if `a < b`, else 0. Thus the first item on the stack will be
    /// a boolean indicating whether the arithmetic underflowed, and the second will be the
    /// result of the subtraction.
    ///
    /// The behavior is undefined if either `b` or `a` are >= 2^32
    U32OverflowingSub,
    /// Same as above, but with `b` provided by the immediate
    U32OverflowingSubImm(u32),
    /// Pops `b, a` from the stack, and places the result of `(a - b) mod 2^32` on the stack.
    ///
    /// The behavior is undefined if either `b` or `a` are >= 2^32
    U32WrappingSub,
    /// Same as above, but with `b` provided by the immediate
    U32WrappingSubImm(u32),
    /// Pops `b, a` from the stack, and places the result of `(a * b) mod 2^32` on the stack,
    /// followed by `(a * b) / 2^32`. Thus the first item on the stack will be the number
    /// of times the multiplication overflowed, followed by the result.
    ///
    /// The behavior is undefined if either `b` or `a` are >= 2^32
    U32OverflowingMul,
    /// Same as above, but with `b` provided by the immediate
    U32OverflowingMulImm(u32),
    /// Pops `b, a` from the stack, and places the result of `(a * b) mod 2^32` on the stack.
    ///
    /// The behavior is undefined if either `b` or `a` are >= 2^32
    U32WrappingMul,
    /// Same as above, but with `b` provided by the immediate
    U32WrappingMulImm(u32),
    /// Pops `c, b, a` off the stack, and calculates `d = c * b + a`, then splits the result
    /// into higher and lower parts, the lower given by `d mod 2^32`, the higher by `d / 2^32`,
    /// and pushes them back on the stack, with the higher part on top of the stack at the end.
    ///
    /// Behavior is undefined if any of `a`, `b`, or `c` are >= 2^32
    U32OverflowingMadd,
    /// Pops `c, b, a` off the stack, and pushes `(c * a + b) mod 2^32` on the stack.
    ///
    /// Behavior is undefined if any of `a`, `b`, or `c` are >= 2^32
    U32WrappingMadd,
    /// Pops `b, a` off the stack, and pushes `a / b` on the stack.
    ///
    /// This operation traps if `b` is zero.
    ///
    /// This operation is unchecked, so the result is undefined if the operands are not valid u32
    U32Div,
    /// Same as above, except `b` is provided by the immediate
    U32DivImm(u32),
    /// Pops `b, a` off the stack, and pushes `a mod b` on the stack.
    ///
    /// This operation traps if `b` is zero.
    ///
    /// This operation is unchecked, so the result is undefined if the operands are not valid u32
    U32Mod,
    /// Same as above, except `b` is provided by the immediate
    U32ModImm(u32),
    /// Pops `b, a` off the stack, and first pushes `a / b` on the stack, followed by `a mod b`.
    ///
    /// This operation traps if `b` is zero.
    ///
    /// This operation is unchecked, so the result is undefined if the operands are not valid u32
    U32DivMod,
    /// Same as above, except `b` is provided by the immediate
    U32DivModImm(u32),
    /// Pops `b, a` off the stack, and places the bitwise AND of `a` and `b` on the stack.
    ///
    /// Traps if either `a` or `b` >= 2^32
    U32And,
    /// Pops `b, a` off the stack, and places the bitwise OR of `a` and `b` on the stack.
    ///
    /// Traps if either `a` or `b` >= 2^32
    U32Or,
    /// Pops `b, a` off the stack, and places the bitwise XOR of `a` and `b` on the stack.
    ///
    /// Traps if either `a` or `b` >= 2^32
    U32Xor,
    /// Pops `a` off the stack, and places the bitwise NOT of `a` on the stack.
    ///
    /// Traps if `a >= 2^32`
    U32Not,
    /// Pops `b, a` off the stack, and places the result of `(a * 2^b) mod 2^32` on the stack.
    ///
    /// Truncates if the shift would cause overflow.
    ///
    /// This operation is unchecked, so the result is undefined if the operands are not valid u32
    U32Shl,
    /// Same as above, except `b` is provided by the immediate
    U32ShlImm(u32),
    /// Pops `b, a` off the stack, and places the result of `a / 2^b` on the stack.
    ///
    /// Truncates if the shift would cause overflow.
    ///
    /// This operation is unchecked, so the result is undefined if the operands are not valid u32
    U32Shr,
    /// Same as above, except `b` is provided by the immediate
    U32ShrImm(u32),
    /// Pops `b, a` off the stack, and places the result of rotating the 32-bit
    /// representation of `a` to the left by `b` bits.
    ///
    /// This operation is unchecked, so the result is undefined if the operands are not valid u32
    U32Rotl,
    /// Same as above, except `b` is provided by the immediate
    U32RotlImm(u32),
    /// Pops `b, a` off the stack, and places the result of rotating the 32-bit
    /// representation of `a` to the right by `b` bits.
    ///
    /// This operation is unchecked, so the result is undefined if the operands are not valid u32
    U32Rotr,
    /// Same as above, except `b` is provided by the immediate
    U32RotrImm(u32),
    /// Pops `a` off the stack, and places the number of set bits in `a` (it's hamming weight).
    ///
    /// This operation is unchecked, so the result is undefined if the operands are not valid u32
    U32Popcnt,
    /// Computes the number of leading zero bits in `a`, and places it on the advice stack
    ///
    /// This operation is unchecked, so the result is undefined if the operands are not valid u32
    U32Clz,
    /// Computes the number of trailing zero bits in `a`, and places it on the advice stack
    ///
    /// This operation is unchecked, so the result is undefined if the operands are not valid u32
    U32Ctz,
    /// Computes the number of leading one bits in `a`, and places it on the advice stack
    ///
    /// This operation is unchecked, so the result is undefined if the operands are not valid u32
    U32Clo,
    /// Computes the number of trailing one bits in `a`, and places it on the advice stack
    ///
    /// This operation is unchecked, so the result is undefined if the operands are not valid u32
    U32Cto,
    /// Pops `b, a` from the stack, and places 1 on the stack if `a < b`, else 0
    ///
    /// This operation is unchecked, so the result is undefined if the operands are not valid u32
    U32Lt,
    /// Same as above, but `b` is provided by the immediate
    U32LtImm(u32),
    /// Pops `b, a` from the stack, and places 1 on the stack if `a <= b`, else 0
    ///
    /// This operation is unchecked, so the result is undefined if the operands are not valid u32
    U32Lte,
    /// Same as above, but `b` is provided by the immediate
    U32LteImm(u32),
    /// Pops `b, a` from the stack, and places 1 on the stack if `a > b`, else 0
    ///
    /// This operation is unchecked, so the result is undefined if the operands are not valid u32
    U32Gt,
    /// Same as above, but `b` is provided by the immediate
    U32GtImm(u32),
    /// Pops `b, a` from the stack, and places 1 on the stack if `a >= b`, else 0
    ///
    /// This operation is unchecked, so the result is undefined if the operands are not valid u32
    U32Gte,
    /// Same as above, but `b` is provided by the immediate
    U32GteImm(u32),
    /// Pops `b, a` from the stack, and places `a` back on the stack if `a < b`, else `b`
    ///
    /// This operation is unchecked, so the result is undefined if the operands are not valid u32
    U32Min,
    /// Same as above, but `b` is provided by the immediate
    U32MinImm(u32),
    /// Pops `b, a` from the stack, and places `a` back on the stack if `a > b`, else `b`
    ///
    /// This operation is unchecked, so the result is undefined if the operands are not valid u32
    U32Max,
    /// Same as above, but `b` is provided by the immediate
    U32MaxImm(u32),
    /// Trigger a breakpoint when this instruction is reached
    Breakpoint,
    /// Print out the contents of the stack
    DebugStack,
    /// Print out the top `n` contents of the stack
    DebugStackN(u8),
    /// Print out the entire contents of RAM
    DebugMemory,
    /// Print out the contents of RAM starting at address `n`
    DebugMemoryAt(u32),
    /// Print out the contents of RAM in the range `n..=m`
    DebugMemoryRange(u32, u32),
    /// Print out the local memory for the current procedure
    DebugFrame,
    /// Print out the local memory for the current procedure starting at index `n`
    DebugFrameAt(u16),
    /// Print out the local memory for the current procedure for indices in the range `n..=m`
    DebugFrameRange(u16, u16),
    /// Emit an event with the given event code
    Emit(u32),
    /// Emit a trace event with the given code
    Trace(u32),
    /// No operation
    Nop,
}

macro_rules! unwrap_imm {
    ($imm:ident) => {{
        match $imm {
            miden_assembly::ast::Immediate::Value(imm) => imm.into_inner(),
            miden_assembly::ast::Immediate::Constant(id) => {
                panic!("invalid reference to constant definition: '{id}'")
            }
        }
    }};
}

macro_rules! unwrap_u32 {
    ($imm:ident) => {{
        match $imm {
            miden_assembly::ast::Immediate::Value(imm) => imm.into_inner(),
            miden_assembly::ast::Immediate::Constant(id) => {
                panic!("invalid reference to constant definition: '{id}'")
            }
        }
    }};
}

macro_rules! unwrap_u16 {
    ($imm:ident) => {{
        match $imm {
            miden_assembly::ast::Immediate::Value(imm) => imm.into_inner(),
            miden_assembly::ast::Immediate::Constant(id) => {
                panic!("invalid reference to constant definition: '{id}'")
            }
        }
    }};
}

macro_rules! unwrap_u8 {
    ($imm:ident) => {{
        match $imm {
            miden_assembly::ast::Immediate::Value(imm) => imm.into_inner(),
            miden_assembly::ast::Immediate::Constant(id) => {
                panic!("invalid reference to constant definition: '{id}'")
            }
        }
    }};
}

impl MasmOp {
    pub fn has_regions(&self) -> bool {
        matches!(self, Self::If(_, _) | Self::While(_) | Self::Repeat(_, _))
    }

    /// The cost of this instruction in cycles
    pub fn cost(&self) -> usize {
        match self {
            Self::Padw => 4,
            Self::Push(_) | Self::PushU8(_) | Self::PushU16(_) | Self::PushU32(_) => 1,
            Self::Push2(_) => 2,
            Self::Pushw(_) => 4,
            Self::Drop => 1,
            Self::Dropw => 4,
            Self::Dup(8) | Self::Dup(10) | Self::Dup(12) | Self::Dup(14) => 3,
            Self::Dup(_) => 1,
            Self::Dupw(_) => 4,
            Self::Swap(1) => 1,
            Self::Swap(2..=8) => 2,
            Self::Swap(_) => 6,
            Self::Swapw(_) | Self::Swapdw => 1,
            Self::Movup(2..=8) => 1,
            Self::Movup(_) => 4,
            Self::Movupw(2) => 2,
            Self::Movupw(_) => 3,
            Self::Movdn(2..=8) => 1,
            Self::Movdn(_) => 4,
            Self::Movdnw(2) => 2,
            Self::Movdnw(_) => 3,
            Self::Cswap => 1,
            Self::Cswapw => 1,
            Self::Cdrop => 2,
            Self::Cdropw => 5,
            Self::Assert | Self::AssertWithError(_) => 1,
            Self::Assertz | Self::AssertzWithError(_) => 2,
            Self::AssertEq | Self::AssertEqWithError(_) => 2,
            Self::AssertEqw | Self::AssertEqwWithError(_) => 11,
            Self::LocAddr(_) => 2,
            Self::LocStore(id) if id.as_usize() == 1 => 5,
            Self::LocStore(_) => 4,
            Self::LocStorew(id) if id.as_usize() == 1 => 4,
            Self::LocStorew(_) => 3,
            Self::LocLoad(id) | Self::LocLoadw(id) if id.as_usize() == 1 => 4,
            Self::LocLoad(_) | Self::LocLoadw(_) => 3,
            Self::MemLoad | Self::MemLoadw => 1,
            Self::MemLoadImm(_) | Self::MemLoadwImm(_) => 2,
            Self::MemStore => 2,
            Self::MemStoreImm(1) => 4,
            Self::MemStoreImm(_) => 3,
            Self::MemStorew => 1,
            Self::MemStorewImm(1) => 3,
            Self::MemStorewImm(_) => 2,
            Self::MemStream => 1,
            Self::AdvPipe => 1,
            Self::AdvPush(n) => *n as usize,
            Self::AdvLoadw => 1,
            // This is based on cycle counts gathered from a simple program that compares a
            // cdrop-based conditional select to an if-based one, where the only
            // difference is the `cdrop` and `if` instructions. The `cdrop` solution
            // was 39 cycles, the `if` solution was 49, with `cdrop` taking 2 cycles,
            // this gives us a difference of 10 cycles, hence 12 for our cost.
            Self::If(..) => 12,
            // The cost for `while` appears to be the same as `if`, however comparisons are tricky
            // as we can only really compare to `repeat`, which has no apparent cost
            Self::While(_) => 12,
            // Comparing a small program with `repeat.1` vs without the `repeat.1` (simply using the
            // body of the `repeat` instead), there is no apparent cycle cost. We give
            // it a cost of 0 to reflect that using `repeat` is no different than
            // copying its body `N` times.
            Self::Repeat(..) => 0,
            Self::ProcRef(_) => 4,
            Self::Exec(_) => 2,
            // A `call` appears to have the same overhead as `if` and `while`
            Self::Call(_) | Self::Syscall(_) => 12,
            // A `dynexec` appears to be 8 cycles, based on comparisons against `exec`, with an
            // extra `dropw` in the callee that we deduct from the cycle count
            Self::DynExec => 8,
            // A `dyncall` requires an additional 8 cycles compared to `dynexec`
            Self::DynCall => 16,
            Self::Add | Self::Sub | Self::Mul => 1,
            Self::AddImm(imm) => match imm.as_int() {
                0 => 0,
                1 => 1,
                _ => 2,
            },
            Self::SubImm(imm) | Self::MulImm(imm) => match imm.as_int() {
                0 => 0,
                _ => 2,
            },
            Self::Div => 2,
            Self::DivImm(imm) => match imm.as_int() {
                1 => 0,
                _ => 2,
            },
            Self::Neg | Self::Inv | Self::Incr => 1,
            Self::Ilog2 => 44,
            Self::Pow2 => 16,
            // The cost of this instruction is 9 + log2(b), but we don't know `b`, so we use a value
            // of 32 to estimate average cost
            Self::Exp => 9 + 32usize.ilog2() as usize,
            Self::ExpImm(0) => 3,
            Self::ExpImm(1) => 1,
            Self::ExpImm(2) => 2,
            Self::ExpImm(3) => 4,
            Self::ExpImm(4) => 6,
            Self::ExpImm(5) => 8,
            Self::ExpImm(6) => 10,
            Self::ExpImm(7) => 12,
            Self::ExpImm(imm) | Self::ExpBitLength(imm) => {
                9 + unsafe { f64::from(*imm).log2().ceil().to_int_unchecked::<usize>() }
            }
            Self::Not | Self::And | Self::Or => 1,
            Self::AndImm(_) | Self::OrImm(_) => 2,
            Self::Xor => 7,
            Self::XorImm(_) => 8,
            Self::Eq => 1,
            Self::EqImm(imm) => match imm.as_int() {
                0 => 1,
                _ => 2,
            },
            Self::Neq => 2,
            Self::NeqImm(imm) => match imm.as_int() {
                0 => 1,
                _ => 3,
            },
            Self::Gt => 15,
            Self::GtImm(_) => 16,
            Self::Gte => 16,
            Self::GteImm(_) => 17,
            Self::Lt => 14,
            Self::LtImm(_) => 15,
            Self::Lte => 15,
            Self::LteImm(_) => 16,
            Self::IsOdd => 5,
            Self::Eqw => 15,
            Self::Hash => 20,
            Self::Hmerge => 16,
            Self::Hperm => 1,
            Self::MtreeGet => 9,
            Self::MtreeSet => 29,
            Self::MtreeMerge => 16,
            Self::MtreeVerify | Self::MtreeVerifyWithError(_) => 1,
            // This hasn't been measured, just a random guess due to the complexity
            Self::FriExt2Fold4 | Self::RCombBase => 50,
            Self::Ext2add => 5,
            Self::Ext2sub => 7,
            Self::Ext2mul => 3,
            Self::Ext2neg => 4,
            Self::Ext2inv => 8,
            Self::Ext2div => 11,
            Self::Clk | Self::Caller | Self::Sdepth => 1,
            Self::U32Test => 5,
            Self::U32Testw => 23,
            Self::U32Assert | Self::U32AssertWithError(_) => 3,
            Self::U32Assert2 | Self::U32Assert2WithError(_) => 1,
            Self::U32Assertw | Self::U32AssertwWithError(_) => 6,
            Self::U32Cast => 2,
            Self::U32Split => 1,
            Self::U32OverflowingAdd => 1,
            Self::U32OverflowingAddImm(_) => 2,
            Self::U32WrappingAdd => 2,
            Self::U32WrappingAddImm(_) => 3,
            Self::U32OverflowingAdd3 => 1,
            Self::U32WrappingAdd3 => 2,
            Self::U32OverflowingSub => 1,
            Self::U32OverflowingSubImm(_) => 2,
            Self::U32WrappingSub => 2,
            Self::U32WrappingSubImm(_) => 3,
            Self::U32OverflowingMul => 1,
            Self::U32OverflowingMulImm(_) => 2,
            Self::U32WrappingMul => 2,
            Self::U32WrappingMulImm(_) => 3,
            Self::U32OverflowingMadd => 1,
            Self::U32WrappingMadd => 2,
            Self::U32Div => 2,
            Self::U32DivImm(_) => 3,
            Self::U32Mod => 3,
            Self::U32ModImm(_) => 4,
            Self::U32DivMod => 1,
            Self::U32DivModImm(_) => 2,
            Self::U32And => 1,
            Self::U32Or => 6,
            Self::U32Xor => 1,
            Self::U32Not => 5,
            Self::U32Shl => 18,
            Self::U32ShlImm(0) => 0,
            Self::U32ShlImm(_) => 3,
            Self::U32Shr => 18,
            Self::U32ShrImm(0) => 0,
            Self::U32ShrImm(_) => 3,
            Self::U32Rotl => 18,
            Self::U32RotlImm(0) => 0,
            Self::U32RotlImm(_) => 3,
            Self::U32Rotr => 22,
            Self::U32RotrImm(0) => 0,
            Self::U32RotrImm(_) => 3,
            Self::U32Popcnt => 33,
            Self::U32Clz => 37,
            Self::U32Ctz => 34,
            Self::U32Clo => 36,
            Self::U32Cto => 33,
            Self::U32Lt => 3,
            Self::U32LtImm(_) => 4,
            Self::U32Lte => 5,
            Self::U32LteImm(_) => 6,
            Self::U32Gt => 4,
            Self::U32GtImm(_) => 5,
            Self::U32Gte => 4,
            Self::U32GteImm(_) => 5,
            Self::U32Min => 8,
            Self::U32MinImm(_) => 9,
            Self::U32Max => 9,
            Self::U32MaxImm(_) => 10,
            // These instructions do not modify the VM state, so we place set their cost at 0 for
            // now
            Self::Emit(_)
            | Self::Trace(_)
            | Self::AdvInjectPushU64Div
            | Self::AdvInjectPushMapVal
            | Self::AdvInjectPushMapValN
            | Self::AdvInjectPushMTreeNode
            | Self::AdvInjectInsertMem
            | Self::AdvInjectInsertHdword
            | Self::AdvInjectInsertHperm
            | Self::AdvInjectPushSignature(_)
            | Self::DebugStack
            | Self::DebugStackN(_)
            | Self::DebugMemory
            | Self::DebugMemoryAt(_)
            | Self::DebugMemoryRange(..)
            | Self::DebugFrame
            | Self::DebugFrameAt(_)
            | Self::DebugFrameRange(..)
            | Self::Breakpoint
            | Self::Nop => 0,
        }
    }

    pub fn from_masm(
        current_module: Ident,
        ix: miden_assembly::ast::Instruction,
    ) -> SmallVec<[Self; 2]> {
        use miden_assembly::ast::{Instruction, InvocationTarget};

        use crate::Symbol;

        let op = match ix {
            Instruction::Assert => Self::Assert,
            Instruction::AssertWithError(code) => Self::AssertWithError(unwrap_u32!(code)),
            Instruction::AssertEq => Self::AssertEq,
            Instruction::AssertEqWithError(code) => Self::AssertEqWithError(unwrap_u32!(code)),
            Instruction::AssertEqw => Self::AssertEqw,
            Instruction::AssertEqwWithError(code) => Self::AssertEqwWithError(unwrap_u32!(code)),
            Instruction::Assertz => Self::Assertz,
            Instruction::AssertzWithError(code) => Self::AssertzWithError(unwrap_u32!(code)),
            Instruction::Add => Self::Add,
            Instruction::AddImm(imm) => Self::AddImm(unwrap_imm!(imm)),
            Instruction::Sub => Self::Sub,
            Instruction::SubImm(imm) => Self::SubImm(unwrap_imm!(imm)),
            Instruction::Mul => Self::Mul,
            Instruction::MulImm(imm) => Self::MulImm(unwrap_imm!(imm)),
            Instruction::Div => Self::Div,
            Instruction::DivImm(imm) => Self::DivImm(unwrap_imm!(imm)),
            Instruction::Neg => Self::Neg,
            Instruction::Inv => Self::Inv,
            Instruction::Incr => Self::Incr,
            Instruction::ILog2 => Self::Ilog2,
            Instruction::Pow2 => Self::Pow2,
            Instruction::Exp => Self::Exp,
            Instruction::ExpImm(imm) => {
                Self::ExpImm(unwrap_imm!(imm).as_int().try_into().expect("invalid exponent"))
            }
            Instruction::ExpBitLength(imm) => Self::ExpBitLength(imm),
            Instruction::Not => Self::Not,
            Instruction::And => Self::And,
            Instruction::Or => Self::Or,
            Instruction::Xor => Self::Xor,
            Instruction::Eq => Self::Eq,
            Instruction::EqImm(imm) => Self::EqImm(unwrap_imm!(imm)),
            Instruction::Neq => Self::Neq,
            Instruction::NeqImm(imm) => Self::NeqImm(unwrap_imm!(imm)),
            Instruction::Eqw => Self::Eqw,
            Instruction::Lt => Self::Lt,
            Instruction::Lte => Self::Lte,
            Instruction::Gt => Self::Gt,
            Instruction::Gte => Self::Gte,
            Instruction::IsOdd => Self::IsOdd,
            Instruction::Hash => Self::Hash,
            Instruction::HMerge => Self::Hmerge,
            Instruction::HPerm => Self::Hperm,
            Instruction::MTreeGet => Self::MtreeGet,
            Instruction::MTreeSet => Self::MtreeSet,
            Instruction::MTreeMerge => Self::MtreeMerge,
            Instruction::MTreeVerify => Self::MtreeVerify,
            Instruction::MTreeVerifyWithError(code) => {
                Self::MtreeVerifyWithError(unwrap_u32!(code))
            }
            Instruction::Ext2Add => Self::Ext2add,
            Instruction::Ext2Sub => Self::Ext2sub,
            Instruction::Ext2Mul => Self::Ext2mul,
            Instruction::Ext2Div => Self::Ext2div,
            Instruction::Ext2Neg => Self::Ext2neg,
            Instruction::Ext2Inv => Self::Ext2inv,
            Instruction::FriExt2Fold4 => Self::FriExt2Fold4,
            Instruction::RCombBase => Self::RCombBase,
            Instruction::U32Test => Self::U32Test,
            Instruction::U32TestW => Self::U32Testw,
            Instruction::U32Assert => Self::U32Assert,
            Instruction::U32AssertWithError(code) => Self::U32AssertWithError(unwrap_u32!(code)),
            Instruction::U32Assert2 => Self::U32Assert2,
            Instruction::U32Assert2WithError(code) => Self::U32Assert2WithError(unwrap_u32!(code)),
            Instruction::U32AssertW => Self::U32Assertw,
            Instruction::U32AssertWWithError(code) => Self::U32AssertwWithError(unwrap_u32!(code)),
            Instruction::U32Split => Self::U32Split,
            Instruction::U32Cast => Self::U32Cast,
            Instruction::U32WrappingAdd => Self::U32WrappingAdd,
            Instruction::U32WrappingAddImm(imm) => Self::U32WrappingAddImm(unwrap_u32!(imm)),
            Instruction::U32OverflowingAdd => Self::U32OverflowingAdd,
            Instruction::U32OverflowingAddImm(imm) => Self::U32OverflowingAddImm(unwrap_u32!(imm)),
            Instruction::U32OverflowingAdd3 => Self::U32OverflowingAdd3,
            Instruction::U32WrappingAdd3 => Self::U32WrappingAdd3,
            Instruction::U32WrappingSub => Self::U32WrappingSub,
            Instruction::U32WrappingSubImm(imm) => Self::U32WrappingSubImm(unwrap_u32!(imm)),
            Instruction::U32OverflowingSub => Self::U32OverflowingSub,
            Instruction::U32OverflowingSubImm(imm) => Self::U32OverflowingSubImm(unwrap_u32!(imm)),
            Instruction::U32WrappingMul => Self::U32WrappingMul,
            Instruction::U32WrappingMulImm(imm) => Self::U32WrappingMulImm(unwrap_u32!(imm)),
            Instruction::U32OverflowingMul => Self::U32OverflowingMul,
            Instruction::U32OverflowingMulImm(imm) => Self::U32OverflowingMulImm(unwrap_u32!(imm)),
            Instruction::U32OverflowingMadd => Self::U32OverflowingMadd,
            Instruction::U32WrappingMadd => Self::U32WrappingMadd,
            Instruction::U32Div => Self::U32Div,
            Instruction::U32DivImm(imm) => Self::U32DivImm(unwrap_u32!(imm)),
            Instruction::U32Mod => Self::U32Mod,
            Instruction::U32ModImm(imm) => Self::U32ModImm(unwrap_u32!(imm)),
            Instruction::U32DivMod => Self::U32DivMod,
            Instruction::U32DivModImm(imm) => Self::U32DivModImm(unwrap_u32!(imm)),
            Instruction::U32And => Self::U32And,
            Instruction::U32Or => Self::U32Or,
            Instruction::U32Xor => Self::U32Xor,
            Instruction::U32Not => Self::U32Not,
            Instruction::U32Shr => Self::U32Shr,
            Instruction::U32ShrImm(imm) => Self::U32ShrImm(unwrap_u8!(imm) as u32),
            Instruction::U32Shl => Self::U32Shl,
            Instruction::U32ShlImm(imm) => Self::U32ShlImm(unwrap_u8!(imm) as u32),
            Instruction::U32Rotr => Self::U32Rotr,
            Instruction::U32RotrImm(imm) => Self::U32RotrImm(unwrap_u8!(imm) as u32),
            Instruction::U32Rotl => Self::U32Rotl,
            Instruction::U32RotlImm(imm) => Self::U32RotlImm(unwrap_u8!(imm) as u32),
            Instruction::U32Popcnt => Self::U32Popcnt,
            Instruction::U32Clz => Self::U32Clz,
            Instruction::U32Ctz => Self::U32Ctz,
            Instruction::U32Clo => Self::U32Clo,
            Instruction::U32Cto => Self::U32Cto,
            Instruction::U32Lt => Self::U32Lt,
            Instruction::U32Lte => Self::U32Lte,
            Instruction::U32Gt => Self::U32Gt,
            Instruction::U32Gte => Self::U32Gte,
            Instruction::U32Min => Self::U32Min,
            Instruction::U32Max => Self::U32Max,
            Instruction::Drop => Self::Drop,
            Instruction::DropW => Self::Dropw,
            Instruction::PadW => Self::Padw,
            Instruction::Dup0 => Self::Dup(0),
            Instruction::Dup1 => Self::Dup(1),
            Instruction::Dup2 => Self::Dup(2),
            Instruction::Dup3 => Self::Dup(3),
            Instruction::Dup4 => Self::Dup(4),
            Instruction::Dup5 => Self::Dup(5),
            Instruction::Dup6 => Self::Dup(6),
            Instruction::Dup7 => Self::Dup(7),
            Instruction::Dup8 => Self::Dup(8),
            Instruction::Dup9 => Self::Dup(9),
            Instruction::Dup10 => Self::Dup(10),
            Instruction::Dup11 => Self::Dup(11),
            Instruction::Dup12 => Self::Dup(12),
            Instruction::Dup13 => Self::Dup(13),
            Instruction::Dup14 => Self::Dup(14),
            Instruction::Dup15 => Self::Dup(15),
            Instruction::DupW0 => Self::Dupw(0),
            Instruction::DupW1 => Self::Dupw(1),
            Instruction::DupW2 => Self::Dupw(2),
            Instruction::DupW3 => Self::Dupw(3),
            Instruction::Swap1 => Self::Swap(1),
            Instruction::Swap2 => Self::Swap(2),
            Instruction::Swap3 => Self::Swap(3),
            Instruction::Swap4 => Self::Swap(4),
            Instruction::Swap5 => Self::Swap(5),
            Instruction::Swap6 => Self::Swap(6),
            Instruction::Swap7 => Self::Swap(7),
            Instruction::Swap8 => Self::Swap(8),
            Instruction::Swap9 => Self::Swap(9),
            Instruction::Swap10 => Self::Swap(10),
            Instruction::Swap11 => Self::Swap(11),
            Instruction::Swap12 => Self::Swap(12),
            Instruction::Swap13 => Self::Swap(13),
            Instruction::Swap14 => Self::Swap(14),
            Instruction::Swap15 => Self::Swap(15),
            Instruction::SwapW1 => Self::Swapw(1),
            Instruction::SwapW2 => Self::Swapw(2),
            Instruction::SwapW3 => Self::Swapw(3),
            Instruction::SwapDw => Self::Swapdw,
            Instruction::MovUp2 => Self::Movup(2),
            Instruction::MovUp3 => Self::Movup(3),
            Instruction::MovUp4 => Self::Movup(4),
            Instruction::MovUp5 => Self::Movup(5),
            Instruction::MovUp6 => Self::Movup(6),
            Instruction::MovUp7 => Self::Movup(7),
            Instruction::MovUp8 => Self::Movup(8),
            Instruction::MovUp9 => Self::Movup(9),
            Instruction::MovUp10 => Self::Movup(10),
            Instruction::MovUp11 => Self::Movup(11),
            Instruction::MovUp12 => Self::Movup(12),
            Instruction::MovUp13 => Self::Movup(13),
            Instruction::MovUp14 => Self::Movup(14),
            Instruction::MovUp15 => Self::Movup(15),
            Instruction::MovUpW2 => Self::Movupw(2),
            Instruction::MovUpW3 => Self::Movupw(3),
            Instruction::MovDn2 => Self::Movdn(2),
            Instruction::MovDn3 => Self::Movdn(3),
            Instruction::MovDn4 => Self::Movdn(4),
            Instruction::MovDn5 => Self::Movdn(5),
            Instruction::MovDn6 => Self::Movdn(6),
            Instruction::MovDn7 => Self::Movdn(7),
            Instruction::MovDn8 => Self::Movdn(8),
            Instruction::MovDn9 => Self::Movdn(9),
            Instruction::MovDn10 => Self::Movdn(10),
            Instruction::MovDn11 => Self::Movdn(11),
            Instruction::MovDn12 => Self::Movdn(12),
            Instruction::MovDn13 => Self::Movdn(13),
            Instruction::MovDn14 => Self::Movdn(14),
            Instruction::MovDn15 => Self::Movdn(15),
            Instruction::MovDnW2 => Self::Movdnw(2),
            Instruction::MovDnW3 => Self::Movdnw(3),
            Instruction::CSwap => Self::Cswap,
            Instruction::CSwapW => Self::Cswapw,
            Instruction::CDrop => Self::Cdrop,
            Instruction::CDropW => Self::Cdropw,
            Instruction::Push(elem) => Self::Push(unwrap_imm!(elem)),
            Instruction::PushU8(elem) => Self::PushU8(elem),
            Instruction::PushU16(elem) => Self::PushU16(elem),
            Instruction::PushU32(elem) => Self::PushU32(elem),
            Instruction::PushFelt(elem) => Self::Push(elem),
            Instruction::PushWord(word) => Self::Pushw(word),
            Instruction::PushU8List(u8s) => return u8s.into_iter().map(Self::PushU8).collect(),
            Instruction::PushU16List(u16s) => return u16s.into_iter().map(Self::PushU16).collect(),
            Instruction::PushU32List(u32s) => return u32s.into_iter().map(Self::PushU32).collect(),
            Instruction::PushFeltList(felts) => return felts.into_iter().map(Self::Push).collect(),
            Instruction::Locaddr(id) => Self::LocAddr(LocalId::from_u16(unwrap_u16!(id))),
            Instruction::LocStore(id) => Self::LocStore(LocalId::from_u16(unwrap_u16!(id))),
            Instruction::LocStoreW(id) => Self::LocStorew(LocalId::from_u16(unwrap_u16!(id))),
            Instruction::Clk => Self::Clk,
            Instruction::MemLoad => Self::MemLoad,
            Instruction::MemLoadImm(addr) => Self::MemLoadImm(unwrap_u32!(addr)),
            Instruction::MemLoadW => Self::MemLoadw,
            Instruction::MemLoadWImm(addr) => Self::MemLoadwImm(unwrap_u32!(addr)),
            Instruction::MemStore => Self::MemStore,
            Instruction::MemStoreImm(addr) => Self::MemStoreImm(unwrap_u32!(addr)),
            Instruction::MemStoreW => Self::MemStorew,
            Instruction::MemStoreWImm(addr) => Self::MemStorewImm(unwrap_u32!(addr)),
            Instruction::LocLoad(imm) => Self::LocLoad(LocalId::from_u16(unwrap_u16!(imm))),
            Instruction::LocLoadW(imm) => Self::LocLoadw(LocalId::from_u16(unwrap_u16!(imm))),
            Instruction::MemStream => Self::MemStream,
            Instruction::AdvPipe => Self::AdvPipe,
            Instruction::AdvPush(byte) => Self::AdvPush(unwrap_u8!(byte)),
            Instruction::AdvLoadW => Self::AdvLoadw,
            Instruction::SysEvent(SystemEventNode::InsertMem) => Self::AdvInjectInsertMem,
            Instruction::SysEvent(SystemEventNode::InsertHperm) => Self::AdvInjectInsertHperm,
            Instruction::SysEvent(SystemEventNode::InsertHdword) => Self::AdvInjectInsertHdword,
            Instruction::SysEvent(SystemEventNode::PushU64Div) => Self::AdvInjectPushU64Div,
            Instruction::SysEvent(SystemEventNode::PushMtNode) => Self::AdvInjectPushMTreeNode,
            Instruction::SysEvent(SystemEventNode::PushMapVal) => Self::AdvInjectPushMapVal,
            Instruction::SysEvent(SystemEventNode::PushMapValN) => Self::AdvInjectPushMapValN,
            Instruction::SysEvent(SystemEventNode::PushSignature { kind }) => {
                Self::AdvInjectPushSignature(kind)
            }
            Instruction::SysEvent(injector) => {
                unimplemented!("unsupported advice injector: {injector:?}")
            }
            ref ix @ (Instruction::Exec(ref target)
            | Instruction::SysCall(ref target)
            | Instruction::Call(ref target)
            | Instruction::ProcRef(ref target)) => {
                let id = match target {
                    InvocationTarget::AbsoluteProcedurePath { name, path } => {
                        let name: &str = name.as_ref();
                        let function = Ident::with_empty_span(Symbol::intern(name));
                        let module = Ident::with_empty_span(Symbol::intern(path.to_string()));
                        FunctionIdent { module, function }
                    }
                    InvocationTarget::ProcedurePath { name, module } => {
                        let name: &str = name.as_ref();
                        let function = Ident::with_empty_span(Symbol::intern(name));
                        let module = Ident::with_empty_span(Symbol::intern(module.as_str()));
                        FunctionIdent { module, function }
                    }
                    InvocationTarget::ProcedureName(name) => {
                        let name: &str = name.as_ref();
                        let function = Ident::with_empty_span(Symbol::intern(name));
                        FunctionIdent {
                            module: current_module,
                            function,
                        }
                    }
                    InvocationTarget::MastRoot(_root) => {
                        todo!("support for referencing mast roots is not yet implemented")
                    }
                };
                match ix {
                    Instruction::Exec(_) => Self::Exec(id),
                    Instruction::SysCall(_) => Self::Syscall(id),
                    Instruction::Call(_) => Self::Call(id),
                    Instruction::ProcRef(_) => Self::ProcRef(id),
                    _ => unreachable!(),
                }
            }
            Instruction::DynExec => Self::DynExec,
            Instruction::DynCall => Self::DynCall,
            Instruction::Caller => Self::Caller,
            Instruction::Sdepth => Self::Sdepth,
            Instruction::Breakpoint => Self::Breakpoint,
            Instruction::Emit(event) => Self::Emit(unwrap_u32!(event)),
            Instruction::Trace(event) => Self::Trace(unwrap_u32!(event)),
            Instruction::Debug(DebugOptions::StackAll) => Self::DebugStack,
            Instruction::Debug(DebugOptions::StackTop(n)) => Self::DebugStackN(unwrap_u8!(n)),
            Instruction::Debug(DebugOptions::MemAll) => Self::DebugMemory,
            Instruction::Debug(DebugOptions::MemInterval(start, end)) => {
                Self::DebugMemoryRange(unwrap_u32!(start), unwrap_u32!(end))
            }
            Instruction::Debug(DebugOptions::LocalAll) => Self::DebugFrame,
            Instruction::Debug(DebugOptions::LocalRangeFrom(start)) => {
                Self::DebugFrameAt(unwrap_u16!(start))
            }
            Instruction::Debug(DebugOptions::LocalInterval(start, end)) => {
                Self::DebugFrameRange(unwrap_u16!(start), unwrap_u16!(end))
            }
            Instruction::Nop => Self::Nop,
        };
        smallvec![op]
    }

    pub fn into_masm(
        self,
        imports: &super::ModuleImportInfo,
        locals: &BTreeSet<FunctionIdent>,
    ) -> SmallVec<[miden_assembly::ast::Instruction; 2]> {
        use miden_assembly::{
            ast::{Instruction, InvocationTarget, ProcedureName},
            LibraryPath,
        };
        let inst = match self {
            Self::Padw => Instruction::PadW,
            Self::Push(v) => Instruction::PushFelt(v),
            Self::Push2([a, b]) => Instruction::PushFeltList(vec![a, b]),
            Self::Pushw(word) => Instruction::PushWord(word),
            Self::PushU8(v) => Instruction::PushU8(v),
            Self::PushU16(v) => Instruction::PushU16(v),
            Self::PushU32(v) => Instruction::PushU32(v),
            Self::Drop => Instruction::Drop,
            Self::Dropw => Instruction::DropW,
            Self::Dup(0) => Instruction::Dup0,
            Self::Dup(1) => Instruction::Dup1,
            Self::Dup(2) => Instruction::Dup2,
            Self::Dup(3) => Instruction::Dup3,
            Self::Dup(4) => Instruction::Dup4,
            Self::Dup(5) => Instruction::Dup5,
            Self::Dup(6) => Instruction::Dup6,
            Self::Dup(7) => Instruction::Dup7,
            Self::Dup(8) => Instruction::Dup8,
            Self::Dup(9) => Instruction::Dup9,
            Self::Dup(10) => Instruction::Dup10,
            Self::Dup(11) => Instruction::Dup11,
            Self::Dup(12) => Instruction::Dup12,
            Self::Dup(13) => Instruction::Dup13,
            Self::Dup(14) => Instruction::Dup14,
            Self::Dup(15) => Instruction::Dup15,
            Self::Dup(n) => {
                panic!("invalid dup instruction, valid index range is 0..=15, got {n}")
            }
            Self::Dupw(0) => Instruction::DupW0,
            Self::Dupw(1) => Instruction::DupW1,
            Self::Dupw(2) => Instruction::DupW2,
            Self::Dupw(3) => Instruction::DupW3,
            Self::Dupw(n) => {
                panic!("invalid dupw instruction, valid index range is 0..=3, got {n}")
            }
            Self::Swap(1) => Instruction::Swap1,
            Self::Swap(2) => Instruction::Swap2,
            Self::Swap(3) => Instruction::Swap3,
            Self::Swap(4) => Instruction::Swap4,
            Self::Swap(5) => Instruction::Swap5,
            Self::Swap(6) => Instruction::Swap6,
            Self::Swap(7) => Instruction::Swap7,
            Self::Swap(8) => Instruction::Swap8,
            Self::Swap(9) => Instruction::Swap9,
            Self::Swap(10) => Instruction::Swap10,
            Self::Swap(11) => Instruction::Swap11,
            Self::Swap(12) => Instruction::Swap12,
            Self::Swap(13) => Instruction::Swap13,
            Self::Swap(14) => Instruction::Swap14,
            Self::Swap(15) => Instruction::Swap15,
            Self::Swap(n) => {
                panic!("invalid swap instruction, valid index range is 1..=15, got {n}")
            }
            Self::Swapw(1) => Instruction::SwapW1,
            Self::Swapw(2) => Instruction::SwapW2,
            Self::Swapw(3) => Instruction::SwapW3,
            Self::Swapw(n) => {
                panic!("invalid swapw instruction, valid index range is 1..=3, got {n}")
            }
            Self::Swapdw => Instruction::SwapDw,
            Self::Movup(2) => Instruction::MovUp2,
            Self::Movup(3) => Instruction::MovUp3,
            Self::Movup(4) => Instruction::MovUp4,
            Self::Movup(5) => Instruction::MovUp5,
            Self::Movup(6) => Instruction::MovUp6,
            Self::Movup(7) => Instruction::MovUp7,
            Self::Movup(8) => Instruction::MovUp8,
            Self::Movup(9) => Instruction::MovUp9,
            Self::Movup(10) => Instruction::MovUp10,
            Self::Movup(11) => Instruction::MovUp11,
            Self::Movup(12) => Instruction::MovUp12,
            Self::Movup(13) => Instruction::MovUp13,
            Self::Movup(14) => Instruction::MovUp14,
            Self::Movup(15) => Instruction::MovUp15,
            Self::Movup(n) => {
                panic!("invalid movup instruction, valid index range is 2..=15, got {n}")
            }
            Self::Movupw(2) => Instruction::MovUpW2,
            Self::Movupw(3) => Instruction::MovUpW3,
            Self::Movupw(n) => {
                panic!("invalid movupw instruction, valid index range is 2..=3, got {n}")
            }
            Self::Movdn(2) => Instruction::MovDn2,
            Self::Movdn(3) => Instruction::MovDn3,
            Self::Movdn(4) => Instruction::MovDn4,
            Self::Movdn(5) => Instruction::MovDn5,
            Self::Movdn(6) => Instruction::MovDn6,
            Self::Movdn(7) => Instruction::MovDn7,
            Self::Movdn(8) => Instruction::MovDn8,
            Self::Movdn(9) => Instruction::MovDn9,
            Self::Movdn(10) => Instruction::MovDn10,
            Self::Movdn(11) => Instruction::MovDn11,
            Self::Movdn(12) => Instruction::MovDn12,
            Self::Movdn(13) => Instruction::MovDn13,
            Self::Movdn(14) => Instruction::MovDn14,
            Self::Movdn(15) => Instruction::MovDn15,
            Self::Movdn(n) => {
                panic!("invalid movdn instruction, valid index range is 2..=15, got {n}")
            }
            Self::Movdnw(2) => Instruction::MovDnW2,
            Self::Movdnw(3) => Instruction::MovDnW3,
            Self::Movdnw(n) => {
                panic!("invalid movdnw instruction, valid index range is 2..=3, got {n}")
            }
            Self::Cswap => Instruction::CSwap,
            Self::Cswapw => Instruction::CSwapW,
            Self::Cdrop => Instruction::CDrop,
            Self::Cdropw => Instruction::CDropW,
            Self::Assert => Instruction::Assert,
            Self::AssertWithError(code) => Instruction::AssertWithError(code.into()),
            Self::Assertz => Instruction::Assertz,
            Self::AssertzWithError(code) => Instruction::AssertzWithError(code.into()),
            Self::AssertEq => Instruction::AssertEq,
            Self::AssertEqWithError(code) => Instruction::AssertEqWithError(code.into()),
            Self::AssertEqw => Instruction::AssertEqw,
            Self::AssertEqwWithError(code) => Instruction::AssertEqwWithError(code.into()),
            Self::LocAddr(id) => Instruction::Locaddr(id.into()),
            Self::LocLoad(id) => Instruction::LocLoad(id.into()),
            Self::LocLoadw(id) => Instruction::LocLoadW(id.into()),
            Self::LocStore(id) => Instruction::LocStore(id.into()),
            Self::LocStorew(id) => Instruction::LocStoreW(id.into()),
            Self::MemLoad => Instruction::MemLoad,
            Self::MemLoadImm(addr) => Instruction::MemLoadImm(addr.into()),
            Self::MemLoadw => Instruction::MemLoadW,
            Self::MemLoadwImm(addr) => Instruction::MemLoadWImm(addr.into()),
            Self::MemStore => Instruction::MemStore,
            Self::MemStoreImm(addr) => Instruction::MemStoreImm(addr.into()),
            Self::MemStorew => Instruction::MemStoreW,
            Self::MemStorewImm(addr) => Instruction::MemStoreWImm(addr.into()),
            Self::MemStream => Instruction::MemStream,
            Self::AdvPipe => Instruction::AdvPipe,
            Self::AdvPush(n) => Instruction::AdvPush(n.into()),
            Self::AdvLoadw => Instruction::AdvLoadW,
            Self::AdvInjectPushU64Div => Instruction::SysEvent(SystemEventNode::PushU64Div),
            Self::AdvInjectPushMTreeNode => Instruction::SysEvent(SystemEventNode::PushMtNode),
            Self::AdvInjectPushMapVal => Instruction::SysEvent(SystemEventNode::PushMapVal),
            Self::AdvInjectPushMapValN => Instruction::SysEvent(SystemEventNode::PushMapValN),
            Self::AdvInjectInsertMem => Instruction::SysEvent(SystemEventNode::InsertMem),
            Self::AdvInjectInsertHperm => Instruction::SysEvent(SystemEventNode::InsertHperm),
            Self::AdvInjectInsertHdword => Instruction::SysEvent(SystemEventNode::InsertHdword),
            Self::AdvInjectPushSignature(kind) => {
                Instruction::SysEvent(SystemEventNode::PushSignature { kind })
            }
            Self::If(..) | Self::While(_) | Self::Repeat(..) => {
                panic!("control flow instructions are meant to be handled specially by the caller")
            }
            op @ (Self::Exec(ref callee)
            | Self::Call(ref callee)
            | Self::Syscall(ref callee)
            | Self::ProcRef(ref callee)) => {
                let target = if locals.contains(callee) {
                    let callee = ProcedureName::new_unchecked(super::utils::translate_ident(
                        callee.function,
                    ));
                    InvocationTarget::ProcedureName(callee)
                } else if let Some(alias) = imports.alias(&callee.module) {
                    let alias = super::utils::translate_ident(alias);
                    let name = ProcedureName::new_unchecked(super::utils::translate_ident(
                        callee.function,
                    ));
                    InvocationTarget::ProcedurePath {
                        name,
                        module: alias,
                    }
                } else {
                    let name = ProcedureName::new_unchecked(super::utils::translate_ident(
                        callee.function,
                    ));
                    let path =
                        LibraryPath::new(callee.module.as_str()).expect("invalid procedure path");
                    InvocationTarget::AbsoluteProcedurePath { name, path }
                };
                match op {
                    Self::Exec(_) => Instruction::Exec(target),
                    Self::Call(_) => Instruction::Call(target),
                    Self::Syscall(_) => Instruction::SysCall(target),
                    Self::ProcRef(_) => Instruction::ProcRef(target),
                    _ => unreachable!(),
                }
            }
            Self::DynExec => Instruction::DynExec,
            Self::DynCall => Instruction::DynCall,
            Self::Add => Instruction::Add,
            Self::AddImm(imm) => Instruction::AddImm(imm.into()),
            Self::Sub => Instruction::Sub,
            Self::SubImm(imm) => Instruction::SubImm(imm.into()),
            Self::Mul => Instruction::Mul,
            Self::MulImm(imm) => Instruction::MulImm(imm.into()),
            Self::Div => Instruction::Div,
            Self::DivImm(imm) => Instruction::DivImm(imm.into()),
            Self::Neg => Instruction::Neg,
            Self::Inv => Instruction::Inv,
            Self::Incr => Instruction::Incr,
            Self::Ilog2 => Instruction::ILog2,
            Self::Pow2 => Instruction::Pow2,
            Self::Exp => Instruction::Exp,
            Self::ExpImm(imm) => Instruction::ExpImm(Felt::new(imm as u64).into()),
            Self::ExpBitLength(imm) => Instruction::ExpBitLength(imm),
            Self::Not => Instruction::Not,
            Self::And => Instruction::And,
            Self::AndImm(imm) => {
                return smallvec![Instruction::PushU8(imm as u8), Instruction::And]
            }
            Self::Or => Instruction::Or,
            Self::OrImm(imm) => return smallvec![Instruction::PushU8(imm as u8), Instruction::Or],
            Self::Xor => Instruction::Xor,
            Self::XorImm(imm) => {
                return smallvec![Instruction::PushU8(imm as u8), Instruction::Xor]
            }
            Self::Eq => Instruction::Eq,
            Self::EqImm(imm) => Instruction::EqImm(imm.into()),
            Self::Neq => Instruction::Neq,
            Self::NeqImm(imm) => Instruction::NeqImm(imm.into()),
            Self::Gt => Instruction::Gt,
            Self::GtImm(imm) => return smallvec![Instruction::PushFelt(imm), Instruction::Gt],
            Self::Gte => Instruction::Gte,
            Self::GteImm(imm) => return smallvec![Instruction::PushFelt(imm), Instruction::Gte],
            Self::Lt => Instruction::Lt,
            Self::LtImm(imm) => return smallvec![Instruction::PushFelt(imm), Instruction::Lt],
            Self::Lte => Instruction::Lte,
            Self::LteImm(imm) => return smallvec![Instruction::PushFelt(imm), Instruction::Lte],
            Self::IsOdd => Instruction::IsOdd,
            Self::Eqw => Instruction::Eqw,
            Self::Ext2add => Instruction::Ext2Add,
            Self::Ext2sub => Instruction::Ext2Sub,
            Self::Ext2mul => Instruction::Ext2Mul,
            Self::Ext2div => Instruction::Ext2Div,
            Self::Ext2neg => Instruction::Ext2Neg,
            Self::Ext2inv => Instruction::Ext2Inv,
            Self::Clk => Instruction::Clk,
            Self::Caller => Instruction::Caller,
            Self::Sdepth => Instruction::Sdepth,
            Self::Hash => Instruction::Hash,
            Self::Hperm => Instruction::HPerm,
            Self::Hmerge => Instruction::HMerge,
            Self::MtreeGet => Instruction::MTreeGet,
            Self::MtreeSet => Instruction::MTreeSet,
            Self::MtreeMerge => Instruction::MTreeMerge,
            Self::MtreeVerify => Instruction::MTreeVerify,
            Self::MtreeVerifyWithError(code) => Instruction::MTreeVerifyWithError(code.into()),
            Self::FriExt2Fold4 => Instruction::FriExt2Fold4,
            Self::RCombBase => Instruction::RCombBase,
            Self::U32Test => Instruction::U32Test,
            Self::U32Testw => Instruction::U32TestW,
            Self::U32Assert => Instruction::U32Assert,
            Self::U32AssertWithError(code) => Instruction::U32AssertWithError(code.into()),
            Self::U32Assert2 => Instruction::U32Assert2,
            Self::U32Assert2WithError(code) => Instruction::U32Assert2WithError(code.into()),
            Self::U32Assertw => Instruction::U32AssertW,
            Self::U32AssertwWithError(code) => Instruction::U32AssertWWithError(code.into()),
            Self::U32Cast => Instruction::U32Cast,
            Self::U32Split => Instruction::U32Split,
            Self::U32OverflowingAdd => Instruction::U32OverflowingAdd,
            Self::U32OverflowingAddImm(imm) => Instruction::U32OverflowingAddImm(imm.into()),
            Self::U32WrappingAdd => Instruction::U32WrappingAdd,
            Self::U32WrappingAddImm(imm) => Instruction::U32WrappingAddImm(imm.into()),
            Self::U32OverflowingAdd3 => Instruction::U32OverflowingAdd3,
            Self::U32WrappingAdd3 => Instruction::U32WrappingAdd3,
            Self::U32OverflowingSub => Instruction::U32OverflowingSub,
            Self::U32OverflowingSubImm(imm) => Instruction::U32OverflowingSubImm(imm.into()),
            Self::U32WrappingSub => Instruction::U32WrappingSub,
            Self::U32WrappingSubImm(imm) => Instruction::U32WrappingSubImm(imm.into()),
            Self::U32OverflowingMul => Instruction::U32OverflowingMul,
            Self::U32OverflowingMulImm(imm) => Instruction::U32OverflowingMulImm(imm.into()),
            Self::U32WrappingMul => Instruction::U32WrappingMul,
            Self::U32WrappingMulImm(imm) => Instruction::U32WrappingMulImm(imm.into()),
            Self::U32OverflowingMadd => Instruction::U32OverflowingMadd,
            Self::U32WrappingMadd => Instruction::U32WrappingMadd,
            Self::U32Div => Instruction::U32Div,
            Self::U32DivImm(imm) => Instruction::U32DivImm(imm.into()),
            Self::U32Mod => Instruction::U32Mod,
            Self::U32ModImm(imm) => Instruction::U32ModImm(imm.into()),
            Self::U32DivMod => Instruction::U32DivMod,
            Self::U32DivModImm(imm) => Instruction::U32DivModImm(imm.into()),
            Self::U32And => Instruction::U32And,
            Self::U32Or => Instruction::U32Or,
            Self::U32Xor => Instruction::U32Xor,
            Self::U32Not => Instruction::U32Not,
            Self::U32Shl => Instruction::U32Shl,
            Self::U32ShlImm(imm) => {
                let shift = u8::try_from(imm).expect("invalid shift");
                Instruction::U32ShlImm(shift.into())
            }
            Self::U32Shr => Instruction::U32Shr,
            Self::U32ShrImm(imm) => {
                let shift = u8::try_from(imm).expect("invalid shift");
                Instruction::U32ShrImm(shift.into())
            }
            Self::U32Rotl => Instruction::U32Rotl,
            Self::U32RotlImm(imm) => {
                let rotate = u8::try_from(imm).expect("invalid rotation");
                Instruction::U32RotlImm(rotate.into())
            }
            Self::U32Rotr => Instruction::U32Rotr,
            Self::U32RotrImm(imm) => {
                let rotate = u8::try_from(imm).expect("invalid rotation");
                Instruction::U32RotrImm(rotate.into())
            }
            Self::U32Popcnt => Instruction::U32Popcnt,
            Self::U32Clz => Instruction::U32Clz,
            Self::U32Ctz => Instruction::U32Ctz,
            Self::U32Clo => Instruction::U32Clo,
            Self::U32Cto => Instruction::U32Cto,
            Self::U32Lt => Instruction::U32Lt,
            Self::U32LtImm(imm) => return smallvec![Instruction::PushU32(imm), Instruction::U32Lt],
            Self::U32Lte => Instruction::U32Lte,
            Self::U32LteImm(imm) => {
                return smallvec![Instruction::PushU32(imm), Instruction::U32Lte]
            }
            Self::U32Gt => Instruction::U32Gt,
            Self::U32GtImm(imm) => return smallvec![Instruction::PushU32(imm), Instruction::U32Gt],
            Self::U32Gte => Instruction::U32Gte,
            Self::U32GteImm(imm) => {
                return smallvec![Instruction::PushU32(imm), Instruction::U32Gte];
            }
            Self::U32Min => Instruction::U32Min,
            Self::U32MinImm(imm) => {
                return smallvec![Instruction::PushU32(imm), Instruction::U32Min];
            }
            Self::U32Max => Instruction::U32Max,
            Self::U32MaxImm(imm) => {
                return smallvec![Instruction::PushU32(imm), Instruction::U32Max];
            }
            Self::Breakpoint => Instruction::Breakpoint,
            Self::DebugStack => Instruction::Debug(DebugOptions::StackAll),
            Self::DebugStackN(n) => Instruction::Debug(DebugOptions::StackTop(n.into())),
            Self::DebugMemory => Instruction::Debug(DebugOptions::MemAll),
            Self::DebugMemoryAt(start) => {
                Instruction::Debug(DebugOptions::MemInterval(start.into(), u32::MAX.into()))
            }
            Self::DebugMemoryRange(start, end) => {
                Instruction::Debug(DebugOptions::MemInterval(start.into(), end.into()))
            }
            Self::DebugFrame => Instruction::Debug(DebugOptions::LocalAll),
            Self::DebugFrameAt(start) => {
                Instruction::Debug(DebugOptions::LocalRangeFrom(start.into()))
            }
            Self::DebugFrameRange(start, end) => {
                Instruction::Debug(DebugOptions::LocalInterval(start.into(), end.into()))
            }
            Self::Emit(ev) => Instruction::Emit(ev.into()),
            Self::Trace(ev) => Instruction::Trace(ev.into()),
            Self::Nop => Instruction::Nop,
        };
        smallvec![inst]
    }
}

/// This implementation displays the opcode name for the given [MasmOp]
impl fmt::Display for MasmOp {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Padw => f.write_str("padw"),
            Self::Push(_)
            | Self::Push2(_)
            | Self::Pushw(_)
            | Self::PushU8(_)
            | Self::PushU16(_)
            | Self::PushU32(_) => f.write_str("push"),
            Self::Drop => f.write_str("drop"),
            Self::Dropw => f.write_str("dropw"),
            Self::Dup(_) => f.write_str("dup"),
            Self::Dupw(_) => f.write_str("dupw"),
            Self::Swap(_) => f.write_str("swap"),
            Self::Swapw(_) => f.write_str("swapw"),
            Self::Swapdw => f.write_str("swapdw"),
            Self::Movup(_) => f.write_str("movup"),
            Self::Movupw(_) => f.write_str("movupw"),
            Self::Movdn(_) => f.write_str("movdn"),
            Self::Movdnw(_) => f.write_str("movdnw"),
            Self::Cswap => f.write_str("cswap"),
            Self::Cswapw => f.write_str("cswapw"),
            Self::Cdrop => f.write_str("cdrop"),
            Self::Cdropw => f.write_str("cdropw"),
            Self::Assert => f.write_str("assert"),
            Self::AssertWithError(code) => write!(f, "assert.err={code}"),
            Self::Assertz => f.write_str("assertz"),
            Self::AssertzWithError(code) => write!(f, "assertz.err={code}"),
            Self::AssertEq => f.write_str("assert_eq"),
            Self::AssertEqWithError(code) => write!(f, "assert_eq.err={code}"),
            Self::AssertEqw => f.write_str("assert_eqw"),
            Self::AssertEqwWithError(code) => write!(f, "assert_eqw.err={code}"),
            Self::LocAddr(_) => f.write_str("locaddr"),
            Self::LocLoad(_) => f.write_str("loc_load"),
            Self::LocLoadw(_) => f.write_str("loc_loadw"),
            Self::LocStore(_) => f.write_str("loc_store"),
            Self::LocStorew(_) => f.write_str("loc_storew"),
            Self::MemLoad | Self::MemLoadImm(_) => f.write_str("mem_load"),
            Self::MemLoadw | Self::MemLoadwImm(_) => f.write_str("mem_loadw"),
            Self::MemStore | Self::MemStoreImm(_) => f.write_str("mem_store"),
            Self::MemStorew | Self::MemStorewImm(_) => f.write_str("mem_storew"),
            Self::MemStream => f.write_str("mem_stream"),
            Self::AdvPipe => f.write_str("adv_pipe"),
            Self::AdvPush(_) => f.write_str("adv_push"),
            Self::AdvLoadw => f.write_str("adv_loadw"),
            Self::AdvInjectPushU64Div => f.write_str("adv.push_u64div"),
            Self::AdvInjectPushMTreeNode => f.write_str("adv.push_mtnode"),
            Self::AdvInjectPushMapVal => f.write_str("adv.push_mapval"),
            Self::AdvInjectPushMapValN => f.write_str("adv.push_mapvaln"),
            Self::AdvInjectInsertMem => f.write_str("adv.insert_mem"),
            Self::AdvInjectInsertHperm => f.write_str("adv.insert_hperm"),
            Self::AdvInjectInsertHdword => f.write_str("adv.insert_hdword"),
            Self::AdvInjectPushSignature(kind) => write!(f, "adv.push_sig.{kind}"),
            Self::If(..) => f.write_str("if.true"),
            Self::While(_) => f.write_str("while.true"),
            Self::Repeat(..) => f.write_str("repeat"),
            Self::Exec(_) => f.write_str("exec"),
            Self::Syscall(_) => f.write_str("syscall"),
            Self::Call(_) => f.write_str("call"),
            Self::DynExec => f.write_str("dynexec"),
            Self::DynCall => f.write_str("dyncall"),
            Self::ProcRef(_) => f.write_str("procref"),
            Self::Add | Self::AddImm(_) => f.write_str("add"),
            Self::Sub | Self::SubImm(_) => f.write_str("sub"),
            Self::Mul | Self::MulImm(_) => f.write_str("mul"),
            Self::Div | Self::DivImm(_) => f.write_str("div"),
            Self::Neg => f.write_str("neg"),
            Self::Inv => f.write_str("inv"),
            Self::Incr => f.write_str("add.1"),
            Self::Ilog2 => f.write_str("ilog2"),
            Self::Pow2 => f.write_str("pow2"),
            Self::Exp => f.write_str("exp"),
            Self::ExpImm(imm) => write!(f, "exp.{imm}"),
            Self::ExpBitLength(imm) => write!(f, "exp.u{imm}"),
            Self::Not => f.write_str("not"),
            Self::And | Self::AndImm(_) => f.write_str("and"),
            Self::Or | Self::OrImm(_) => f.write_str("or"),
            Self::Xor | Self::XorImm(_) => f.write_str("xor"),
            Self::Eq | Self::EqImm(_) => f.write_str("eq"),
            Self::Neq | Self::NeqImm(_) => f.write_str("neq"),
            Self::Gt | Self::GtImm(_) => f.write_str("gt"),
            Self::Gte | Self::GteImm(_) => f.write_str("gte"),
            Self::Lt | Self::LtImm(_) => f.write_str("lt"),
            Self::Lte | Self::LteImm(_) => f.write_str("lte"),
            Self::IsOdd => f.write_str("is_odd"),
            Self::Eqw => f.write_str("eqw"),
            Self::Ext2add => f.write_str("ext2add"),
            Self::Ext2sub => f.write_str("ext2sub"),
            Self::Ext2mul => f.write_str("ext2mul"),
            Self::Ext2div => f.write_str("ext2div"),
            Self::Ext2neg => f.write_str("ext2neg"),
            Self::Ext2inv => f.write_str("ext2inv"),
            Self::Clk => f.write_str("clk"),
            Self::Caller => f.write_str("caller"),
            Self::Sdepth => f.write_str("sdepth"),
            Self::Hash => f.write_str("hash"),
            Self::Hperm => f.write_str("hperm"),
            Self::Hmerge => f.write_str("hmerge"),
            Self::MtreeGet => f.write_str("mtree_get"),
            Self::MtreeSet => f.write_str("mtree_set"),
            Self::MtreeMerge => f.write_str("mtree_merge"),
            Self::MtreeVerify => f.write_str("mtree_verify"),
            Self::MtreeVerifyWithError(code) => write!(f, "mtree_verify.err={code}"),
            Self::FriExt2Fold4 => f.write_str("fri_ext2fold4"),
            Self::RCombBase => f.write_str("rcomb_base"),
            Self::U32Test => f.write_str("u32test"),
            Self::U32Testw => f.write_str("u32testw"),
            Self::U32Assert => f.write_str("u32assert"),
            Self::U32AssertWithError(code) => write!(f, "u32assert.err={code}"),
            Self::U32Assert2 => f.write_str("u32assert2"),
            Self::U32Assert2WithError(code) => write!(f, "u32assert2.err={code}"),
            Self::U32Assertw => f.write_str("u32assertw"),
            Self::U32AssertwWithError(code) => write!(f, "u32assertw.err={code}"),
            Self::U32Cast => f.write_str("u32cast"),
            Self::U32Split => f.write_str("u32split"),
            Self::U32OverflowingAdd | Self::U32OverflowingAddImm(_) => {
                f.write_str("u32overflowing_add")
            }
            Self::U32WrappingAdd | Self::U32WrappingAddImm(_) => f.write_str("u32wrapping_add"),
            Self::U32OverflowingAdd3 => f.write_str("u32overflowing_add3"),
            Self::U32WrappingAdd3 => f.write_str("u32wrapping_add3"),
            Self::U32OverflowingSub | Self::U32OverflowingSubImm(_) => {
                f.write_str("u32overflowing_sub")
            }
            Self::U32WrappingSub | Self::U32WrappingSubImm(_) => f.write_str("u32wrapping_sub"),
            Self::U32OverflowingMul | Self::U32OverflowingMulImm(_) => {
                f.write_str("u32overflowing_mul")
            }
            Self::U32WrappingMul | Self::U32WrappingMulImm(_) => f.write_str("u32wrapping_mul"),
            Self::U32OverflowingMadd => f.write_str("u32overflowing_madd"),
            Self::U32WrappingMadd => f.write_str("u32wrapping_madd"),
            Self::U32Div | Self::U32DivImm(_) => f.write_str("u32div"),
            Self::U32Mod | Self::U32ModImm(_) => f.write_str("u32mod"),
            Self::U32DivMod | Self::U32DivModImm(_) => f.write_str("u32divmod"),
            Self::U32And => f.write_str("u32and"),
            Self::U32Or => f.write_str("u32or"),
            Self::U32Xor => f.write_str("u32xor"),
            Self::U32Not => f.write_str("u32not"),
            Self::U32Shl | Self::U32ShlImm(_) => f.write_str("u32shl"),
            Self::U32Shr | Self::U32ShrImm(_) => f.write_str("u32shr"),
            Self::U32Rotl | Self::U32RotlImm(_) => f.write_str("u32rotl"),
            Self::U32Rotr | Self::U32RotrImm(_) => f.write_str("u32rotr"),
            Self::U32Popcnt => f.write_str("u32popcnt"),
            Self::U32Clz => f.write_str("u32clz"),
            Self::U32Ctz => f.write_str("u32ctz"),
            Self::U32Clo => f.write_str("u32clo"),
            Self::U32Cto => f.write_str("u32cto"),
            Self::U32Lt | Self::U32LtImm(_) => f.write_str("u32lt"),
            Self::U32Lte | Self::U32LteImm(_) => f.write_str("u32lte"),
            Self::U32Gt | Self::U32GtImm(_) => f.write_str("u32gt"),
            Self::U32Gte | Self::U32GteImm(_) => f.write_str("u32gte"),
            Self::U32Min | Self::U32MinImm(_) => f.write_str("u32min"),
            Self::U32Max | Self::U32MaxImm(_) => f.write_str("u32max"),
            Self::Breakpoint => f.write_str("breakpoint"),
            Self::DebugStack | Self::DebugStackN(_) => f.write_str("debug.stack"),
            Self::DebugMemory | Self::DebugMemoryAt(_) | Self::DebugMemoryRange(..) => {
                f.write_str("debug.mem")
            }
            Self::DebugFrame | Self::DebugFrameAt(_) | Self::DebugFrameRange(..) => {
                f.write_str("debug.local")
            }
            Self::Emit(_) => f.write_str("emit"),
            Self::Trace(_) => f.write_str("trace"),
            Self::Nop => f.write_str("nop"),
        }
    }
}
