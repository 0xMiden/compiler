use alloc::collections::VecDeque;

use super::*;

/// This implements a stack data structure for [Operand]
#[derive(Default, Debug, Clone)]
pub struct Stack {
    stack: Vec<Operand>,
}
impl FromIterator<ValueOrAlias> for Stack {
    fn from_iter<I>(iter: I) -> Self
    where
        I: IntoIterator<Item = ValueOrAlias>,
    {
        let mut stack = VecDeque::new();
        for value in iter.into_iter() {
            stack.push_front(Operand { pos: 0, value });
        }
        let mut stack = Vec::from(stack);
        for (pos, operand) in stack.iter_mut().rev().enumerate() {
            operand.pos = pos as u8;
        }
        Self { stack }
    }
}
impl From<&crate::OperandStack> for Stack {
    fn from(stack: &crate::OperandStack) -> Self {
        Self::from_iter(stack.iter().rev().map(|o| {
            o.as_value()
                .unwrap_or_else(|| panic!("expected value operand, got {o:#?}"))
                .into()
        }))
    }
}
impl Stack {
    pub fn len(&self) -> usize {
        self.stack.len()
    }

    pub fn push(&mut self, value: ValueOrAlias) {
        self.stack.push(Operand { pos: 0, value });
        if self.stack.len() > 1 {
            for (pos, operand) in self.iter_mut().rev().enumerate() {
                operand.pos = pos as u8;
            }
        }
    }

    pub fn position(&self, value: &ValueOrAlias) -> Option<usize> {
        self.stack.iter().rev().position(|o| value == &o.value)
    }

    pub fn contains(&self, operand: &Operand) -> bool {
        self[operand.pos as usize].value == operand.value
    }

    pub fn iter(&self) -> impl DoubleEndedIterator<Item = &Operand> {
        self.stack.iter()
    }

    pub fn iter_mut(&mut self) -> impl DoubleEndedIterator<Item = &mut Operand> {
        self.stack.iter_mut()
    }

    pub fn dup(&mut self, n: usize, alias_id: core::num::NonZeroU8) {
        let value = self[n].value;
        self.stack.push(Operand {
            pos: 0,
            value: value.copy(alias_id),
        });
        for (pos, operand) in self.stack.iter_mut().rev().enumerate() {
            operand.pos = pos as u8;
        }
    }

    pub fn swap(&mut self, n: usize) {
        let len = self.stack.len();
        let a = len - 1;
        let b = a - n;
        let a_pos = self.stack[a].pos;
        let b_pos = self.stack[b].pos;
        self.stack.swap(a, b);
        self.stack[a].pos = a_pos;
        self.stack[b].pos = b_pos;
    }

    pub fn movup(&mut self, n: usize) {
        let len = self.stack.len();
        let mid = len - (n + 1);
        let (_, r) = self.stack.split_at_mut(mid);
        r.rotate_left(1);
        for (pos, operand) in r.iter_mut().rev().enumerate() {
            operand.pos = pos as u8;
        }
    }

    pub fn movdn(&mut self, n: usize) {
        let len = self.stack.len();
        let mid = len - (n + 1);
        let (_, r) = self.stack.split_at_mut(mid);
        r.rotate_right(1);
        for (pos, operand) in r.iter_mut().rev().enumerate() {
            operand.pos = pos as u8;
        }
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

    pub fn get(&self, index: usize) -> Option<&Operand> {
        let len = self.stack.len();
        self.stack.get(len - index - 1)
    }
}
impl core::ops::Index<usize> for Stack {
    type Output = Operand;

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
