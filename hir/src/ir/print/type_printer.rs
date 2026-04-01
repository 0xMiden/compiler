use super::*;
use crate::{FunctionType, Type};

/// Wraps a [Type] for printing in assembly format
pub struct TypePrinter<'a>(pub &'a Type);

impl fmt::Display for TypePrinter<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use crate::Type;
        match self.0 {
            Type::Unknown => f.write_str("?"),
            Type::Never => f.write_str("never"),
            Type::Ptr(ty) => {
                write!(f, "ptr<{}, {}>", &TypePrinter(ty.pointee()), &ty.addrspace())
            }
            Type::Array(ty) => {
                write!(f, "array<{}; {}>", &TypePrinter(ty.element_type()), &ty.len())
            }
            Type::List(ty) => {
                write!(f, "list<{}>", &TypePrinter(ty))
            }
            Type::Struct(ty) => {
                let fields =
                    crate::formatter::DisplayValues::new(ty.fields().iter().map(|field| {
                        let align = if field.align as usize != field.ty.min_alignment() {
                            Cow::Owned(format!(" align({})", &field.align))
                        } else {
                            Cow::Borrowed("")
                        };
                        format!("{}{align}", TypePrinter(&field.ty))
                    }));
                if matches!(ty.repr(), crate::TypeRepr::Default) {
                    write!(f, "struct<{fields}>")
                } else {
                    write!(f, "struct<{}; {fields}>", ty.repr())
                }
            }
            Type::Function(ty) => write!(f, "{}", FunctionTypePrinter::new(ty)),
            ty => write!(f, "{ty}"),
        }
    }
}

/// Wraps a [FunctionType] for printing in assembly format
pub struct FunctionTypePrinter<'a> {
    ty: &'a FunctionType,
    elide_single_result_parens: bool,
}

impl<'a> FunctionTypePrinter<'a> {
    #[inline]
    pub const fn new(ty: &'a FunctionType) -> Self {
        Self {
            ty,
            elide_single_result_parens: false,
        }
    }

    #[inline]
    pub fn elide_single_result_parens(mut self, yes: bool) -> Self {
        self.elide_single_result_parens = yes;
        self
    }
}

impl fmt::Display for FunctionTypePrinter<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("(")?;
        for (i, param) in self.ty.params().iter().enumerate() {
            if i > 0 {
                f.write_str(", ")?;
            }
            write!(f, "{}", TypePrinter(param))?;
        }
        f.write_str(") -> ")?;
        match self.ty.results().len() {
            0 => f.write_str("()"),
            1 if self.elide_single_result_parens => {
                write!(f, "{}", TypePrinter(&self.ty.results()[0]))
            }
            _ => {
                f.write_str("(")?;
                for (i, result) in self.ty.results().iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{}", TypePrinter(result))?;
                }
                f.write_str(")")
            }
        }
    }
}
