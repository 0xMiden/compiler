use super::*;

/// This is an simple representation of a [ValueRef] on the Miden operand stack
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct StackOperand {
    /// The value this operand corresponds to
    pub value: ValueOrAlias,
    /// The position of this operand on the corresponding stack
    pub pos: u8,
}

impl core::fmt::Display for StackOperand {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}:{}", &self.pos, &self.value)
    }
}

impl Ord for StackOperand {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.pos.cmp(&other.pos).then(self.value.cmp(&other.value))
    }
}

impl PartialOrd for StackOperand {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl From<(usize, ValueOrAlias)> for StackOperand {
    #[inline(always)]
    fn from(pair: (usize, ValueOrAlias)) -> Self {
        Self {
            pos: pair.0 as u8,
            value: pair.1,
        }
    }
}

impl PartialEq<ValueOrAlias> for StackOperand {
    #[inline(always)]
    fn eq(&self, other: &ValueOrAlias) -> bool {
        self.value.eq(other)
    }
}
