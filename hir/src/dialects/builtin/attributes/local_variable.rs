use crate::{
    AttrPrinter, Type,
    derive::DialectAttribute,
    dialects::builtin::{BuiltinDialect, FunctionRef},
};

#[derive(DialectAttribute, Copy, Clone, PartialEq, Eq, Hash)]
#[attribute(
    dialect = BuiltinDialect,
    implements(AttrPrinter)
)]
pub struct LocalVariable {
    function: FunctionRef,
    index: u16,
    is_uninit: bool,
}

impl Default for LocalVariable {
    fn default() -> Self {
        Self {
            function: FunctionRef::dangling(),
            index: 0,
            is_uninit: true,
        }
    }
}

impl LocalVariable {
    pub(in crate::dialects::builtin) fn new(function: FunctionRef, id: usize) -> Self {
        assert!(
            id <= u16::MAX as usize,
            "system limit: unable to allocate more than u16::MAX locals per function"
        );
        Self {
            function,
            index: id as u16,
            is_uninit: false,
        }
    }

    #[inline(always)]
    pub const fn as_usize(&self) -> usize {
        self.index as usize
    }

    #[inline(always)]
    pub const fn function(&self) -> FunctionRef {
        assert!(!self.is_uninit);
        self.function
    }

    pub fn ty(&self) -> Type {
        assert!(!self.is_uninit);
        self.function.borrow().get_local(self).clone()
    }

    /// Compute the absolute offset from the start of the procedure locals for this local variable
    pub fn absolute_offset(&self) -> usize {
        assert!(!self.is_uninit);
        let index = self.as_usize();
        self.function.borrow().locals()[..index]
            .iter()
            .map(|ty| ty.size_in_felts())
            .sum::<usize>()
    }
}

impl core::fmt::Debug for LocalVariable {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("LocalVariable")
            .field_with("function", |f| {
                if self.is_uninit {
                    f.write_str("<dangling>")
                } else {
                    write!(f, "{}", self.function.borrow().name().as_str())
                }
            })
            .field("index", &self.index)
            .finish()
    }
}

impl core::fmt::Display for LocalVariable {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "local{}", self.as_usize())
    }
}

impl AttrPrinter for LocalVariableAttr {
    fn print(&self, printer: &mut crate::print::AsmPrinter<'_>) {
        printer.print_decimal_integer(self.value.index);
    }
}
