pub trait IteratorExt {
    /// Returns true if the given iterator consists of exactly one element
    fn has_single_element(&mut self) -> bool;
}

impl<I: Iterator> IteratorExt for I {
    default fn has_single_element(&mut self) -> bool {
        self.next().is_some_and(|_| self.next().is_none())
    }
}

impl<I: ExactSizeIterator> IteratorExt for I {
    #[inline]
    fn has_single_element(&mut self) -> bool {
        self.len() == 1
    }
}
