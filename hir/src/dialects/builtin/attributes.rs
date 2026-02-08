mod array;
mod boolean;
mod bytes;
mod integer;
mod list;
mod local_variable;
mod location;
mod overflow;
mod signature;
mod string;
mod symbol_ref;
mod r#type;
mod unit;
pub mod version;
mod visibility;

pub use self::{
    array::{Array, ArrayAttr},
    boolean::BoolAttr,
    bytes::{Bytes, BytesAttr},
    integer::*,
    list::{List, ListAttr},
    local_variable::{LocalVariable, LocalVariableAttr},
    location::{Location, LocationAttr},
    overflow::{Overflow, OverflowAttr},
    signature::{
        AbiParam, ArgumentExtension, ArgumentPurpose, SextAttr, Signature, SignatureAttr, SretAttr,
        ZextAttr,
    },
    string::StringAttr,
    symbol_ref::{SymbolRef, SymbolRefAttr},
    r#type::{FunctionTypeAttr, TypeAttr},
    unit::UnitAttr,
    version::VersionAttr,
    visibility::{Visibility, VisibilityAttr},
};
pub use crate::ir::{IdentAttr, ImmediateAttr};
