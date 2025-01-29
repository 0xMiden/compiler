use midenc_hir::FunctionType;
use midenc_hir_type::Type::*;

use crate::miden_abi::{FunctionTypeMap, ModuleFunctionTypeMap};

pub const MODULE_ID: &str = "miden::note";

pub const GET_INPUTS: &str = "get_inputs";

pub(crate) fn signatures() -> ModuleFunctionTypeMap {
    let mut m: ModuleFunctionTypeMap = Default::default();
    let mut note: FunctionTypeMap = Default::default();
    note.insert(GET_INPUTS, FunctionType::new([I32], [I32, I32]));
    m.insert(MODULE_ID, note);
    m
}
