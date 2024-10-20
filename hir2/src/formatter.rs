use core::{cell::Cell, fmt};

pub use miden_core::{
    prettier::*,
    utils::{DisplayHex, ToHex},
};

pub struct DisplayIndent(pub usize);
impl fmt::Display for DisplayIndent {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        const INDENT: &str = "  ";
        for _ in 0..self.0 {
            f.write_str(INDENT)?;
        }
        Ok(())
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
impl<'a, T: fmt::Display> fmt::Display for DisplayOptional<'a, T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.0 {
            None => f.write_str("None"),
            Some(item) => write!(f, "Some({item})"),
        }
    }
}
impl<'a, T: fmt::Display> fmt::Debug for DisplayOptional<'a, T> {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}
