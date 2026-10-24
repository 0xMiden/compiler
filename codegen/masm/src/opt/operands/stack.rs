use super::*;

/// The shuffle solver's stack of grouped value occurrences.
///
/// Index zero is the top, while the backing vector and `iter()` run bottom to top. Constructing
/// from an iterator takes top-first values and reverses them. Widths stay on each value so tactics
/// can translate grouped operand indices into actual raw-lane depths; this is not a VM stack.
#[derive(Default, Debug, Clone)]
pub(crate) struct Stack {
    stack: Vec<ValueOrAlias>,
}
impl FromIterator<ValueOrAlias> for Stack {
    fn from_iter<I>(iter: I) -> Self
    where
        I: IntoIterator<Item = ValueOrAlias>,
    {
        let mut stack = iter.into_iter().collect::<Vec<_>>();
        stack.reverse();

        Self { stack }
    }
}
impl Stack {
    pub fn len(&self) -> usize {
        self.stack.len()
    }

    pub fn push(&mut self, value: ValueOrAlias) {
        self.stack.push(value);
    }

    pub fn position(&self, value: &ValueOrAlias) -> Option<usize> {
        self.stack.iter().rev().position(|stack_value| value == stack_value)
    }

    pub fn position_skip(&self, start_index: usize, value: &ValueOrAlias) -> Option<usize> {
        self.stack
            .iter()
            .rev()
            .skip(start_index)
            .position(|stack_value| value == stack_value)
            .map(|pos| pos + start_index)
    }

    pub fn iter(&self) -> impl DoubleEndedIterator<Item = &ValueOrAlias> {
        self.stack.iter()
    }

    #[allow(dead_code)]
    pub fn iter_mut(&mut self) -> impl DoubleEndedIterator<Item = &mut ValueOrAlias> {
        self.stack.iter_mut()
    }

    pub fn dup(&mut self, n: usize, alias_id: core::num::NonZeroU8) {
        let value = self[n];
        self.stack.push(value.copy(alias_id));
    }

    pub fn swap(&mut self, n: usize) {
        let len = self.stack.len();
        let a_idx = len - 1;
        let b_idx = a_idx - n;
        self.stack.swap(a_idx, b_idx);
    }

    pub fn movup(&mut self, n: usize) {
        let len = self.stack.len();
        let mid = len - (n + 1);
        let (_, r) = self.stack.split_at_mut(mid);
        r.rotate_left(1);
    }

    pub fn movdn(&mut self, n: usize) {
        let len = self.stack.len();
        let mid = len - (n + 1);
        let (_, r) = self.stack.split_at_mut(mid);
        r.rotate_right(1);
    }

    pub fn reset_to(&mut self, snapshot: &Self) {
        self.stack.clear();
        let x = self.stack.capacity();
        let y = snapshot.stack.capacity();
        if x != y {
            let a = core::cmp::max(x, y);
            if a > x {
                self.stack.reserve(a - x);
            }
        }
        self.stack.extend_from_slice(&snapshot.stack);
    }

    pub fn get(&self, index: usize) -> Option<&ValueOrAlias> {
        let len = self.stack.len();
        self.stack.get(len - index - 1)
    }
}
impl core::ops::Index<usize> for Stack {
    type Output = ValueOrAlias;

    fn index(&self, index: usize) -> &Self::Output {
        let len = self.stack.len();
        let index = len
            .checked_sub(index)
            .and_then(|idx| idx.checked_sub(1))
            .expect("invalid stack index");
        &self.stack[index]
    }
}
impl core::ops::IndexMut<usize> for Stack {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        let len = self.stack.len();
        let index = len
            .checked_sub(index)
            .and_then(|idx| idx.checked_sub(1))
            .expect("invalid stack index");
        &mut self.stack[index]
    }
}
