use core::{cell::Cell, fmt};

use super::{Block, Inst};

/// This trait is used to decorate the textual formatting of blocks and instructions
/// with additional information, e.g liveness.
pub trait Decorator {
    type Display<'a>: fmt::Display
    where
        Self: 'a;

    /// Emit no decoration for this block when true
    fn skip_block(&self, _block: Block) -> bool {
        false
    }
    /// Emit no decoration for this instruction when true
    fn skip_inst(&self, _inst: Inst) -> bool {
        false
    }
    /// Emit decoration for `block` by returning a displayable object
    fn decorate_block<'a, 'd: 'a>(&'d self, block: Block) -> Self::Display<'a>;
    /// Emit decoration for `inst` by returning a displayable object
    fn decorate_inst<'a, 'd: 'a>(&'d self, inst: Inst) -> Self::Display<'a>;
}
impl Decorator for () {
    type Display<'a> = &'a str;

    fn skip_block(&self, _block: Block) -> bool {
        true
    }

    fn skip_inst(&self, _inst: Inst) -> bool {
        true
    }

    fn decorate_block<'a, 'd: 'a>(&'d self, _block: Block) -> Self::Display<'a> {
        ""
    }

    fn decorate_inst<'a, 'd: 'a>(&'d self, _inst: Inst) -> Self::Display<'a> {
        ""
    }
}

/// Render an iterator of `T`, comma-separated
pub struct DisplayValues<T>(Cell<Option<T>>);
impl<T> DisplayValues<T> {
    pub fn new(inner: T) -> Self {
        Self(Cell::new(Some(inner)))
    }
}
impl<T, I> fmt::Display for DisplayValues<I>
where
    T: fmt::Display,
    I: Iterator<Item = T>,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let iter = self.0.take().unwrap();
        for (i, item) in iter.enumerate() {
            if i == 0 {
                write!(f, "{}", item)?;
            } else {
                write!(f, ", {}", item)?;
            }
        }
        Ok(())
    }
}

/// Render an `Option<T>` using the `Display` impl for `T`
pub struct DisplayOptional<'a, T>(pub Option<&'a T>);
impl<T: fmt::Display> fmt::Display for DisplayOptional<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.0 {
            None => f.write_str("None"),
            Some(item) => write!(f, "Some({item})"),
        }
    }
}
impl<T: fmt::Display> fmt::Debug for DisplayOptional<'_, T> {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}
