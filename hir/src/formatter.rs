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
                write!(f, "{item}")?;
            } else {
                write!(f, ", {item}")?;
            }
        }
        Ok(())
    }
}

/// Render an iterator of `T`, comma-separated
pub struct DisplayMany<T, U> {
    iterator: Cell<Option<T>>,
    separator: U,
}
impl<T, U> DisplayMany<T, U> {
    pub fn new(inner: T, separator: U) -> Self {
        Self {
            iterator: Cell::new(Some(inner)),
            separator,
        }
    }
}
impl<T, U, I> fmt::Display for DisplayMany<I, U>
where
    T: fmt::Display,
    U: fmt::Display,
    I: Iterator<Item = T>,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let iter = self.iterator.take().unwrap();
        for (i, item) in iter.enumerate() {
            if i == 0 {
                write!(f, "{item}")?;
            } else {
                write!(f, "{}{item}", &self.separator)?;
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
