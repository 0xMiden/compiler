use crate::{
    AttrPrinter, Immediate,
    attributes::{AttrParser, IntegerLikeAttr},
    derive::DialectAttribute,
    dialects::builtin::BuiltinDialect,
    parse::ParserExt,
    print::AsmPrinter,
};

macro_rules! define_integer_attr {
    ($name:ident, $t:ident) => {
        __define_integer_attr!($name, $t, $t);
    };
}

macro_rules! __define_integer_attr {
    ($name:ident, $t:ty, $t_id:ident) => {
        #[derive(DialectAttribute)]
        #[attribute(dialect = BuiltinDialect, remote = $t_id, implements(IntegerLikeAttr, AttrPrinter))]
        #[repr(transparent)]
        pub struct $name;

        paste::paste! {
            impl IntegerLikeAttr for [<$name Attr>] {
                #[inline]
                fn as_immediate(&self) -> Immediate {
                    Immediate::from(self.value)
                }

                fn set_from_immediate_lossy(&mut self, value: Immediate) {
                    if let Some(value) = value.[<as_ $t_id>]() {
                        self.value = value;
                    } else {
                        self.value = value.as_u128().unwrap() as $t;
                    }
                }
            }

            impl AttrPrinter for [<$name Attr>] {
                fn print(&self, printer: &mut AsmPrinter<'_>) {
                    printer.print_decimal_integer(self.as_immediate());
                }
            }

            impl AttrParser for [<$name Attr>] {
                fn parse(
                    parser: &mut dyn crate::parse::Parser<'_>,
                ) -> crate::parse::ParseResult<crate::AttributeRef> {
                    let value = parser.parse_decimal_integer::<$t>()?;
                    Ok(parser
                        .context_rc()
                        .create_attribute::<[<$name Attr>], _>(value.into_inner())
                        .as_attribute_ref())
                }
            }
        }
    };
}

define_integer_attr!(I8, i8);
define_integer_attr!(U8, u8);
define_integer_attr!(I16, i16);
define_integer_attr!(U16, u16);
define_integer_attr!(I32, i32);
define_integer_attr!(U32, u32);
define_integer_attr!(I64, i64);
define_integer_attr!(U64, u64);
define_integer_attr!(I128, i128);
define_integer_attr!(U128, u128);
