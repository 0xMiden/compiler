use alloc::{
    borrow::Cow,
    format,
    string::{String, ToString},
};
use core::str::FromStr;

/// ColorChoice represents the color preferences of an end user.
///
/// The `Default` implementation for this type will select `Auto`, which tries
/// to do the right thing based on the current environment.
///
/// The `FromStr` implementation for this type converts a lowercase kebab-case
/// string of the variant name to the corresponding variant. Any other string
/// results in an error.
#[derive(Default, Debug, Clone, PartialEq, Eq)]
pub enum IrFilter {
    /// Apply to any IR
    #[default]
    Any,
    /// Apply to any operation that implements `Symbol`, optionally restricted with a specific
    /// string that the name of the symbol must contain
    Symbol(Option<Cow<'static, str>>),
    /// Apply to a specific operation, given by its dialect and opcode
    Op {
        dialect: midenc_hir_symbol::Symbol,
        op: midenc_hir_symbol::Symbol,
    },
}

impl FromStr for IrFilter {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.split_once(':') {
            Some(("symbol", "" | "*")) => Ok(Self::Symbol(None)),
            Some(("symbol", pattern)) => Ok(Self::Symbol(Some(pattern.to_string().into()))),
            Some(("op", name)) => match name.split_once(".") {
                Some((dialect, op)) => Ok(Self::Op {
                    dialect: midenc_hir_symbol::Symbol::intern(dialect),
                    op: midenc_hir_symbol::Symbol::intern(op),
                }),
                None => Err(format!(
                    "invalid operation name '{name}': must be dialect-qualified, e.g. \
                     `dialect.{name}`"
                )),
            },
            Some((ty, _)) => {
                Err(format!("unrecognized filter type '{ty}': expected `symbol` or `op`"))
            }
            None if s == "any" => Ok(Self::Any),
            None => Err(format!(
                "unrecognized filter '{s}': expected `symbol:<pattern|*>`, \
                 `op:<dialect>.<opcode>`, or `any`"
            )),
        }
    }
}
