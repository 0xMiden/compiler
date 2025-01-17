use core::ops::{Deref, DerefMut};
use std::collections::BTreeSet;

use cranelift_entity::entity_impl;
use intrusive_collections::{intrusive_adapter, LinkedListLink};
use smallvec::{smallvec, SmallVec};

use self::formatter::PrettyPrint;
use crate::{
    diagnostics::{Span, Spanned},
    *,
};

/// A handle to a single instruction
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Inst(u32);
entity_impl!(Inst, "inst");

/// Represents the data associated with an `Inst`.
///
/// Specifically, this represents a leaf node in the control flow graph of
/// a function, i.e. it links a specific instruction in to the sequence of
/// instructions belonging to a specific block.
#[derive(Spanned)]
pub struct InstNode {
    pub link: LinkedListLink,
    pub key: Inst,
    pub block: Block,
    #[span]
    pub data: Span<Instruction>,
}
impl fmt::Debug for InstNode {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", &self.data)
    }
}
impl InstNode {
    pub fn new(key: Inst, block: Block, data: Span<Instruction>) -> Self {
        Self {
            link: LinkedListLink::default(),
            key,
            block,
            data,
        }
    }

    pub fn deep_clone(&self, value_lists: &mut ValueListPool) -> Self {
        let span = self.data.span();
        Self {
            link: LinkedListLink::default(),
            key: self.key,
            block: self.block,
            data: Span::new(span, self.data.deep_clone(value_lists)),
        }
    }

    pub fn replace(&mut self, data: Span<Instruction>) {
        self.data = data;
    }
}
impl Deref for InstNode {
    type Target = Instruction;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.data
    }
}
impl DerefMut for InstNode {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.data
    }
}
impl AsRef<Instruction> for InstNode {
    #[inline]
    fn as_ref(&self) -> &Instruction {
        &self.data
    }
}
impl AsMut<Instruction> for InstNode {
    #[inline]
    fn as_mut(&mut self) -> &mut Instruction {
        &mut self.data
    }
}

intrusive_adapter!(pub InstAdapter = UnsafeRef<InstNode>: InstNode { link: LinkedListLink });

/// A type alias for `LinkedList<InstAdapter>`
pub type InstructionList = intrusive_collections::LinkedList<InstAdapter>;

pub type InstructionCursor<'a> = intrusive_collections::linked_list::Cursor<'a, InstAdapter>;
pub type InstructionCursorMut<'a> = intrusive_collections::linked_list::CursorMut<'a, InstAdapter>;

/// Represents the type of instruction associated with a particular opcode
#[derive(Debug)]
pub enum Instruction {
    GlobalValue(GlobalValueOp),
    LocalVar(LocalVarOp),
    BinaryOp(BinaryOp),
    BinaryOpImm(BinaryOpImm),
    UnaryOp(UnaryOp),
    UnaryOpImm(UnaryOpImm),
    Call(Call),
    Br(Br),
    CondBr(CondBr),
    Switch(Switch),
    Ret(Ret),
    RetImm(RetImm),
    Load(LoadOp),
    PrimOp(PrimOp),
    PrimOpImm(PrimOpImm),
    Test(Test),
    InlineAsm(InlineAsm),
}
impl Instruction {
    pub fn deep_clone(&self, value_lists: &mut ValueListPool) -> Self {
        match self {
            Self::GlobalValue(gv) => Self::GlobalValue(gv.clone()),
            Self::LocalVar(lv) => Self::LocalVar(lv.clone()),
            Self::BinaryOp(op) => Self::BinaryOp(op.clone()),
            Self::BinaryOpImm(op) => Self::BinaryOpImm(op.clone()),
            Self::UnaryOp(op) => Self::UnaryOp(op.clone()),
            Self::UnaryOpImm(op) => Self::UnaryOpImm(op.clone()),
            Self::Call(call) => Self::Call(Call {
                args: call.args.deep_clone(value_lists),
                ..call.clone()
            }),
            Self::Br(br) => Self::Br(Br {
                successor: br.successor.deep_clone(value_lists),
                ..br.clone()
            }),
            Self::CondBr(br) => Self::CondBr(CondBr {
                then_dest: br.then_dest.deep_clone(value_lists),
                else_dest: br.else_dest.deep_clone(value_lists),
                ..br.clone()
            }),
            Self::Switch(op) => Self::Switch(Switch {
                arms: op
                    .arms
                    .iter()
                    .map(|arm| SwitchArm {
                        value: arm.value,
                        successor: arm.successor.deep_clone(value_lists),
                    })
                    .collect(),
                default: op.default.deep_clone(value_lists),
                ..op.clone()
            }),
            Self::Ret(op) => Self::Ret(Ret {
                args: op.args.deep_clone(value_lists),
                ..op.clone()
            }),
            Self::RetImm(op) => Self::RetImm(op.clone()),
            Self::Load(op) => Self::Load(op.clone()),
            Self::PrimOp(op) => Self::PrimOp(PrimOp {
                args: op.args.deep_clone(value_lists),
                ..op.clone()
            }),
            Self::PrimOpImm(op) => Self::PrimOpImm(PrimOpImm {
                args: op.args.deep_clone(value_lists),
                ..op.clone()
            }),
            Self::Test(op) => Self::Test(op.clone()),
            Self::InlineAsm(op) => Self::InlineAsm(InlineAsm {
                args: op.args.deep_clone(value_lists),
                ..op.clone()
            }),
        }
    }

    pub fn opcode(&self) -> Opcode {
        match self {
            Self::GlobalValue(GlobalValueOp { ref op, .. })
            | Self::LocalVar(LocalVarOp { ref op, .. })
            | Self::BinaryOp(BinaryOp { ref op, .. })
            | Self::BinaryOpImm(BinaryOpImm { ref op, .. })
            | Self::UnaryOp(UnaryOp { ref op, .. })
            | Self::UnaryOpImm(UnaryOpImm { ref op, .. })
            | Self::Call(Call { ref op, .. })
            | Self::Br(Br { ref op, .. })
            | Self::CondBr(CondBr { ref op, .. })
            | Self::Switch(Switch { ref op, .. })
            | Self::Ret(Ret { ref op, .. })
            | Self::RetImm(RetImm { ref op, .. })
            | Self::Load(LoadOp { ref op, .. })
            | Self::PrimOp(PrimOp { ref op, .. })
            | Self::PrimOpImm(PrimOpImm { ref op, .. })
            | Self::Test(Test { ref op, .. })
            | Self::InlineAsm(InlineAsm { ref op, .. }) => *op,
        }
    }

    /// Returns true if this instruction has side effects, or may have side effects
    ///
    /// Side effects are defined as control flow, writing memory, trapping execution,
    /// I/O, etc.
    #[inline]
    pub fn has_side_effects(&self) -> bool {
        self.opcode().has_side_effects()
    }

    /// Returns true if this instruction is a binary operator requiring two operands
    ///
    /// NOTE: Binary operators with immediate operands are not considered binary for
    /// this purpose, as they only require a single operand to be provided to the
    /// instruction, the immediate being the other one provided by the instruction
    /// itself.
    pub fn is_binary(&self) -> bool {
        matches!(self, Self::BinaryOp(_))
    }

    /// Returns true if this instruction is a binary operator whose operands may
    /// appear in any order.
    #[inline]
    pub fn is_commutative(&self) -> bool {
        self.opcode().is_commutative()
    }

    /// Get the [Overflow] flag for this instruction, if applicable
    pub fn overflow(&self) -> Option<Overflow> {
        match self {
            Self::BinaryOp(BinaryOp { overflow, .. })
            | Self::BinaryOpImm(BinaryOpImm { overflow, .. })
            | Self::UnaryOp(UnaryOp { overflow, .. })
            | Self::UnaryOpImm(UnaryOpImm { overflow, .. }) => *overflow,
            _ => None,
        }
    }

    pub fn arguments<'a>(&'a self, pool: &'a ValueListPool) -> &'a [Value] {
        match self {
            Self::BinaryOp(BinaryOp { ref args, .. }) => args.as_slice(),
            Self::BinaryOpImm(BinaryOpImm { ref arg, .. }) => core::slice::from_ref(arg),
            Self::UnaryOp(UnaryOp { ref arg, .. }) => core::slice::from_ref(arg),
            Self::Call(Call { ref args, .. }) => args.as_slice(pool),
            Self::CondBr(CondBr { ref cond, .. }) => core::slice::from_ref(cond),
            Self::Switch(Switch { ref arg, .. }) => core::slice::from_ref(arg),
            Self::Ret(Ret { ref args, .. }) => args.as_slice(pool),
            Self::Load(LoadOp { ref addr, .. }) => core::slice::from_ref(addr),
            Self::PrimOp(PrimOp { ref args, .. }) => args.as_slice(pool),
            Self::PrimOpImm(PrimOpImm { ref args, .. }) => args.as_slice(pool),
            Self::Test(Test { ref arg, .. }) => core::slice::from_ref(arg),
            Self::InlineAsm(InlineAsm { ref args, .. }) => args.as_slice(pool),
            Self::LocalVar(LocalVarOp { ref args, .. }) => args.as_slice(pool),
            Self::GlobalValue(_) | Self::UnaryOpImm(_) | Self::Br(_) | Self::RetImm(_) => &[],
        }
    }

    pub fn arguments_mut<'a>(&'a mut self, pool: &'a mut ValueListPool) -> &'a mut [Value] {
        match self {
            Self::BinaryOp(BinaryOp { ref mut args, .. }) => args.as_mut_slice(),
            Self::BinaryOpImm(BinaryOpImm { ref mut arg, .. }) => core::slice::from_mut(arg),
            Self::UnaryOp(UnaryOp { ref mut arg, .. }) => core::slice::from_mut(arg),
            Self::Call(Call { ref mut args, .. }) => args.as_mut_slice(pool),
            Self::CondBr(CondBr { ref mut cond, .. }) => core::slice::from_mut(cond),
            Self::Switch(Switch { ref mut arg, .. }) => core::slice::from_mut(arg),
            Self::Ret(Ret { ref mut args, .. }) => args.as_mut_slice(pool),
            Self::Load(LoadOp { ref mut addr, .. }) => core::slice::from_mut(addr),
            Self::PrimOp(PrimOp { ref mut args, .. }) => args.as_mut_slice(pool),
            Self::PrimOpImm(PrimOpImm { ref mut args, .. }) => args.as_mut_slice(pool),
            Self::Test(Test { ref mut arg, .. }) => core::slice::from_mut(arg),
            Self::InlineAsm(InlineAsm { ref mut args, .. }) => args.as_mut_slice(pool),
            Self::LocalVar(LocalVarOp { ref mut args, .. }) => args.as_mut_slice(pool),
            Self::GlobalValue(_) | Self::UnaryOpImm(_) | Self::Br(_) | Self::RetImm(_) => &mut [],
        }
    }

    pub fn analyze_branch<'a>(&'a self, pool: &'a ValueListPool) -> BranchInfo<'a> {
        match self {
            Self::Br(Br { successor, .. }) => {
                BranchInfo::SingleDest(SuccessorInfo::new(successor, pool))
            }
            Self::CondBr(CondBr {
                ref then_dest,
                ref else_dest,
                ..
            }) => BranchInfo::MultiDest(vec![
                SuccessorInfo::new(then_dest, pool),
                SuccessorInfo::new(else_dest, pool),
            ]),
            Self::Switch(Switch {
                ref arms,
                ref default,
                ..
            }) => {
                let mut targets = arms
                    .iter()
                    .map(|succ| SuccessorInfo::new(&succ.successor, pool))
                    .collect::<Vec<_>>();
                targets.push(SuccessorInfo::new(default, pool));
                BranchInfo::MultiDest(targets)
            }
            _ => BranchInfo::NotABranch,
        }
    }

    pub fn analyze_call<'a>(&'a self, pool: &'a ValueListPool) -> CallInfo<'a> {
        match self {
            Self::Call(ref c) => CallInfo::Direct(c.callee, c.args.as_slice(pool)),
            _ => CallInfo::NotACall,
        }
    }
}

#[derive(Debug)]
pub enum BranchInfo<'a> {
    NotABranch,
    SingleDest(SuccessorInfo<'a>),
    MultiDest(Vec<SuccessorInfo<'a>>),
}

pub enum CallInfo<'a> {
    NotACall,
    Direct(FunctionIdent, &'a [Value]),
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum Opcode {
    /// Asserts the given value is 1
    Assert,
    /// Asserts the given value is 0
    Assertz,
    /// Asserts the two given values are equal
    AssertEq,
    /// Represents an immediate boolean value (1-bit integer)
    ImmI1,
    /// Represents an immediate unsigned 8-bit integer value
    ImmU8,
    /// Represents an immediate signed 8-bit integer value
    ImmI8,
    /// Represents an immediate unsigned 16-bit integer value
    ImmU16,
    /// Represents an immediate signed 16-bit integer value
    ImmI16,
    /// Represents an immediate unsigned 32-bit integer value
    ImmU32,
    /// Represents an immediate signed 32-bit integer value
    ImmI32,
    /// Represents an immediate unsigned 64-bit integer value
    ImmU64,
    /// Represents an immediate signed 64-bit integer value
    ImmI64,
    /// Represents an immediate unsigned 128-bit integer value
    ImmU128,
    /// Represents an immediate signed 128-bit integer value
    ImmI128,
    /// Represents an immediate field element
    ImmFelt,
    /// Represents an immediate 64-bit floating-point value
    ImmF64,
    /// Allocates a new "null" value in a temporary memory slot, where null is defined by
    /// the semantics of the type. The result of this instruction is always a pointer to
    /// the allocated type.
    ///
    /// For integral types, the null value is always zero.
    ///
    /// For pointer types, the null value is equal to the address of the start of the linear
    /// memory range, i.e. address `0x0`.
    ///
    /// For structs and arrays, the null value is a value equal in size (in bytes) to the size
    /// of the type, but whose contents are undefined, i.e. you cannot assume that the binary
    /// representation of the value is zeroed.
    Alloca,
    /// Like the WebAssembly `memory.grow` instruction, this allocates a given number of pages from
    /// the global heap, and returns the previous size of the heap, in pages. Each page is 64kb
    /// by default.
    ///
    /// For the time being, this instruction is emulated using a heap pointer global which tracks
    /// the "end" of the available heap. Nothing actually prevents one from accessing memory past
    /// that point (assuming it is within the 32-bit address range), however this allows us to
    /// support code compiled for the `wasm32-unknown-unknown` target cleanly.
    MemGrow,
    /// Corresponds to the WebAssembly `memory.size` instruction, this returns the current number
    /// of pages allocated to the global heap. Each page is 64kb by default.
    ///
    /// This simply computes the offset in pages of the current global heap pointer
    MemSize,
    /// This instruction is used to represent a global value in the IR
    ///
    /// See [GlobalValueOp] and [GlobalValueData] for details on what types of values are
    /// represented behind this opcode.
    GlobalValue,
    /// Loads a value from a pointer to memory
    Load,
    /// Stores a value to a pointer to memory
    Store,
    /// Writes `n` copies of a value starting at a given address in memory
    MemSet,
    /// Copies `n` values of a given type from a source pointer to a destination pointer
    MemCpy,
    /// Casts a pointer value to an integral type
    PtrToInt,
    /// Casts an integral type to a pointer value
    IntToPtr,
    /// Casts from a field element type to an integral type
    ///
    /// It is not valid to perform a cast on any value other than a field element, see
    /// `Trunc`, `Zext`, and `Sext` for casts between machine integer types.
    Cast,
    /// Reinterprets the bits of an integral type from signed to unsigned or vice versa
    ///
    /// This is intended for representing casts that have no validation whatsoever. In
    /// particular, it is designed to support WebAssembly translation, which represents all
    /// numeric types using signed types by default, and selectively interprets them as
    /// unsigned as needed. This results in many casts where we do not want to generate
    /// asserts and other forms of dynamic checks, as the types are already validated.
    Bitcast,
    /// Truncates a larger integral type to a smaller integral type, e.g. i64 -> i32
    Trunc,
    /// Zero-extends a smaller unsigned integral type to a larger unsigned integral type, e.g. u32
    /// -> u64
    Zext,
    /// Sign-extends a smaller signed integral type to a larger signed integral type, e.g. i32 ->
    /// i64
    Sext,
    /// Returns true if argument fits in the given integral type, e.g. u32, otherwise false
    Test,
    /// Selects between two values given a conditional
    Select,
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    DivMod,
    Neg,
    Inv,
    Incr,
    Ilog2,
    Pow2,
    Exp,
    Not,
    Bnot,
    And,
    Band,
    Or,
    Bor,
    Xor,
    Bxor,
    Shl,
    Shr,
    Rotl,
    Rotr,
    Popcnt,
    Clz,
    Ctz,
    Clo,
    Cto,
    Eq,
    Neq,
    Gt,
    Gte,
    Lt,
    Lte,
    IsOdd,
    Min,
    Max,
    Exec,
    Call,
    Syscall,
    Br,
    CondBr,
    Switch,
    Ret,
    Unreachable,
    InlineAsm,
    /// NOTE: Internal Use Only!
    /// This is used during the spill pass to record spilling a value to temporary memory,
    /// which will eventually be allocated to a local variable slot and rewritten as store.local
    /// before that pass completes
    Spill,
    /// NOTE: Internal Use Only!
    /// This is used during the spill pass to record reloading a spilled value as a new copy.
    /// This is rewritten to a load.local before the pass completes.
    Reload,
}
impl Opcode {
    pub fn is_terminator(&self) -> bool {
        matches!(self, Self::Br | Self::CondBr | Self::Switch | Self::Ret | Self::Unreachable)
    }

    pub fn is_branch(&self) -> bool {
        matches!(self, Self::Br | Self::CondBr | Self::Switch)
    }

    pub fn is_call(&self) -> bool {
        matches!(self, Self::Exec | Self::Call | Self::Syscall)
    }

    pub fn is_commutative(&self) -> bool {
        matches!(
            self,
            Self::Add
                | Self::Mul
                | Self::Min
                | Self::Max
                | Self::Eq
                | Self::Neq
                | Self::And
                | Self::Band
                | Self::Or
                | Self::Bor
                | Self::Xor
                | Self::Bxor
        )
    }

    pub fn is_assertion(&self) -> bool {
        matches!(self, Self::Assert | Self::Assertz | Self::AssertEq)
    }

    pub fn reads_memory(&self) -> bool {
        matches!(
            self,
            Self::MemGrow
                | Self::MemCpy
                | Self::MemSize
                | Self::Load
                | Self::Exec
                | Self::Syscall
                | Self::InlineAsm
                | Self::Reload
        )
    }

    pub fn writes_memory(&self) -> bool {
        matches!(
            self,
            Self::MemGrow
                | Self::MemSet
                | Self::MemCpy
                | Self::Store
                | Self::Exec
                | Self::Syscall
                | Self::InlineAsm
                | Self::Spill
        )
    }

    pub fn has_side_effects(&self) -> bool {
        match self {
            // These opcodes are all effectful
            Self::Assert
            | Self::Assertz
            | Self::AssertEq
            | Self::Store
            | Self::Alloca
            | Self::MemGrow
            | Self::MemSet
            | Self::MemCpy
            | Self::Exec
            | Self::Call
            | Self::Syscall
            | Self::Br
            | Self::CondBr
            | Self::Switch
            | Self::Ret
            | Self::Unreachable
            | Self::InlineAsm
            | Self::Spill
            | Self::Reload => true,
            // These opcodes are not
            Self::ImmI1
            | Self::ImmU8
            | Self::ImmI8
            | Self::ImmU16
            | Self::ImmI16
            | Self::ImmU32
            | Self::ImmI32
            | Self::ImmU64
            | Self::ImmI64
            | Self::ImmU128
            | Self::ImmI128
            | Self::ImmFelt
            | Self::ImmF64
            | Self::MemSize
            | Self::GlobalValue
            | Self::Load
            | Self::PtrToInt
            | Self::IntToPtr
            | Self::Cast
            | Self::Bitcast
            | Self::Trunc
            | Self::Zext
            | Self::Sext
            | Self::Test
            | Self::Select
            | Self::Add
            | Self::Sub
            | Self::Mul
            | Self::Div
            | Self::Mod
            | Self::DivMod
            | Self::Neg
            | Self::Inv
            | Self::Incr
            | Self::Ilog2
            | Self::Pow2
            | Self::Exp
            | Self::Not
            | Self::Bnot
            | Self::And
            | Self::Band
            | Self::Or
            | Self::Bor
            | Self::Xor
            | Self::Bxor
            | Self::Shl
            | Self::Shr
            | Self::Rotl
            | Self::Rotr
            | Self::Popcnt
            | Self::Clz
            | Self::Ctz
            | Self::Clo
            | Self::Cto
            | Self::Eq
            | Self::Neq
            | Self::Gt
            | Self::Gte
            | Self::Lt
            | Self::Lte
            | Self::IsOdd
            | Self::Min
            | Self::Max => false,
        }
    }

    pub fn num_fixed_args(&self) -> usize {
        match self {
            Self::Assert | Self::Assertz => 1,
            Self::AssertEq => 2,
            // Immediates/constants have none
            Self::ImmI1
            | Self::ImmU8
            | Self::ImmI8
            | Self::ImmU16
            | Self::ImmI16
            | Self::ImmU32
            | Self::ImmI32
            | Self::ImmU64
            | Self::ImmI64
            | Self::ImmU128
            | Self::ImmI128
            | Self::ImmFelt
            | Self::ImmF64 => 0,
            // Binary ops always have two
            Self::Store
            | Self::Add
            | Self::Sub
            | Self::Mul
            | Self::Div
            | Self::Mod
            | Self::DivMod
            | Self::Exp
            | Self::And
            | Self::Band
            | Self::Or
            | Self::Bor
            | Self::Xor
            | Self::Bxor
            | Self::Shl
            | Self::Shr
            | Self::Rotl
            | Self::Rotr
            | Self::Eq
            | Self::Neq
            | Self::Gt
            | Self::Gte
            | Self::Lt
            | Self::Lte
            | Self::Min
            | Self::Max => 2,
            // Unary ops always have one
            Self::MemGrow
            | Self::Load
            | Self::PtrToInt
            | Self::IntToPtr
            | Self::Cast
            | Self::Bitcast
            | Self::Trunc
            | Self::Zext
            | Self::Sext
            | Self::Test
            | Self::Neg
            | Self::Inv
            | Self::Incr
            | Self::Ilog2
            | Self::Pow2
            | Self::Popcnt
            | Self::Clz
            | Self::Ctz
            | Self::Clo
            | Self::Cto
            | Self::Not
            | Self::Bnot
            | Self::IsOdd => 1,
            // Select requires condition, arg1, and arg2
            Self::Select => 3,
            // memset requires destination, arity, and value
            // memcpy requires source, destination, and arity
            Self::MemSet | Self::MemCpy => 3,
            // Calls are entirely variable
            Self::Exec | Self::Call | Self::Syscall => 0,
            // Unconditional branches have no fixed arguments
            Self::Br => 0,
            // Ifs have a single argument, the conditional
            Self::CondBr => 1,
            // Switches have a single argument, the input value
            Self::Switch => 1,
            // Returns require at least one argument
            Self::Ret => 1,
            // The following require no arguments
            Self::MemSize
            | Self::GlobalValue
            | Self::Alloca
            | Self::Unreachable
            | Self::InlineAsm => 0,
            // Spills/reloads take a single argument
            Self::Spill | Self::Reload => 1,
        }
    }

    pub(super) fn results(&self, overflow: Option<Overflow>, ctrl_ty: Type) -> SmallVec<[Type; 1]> {
        use smallvec::smallvec;

        match self {
            // These ops have no results
            Self::Assert
            | Self::Assertz
            | Self::AssertEq
            | Self::Store
            | Self::MemSet
            | Self::MemCpy
            | Self::Br
            | Self::CondBr
            | Self::Switch
            | Self::Ret
            | Self::Unreachable
            | Self::Spill => smallvec![],
            // These ops have fixed result types
            Self::Test
            | Self::IsOdd
            | Self::Not
            | Self::And
            | Self::Or
            | Self::Xor
            | Self::Eq
            | Self::Neq
            | Self::Gt
            | Self::Gte
            | Self::Lt
            | Self::Lte => smallvec![Type::I1],
            // For these ops, the controlling type variable determines the type for the op
            Self::ImmI1
            | Self::ImmU8
            | Self::ImmI8
            | Self::ImmU16
            | Self::ImmI16
            | Self::ImmU32
            | Self::ImmI32
            | Self::ImmU64
            | Self::ImmI64
            | Self::ImmU128
            | Self::ImmI128
            | Self::ImmFelt
            | Self::ImmF64
            | Self::GlobalValue
            | Self::Alloca
            | Self::PtrToInt
            | Self::IntToPtr
            | Self::Cast
            | Self::Bitcast
            | Self::Trunc
            | Self::Zext
            | Self::Sext
            | Self::Select
            | Self::Div
            | Self::Min
            | Self::Max
            | Self::Neg
            | Self::Inv
            | Self::Pow2
            | Self::Mod
            | Self::DivMod
            | Self::Exp
            | Self::Bnot
            | Self::Band
            | Self::Bor
            | Self::Bxor
            | Self::Rotl
            | Self::Rotr
            | Self::MemGrow
            | Self::MemSize
            | Self::Reload => {
                smallvec![ctrl_ty]
            }
            // These ops always return a usize/u32 type
            Self::Ilog2 | Self::Popcnt | Self::Clz | Self::Clo | Self::Ctz | Self::Cto => {
                smallvec![Type::U32]
            }
            // These ops have overflowing variants which returns an additional result in that case
            Self::Add | Self::Sub | Self::Mul | Self::Incr | Self::Shl | Self::Shr => {
                match overflow {
                    Some(Overflow::Overflowing) => smallvec![Type::I1, ctrl_ty],
                    _ => smallvec![ctrl_ty],
                }
            }
            // The result type of a load is derived from the pointee type
            Self::Load => {
                smallvec![ctrl_ty.pointee().expect("expected pointer type").clone()]
            }
            // Call results are handled separately
            Self::Exec | Self::Call | Self::Syscall | Self::InlineAsm => unreachable!(),
        }
    }
}
impl fmt::Display for Opcode {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Assert => f.write_str("assert"),
            Self::Assertz => f.write_str("assertz"),
            Self::AssertEq => f.write_str("assert.eq"),
            Self::ImmI1 => f.write_str("const.i1"),
            Self::ImmU8 => f.write_str("const.u8"),
            Self::ImmI8 => f.write_str("const.i8"),
            Self::ImmU16 => f.write_str("const.u16"),
            Self::ImmI16 => f.write_str("const.i16"),
            Self::ImmU32 => f.write_str("const.u32"),
            Self::ImmI32 => f.write_str("const.i32"),
            Self::ImmU64 => f.write_str("const.u64"),
            Self::ImmI64 => f.write_str("const.i64"),
            Self::ImmU128 => f.write_str("const.u128"),
            Self::ImmI128 => f.write_str("const.i128"),
            Self::ImmFelt => f.write_str("const.felt"),
            Self::ImmF64 => f.write_str("const.f64"),
            Self::GlobalValue => f.write_str("global"),
            Self::Alloca => f.write_str("alloca"),
            Self::MemGrow => f.write_str("memory.grow"),
            Self::MemSize => f.write_str("memory.size"),
            Self::Load => f.write_str("load"),
            Self::Store => f.write_str("store"),
            Self::MemSet => f.write_str("memset"),
            Self::MemCpy => f.write_str("memcpy"),
            Self::PtrToInt => f.write_str("ptrtoint"),
            Self::IntToPtr => f.write_str("inttoptr"),
            Self::Cast => f.write_str("cast"),
            Self::Bitcast => f.write_str("bitcast"),
            Self::Trunc => f.write_str("trunc"),
            Self::Zext => f.write_str("zext"),
            Self::Sext => f.write_str("sext"),
            Self::Br => f.write_str("br"),
            Self::CondBr => f.write_str("condbr"),
            Self::Switch => f.write_str("switch"),
            Self::Exec => f.write_str("exec"),
            Self::Call => f.write_str("call"),
            Self::Syscall => f.write_str("syscall"),
            Self::Ret => f.write_str("ret"),
            Self::Test => f.write_str("test"),
            Self::Select => f.write_str("select"),
            Self::Add => f.write_str("add"),
            Self::Sub => f.write_str("sub"),
            Self::Mul => f.write_str("mul"),
            Self::Div => f.write_str("div"),
            Self::Mod => f.write_str("mod"),
            Self::DivMod => f.write_str("divmod"),
            Self::Exp => f.write_str("exp"),
            Self::Neg => f.write_str("neg"),
            Self::Inv => f.write_str("inv"),
            Self::Incr => f.write_str("incr"),
            Self::Ilog2 => f.write_str("ilog2"),
            Self::Pow2 => f.write_str("pow2"),
            Self::Not => f.write_str("not"),
            Self::Bnot => f.write_str("bnot"),
            Self::And => f.write_str("and"),
            Self::Band => f.write_str("band"),
            Self::Or => f.write_str("or"),
            Self::Bor => f.write_str("bor"),
            Self::Xor => f.write_str("xor"),
            Self::Bxor => f.write_str("bxor"),
            Self::Shl => f.write_str("shl"),
            Self::Shr => f.write_str("shr"),
            Self::Rotl => f.write_str("rotl"),
            Self::Rotr => f.write_str("rotr"),
            Self::Popcnt => f.write_str("popcnt"),
            Self::Clz => f.write_str("clz"),
            Self::Ctz => f.write_str("ctz"),
            Self::Clo => f.write_str("clo"),
            Self::Cto => f.write_str("cto"),
            Self::Eq => f.write_str("eq"),
            Self::Neq => f.write_str("neq"),
            Self::Gt => f.write_str("gt"),
            Self::Gte => f.write_str("gte"),
            Self::Lt => f.write_str("lt"),
            Self::Lte => f.write_str("lte"),
            Self::IsOdd => f.write_str("is_odd"),
            Self::Min => f.write_str("min"),
            Self::Max => f.write_str("max"),
            Self::Unreachable => f.write_str("unreachable"),
            Self::InlineAsm => f.write_str("asm"),
            Self::Spill => f.write_str("spill"),
            Self::Reload => f.write_str("reload"),
        }
    }
}

/// This enumeration represents the various ways in which arithmetic operations
/// can be configured to behave when either the operands or results over/underflow
/// the range of the integral type.
///
/// Always check the documentation of the specific instruction involved to see if there
/// are any specific differences in how this enum is interpreted compared to the default
/// meaning of each variant.
#[derive(Copy, Clone, Default, Debug, PartialEq, Eq)]
pub enum Overflow {
    /// Typically, this means the operation is performed using the equivalent field element
    /// operation, rather than a dedicated operation for the given type. Because of this, the
    /// result of the operation may exceed that of the integral type expected, but this will
    /// not be caught right away.
    ///
    /// It is the callers responsibility to ensure that resulting value is in range.
    #[default]
    Unchecked,
    /// The operation will trap if the operands, or the result, is not valid for the range of the
    /// integral type involved, e.g. u32.
    Checked,
    /// The operation will wrap around, depending on the range of the integral type. For example,
    /// given a u32 value, this is done by applying `mod 2^32` to the result.
    Wrapping,
    /// The result of the operation will be computed as in [Wrapping], however in addition to the
    /// result, this variant also pushes a value on the stack which represents whether or not the
    /// operation over/underflowed; either 1 if over/underflow occurred, or 0 otherwise.
    Overflowing,
}
impl Overflow {
    /// Returns true if overflow is unchecked
    pub fn is_unchecked(&self) -> bool {
        matches!(self, Self::Unchecked)
    }

    /// Returns true if overflow will cause a trap
    pub fn is_checked(&self) -> bool {
        matches!(self, Self::Checked)
    }

    /// Returns true if overflow will add an extra boolean on top of the stack
    pub fn is_overflowing(&self) -> bool {
        matches!(self, Self::Overflowing)
    }
}
impl fmt::Display for Overflow {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Unchecked => f.write_str("unchecked"),
            Self::Checked => f.write_str("checked"),
            Self::Wrapping => f.write_str("wrapping"),
            Self::Overflowing => f.write_str("overflow"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct GlobalValueOp {
    pub op: Opcode,
    pub global: GlobalValue,
}

#[derive(Debug, Clone)]
pub struct LocalVarOp {
    pub op: Opcode,
    pub local: LocalId,
    pub args: ValueList,
}

#[derive(Debug, Clone)]
pub struct BinaryOp {
    pub op: Opcode,
    pub overflow: Option<Overflow>,
    /// NOTE: These arguments are in stack order, i.e. `a + b` will appear here as `[b, a]`
    pub args: [Value; 2],
}

#[derive(Debug, Clone)]
pub struct BinaryOpImm {
    pub op: Opcode,
    pub overflow: Option<Overflow>,
    pub arg: Value,
    pub imm: Immediate,
}

#[derive(Debug, Clone)]
pub struct UnaryOp {
    pub op: Opcode,
    pub overflow: Option<Overflow>,
    pub arg: Value,
}

#[derive(Debug, Clone)]
pub struct UnaryOpImm {
    pub op: Opcode,
    pub overflow: Option<Overflow>,
    pub imm: Immediate,
}

#[derive(Debug, Clone)]
pub struct Call {
    pub op: Opcode,
    pub callee: FunctionIdent,
    /// NOTE: Call arguments are always in stack order, i.e. the top operand on
    /// the stack is the first function argument
    pub args: ValueList,
}

/// Branch
#[derive(Debug, Clone)]
pub struct Br {
    pub op: Opcode,
    pub successor: Successor,
}

/// Conditional Branch
#[derive(Debug, Clone)]
pub struct CondBr {
    pub op: Opcode,
    pub cond: Value,
    pub then_dest: Successor,
    pub else_dest: Successor,
}

/// Multi-way Branch w/Selector
#[derive(Debug, Clone)]
pub struct Switch {
    pub op: Opcode,
    pub arg: Value,
    pub arms: Vec<SwitchArm>,
    pub default: Successor,
}

#[derive(Debug, Clone)]
pub struct SwitchArm {
    pub value: u32,
    pub successor: Successor,
}

#[derive(Debug, Clone)]
pub struct Successor {
    pub destination: Block,
    /// NOTE: Block arguments are always in stack order, i.e. the top operand on
    /// the stack is the first block argument
    pub args: ValueList,
}
impl Successor {
    pub fn deep_clone(&self, pool: &mut ValueListPool) -> Self {
        Self {
            destination: self.destination,
            args: self.args.deep_clone(pool),
        }
    }
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct SuccessorInfo<'a> {
    pub destination: Block,
    pub args: &'a [Value],
}
impl<'a> SuccessorInfo<'a> {
    pub fn new(successor: &Successor, pool: &'a ValueListPool) -> Self {
        Self {
            destination: successor.destination,
            args: successor.args.as_slice(pool),
        }
    }
}
impl formatter::PrettyPrint for SuccessorInfo<'_> {
    fn render(&self) -> miden_core::prettier::Document {
        use crate::formatter::*;

        if self.args.is_empty() {
            return self.destination.render();
        }

        let args = self
            .args
            .iter()
            .copied()
            .map(display)
            .reduce(|acc, arg| acc + const_text(" ") + arg)
            .map(|args| const_text(" ") + args)
            .unwrap_or_default();
        const_text("(")
            + const_text("block")
            + const_text(" ")
            + display(self.destination.as_u32())
            + args
            + const_text(")")
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct SuccessorInfoMut<'a> {
    pub destination: Block,
    pub args: &'a mut [Value],
}
impl<'a> SuccessorInfoMut<'a> {
    pub fn new(successor: &'a mut Successor, pool: &'a mut ValueListPool) -> Self {
        Self {
            destination: successor.destination,
            args: successor.args.as_mut_slice(pool),
        }
    }
}

/// Return
#[derive(Debug, Clone)]
pub struct Ret {
    pub op: Opcode,
    /// NOTE: Return arguments are always in stack order, i.e. `ret a, b`
    /// will appear on the stack as `[a, b]`
    pub args: ValueList,
}

/// Return an immediate
#[derive(Debug, Clone)]
pub struct RetImm {
    pub op: Opcode,
    pub arg: Immediate,
}

/// Test
#[derive(Debug, Clone)]
pub struct Test {
    pub op: Opcode,
    pub arg: Value,
    pub ty: Type,
}

/// Load a value of type `ty` from `addr`
#[derive(Debug, Clone)]
pub struct LoadOp {
    pub op: Opcode,
    pub addr: Value,
    pub ty: Type,
}

/// A primop/intrinsic that takes a variable number of arguments
#[derive(Debug, Clone)]
pub struct PrimOp {
    pub op: Opcode,
    /// NOTE: Primops should be defined such that their arguments are in stack order
    /// when they correspond to a MASM instruction, i.e. `assert_eq a, b` should appear `[b, a]`
    pub args: ValueList,
}

/// A primop that takes an immediate for its first argument, followed by a variable number of
/// arguments
#[derive(Debug, Clone)]
pub struct PrimOpImm {
    pub op: Opcode,
    pub imm: Immediate,
    /// NOTE: Primops should be defined such that their arguments are in stack order
    /// when they correspond to a MASM instruction, i.e. `assert_eq a, b` should appear `[b, a]`,
    /// not counting the immediate argument
    pub args: ValueList,
}

#[doc(hidden)]
pub struct InstructionWithValueListPool<'a> {
    pub inst: &'a Instruction,
    pub value_lists: &'a ValueListPool,
}
impl PartialEq for InstructionWithValueListPool<'_> {
    fn eq(&self, other: &Self) -> bool {
        if core::mem::discriminant(self.inst) != core::mem::discriminant(other.inst) {
            return false;
        }

        if self.inst.opcode() != other.inst.opcode() {
            return false;
        }

        match (self.inst, other.inst) {
            (Instruction::GlobalValue(l), Instruction::GlobalValue(r)) => l.global == r.global,
            (Instruction::BinaryOp(l), Instruction::BinaryOp(r)) => {
                l.overflow == r.overflow && l.args == r.args
            }
            (Instruction::BinaryOpImm(l), Instruction::BinaryOpImm(r)) => {
                l.arg == r.arg && l.imm == r.imm && l.overflow == r.overflow
            }
            (Instruction::UnaryOp(l), Instruction::UnaryOp(r)) => {
                l.arg == r.arg && l.overflow == r.overflow
            }
            (Instruction::UnaryOpImm(l), Instruction::UnaryOpImm(r)) => {
                l.imm == r.imm && l.overflow == r.overflow
            }
            (Instruction::Call(l), Instruction::Call(r)) => {
                l.callee == r.callee
                    && l.args.as_slice(self.value_lists) == r.args.as_slice(self.value_lists)
            }
            (Instruction::Br(l), Instruction::Br(r)) => {
                let l = SuccessorInfo::new(&l.successor, self.value_lists);
                let r = SuccessorInfo::new(&r.successor, self.value_lists);
                l == r
            }
            (Instruction::CondBr(l), Instruction::CondBr(r)) => {
                let l_then = SuccessorInfo::new(&l.then_dest, self.value_lists);
                let l_else = SuccessorInfo::new(&l.else_dest, self.value_lists);
                let r_then = SuccessorInfo::new(&r.then_dest, self.value_lists);
                let r_else = SuccessorInfo::new(&r.else_dest, self.value_lists);
                l.cond == r.cond && l_then == r_then && l_else == r_else
            }
            (Instruction::Switch(l), Instruction::Switch(r)) => {
                if l.arg != r.arg {
                    return false;
                }
                let l_arms = BTreeSet::from_iter(
                    l.arms.iter().map(|arm| SuccessorInfo::new(&arm.successor, self.value_lists)),
                );
                let r_arms = BTreeSet::from_iter(
                    r.arms.iter().map(|arm| SuccessorInfo::new(&arm.successor, self.value_lists)),
                );
                let same_arms = l_arms == r_arms;
                let same_default = SuccessorInfo::new(&l.default, self.value_lists)
                    == SuccessorInfo::new(&r.default, self.value_lists);
                same_arms && same_default
            }
            (Instruction::Ret(l), Instruction::Ret(r)) => {
                l.args.as_slice(self.value_lists) == r.args.as_slice(other.value_lists)
            }
            (Instruction::RetImm(l), Instruction::RetImm(r)) => l.arg == r.arg,
            (Instruction::Load(l), Instruction::Load(r)) => l.addr == r.addr && l.ty == r.ty,
            (Instruction::PrimOp(l), Instruction::PrimOp(r)) => {
                l.args.as_slice(self.value_lists) == r.args.as_slice(other.value_lists)
            }
            (Instruction::PrimOpImm(l), Instruction::PrimOpImm(r)) => {
                l.imm == r.imm
                    && l.args.as_slice(self.value_lists) == r.args.as_slice(other.value_lists)
            }
            (Instruction::Test(l), Instruction::Test(r)) => l.arg == r.arg && l.ty == r.ty,
            (Instruction::InlineAsm(l), Instruction::InlineAsm(r)) => {
                l.args.as_slice(self.value_lists) == r.args.as_slice(other.value_lists)
                    && l.results == r.results
                    && l.body == r.body
                    && l.blocks == r.blocks
            }
            (..) => unreachable!(),
        }
    }
}

#[doc(hidden)]
pub struct InstPrettyPrinter<'a> {
    pub current_function: FunctionIdent,
    pub id: Inst,
    pub dfg: &'a DataFlowGraph,
}
impl fmt::Display for InstPrettyPrinter<'_> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.pretty_print(f)
    }
}
impl formatter::PrettyPrint for InstPrettyPrinter<'_> {
    fn render(&self) -> formatter::Document {
        use crate::formatter::*;

        let inst_data = self.dfg.inst(self.id);
        let mut results = vec![];
        for result in self.dfg.inst_results(self.id) {
            let v = const_text("(") + display(*result) + const_text(" ");
            let t = text(format!("{}", self.dfg.value_type(*result)));
            results.push(v + t + const_text(")"));
        }

        let wrapper = match results.len() {
            0 => None,
            1 => Some(
                const_text("(")
                    + const_text("let")
                    + const_text(" ")
                    + results.pop().unwrap()
                    + const_text(" "),
            ),
            _ => {
                let open = const_text("(") + const_text("let") + const_text(" ") + const_text("[");
                let bindings =
                    results.into_iter().reduce(|acc, doc| acc + const_text(" ") + doc).unwrap();
                let close = const_text("]") + const_text(" ");
                Some(open + bindings + close)
            }
        };

        let inner = const_text("(") + display(inst_data.opcode());

        let (attrs, operands) = match inst_data {
            Instruction::BinaryOp(BinaryOp {
                overflow: None,
                args,
                ..
            }) => {
                let lhs = display(args[1]);
                let rhs = display(args[0]);
                (vec![], vec![lhs, rhs])
            }
            Instruction::BinaryOp(BinaryOp {
                overflow: Some(overflow),
                args,
                ..
            }) => {
                let lhs = display(args[1]);
                let rhs = display(args[0]);
                (vec![display(*overflow)], vec![lhs, rhs])
            }
            Instruction::BinaryOpImm(BinaryOpImm {
                overflow: None,
                arg,
                imm,
                ..
            }) => {
                let lhs = display(*arg);
                let rhs = display(*imm);
                (vec![], vec![lhs, rhs])
            }
            Instruction::BinaryOpImm(BinaryOpImm {
                overflow: Some(overflow),
                arg,
                imm,
                ..
            }) => {
                let lhs = display(*arg);
                let rhs = display(*imm);
                (vec![display(*overflow)], vec![lhs, rhs])
            }
            Instruction::UnaryOp(UnaryOp {
                overflow: None,
                arg,
                ..
            }) => (vec![], vec![display(*arg)]),
            Instruction::UnaryOp(UnaryOp {
                overflow: Some(overflow),
                arg,
                ..
            }) => (vec![display(*overflow)], vec![display(*arg)]),
            Instruction::UnaryOpImm(UnaryOpImm {
                overflow: None,
                imm,
                ..
            }) => (vec![], vec![display(*imm)]),
            Instruction::UnaryOpImm(UnaryOpImm {
                overflow: Some(overflow),
                imm,
                ..
            }) => (vec![display(*overflow)], vec![display(*imm)]),
            Instruction::Ret(Ret { args, .. }) => {
                let args =
                    args.as_slice(&self.dfg.value_lists).iter().copied().map(display).collect();
                (vec![], args)
            }
            Instruction::RetImm(RetImm { arg, .. }) => (vec![], vec![display(*arg)]),
            Instruction::Call(Call { callee, args, .. }) => {
                let mut operands = if callee.module == self.current_function.module {
                    vec![display(callee.function)]
                } else {
                    vec![
                        const_text("(")
                            + display(callee.module)
                            + const_text(" ")
                            + display(callee.function)
                            + const_text(")"),
                    ]
                };
                operands.extend(args.as_slice(&self.dfg.value_lists).iter().copied().map(display));
                (vec![], operands)
            }
            Instruction::CondBr(CondBr {
                cond,
                ref then_dest,
                ref else_dest,
                ..
            }) => (
                vec![],
                vec![
                    display(*cond),
                    SuccessorInfo::new(then_dest, &self.dfg.value_lists).render(),
                    SuccessorInfo::new(else_dest, &self.dfg.value_lists).render(),
                ],
            ),
            Instruction::Br(Br { ref successor, .. }) => {
                (vec![], vec![SuccessorInfo::new(successor, &self.dfg.value_lists).render()])
            }
            Instruction::Switch(Switch {
                arg,
                arms,
                ref default,
                ..
            }) => {
                let default = const_text("(")
                    + const_text("_")
                    + const_text(" ")
                    + const_text(".")
                    + const_text(" ")
                    + SuccessorInfo::new(default, &self.dfg.value_lists).render()
                    + const_text(")");
                let arms = arms
                    .iter()
                    .map(|arm| {
                        const_text("(")
                            + display(arm.value)
                            + const_text(" ")
                            + const_text(".")
                            + const_text(" ")
                            + SuccessorInfo::new(&arm.successor, &self.dfg.value_lists).render()
                            + const_text(")")
                    })
                    .chain(core::iter::once(default))
                    .reduce(|acc, arm| acc + nl() + arm)
                    .unwrap();
                return inner + display(*arg) + indent(4, nl() + arms) + const_text(")");
            }
            Instruction::Test(Test { arg, ref ty, .. }) => {
                (vec![text(format!("{}", ty))], vec![display(*arg)])
            }
            Instruction::PrimOp(PrimOp { args, .. }) => {
                let args =
                    args.as_slice(&self.dfg.value_lists).iter().copied().map(display).collect();
                (vec![], args)
            }
            Instruction::PrimOpImm(PrimOpImm { imm, args, .. }) => {
                let mut operands = vec![display(*imm)];
                operands.extend(args.as_slice(&self.dfg.value_lists).iter().copied().map(display));
                (vec![], operands)
            }
            Instruction::Load(LoadOp { addr, .. }) => (vec![], vec![display(*addr)]),
            Instruction::InlineAsm(ref asm) => {
                let inner = asm.render(self.current_function, self.dfg);
                return match wrapper {
                    None => inner,
                    Some(wrapper) => wrapper + inner + const_text(")"),
                };
            }
            Instruction::LocalVar(LocalVarOp { local, args, .. }) => {
                let args = core::iter::once(display(local))
                    .chain(args.as_slice(&self.dfg.value_lists).iter().copied().map(display))
                    .collect();
                (vec![const_text("local")], args)
            }
            Instruction::GlobalValue(GlobalValueOp { global, .. }) => {
                let inner = self.dfg.global_value(*global).render(self.dfg);
                return match wrapper {
                    None => inner,
                    Some(wrapper) => wrapper + inner + const_text(")"),
                };
            }
        };

        let inner = attrs.into_iter().fold(inner, |acc, attr| acc + const_text(".") + attr);
        let inner =
            operands.into_iter().fold(inner, |acc, operand| acc + const_text(" ") + operand)
                + const_text(")");

        match wrapper {
            None => inner,
            Some(wrapper) => wrapper + inner + const_text(")"),
        }
    }
}
