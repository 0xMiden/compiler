mod ops;

pub use self::ops::*;
use crate::{interner, Dialect, DialectName};

#[derive(Default, Debug)]
pub struct HirDialect;
impl Dialect for HirDialect {
    const INIT: Self = HirDialect;

    fn name(&self) -> DialectName {
        DialectName::from_symbol(interner::symbols::Hir)
    }
}
