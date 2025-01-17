use core::{
    fmt,
    ops::{Index, IndexMut},
};

use crate::{Felt, FieldElement, Type};

/// This trait is used to represent the basic plumbing of the operand stack in
/// Miden Assembly.
///
/// Implementations of this trait may attach different semantics to the meaning of
/// elements on the stack. As a result, certain operations which are contingent on the
/// specific value of an element, may behave differently depending on the specific
/// implementation.
///
/// In general however, it is expected that use of this trait in a generic context will
/// be rare, if ever the case. As mentioned above, it is meant to handle the common
/// plumbing of an operand stack implementation, but in practice users will be working
/// with a concrete implementation with this trait in scope to provide access to the
/// basic functionality of the stack.
///
/// It is expected that implementations will override functions in this trait as necessary
/// to implement custom behavior above and beyond what is provided by the default implementation.
pub trait Stack: IndexMut<usize, Output = <Self as Stack>::Element> {
    type Element: StackElement;

    /// Return a reference to the underlying "raw" stack data structure, a vector
    fn stack(&self) -> &Vec<Self::Element>;
    /// Return a mutable reference to the underlying "raw" stack data structure, a vector
    fn stack_mut(&mut self) -> &mut Vec<Self::Element>;

    /// Clear the contents of this stack while keeping the underlying
    /// memory allocated for reuse.
    fn clear(&mut self);

    /// Display this stack using its debugging representation
    fn debug(&self) -> DebugStack<Self> {
        DebugStack::new(self)
    }

    /// Returns true if the operand stack is empty
    #[inline(always)]
    fn is_empty(&self) -> bool {
        self.stack().is_empty()
    }

    /// Returns the number of elements on the stack
    #[inline]
    fn len(&self) -> usize {
        self.stack().len()
    }

    /// Returns the value on top of the stack, without consuming it
    #[inline]
    fn peek(&self) -> Option<Self::Element> {
        self.stack().last().cloned()
    }

    /// Returns the word on top of the stack, without consuming it
    ///
    /// The top of the stack will be the first element of the word
    #[inline]
    fn peekw(&self) -> Option<[Self::Element; 4]> {
        let stack = self.stack();
        let end = stack.len().checked_sub(1)?;
        Some([
            stack[end].clone(),
            stack[end - 1].clone(),
            stack[end - 2].clone(),
            stack[end - 3].clone(),
        ])
    }

    /// Pushes a word of zeroes on top of the stack
    fn padw(&mut self) {
        self.stack_mut().extend([
            Self::Element::DEFAULT,
            Self::Element::DEFAULT,
            Self::Element::DEFAULT,
            Self::Element::DEFAULT,
        ]);
    }

    /// Pushes `value` on top of the stac
    fn push(&mut self, value: Self::Element) {
        self.stack_mut().push(value);
    }

    /// Pushes `word` on top of the stack
    ///
    /// The first element of `word` will be on top of the stack
    fn pushw(&mut self, mut word: [Self::Element; 4]) {
        word.reverse();
        self.stack_mut().extend(word);
    }

    /// Pops the value on top of the stack
    fn pop(&mut self) -> Option<Self::Element> {
        self.stack_mut().pop()
    }

    /// Pops the first word on top of the stack
    ///
    /// The top of the stack will be the first element in the result
    fn popw(&mut self) -> Option<[Self::Element; 4]> {
        let stack = self.stack_mut();
        let a = stack.pop()?;
        let b = stack.pop()?;
        let c = stack.pop()?;
        let d = stack.pop()?;
        Some([a, b, c, d])
    }

    /// Drops the top item on the stack
    fn drop(&mut self) {
        self.dropn(1);
    }

    /// Drops the top word on the stack
    fn dropw(&mut self) {
        self.dropn(4);
    }

    #[inline]
    fn dropn(&mut self, n: usize) {
        let stack = self.stack_mut();
        let len = stack.len();
        assert!(n <= len, "unable to drop {} elements, operand stack only has {}", n, len);
        stack.truncate(len - n);
    }

    /// Duplicates the value in the `n`th position on the stack
    ///
    /// If `n` is 0, duplicates the top of the stack.
    fn dup(&mut self, n: usize) {
        let value = self[n].clone();
        self.stack_mut().push(value);
    }

    /// Duplicates the `n`th word on the stack, to the top of the stack.
    ///
    /// Valid values for `n` are 0, 1, 2, or 3.
    ///
    /// If `n` is 0, duplicates the top word of the stack.
    fn dupw(&mut self, n: usize) {
        assert!(n < 4, "invalid word index: must be in the range 0..=3");
        let len = self.stack().len();
        let index = n * 4;
        assert!(
            index < len,
            "invalid operand stack index ({}), only {} elements are available",
            index,
            len
        );
        match index {
            0 => {
                let word = self.peekw().expect("operand stack is empty");
                self.pushw(word);
            }
            n => {
                let end = len - n - 1;
                let word = {
                    let stack = self.stack();
                    [
                        stack[end].clone(),
                        stack[end - 1].clone(),
                        stack[end - 2].clone(),
                        stack[end - 3].clone(),
                    ]
                };
                self.pushw(word);
            }
        }
    }

    /// Swaps the `n`th value from the top of the stack, with the top of the stack
    ///
    /// If `n` is 1, it swaps the first two elements on the stack.
    ///
    /// NOTE: This function will panic if `n` is 0, or out of bounds.
    fn swap(&mut self, n: usize) {
        assert_ne!(n, 0, "invalid swap, index must be in the range 1..=15");
        let stack = self.stack_mut();
        let len = stack.len();
        assert!(
            n < len,
            "invalid operand stack index ({}), only {} elements are available",
            n,
            len
        );
        let a = len - 1;
        let b = a - n;
        stack.swap(a, b);
    }

    /// Swaps the `n`th word from the top of the stack, with the word on top of the stack
    ///
    /// If `n` is 1, it swaps the first two words on the stack.
    ///
    /// Valid values for `n` are: 1, 2, 3.
    fn swapw(&mut self, n: usize) {
        assert_ne!(n, 0, "invalid swap, index must be in the range 1..=3");
        let stack = self.stack_mut();
        let len = stack.len();
        let index = n * 4;
        assert!(
            index < len,
            "invalid operand stack index ({}), only {} elements are available",
            index,
            len
        );
        let end = len - 1;
        for offset in 0..4 {
            // The index of the element in the top word
            let a = end - offset;
            // The index of the element in the `n`th word
            let b = end - offset - index;
            stack.swap(a, b);
        }
    }

    /// Swaps the top two and bottom two words on the stack.
    fn swapdw(&mut self) {
        let stack = self.stack_mut();
        stack.rotate_left(8);
    }

    /// Moves the `n`th value to the top of the stack
    ///
    /// If `n` is 1, this is equivalent to `swap(1)`.
    ///
    /// NOTE: This function will panic if `n` is 0, or out of bounds.
    fn movup(&mut self, n: usize) {
        assert_ne!(n, 0, "invalid move, index must be in the range 1..=15");
        let stack = self.stack_mut();
        let len = stack.len();
        assert!(
            n < len,
            "invalid operand stack index ({}), only {} elements are available",
            n,
            len
        );
        // Pick the midpoint by counting backwards from the end
        let mid = len - (n + 1);
        // Split the stack, and rotate the half that
        // contains our desired value to place it on top.
        let (_, r) = stack.split_at_mut(mid);
        r.rotate_left(1);
    }

    /// Moves the `n`th word to the top of the stack
    ///
    /// If `n` is 1, this is equivalent to `swapw(1)`.
    ///
    /// Valid values for `n` are: 1, 2, 3
    fn movupw(&mut self, n: usize) {
        assert_ne!(n, 0, "invalid move, index must be in the range 1..=3");
        let stack = self.stack_mut();
        let len = stack.len();
        let index = (n * 4) + 4;
        assert!(
            index < len,
            "invalid operand stack index ({}), only {} elements are available",
            index,
            len
        );
        // Pick the midpoint index by counting backwards from the end
        let mid = len - index;
        // Split the stack, and rotate the half that
        // contains our desired word to place it on top.
        let (_, r) = stack.split_at_mut(mid);
        r.rotate_left(4);
    }

    /// Makes the value on top of the stack, the `n`th value on the stack
    ///
    /// If `n` is 1, this is equivalent to `swap(1)`.
    ///
    /// NOTE: This function will panic if `n` is 0, or out of bounds.
    fn movdn(&mut self, n: usize) {
        assert_ne!(n, 0, "invalid move: index must be in the range 1..=15");
        let stack = self.stack_mut();
        let len = stack.len();
        assert!(
            n < len,
            "invalid operand stack index ({}), only {} elements are available",
            n,
            len
        );
        // Split the stack so that the desired position is in the top half
        let mid = len - (n + 1);
        let (_, r) = stack.split_at_mut(mid);
        // Move all elements above the `n`th position up by one, moving the top element to the `n`th
        // position
        r.rotate_right(1);
    }

    /// Makes the word on top of the stack, the `n`th word on the stack
    ///
    /// If `n` is 1, this is equivalent to `swapw(1)`.
    ///
    /// Valid values for `n` are: 1, 2, 3
    fn movdnw(&mut self, n: usize) {
        assert_ne!(n, 0, "invalid move, index must be in the range 1..=3");
        let stack = self.stack_mut();
        let len = stack.len();
        let index = (n * 4) + 4;
        assert!(
            index < len,
            "invalid operand stack index ({}), only {} elements are available",
            index,
            len
        );
        // Split the stack so that the desired position is in the top half
        let mid = len - index;
        let (_, r) = stack.split_at_mut(mid);
        // Move all elements above the `n`th word up by one word, moving the top word to the `n`th
        // position
        r.rotate_right(4);
    }
}

/// This trait is used to represent expected behavior/properties of elements
/// that can be used in conjunction with the [Stack] trait.
pub trait StackElement: Clone {
    type Debug: fmt::Debug;

    /// A value of this type which represents the "zero" value for the type
    const DEFAULT: Self;

    /// Format this stack element for display
    fn debug(&self) -> Self::Debug;
}

impl StackElement for Felt {
    type Debug = u64;

    const DEFAULT: Self = Felt::ZERO;

    #[inline]
    fn debug(&self) -> Self::Debug {
        self.as_int()
    }
}
impl StackElement for Type {
    type Debug = Type;

    const DEFAULT: Self = Type::Felt;

    #[inline]
    fn debug(&self) -> Self::Debug {
        self.clone()
    }
}

/// This structure is a concrete implementation of the [Stack] trait, implemented
/// for use with two different element types:
///
/// * [Felt], for actual emulation of the Miden VM operand stack
/// * [Type], for tracking the state of the operand stack in abstract
pub struct OperandStack<T> {
    stack: Vec<T>,
}
impl<T: Clone> Clone for OperandStack<T> {
    fn clone(&self) -> Self {
        Self {
            stack: self.stack.clone(),
        }
    }
}
impl<T> Default for OperandStack<T> {
    fn default() -> Self {
        Self { stack: vec![] }
    }
}
impl<T: StackElement> Stack for OperandStack<T> {
    type Element = T;

    #[inline(always)]
    fn stack(&self) -> &Vec<Self::Element> {
        &self.stack
    }

    #[inline(always)]
    fn stack_mut(&mut self) -> &mut Vec<Self::Element> {
        &mut self.stack
    }

    #[inline(always)]
    fn clear(&mut self) {
        self.stack.clear();
    }
}
impl OperandStack<Felt> {
    /// Pushes `value` on top of the stack, with an optional set of aliases
    pub fn push_u8(&mut self, value: u8) {
        self.stack.push(Felt::new(value as u64));
    }

    /// Pushes `value` on top of the stack, with an optional set of aliases
    pub fn push_u16(&mut self, value: u16) {
        self.stack.push(Felt::new(value as u64));
    }

    /// Pushes `value` on top of the stack, with an optional set of aliases
    pub fn push_u32(&mut self, value: u32) {
        self.stack.push(Felt::new(value as u64));
    }
}
impl<T: StackElement> Index<usize> for OperandStack<T> {
    type Output = T;

    fn index(&self, index: usize) -> &Self::Output {
        let len = self.stack.len();
        assert!(
            index < 16,
            "invalid operand stack index ({}), only the top 16 elements are directly accessible",
            index
        );
        assert!(
            index < len,
            "invalid operand stack index ({}), only {} elements are available",
            index,
            len
        );
        &self.stack[len - index - 1]
    }
}
impl<T: StackElement> IndexMut<usize> for OperandStack<T> {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        let len = self.stack.len();
        assert!(
            index < 16,
            "invalid operand stack index ({}), only the top 16 elements are directly accessible",
            index
        );
        assert!(
            index < len,
            "invalid operand stack index ({}), only {} elements are available",
            index,
            len
        );
        &mut self.stack[len - index - 1]
    }
}

#[doc(hidden)]
pub struct DebugStack<'a, T: ?Sized + Stack> {
    stack: &'a T,
    limit: Option<usize>,
}
impl<'a, E: StackElement, T: ?Sized + Stack<Element = E>> DebugStack<'a, T> {
    pub fn new(stack: &'a T) -> Self {
        Self { stack, limit: None }
    }

    pub fn take(mut self, n: usize) -> Self {
        self.limit = Some(n);
        self
    }
}
impl<E: StackElement, T: ?Sized + Stack<Element = E>> fmt::Debug for DebugStack<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        #[allow(unused)]
        struct StackEntry<'a, E: StackElement> {
            index: usize,
            value: &'a E,
        }
        impl<E: StackElement> fmt::Debug for StackEntry<'_, E> {
            fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.debug_struct("StackEntry")
                    .field("index", &self.index)
                    .field("value", &self.value.debug())
                    .finish()
            }
        }

        f.debug_list()
            .entries(
                self.stack
                    .stack()
                    .iter()
                    .rev()
                    .enumerate()
                    .take(self.limit.unwrap_or(self.stack.len()))
                    .map(|(index, value)| StackEntry { index, value }),
            )
            .finish()
    }
}
impl<E: StackElement + fmt::Debug, T: ?Sized + Stack<Element = E>> fmt::Display
    for DebugStack<'_, T>
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_list()
            .entries(
                self.stack
                    .stack()
                    .iter()
                    .rev()
                    .enumerate()
                    .take(self.limit.unwrap_or(self.stack.len())),
            )
            .finish()
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operand_stack_primitive_ops_test() {
        let mut stack = OperandStack::<Felt>::default();

        let zero = Felt::new(0);
        let one = Felt::new(1);
        let two = Felt::new(2);
        let three = Felt::new(3);
        let four = Felt::new(4);
        let five = Felt::new(5);
        let six = Felt::new(6);
        let seven = Felt::new(7);

        // push
        stack.push(zero);
        stack.push(one);
        stack.push(two);
        stack.push(three);
        assert_eq!(stack.len(), 4);
        assert_eq!(stack[0].as_int(), 3);
        assert_eq!(stack[1].as_int(), 2);
        assert_eq!(stack[2].as_int(), 1);
        assert_eq!(stack[3].as_int(), 0);

        #[inline(always)]
        fn as_int(word: [Felt; 4]) -> [u64; 4] {
            [word[0].as_int(), word[1].as_int(), word[2].as_int(), word[3].as_int()]
        }

        // peekw
        assert_eq!(stack.peekw().map(as_int), Some([3, 2, 1, 0]));

        // dupw
        stack.dupw(0);
        assert_eq!(stack.len(), 8);
        assert_eq!(stack.peekw().map(as_int), Some([3, 2, 1, 0]));

        // padw
        stack.padw();
        assert_eq!(stack.len(), 12);
        assert_eq!(stack.peekw().map(as_int), Some([0; 4]));

        // swapw
        stack.swapw(1);
        assert_eq!(stack.len(), 12);
        assert_eq!(stack.peekw().map(as_int), Some([3, 2, 1, 0]));

        // popw
        let word = stack.popw();
        assert_eq!(stack.len(), 8);
        assert_eq!(word.map(as_int), Some([3, 2, 1, 0]));

        // pushw
        stack.pushw(word.unwrap());
        stack.pushw([seven, six, five, four]);
        assert_eq!(stack.len(), 16);
        assert_eq!(stack.peekw().map(as_int), Some([7, 6, 5, 4]));

        // movupw
        stack.movupw(2);
        assert_eq!(stack.len(), 16);
        assert_eq!(stack.peekw().map(as_int), Some([0; 4]));
        assert_eq!(stack[8].as_int(), 3);
        assert_eq!(stack[9].as_int(), 2);
        assert_eq!(stack[10].as_int(), 1);
        assert_eq!(stack[11].as_int(), 0);

        // movdnw
        stack.movdnw(2);
        assert_eq!(stack.len(), 16);
        assert_eq!(stack.peekw().map(as_int), Some([7, 6, 5, 4]));
        assert_eq!(stack[8].as_int(), 0);
        assert_eq!(stack[9].as_int(), 0);
        assert_eq!(stack[10].as_int(), 0);
        assert_eq!(stack[11].as_int(), 0);

        // dropw
        stack.movupw(2);
        stack.dropw();
        assert_eq!(stack.len(), 12);
        assert_eq!(stack.peekw().map(as_int), Some([7, 6, 5, 4]));

        // dup(n)
        stack.dup(0);
        assert_eq!(stack.len(), 13);
        assert_eq!(stack[0].as_int(), 7);
        assert_eq!(stack[1].as_int(), 7);
        assert_eq!(stack[2].as_int(), 6);
        assert_eq!(stack[3].as_int(), 5);
        assert_eq!(stack[4].as_int(), 4);
        stack.dup(3);
        assert_eq!(stack.len(), 14);
        assert_eq!(stack[0].as_int(), 5);
        assert_eq!(stack[1].as_int(), 7);
        assert_eq!(stack[2].as_int(), 7);
        assert_eq!(stack[3].as_int(), 6);
        assert_eq!(stack[4].as_int(), 5);
        assert_eq!(stack[5].as_int(), 4);

        // swap(n)
        stack.swap(1);
        assert_eq!(stack.len(), 14);
        assert_eq!(stack[0].as_int(), 7);
        assert_eq!(stack[1].as_int(), 5);
        assert_eq!(stack[2].as_int(), 7);
        assert_eq!(stack[3].as_int(), 6);
        assert_eq!(stack[4].as_int(), 5);
        assert_eq!(stack[5].as_int(), 4);

        // movup(n)
        stack.movup(3);
        assert_eq!(stack.len(), 14);
        assert_eq!(stack[0].as_int(), 6);
        assert_eq!(stack[1].as_int(), 7);
        assert_eq!(stack[2].as_int(), 5);
        assert_eq!(stack[3].as_int(), 7);
        assert_eq!(stack[4].as_int(), 5);
        assert_eq!(stack[5].as_int(), 4);

        // movdn(n)
        stack.movdn(3);
        assert_eq!(stack.len(), 14);
        assert_eq!(stack[0].as_int(), 7);
        assert_eq!(stack[1].as_int(), 5);
        assert_eq!(stack[2].as_int(), 7);
        assert_eq!(stack[3].as_int(), 6);
        assert_eq!(stack[4].as_int(), 5);
        assert_eq!(stack[5].as_int(), 4);

        // drop
        stack.drop();
        assert_eq!(stack.len(), 13);
        assert_eq!(stack[0].as_int(), 5);
        assert_eq!(stack[1].as_int(), 7);
        assert_eq!(stack[2].as_int(), 6);
        assert_eq!(stack[3].as_int(), 5);
        assert_eq!(stack[4].as_int(), 4);

        // dropn
        stack.dropn(2);
        assert_eq!(stack.len(), 11);
        assert_eq!(stack[0].as_int(), 6);
        assert_eq!(stack[1].as_int(), 5);
        assert_eq!(stack[2].as_int(), 4);

        // push
        stack.push(six);
        stack.push(seven);
        assert_eq!(stack.len(), 13);
        assert_eq!(stack[0].as_int(), 7);
        assert_eq!(stack[1].as_int(), 6);
        assert_eq!(stack[2].as_int(), 6);
        assert_eq!(stack[3].as_int(), 5);
        assert_eq!(stack[4].as_int(), 4);
    }
}
