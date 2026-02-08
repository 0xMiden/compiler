use alloc::{format, rc::Rc, vec::Vec};
use core::fmt;

use super::Visibility;
use crate::{
    AttrPrinter, CallConv, Context, NamedAttribute, OpPrintingFlags, Type,
    attributes::AttributeDict, derive::DialectAttribute, dialects::builtin::BuiltinDialect,
    formatter, print::AsmPrinter,
};

/// A marker attribute for "struct return" parameters of a function.
///
/// An sret parameter is a parameter introduced when the ABI of a function requires the caller to
/// allocate memory to hold the function results, and then pass a pointer to that allocation to
/// the callee at a given parameter position (typically the first or last parameter). It is up to
/// the caller to ensure the given pointer is of sufficient size and alignment to hold the results.
#[derive(DialectAttribute, Debug, Copy, Clone, PartialEq, Eq, Default, Hash)]
#[attribute(dialect = BuiltinDialect, implements(AttrPrinter))]
#[allow(dead_code)]
pub struct Sret;

impl From<()> for Sret {
    fn from(_value: ()) -> Self {
        Self
    }
}

impl fmt::Display for Sret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("sret")
    }
}

impl AttrPrinter for SretAttr {
    fn print(&self, _printer: &mut crate::print::AsmPrinter<'_>) {}
}

/// Represents whether an argument or return value has a special purpose in
/// the calling convention of a function.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default, Hash)]
#[repr(u8)]
pub enum ArgumentPurpose {
    /// No special purpose, the argument is passed/returned by value
    #[default]
    Default,
    /// Used for platforms where the calling convention expects return values of
    /// a certain size to be written to a pointer passed in by the caller.
    StructReturn,
}

impl fmt::Display for ArgumentPurpose {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Default => f.write_str("default"),
            Self::StructReturn => f.write_str("sret"),
        }
    }
}

/// A marker attribute for argument parameters or results which should be zero-extended to the
/// target architecture's native machine word size, _if and only if_ the parameter type is smaller
/// than the native machine word size.
#[derive(DialectAttribute, Debug, Copy, Clone, PartialEq, Eq, Default, Hash)]
#[attribute(dialect = BuiltinDialect, implements(AttrPrinter))]
#[allow(dead_code)]
pub struct Zext;

impl From<()> for Zext {
    fn from(_value: ()) -> Self {
        Self
    }
}

impl fmt::Display for Zext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("zext")
    }
}

impl AttrPrinter for ZextAttr {
    fn print(&self, _printer: &mut crate::print::AsmPrinter<'_>) {}
}

/// A marker attribute for argument parameters or results which should be sign-extended to the
/// target architecture's native machine word size, _if and only if_ the parameter type is smaller
/// than the native machine word size.
#[derive(DialectAttribute, Debug, Copy, Clone, PartialEq, Eq, Default, Hash)]
#[attribute(dialect = BuiltinDialect, implements(AttrPrinter))]
#[allow(dead_code)]
pub struct Sext;

impl From<()> for Sext {
    fn from(_value: ()) -> Self {
        Self
    }
}

impl fmt::Display for Sext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("sext")
    }
}

impl AttrPrinter for SextAttr {
    fn print(&self, _printer: &mut crate::print::AsmPrinter<'_>) {}
}

/// Represents how to extend a small integer value to native machine integer width.
///
/// For Miden, native integrals are unsigned 64-bit field elements, but it is typically
/// going to be the case that we are targeting the subset of Miden Assembly where integrals
/// are unsigned 32-bit integers with a standard twos-complement binary representation.
///
/// It is for the latter scenario that argument extension is really relevant.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default, Hash)]
#[repr(u8)]
pub enum ArgumentExtension {
    /// Do not perform any extension, high bits have undefined contents
    #[default]
    None,
    /// Zero-extend the value
    Zext,
    /// Sign-extend the value
    Sext,
}
impl fmt::Display for ArgumentExtension {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::None => f.write_str("none"),
            Self::Zext => f.write_str("zext"),
            Self::Sext => f.write_str("sext"),
        }
    }
}

/// Describes a function parameter or result.
#[derive(Debug, PartialEq, Eq, Hash)]
pub struct AbiParam {
    /// The type associated with this value
    pub ty: Type,
    /// The attributes associated with this value
    pub attrs: AttributeDict,
}

impl Clone for AbiParam {
    fn clone(&self) -> Self {
        let mut attrs = AttributeDict::new();
        for attr in self.attrs.iter() {
            let value = attr.value.borrow();
            let new_value = value.dyn_clone();
            let new_attr = value.context_rc().alloc_map_item(NamedAttribute {
                name: attr.name,
                value: new_value,
            });
            attrs.insert(new_attr);
        }
        Self {
            ty: self.ty.clone(),
            attrs,
        }
    }
}

impl AbiParam {
    pub fn new(ty: Type) -> Self {
        Self::new_with_attribute_dict(ty, AttributeDict::new())
    }

    pub fn zext(ty: Type, context: Rc<Context>) -> Self {
        let zext = context.create_attribute::<ZextAttr, _>(());
        Self::new_with_attrs(ty, [NamedAttribute::new("extend", zext)])
    }

    pub fn sext(ty: Type, context: Rc<Context>) -> Self {
        let sext = context.create_attribute::<SextAttr, _>(());
        Self::new_with_attrs(ty, [NamedAttribute::new("extend", sext)])
    }

    pub fn sret(ty: Type, context: Rc<Context>) -> Self {
        let sret = context.create_attribute::<SretAttr, _>(());
        Self::new_with_attrs(ty, [NamedAttribute::new("sret", sret)])
    }

    pub fn mark_sret(&mut self, context: &Rc<Context>) {
        let sret = context.create_attribute::<SretAttr, _>(());
        let attr = context.alloc_map_item(NamedAttribute {
            name: crate::interner::symbols::Sret,
            value: sret,
        });
        self.attrs.insert(attr);
    }

    pub fn new_with_attribute_dict(ty: Type, attrs: AttributeDict) -> Self {
        assert!(ty.is_pointer(), "sret parameters must be pointers");
        Self { ty, attrs }
    }

    pub fn new_with_attrs(ty: Type, attributes: impl IntoIterator<Item = NamedAttribute>) -> Self {
        let mut attrs = AttributeDict::new();
        for attr in attributes {
            let context = attr.value.borrow().context_rc();
            attrs.insert(context.alloc_map_item(attr));
        }
        Self::new_with_attribute_dict(ty, attrs)
    }

    pub fn is_sret_param(&self) -> bool {
        self.attrs.contains("sret")
    }

    pub fn extension(&self) -> ArgumentExtension {
        match self.attrs.find("extension").get() {
            None => ArgumentExtension::None,
            Some(attr) => {
                let value = attr.value.borrow();
                if value.is::<ZextAttr>() {
                    ArgumentExtension::Zext
                } else if value.is::<SextAttr>() {
                    ArgumentExtension::Sext
                } else {
                    ArgumentExtension::None
                }
            }
        }
    }

    pub fn should_zero_extend(&self) -> bool {
        matches!(self.extension(), ArgumentExtension::Zext)
    }

    pub fn should_sign_extend(&self) -> bool {
        matches!(self.extension(), ArgumentExtension::Sext)
    }
}

impl formatter::PrettyPrint for AbiParam {
    fn render(&self) -> formatter::Document {
        use formatter::*;

        let ty = text(format!("{}", &self.ty));

        let mut doc = Document::Empty;
        let flags = OpPrintingFlags::default();
        for (i, attr) in self.attrs.iter().enumerate() {
            let (key, value) = match attr.name.as_str() {
                "sret" => (const_text("sret"), Document::Empty),
                "extend" => {
                    let value = attr.value.borrow();
                    if value.is::<ZextAttr>() {
                        (const_text("zext"), Document::Empty)
                    } else if value.is::<SextAttr>() {
                        (const_text("sext"), Document::Empty)
                    } else {
                        let mut printer = AsmPrinter::new(value.context_rc(), &flags);
                        printer.print_attribute_value(&*value);
                        let value_pp = printer.finish();
                        (const_text("extend"), value_pp)
                    }
                }
                other => {
                    let value = attr.value.borrow();
                    let mut printer = AsmPrinter::new(value.context_rc(), &flags);
                    printer.print_attribute_value(&*value);
                    (const_text(other), printer.finish())
                }
            };

            if i == 0 {
                doc += const_text(" { ");
            } else {
                doc += const_text(", ");
            }
            if value.is_empty() {
                doc += key;
            } else {
                doc += key + const_text(" = ") + value;
            }
        }

        if doc.is_empty() {
            ty
        } else {
            ty + doc + const_text(" }")
        }
    }
}

impl fmt::Display for AbiParam {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use crate::formatter::PrettyPrint;
        write!(f, "{}", self.render())
    }
}

/// A [Signature] represents the type, ABI, and linkage of a function.
///
/// A function signature provides us with all of the necessary detail to correctly
/// validate and emit code for a function, whether from the perspective of a caller,
/// or the callee.
#[derive(DialectAttribute, Debug, Clone, PartialEq, Eq, Hash)]
#[attribute(
    dialect = BuiltinDialect,
    implements(AttrPrinter)
)]
pub struct Signature {
    /// The arguments expected by this function
    pub params: Vec<AbiParam>,
    /// The results returned by this function
    pub results: Vec<AbiParam>,
    /// The calling convention that applies to this function
    pub cc: CallConv,
    /// The linkage/visibility that should be used for this function
    pub visibility: Visibility,
}

impl Default for Signature {
    fn default() -> Self {
        Self::new([], [])
    }
}

impl fmt::Display for Signature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map()
            .key(&"params")
            .value_with(|f| {
                let mut builder = f.debug_list();
                for param in self.params.iter() {
                    builder.entry(&format_args!("{param}"));
                }
                builder.finish()
            })
            .key(&"results")
            .value_with(|f| {
                let mut builder = f.debug_list();
                for param in self.params.iter() {
                    builder.entry(&format_args!("{param}"));
                }
                builder.finish()
            })
            .entry(&"cc", &format_args!("{}", &self.cc))
            .entry(&"visibility", &format_args!("{}", &self.visibility))
            .finish()
    }
}

impl Signature {
    /// Create a new signature with the given parameter and result types,
    /// for a public function using the `SystemV` calling convention
    pub fn new<P: IntoIterator<Item = AbiParam>, R: IntoIterator<Item = AbiParam>>(
        params: P,
        results: R,
    ) -> Self {
        Self {
            params: params.into_iter().collect(),
            results: results.into_iter().collect(),
            cc: CallConv::SystemV,
            visibility: Visibility::Public,
        }
    }

    /// Returns true if this function is externally visible
    pub fn is_public(&self) -> bool {
        matches!(self.visibility, Visibility::Public)
    }

    /// Returns true if this function is only visible within it's containing module
    pub fn is_private(&self) -> bool {
        matches!(self.visibility, Visibility::Public)
    }

    /// Returns true if this function is a kernel function
    pub fn is_kernel(&self) -> bool {
        matches!(self.cc, CallConv::Kernel)
    }

    /// Returns the number of arguments expected by this function
    pub fn arity(&self) -> usize {
        self.params().len()
    }

    /// Returns a slice containing the parameters for this function
    pub fn params(&self) -> &[AbiParam] {
        self.params.as_slice()
    }

    /// Returns the parameter at `index`, if present
    #[inline]
    pub fn param(&self, index: usize) -> Option<&AbiParam> {
        self.params.get(index)
    }

    /// Returns a slice containing the results of this function
    pub fn results(&self) -> &[AbiParam] {
        match self.results.as_slice() {
            [
                AbiParam {
                    ty: Type::Never, ..
                },
            ] => &[],
            results => results,
        }
    }
}

impl AttrPrinter for SignatureAttr {
    fn print(&self, _printer: &mut AsmPrinter<'_>) {
        todo!()
    }
}
