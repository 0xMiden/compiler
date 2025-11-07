#![no_std]

use miden::{component, export_type, Asset, Felt, Word};

pub mod my_types {
    use miden::{export_type, Felt};

    #[export_type]
    pub enum EnumA {
        VariantA,
        VariantB,
    }

    #[export_type]
    pub struct StructC {
        pub inner1: Felt,
        pub inner2: Felt,
    }
}

#[export_type]
pub struct StructA {
    pub foo: Word,
    pub asset: Asset,
}

#[export_type]
pub struct StructB {
    pub bar: Felt,
    pub baz: Felt,
}

#[export_type]
pub struct StructD {
    pub bar: Felt,
    pub baz: Felt,
}

#[export_type]
pub struct ForwardHolder {
    pub nested: LaterDefined,
}

#[export_type]
pub struct LaterDefined {
    pub value: Felt,
}

#[component]
struct MyAccount;

#[component]
impl MyAccount {
    /// Exercises exported user-defined type and SDK type in signatures and return value.
    pub fn test_custom_types(&self, a: StructA, asset: Asset) -> StructB {
        let foo_val =
            Word::from([a.foo.inner.0, asset.inner.inner.0, a.foo.inner.1, a.foo.inner.2]);

        let val_a = StructA {
            foo: foo_val,
            asset,
        };
        let c = self.test_custom_types2(val_a, asset);
        StructB {
            bar: c.inner1,
            baz: c.inner2,
        }
    }

    /// Exercises user-defined types in a sub-module
    pub fn test_custom_types2(&self, a: StructA, asset: Asset) -> my_types::StructC {
        let d = self.test_custom_types_private(a, asset);

        let _forward = ForwardHolder {
            nested: LaterDefined { value: d.bar },
        };

        my_types::StructC {
            inner1: d.bar,
            inner2: d.baz,
        }
    }

    fn test_custom_types_private(&self, a: StructA, _asset: Asset) -> StructD {
        StructD {
            bar: a.foo.inner.0,
            baz: a.foo.inner.1,
        }
    }
}
