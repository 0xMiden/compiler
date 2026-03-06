use core::fmt;

use smallvec::SmallVec;

use crate::{
    AttrPrinter, attributes::InferAttributeValueType, derive::DialectAttribute,
    dialects::builtin::BuiltinDialect, formatter::DisplayValues, print::AsmPrinter,
};

#[derive(DialectAttribute)]
#[attribute(
    name = "array_u32",
    dialect = BuiltinDialect,
    remote = "Array<u32>",
    implements(AttrPrinter),
)]
#[allow(unused)]
struct U32Array;

#[derive(DialectAttribute)]
#[attribute(
    dialect = BuiltinDialect,
    remote = "Array<crate::Type>",
    implements(AttrPrinter),
)]
#[allow(unused)]
struct TypeArray;

#[derive(Clone, PartialEq, Eq, Hash)]
pub struct Array<T>(SmallVec<[T; 2]>);

impl<T> Array<T> {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn iter(&self) -> core::slice::Iter<'_, T> {
        self.0.iter()
    }

    pub fn push(&mut self, value: T) {
        self.0.push(value);
    }

    pub fn remove(&mut self, index: usize) -> T {
        self.0.remove(index)
    }
}

default impl<I: IntoIterator> From<I> for Array<<I as IntoIterator>::Item> {
    #[inline(always)]
    fn from(iter: I) -> Self {
        Self::from_iter(iter)
    }
}

impl<T, const N: usize> From<[T; N]> for Array<T> {
    #[inline]
    fn from(value: [T; N]) -> Self {
        Self(SmallVec::from_iter(value))
    }
}

impl<T> From<SmallVec<[T; 2]>> for Array<T> {
    fn from(value: SmallVec<[T; 2]>) -> Self {
        Self(value)
    }
}

impl<T> From<alloc::vec::Vec<T>> for Array<T> {
    fn from(value: alloc::vec::Vec<T>) -> Self {
        Self(SmallVec::from_vec(value))
    }
}

impl<T> FromIterator<T> for Array<T> {
    #[inline]
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        Self(SmallVec::from_iter(iter))
    }
}

impl<T> Default for Array<T> {
    #[inline]
    fn default() -> Self {
        Self(SmallVec::new_const())
    }
}

impl<T> Array<T>
where
    T: Eq,
{
    pub fn contains(&self, value: &T) -> bool {
        self.0.contains(value)
    }
}

impl<T> fmt::Debug for Array<T>
where
    T: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.0.iter()).finish()
    }
}

impl<T> fmt::Display for Array<T>
where
    T: fmt::Display,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}]", DisplayValues::new(self.0.iter()))
    }
}

impl<T> crate::formatter::PrettyPrint for Array<T>
where
    T: crate::formatter::PrettyPrint,
{
    fn render(&self) -> crate::formatter::Document {
        use crate::formatter::*;

        let entries = self.iter().fold(Document::Empty, |acc, v| match acc {
            Document::Empty => v.render(),
            _ => acc + const_text(", ") + v.render(),
        });
        if self.is_empty() {
            const_text("[]")
        } else {
            const_text("[") + entries + const_text("]")
        }
    }
}

impl<T> core::ops::Index<usize> for Array<T> {
    type Output = T;

    #[inline]
    fn index(&self, index: usize) -> &Self::Output {
        &self.0[index]
    }
}

impl<T> core::ops::Index<core::ops::Range<usize>> for Array<T> {
    type Output = [T];

    #[inline]
    fn index(&self, range: core::ops::Range<usize>) -> &Self::Output {
        &self.0[range]
    }
}

impl<T> core::ops::Index<core::ops::RangeTo<usize>> for Array<T> {
    type Output = [T];

    #[inline]
    fn index(&self, range: core::ops::RangeTo<usize>) -> &Self::Output {
        &self.0[range]
    }
}

impl<T> core::ops::Index<core::ops::RangeToInclusive<usize>> for Array<T> {
    type Output = [T];

    #[inline]
    fn index(&self, range: core::ops::RangeToInclusive<usize>) -> &Self::Output {
        &self.0[range]
    }
}

impl<T> core::ops::Index<core::ops::RangeFrom<usize>> for Array<T> {
    type Output = [T];

    #[inline]
    fn index(&self, range: core::ops::RangeFrom<usize>) -> &Self::Output {
        &self.0[range]
    }
}

impl<T> core::ops::Index<core::ops::RangeFull> for Array<T> {
    type Output = [T];

    #[inline]
    fn index(&self, _range: core::ops::RangeFull) -> &Self::Output {
        self.0.as_slice()
    }
}

impl<T> core::ops::IndexMut<usize> for Array<T> {
    #[inline]
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        &mut self.0[index]
    }
}

impl<T: Clone + InferAttributeValueType> InferAttributeValueType for Array<T> {
    fn infer_type() -> crate::Type {
        <T as InferAttributeValueType>::infer_type()
    }

    fn infer_type_from_value(&self) -> crate::Type {
        if self.0.is_empty() {
            Self::infer_type()
        } else {
            <T as InferAttributeValueType>::infer_type_from_value(&self.0[0])
        }
    }
}

impl crate::print::AttrPrinter for U32ArrayAttr {
    fn print(&self, printer: &mut AsmPrinter<'_>) {
        use crate::formatter::*;

        for (i, value) in self.0.iter().enumerate() {
            if i > 0 {
                *printer += const_text(", ");
            }
            *printer += value.render();
        }
    }
}

impl crate::print::AttrPrinter for TypeArrayAttr {
    fn print(&self, printer: &mut AsmPrinter<'_>) {
        use crate::formatter::*;

        for (i, value) in self.0.iter().enumerate() {
            if i > 0 {
                *printer += const_text(", ");
            }
            printer.print_type(value);
        }
    }
}
