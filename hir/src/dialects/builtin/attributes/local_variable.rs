use crate::{
    AttrPrinter, Type,
    attributes::AttrParser,
    derive::DialectAttribute,
    dialects::builtin::{BuiltinDialect, FunctionRef},
    parse::ParserExt,
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

    /// Returns true if this local is bound to its containing function.
    ///
    /// Locals reconstructed from parsed HIR are unbound: the local's type is carried by the
    /// attribute's type slot instead of the function's locals table. Use
    /// [`LocalVariableAttr::local_type`] to obtain the type regardless of binding.
    #[inline(always)]
    pub const fn is_bound(&self) -> bool {
        !self.is_uninit
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

impl LocalVariableAttr {
    /// Get the type of this local variable.
    ///
    /// For locals bound to a function this reads the function's locals table; for locals
    /// reconstructed from parsed HIR (which are unbound), it reads the type recorded on the
    /// attribute itself.
    pub fn local_type(&self) -> Type {
        if self.value.is_bound() {
            self.value.ty()
        } else {
            crate::Attribute::ty(self).clone()
        }
    }
}

impl AttrPrinter for LocalVariableAttr {
    fn print(&self, printer: &mut crate::print::AsmPrinter<'_>) {
        use crate::formatter::*;

        printer.print_decimal_integer(self.value.index);
        *printer += const_text(", ");
        printer.print_type(&self.local_type());
    }
}

impl AttrParser for LocalVariableAttr {
    fn parse(
        parser: &mut dyn crate::parse::Parser<'_>,
    ) -> crate::parse::ParseResult<crate::AttributeRef> {
        let index = parser.parse_decimal_integer::<u16>()?.into_inner();
        parser.parse_comma()?;
        let ty = parser.parse_non_function_type()?.into_inner();

        let attr = parser.context_rc().create_attribute_with_type::<LocalVariableAttr, _>(
            LocalVariable {
                index,
                function: FunctionRef::dangling(),
                is_uninit: true,
            },
            ty,
        );

        Ok(attr)
    }
}
