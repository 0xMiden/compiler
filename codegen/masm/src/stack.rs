use core::{
    fmt,
    ops::{Index, IndexMut},
};

use miden_core::{Felt, FieldElement};
use midenc_hir::{AttributeValue, Immediate, Type, ValueRef};
use smallvec::{SmallVec, smallvec};

/// This represents a constraint an operand's usage at
/// a given program point, namely when used as an instruction
/// or block argument.
#[derive(Debug, Copy, Clone)]
pub enum Constraint {
    /// The operand should be moved, consuming it
    /// from the stack and making it unavailable for
    /// further use.
    Move,
    /// The operand should be copied, preserving the
    /// original value for later use.
    Copy,
}

/// Represents the type of operand represented on the operand stack
pub enum OperandType {
    /// The operand is a literal, unassociated with any value in the IR
    Const(Box<dyn AttributeValue>),
    /// The operand is an SSA value of known type
    Value(ValueRef),
    /// The operand is an intermediate runtime value of a known type, but
    /// unassociated with any value in the IR
    Type(Type),
}
impl Clone for OperandType {
    fn clone(&self) -> Self {
        match self {
            Self::Const(value) => Self::Const(value.clone_value()),
            Self::Value(value) => Self::Value(*value),
            Self::Type(ty) => Self::Type(ty.clone()),
        }
    }
}
impl OperandType {
    /// Get the type representation of this operand
    pub fn ty(&self) -> Type {
        match self {
            Self::Const(imm) => {
                imm.downcast_ref::<Immediate>().expect("unexpected constant value type").ty()
            }
            Self::Value(value) => value.borrow().ty().clone(),
            Self::Type(ty) => ty.clone(),
        }
    }
}
impl fmt::Debug for OperandType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Const(value) => write!(f, "Const({value:?})"),
            Self::Value(value) => write!(f, "Value({value})"),
            Self::Type(ty) => write!(f, "Type({ty})"),
        }
    }
}
impl Eq for OperandType {}
impl PartialEq for OperandType {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Value(a), Self::Value(b)) => a == b,
            (Self::Value(_), _) | (_, Self::Value(_)) => false,
            (Self::Const(a), Self::Const(b)) => a == b,
            (Self::Const(_), _) | (_, Self::Const(_)) => false,
            (Self::Type(a), Self::Type(b)) => a == b,
        }
    }
}
impl PartialEq<Type> for OperandType {
    fn eq(&self, other: &Type) -> bool {
        match self {
            Self::Type(a) => a == other,
            _ => false,
        }
    }
}
impl PartialEq<Immediate> for OperandType {
    fn eq(&self, other: &Immediate) -> bool {
        match self {
            Self::Const(a) => a.downcast_ref::<Immediate>().is_some_and(|a| a == other),
            _ => false,
        }
    }
}
impl PartialEq<dyn AttributeValue> for OperandType {
    fn eq(&self, other: &dyn AttributeValue) -> bool {
        match self {
            Self::Const(a) => a.as_ref() == other,
            _ => false,
        }
    }
}
impl PartialEq<ValueRef> for OperandType {
    fn eq(&self, other: &ValueRef) -> bool {
        match self {
            Self::Value(this) => this == other,
            _ => false,
        }
    }
}
impl From<ValueRef> for OperandType {
    fn from(value: ValueRef) -> Self {
        Self::Value(value)
    }
}
impl From<Type> for OperandType {
    fn from(ty: Type) -> Self {
        Self::Type(ty)
    }
}
impl From<Immediate> for OperandType {
    fn from(value: Immediate) -> Self {
        Self::Const(Box::new(value))
    }
}
impl From<Box<dyn AttributeValue>> for OperandType {
    fn from(value: Box<dyn AttributeValue>) -> Self {
        Self::Const(value)
    }
}

/// This type represents a logical operand on the stack, which may consist
/// of one or more "parts", up to a word in size, on the actual stack.
///
/// The [OperandStack] operates in terms of [Operand], but when emitting
/// Miden Assembly, we must know how to translate operand-oriented operations
/// into equivalent element-/word-oriented operations. This is accomplished
/// by tracking the low-level representation of a given operand in this struct.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Operand {
    /// The section of stack corresponding to this operand, containing
    /// up to a full word of elements. No chunk will ever exceed a word
    /// in size. This field behaves like a miniature [OperandStack], i.e.
    /// elements are pushed and popped off the end to modify it.
    ///
    /// An operand is encoded on this stack in order of lowest
    /// addressed bytes first. For example, given a struct operand,
    /// the first field of the struct will be closest to the top of
    /// the stack.
    word: SmallVec<[Type; 4]>,
    /// The high-level operand represented by this item.
    ///
    /// If the operand stack is manipulated in such a way that the operand
    /// is torn apart, say one field of a struct is popped; then this will
    /// be set to a `Type` operand, representing what high-level information
    /// we have about the remaining parts of the original operand on the stack.
    operand: OperandType,
}
impl Default for Operand {
    fn default() -> Self {
        Self {
            word: smallvec![Type::Felt],
            operand: OperandType::Const(Box::new(Immediate::Felt(Felt::ZERO))),
        }
    }
}
impl PartialEq<ValueRef> for Operand {
    #[inline(always)]
    fn eq(&self, other: &ValueRef) -> bool {
        self.operand.eq(other)
    }
}
impl PartialEq<dyn AttributeValue> for Operand {
    #[inline(always)]
    fn eq(&self, other: &dyn AttributeValue) -> bool {
        self.operand.eq(other)
    }
}
impl PartialEq<Immediate> for Operand {
    #[inline(always)]
    fn eq(&self, other: &Immediate) -> bool {
        self.operand.eq(other)
    }
}
impl PartialEq<Immediate> for &Operand {
    #[inline(always)]
    fn eq(&self, other: &Immediate) -> bool {
        self.operand.eq(other)
    }
}
impl PartialEq<Type> for Operand {
    #[inline(always)]
    fn eq(&self, other: &Type) -> bool {
        self.operand.eq(other)
    }
}
impl PartialEq<Type> for &Operand {
    #[inline(always)]
    fn eq(&self, other: &Type) -> bool {
        self.operand.eq(other)
    }
}
impl From<Immediate> for Operand {
    #[inline]
    fn from(imm: Immediate) -> Self {
        Self::new(imm.into())
    }
}
impl From<u32> for Operand {
    #[inline]
    fn from(imm: u32) -> Self {
        Self::new(Immediate::U32(imm).into())
    }
}
impl TryFrom<&Operand> for ValueRef {
    type Error = ();

    fn try_from(operand: &Operand) -> Result<Self, Self::Error> {
        match operand.operand {
            OperandType::Value(value) => Ok(value),
            _ => Err(()),
        }
    }
}
#[cfg(test)]
impl TryFrom<&Operand> for Immediate {
    type Error = ();

    fn try_from(operand: &Operand) -> Result<Self, Self::Error> {
        match &operand.operand {
            OperandType::Const(value) => value.downcast_ref::<Immediate>().copied().ok_or(()),
            _ => Err(()),
        }
    }
}
#[cfg(test)]
impl TryFrom<&Operand> for Type {
    type Error = ();

    fn try_from(operand: &Operand) -> Result<Self, Self::Error> {
        match operand.operand {
            OperandType::Type(ref ty) => Ok(ty.clone()),
            _ => Err(()),
        }
    }
}
#[cfg(test)]
impl TryFrom<Operand> for Type {
    type Error = ();

    fn try_from(operand: Operand) -> Result<Self, Self::Error> {
        match operand.operand {
            OperandType::Type(ty) => Ok(ty),
            _ => Err(()),
        }
    }
}
impl From<Type> for Operand {
    #[inline]
    fn from(ty: Type) -> Self {
        Self::new(OperandType::Type(ty))
    }
}
impl From<ValueRef> for Operand {
    #[inline]
    fn from(value: ValueRef) -> Self {
        Self::new(OperandType::Value(value))
    }
}
impl Operand {
    pub fn new(operand: OperandType) -> Self {
        let ty = operand.ty();
        let mut word = ty.to_raw_parts().expect("invalid operand type");
        assert!(!word.is_empty(), "invalid operand: must be a sized type");
        assert!(word.len() <= 4, "invalid operand: must be smaller than or equal to a word");
        if word.len() > 1 {
            word.reverse();
        }
        Self { word, operand }
    }

    /// Get the size of this operand in field elements
    pub fn size(&self) -> usize {
        self.word.len()
    }

    /// Get the [OperandType] representing the value of this operand
    #[inline(always)]
    pub fn value(&self) -> &OperandType {
        &self.operand
    }

    /// Get this operand as a [Value]
    #[inline]
    pub fn as_value(&self) -> Option<ValueRef> {
        self.try_into().ok()
    }

    /// Get the [Type] of this operand
    #[inline]
    pub fn ty(&self) -> Type {
        self.operand.ty()
    }
}

/// This structure emulates the state of the VM's operand stack while
/// generating code from the SSA representation of a function.
///
/// In order to emit efficient and correct stack manipulation code, we must be able to
/// reason about where values are on the operand stack at a given program point. This
/// structure tracks what SSA values have been pushed on the operand stack, where they are
/// on the stack relative to the top, and whether a given stack slot aliases multiple
/// values.
///
/// In addition to the state tracked, this structure also has an API that mimics the
/// stack manipulation instructions we can emit in the code generator, so that as we
/// emit instructions and modify this structure at the same time, 1:1.
#[derive(Clone, PartialEq, Eq)]
pub struct OperandStack {
    stack: Vec<Operand>,
}
impl Default for OperandStack {
    fn default() -> Self {
        Self {
            stack: Vec::with_capacity(16),
        }
    }
}
impl OperandStack {
    /// Renames the `n`th operand from the top of the stack to `value`
    ///
    /// The type is assumed to remain unchanged
    pub fn rename(&mut self, n: usize, value: ValueRef) {
        match &mut self[n].operand {
            OperandType::Value(prev_value) => {
                *prev_value = value;
            }
            prev => {
                *prev = OperandType::Value(value);
            }
        }
    }

    /// Searches for the position on the stack containing the operand corresponding to `value`.
    ///
    /// NOTE: This function will panic if `value` is not on the stack
    pub fn find(&self, value: &ValueRef) -> Option<usize> {
        self.stack.iter().rev().position(|v| v == value)
    }

    /// Returns true if the operand stack is empty
    #[allow(unused)]
    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.stack.is_empty()
    }

    /// Returns the number of field elements on the stack
    #[inline]
    pub fn raw_len(&self) -> usize {
        self.stack.iter().map(|operand| operand.size()).sum()
    }

    /// Returns the index in the actual runtime stack which corresponds to
    /// the first element of the operand at `index`.
    #[track_caller]
    pub fn effective_index(&self, index: usize) -> usize {
        assert!(
            index < self.stack.len(),
            "expected {} to be less than {}",
            index,
            self.stack.len()
        );

        self.stack.iter().rev().take(index).map(|o| o.size()).sum()
    }

    /// Returns the index in the actual runtime stack which corresponds to
    /// the last element of the operand at `index`.
    #[track_caller]
    pub fn effective_index_inclusive(&self, index: usize) -> usize {
        assert!(index < self.stack.len());

        self.stack.iter().rev().take(index + 1).map(|o| o.size()).sum::<usize>() - 1
    }

    /// Returns the number of operands on the stack
    #[inline]
    pub fn len(&self) -> usize {
        self.stack.len()
    }

    /// Returns the Some(operand) at the given index, without consuming it.
    /// If the index is out of bounds, returns None.
    #[inline]
    pub fn get(&self, index: usize) -> Option<&Operand> {
        let effective_len: usize = self.stack.iter().rev().take(index + 1).map(|o| o.size()).sum();
        assert!(
            effective_len <= 16,
            "invalid operand stack index ({index}): requires access to more than 16 elements, \
             which is not supported in Miden"
        );
        let len = self.stack.len();
        if index >= len {
            return None;
        }
        self.stack.get(len - index - 1)
    }

    /// Returns the operand on top of the stack, without consuming it
    #[inline]
    pub fn peek(&self) -> Option<&Operand> {
        self.stack.last()
    }

    /// Pushes an operand on top of the stack
    #[inline]
    pub fn push<V: Into<Operand>>(&mut self, value: V) {
        self.stack.push(value.into());
    }

    /// Pops the operand on top of the stack
    #[inline]
    #[track_caller]
    pub fn pop(&mut self) -> Option<Operand> {
        self.stack.pop()
    }

    /// Drops the top operand on the stack
    #[allow(clippy::should_implement_trait)]
    #[track_caller]
    pub fn drop(&mut self) {
        self.stack.pop().expect("operand stack is empty");
    }

    /// Drops the top `n` operands on the stack
    #[inline]
    #[track_caller]
    pub fn dropn(&mut self, n: usize) {
        let len = self.stack.len();
        assert!(n <= len, "unable to drop {n} operands, operand stack only has {len}");
        self.stack.truncate(len - n);
    }

    /// Duplicates the operand in the `n`th position on the stack
    ///
    /// If `n` is 0, duplicates the top of the stack.
    #[track_caller]
    pub fn dup(&mut self, n: usize) {
        let operand = self[n].clone();
        self.stack.push(operand);
    }

    /// Swaps the `n`th operand from the top of the stack, with the top of the stack
    ///
    /// If `n` is 1, it swaps the first two operands on the stack.
    ///
    /// NOTE: This function will panic if `n` is 0, or out of bounds.
    #[track_caller]
    pub fn swap(&mut self, n: usize) {
        assert_ne!(n, 0, "invalid swap, index must be in the range 1..=15");
        let len = self.stack.len();
        assert!(n < len, "invalid operand stack index ({n}), only {len} operands are available");
        let a = len - 1;
        let b = a - n;
        self.stack.swap(a, b);
    }

    /// Moves the `n`th operand to the top of the stack
    ///
    /// If `n` is 1, this is equivalent to `swap(1)`.
    ///
    /// NOTE: This function will panic if `n` is 0, or out of bounds.
    pub fn movup(&mut self, n: usize) {
        assert_ne!(n, 0, "invalid move, index must be in the range 1..=15");
        let len = self.stack.len();
        assert!(n < len, "invalid operand stack index ({n}), only {len} operands are available");
        // Pick the midpoint by counting backwards from the end
        let mid = len - (n + 1);
        // Split the stack, and rotate the half that
        // contains our desired value to place it on top.
        let (_, r) = self.stack.split_at_mut(mid);
        r.rotate_left(1);
    }

    /// Makes the operand on top of the stack, the `n`th operand on the stack
    ///
    /// If `n` is 1, this is equivalent to `swap(1)`.
    ///
    /// NOTE: This function will panic if `n` is 0, or out of bounds.
    pub fn movdn(&mut self, n: usize) {
        assert_ne!(n, 0, "invalid move, index must be in the range 1..=15");
        let len = self.stack.len();
        assert!(n < len, "invalid operand stack index ({n}), only {len} operands are available");
        // Split the stack so that the desired position is in the top half
        let mid = len - (n + 1);
        let (_, r) = self.stack.split_at_mut(mid);
        // Move all elements above the `n`th position up by one, moving the top element to the `n`th
        // position
        r.rotate_right(1);
    }

    #[allow(unused)]
    #[inline(always)]
    pub fn iter(&self) -> impl DoubleEndedIterator<Item = &Operand> {
        self.stack.iter()
    }
}
impl Index<usize> for OperandStack {
    type Output = Operand;

    #[track_caller]
    fn index(&self, index: usize) -> &Self::Output {
        let len = self.stack.len();
        assert!(
            index < len,
            "invalid operand stack index ({index}): only {len} operands are available"
        );
        let effective_len: usize = self.stack.iter().rev().take(index + 1).map(|o| o.size()).sum();
        assert!(
            effective_len <= 16,
            "invalid operand stack index ({index}): requires access to more than 16 elements, \
             which is not supported in Miden"
        );
        &self.stack[len - index - 1]
    }
}
impl IndexMut<usize> for OperandStack {
    #[track_caller]
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        let len = self.stack.len();
        assert!(
            index < len,
            "invalid operand stack index ({index}): only {len} elements are available"
        );
        let effective_len: usize = self.stack.iter().rev().take(index + 1).map(|o| o.size()).sum();
        assert!(
            effective_len <= 16,
            "invalid operand stack index ({index}): requires access to more than 16 elements, \
             which is not supported in Miden"
        );
        &mut self.stack[len - index - 1]
    }
}

impl fmt::Debug for OperandStack {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let mut builder = f.debug_list();
        for (index, value) in self.stack.iter().rev().enumerate() {
            builder.entry_with(|f| write!(f, "{index} => {value:?}"));
        }
        builder.finish()
    }
}

#[cfg(test)]
mod tests {
    use alloc::rc::Rc;

    use midenc_hir::{ArrayType, BuilderExt, Context, PointerType, StructType};

    use super::*;

    #[test]
    fn operand_stack_homogenous_operand_sizes_test() {
        let mut stack = OperandStack::default();

        let zero = Immediate::U32(0);
        let one = Immediate::U32(1);
        let two = Immediate::U32(2);
        let three = Immediate::U32(3);

        #[inline]
        #[allow(unused)]
        fn as_imms(word: [Operand; 4]) -> [Immediate; 4] {
            [
                (&word[0]).try_into().unwrap(),
                (&word[1]).try_into().unwrap(),
                (&word[2]).try_into().unwrap(),
                (&word[3]).try_into().unwrap(),
            ]
        }

        #[inline]
        fn as_imm(operand: Operand) -> Immediate {
            (&operand).try_into().unwrap()
        }

        // push
        stack.push(zero);
        stack.push(one);
        stack.push(two);
        stack.push(three);
        assert_eq!(stack.len(), 4);
        assert_eq!(stack[0], three);
        assert_eq!(stack[1], two);
        assert_eq!(stack[2], one);
        assert_eq!(stack[3], zero);

        // peek
        assert_eq!(stack.peek().unwrap(), three);

        // dup
        stack.dup(0);
        assert_eq!(stack.len(), 5);
        assert_eq!(stack[0], three);
        assert_eq!(stack[1], three);
        assert_eq!(stack[2], two);
        assert_eq!(stack[3], one);
        assert_eq!(stack[4], zero);

        stack.dup(3);
        assert_eq!(stack.len(), 6);
        assert_eq!(stack[0], one);
        assert_eq!(stack[1], three);
        assert_eq!(stack[2], three);
        assert_eq!(stack[3], two);
        assert_eq!(stack[4], one);
        assert_eq!(stack[5], zero);

        // drop
        stack.drop();
        assert_eq!(stack.len(), 5);
        assert_eq!(stack[0], three);
        assert_eq!(stack[1], three);
        assert_eq!(stack[2], two);
        assert_eq!(stack[3], one);
        assert_eq!(stack[4], zero);

        // swap
        stack.swap(2);
        assert_eq!(stack.len(), 5);
        assert_eq!(stack[0], two);
        assert_eq!(stack[1], three);
        assert_eq!(stack[2], three);
        assert_eq!(stack[3], one);
        assert_eq!(stack[4], zero);

        stack.swap(1);
        assert_eq!(stack.len(), 5);
        assert_eq!(stack[0], three);
        assert_eq!(stack[1], two);
        assert_eq!(stack[2], three);
        assert_eq!(stack[3], one);
        assert_eq!(stack[4], zero);

        // movup
        stack.movup(2);
        assert_eq!(stack.len(), 5);
        assert_eq!(stack[0], three);
        assert_eq!(stack[1], three);
        assert_eq!(stack[2], two);
        assert_eq!(stack[3], one);
        assert_eq!(stack[4], zero);

        // movdn
        stack.movdn(3);
        assert_eq!(stack.len(), 5);
        assert_eq!(stack[0], three);
        assert_eq!(stack[1], two);
        assert_eq!(stack[2], one);
        assert_eq!(stack[3], three);
        assert_eq!(stack[4], zero);

        // pop
        assert_eq!(stack.pop().map(as_imm), Some(three));
        assert_eq!(stack.len(), 4);
        assert_eq!(stack[0], two);
        assert_eq!(stack[1], one);
        assert_eq!(stack[2], three);
        assert_eq!(stack[3], zero);

        // dropn
        stack.dropn(2);
        assert_eq!(stack.len(), 2);
        assert_eq!(stack[0], three);
        assert_eq!(stack[1], zero);
    }

    #[test]
    fn operand_stack_values_test() {
        use midenc_dialect_hir::Load;

        let mut stack = OperandStack::default();
        let context = Rc::new(Context::default());

        let ptr_u8 = Type::from(PointerType::new(Type::U8));
        let array_u8 = Type::from(ArrayType::new(Type::U8, 4));
        let struct_ty = Type::from(StructType::new([Type::U64, Type::U8]));
        let block = context.create_block_with_params([ptr_u8, array_u8, Type::U32, struct_ty]);
        let block = block.borrow();
        let values = block.arguments();

        let zero = values[0] as ValueRef;
        let one = values[1] as ValueRef;
        let two = values[2] as ValueRef;
        let three = values[3] as ValueRef;

        drop(block);

        // push
        stack.push(zero);
        stack.push(one);
        stack.push(two);
        stack.push(three);
        assert_eq!(stack.len(), 4);
        assert_eq!(stack.raw_len(), 6);

        assert_eq!(stack.find(&zero), Some(3));
        assert_eq!(stack.find(&one), Some(2));
        assert_eq!(stack.find(&two), Some(1));
        assert_eq!(stack.find(&three), Some(0));

        // dup
        stack.dup(0);
        assert_eq!(stack.find(&three), Some(0));

        stack.dup(3);
        assert_eq!(stack.find(&one), Some(0));

        // drop
        stack.drop();
        assert_eq!(stack.find(&one), Some(3));
        assert_eq!(stack.find(&three), Some(0));
        assert_eq!(stack[1], three);

        // padw
        stack.push(Immediate::Felt(Felt::ZERO));
        stack.push(Immediate::Felt(Felt::ZERO));
        stack.push(Immediate::Felt(Felt::ZERO));
        stack.push(Immediate::Felt(Felt::ZERO));
        assert_eq!(stack.find(&one), Some(7));
        assert_eq!(stack.find(&three), Some(4));

        // rename
        let four = {
            let mut builder = midenc_hir::OpBuilder::new(context.clone());
            let load_builder = builder.create::<Load, _>(Default::default());
            let load = load_builder(zero).unwrap();
            load.borrow().result().as_value_ref()
        };
        stack.rename(1, four);
        assert_eq!(stack.find(&four), Some(1));
        assert_eq!(stack.find(&three), Some(4));

        // pop
        let top = stack.pop().unwrap();
        assert_eq!((&top).try_into(), Ok(Immediate::Felt(Felt::ZERO)));
        assert_eq!(stack.find(&four), Some(0));
        assert_eq!(stack[1], Immediate::Felt(Felt::ZERO));
        assert_eq!(stack[2], Immediate::Felt(Felt::ZERO));
        assert_eq!(stack.find(&three), Some(3));

        // dropn
        stack.dropn(3);
        assert_eq!(stack.find(&four), None);
        assert_eq!(stack.find(&three), Some(0));
        assert_eq!(stack[1], three);
        assert_eq!(stack.find(&two), Some(2));
        assert_eq!(stack.find(&one), Some(3));
        assert_eq!(stack.find(&zero), Some(4));

        // swap
        stack.swap(3);
        assert_eq!(stack.find(&one), Some(0));
        assert_eq!(stack.find(&three), Some(1));
        assert_eq!(stack.find(&two), Some(2));
        assert_eq!(stack[3], three);

        stack.swap(1);
        assert_eq!(stack.find(&three), Some(0));
        assert_eq!(stack.find(&one), Some(1));
        assert_eq!(stack.find(&two), Some(2));
        assert_eq!(stack.find(&zero), Some(4));

        // movup
        stack.movup(2);
        assert_eq!(stack.find(&two), Some(0));
        assert_eq!(stack.find(&three), Some(1));
        assert_eq!(stack.find(&one), Some(2));
        assert_eq!(stack.find(&zero), Some(4));

        // movdn
        stack.movdn(3);
        assert_eq!(stack.find(&three), Some(0));
        assert_eq!(stack.find(&one), Some(1));
        assert_eq!(stack[2], three);
        assert_eq!(stack.find(&two), Some(3));
        assert_eq!(stack.find(&zero), Some(4));
    }

    #[test]
    fn operand_stack_heterogenous_operand_sizes_test() {
        let mut stack = OperandStack::default();

        let zero = Immediate::U32(0);
        let one = Immediate::U32(1);
        let two = Type::U64;
        let three = Type::U64;
        let struct_a = Type::from(StructType::new([
            Type::from(PointerType::new(Type::U8)),
            Type::U16,
            Type::U32,
        ]));

        // push
        stack.push(zero);
        stack.push(one);
        stack.push(two.clone());
        stack.push(three.clone());
        stack.push(struct_a.clone());
        assert_eq!(stack.len(), 5);
        assert_eq!(stack.raw_len(), 9);
        assert_eq!(stack[0], struct_a);
        assert_eq!(stack[1], three);
        assert_eq!(stack[2], two);
        assert_eq!(stack[3], one);
        assert_eq!(stack[4], zero);

        // peek
        assert_eq!(stack.peek().unwrap(), struct_a);

        // dup
        stack.dup(0);
        assert_eq!(stack.len(), 6);
        assert_eq!(stack.raw_len(), 12);
        assert_eq!(stack[0], struct_a);
        assert_eq!(stack[1], struct_a);
        assert_eq!(stack[2], three);
        assert_eq!(stack[3], two);
        assert_eq!(stack[4], one);
        assert_eq!(stack[5], zero);
        assert_eq!(stack.effective_index(3), 8);

        stack.dup(3);
        assert_eq!(stack.len(), 7);
        assert_eq!(stack.raw_len(), 14);
        assert_eq!(stack[0], two);
        assert_eq!(stack[1], struct_a);
        assert_eq!(stack[2], struct_a);

        // drop
        stack.drop();
        assert_eq!(stack.len(), 6);
        assert_eq!(stack.raw_len(), 12);
        assert_eq!(stack[0], struct_a);
        assert_eq!(stack[1], struct_a);
        assert_eq!(stack[2], three);
        assert_eq!(stack[3], two);
        assert_eq!(stack[4], one);
        assert_eq!(stack[5], zero);

        // swap
        stack.swap(2);
        assert_eq!(stack.len(), 6);
        assert_eq!(stack.raw_len(), 12);
        assert_eq!(stack[0], three);
        assert_eq!(stack[1], struct_a);
        assert_eq!(stack[2], struct_a);
        assert_eq!(stack[3], two);
        assert_eq!(stack[4], one);

        stack.swap(1);
        assert_eq!(stack.len(), 6);
        assert_eq!(stack.raw_len(), 12);
        assert_eq!(stack[0], struct_a);
        assert_eq!(stack[1], three);
        assert_eq!(stack[2], struct_a);
        assert_eq!(stack[3], two);
        assert_eq!(stack[4], one);
        assert_eq!(stack[5], zero);

        // movup
        stack.movup(4);
        assert_eq!(stack.len(), 6);
        assert_eq!(stack.raw_len(), 12);
        assert_eq!(stack[0], one);
        assert_eq!(stack[1], struct_a);
        assert_eq!(stack[2], three);
        assert_eq!(stack[3], struct_a);
        assert_eq!(stack[4], two);
        assert_eq!(stack[5], zero);

        // movdn
        stack.movdn(3);
        assert_eq!(stack.len(), 6);
        assert_eq!(stack.raw_len(), 12);
        assert_eq!(stack[0], struct_a);
        assert_eq!(stack[1], three);
        assert_eq!(stack[2], struct_a);
        assert_eq!(stack[3], one);
        assert_eq!(stack[4], two);

        // pop
        let operand: Type = stack.pop().unwrap().try_into().unwrap();
        assert_eq!(operand, struct_a);
        assert_eq!(stack.len(), 5);
        assert_eq!(stack.raw_len(), 9);
        assert_eq!(stack[0], three);
        assert_eq!(stack[1], struct_a);
        assert_eq!(stack[2], one);
        assert_eq!(stack[3], two);

        // dropn
        stack.dropn(2);
        assert_eq!(stack.len(), 3);
        assert_eq!(stack.raw_len(), 4);
        assert_eq!(stack[0], one);
        assert_eq!(stack[1], two);
        assert_eq!(stack[2], zero);
    }
}
