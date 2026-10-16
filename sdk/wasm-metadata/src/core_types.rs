//! The records of the SDK's core types (`miden.wit` in the SDK macros, interface
//! [`CORE_TYPES_INTERFACE_ID`](crate::namespace::CORE_TYPES_INTERFACE_ID)): every record's name,
//! its fields in declaration order and the type of each field.
//!
//! The SDK macros' tests pin [`CORE_RECORDS`] against `miden.wit`, so the WIT generator and the
//! note schema reader, which check manifest and WIT types against these records, follow the SDK.

use alloc::vec::Vec;

/// The type of one field of a [`CoreRecord`], as `miden.wit` declares it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoreField {
    /// A field element: WIT `f32`, the backing type of the `felt` record.
    Felt,
    /// WIT `u8`.
    U8,
    /// WIT `u16`.
    U16,
    /// WIT `u32`.
    U32,
    /// Another core record, by name.
    Record(&'static str),
}

/// The scalar a [`CoreRecord`] flattens to at one position; each occupies one field element.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoreLeaf {
    /// A field element.
    Felt,
    /// `u8`.
    U8,
    /// `u16`.
    U16,
    /// `u32`.
    U32,
}

/// One record of the SDK's core types.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CoreRecord {
    /// The record's WIT name, e.g. `account-id`.
    pub name: &'static str,
    /// The fields, `(WIT name, type)`, in declaration order.
    pub fields: &'static [(&'static str, CoreField)],
}

impl CoreRecord {
    /// The field names, in declaration order.
    pub fn field_names(&self) -> impl Iterator<Item = &'static str> {
        self.fields.iter().map(|(name, _)| *name)
    }

    /// The scalars the record flattens to, field by field in declaration order, nested records
    /// included.
    pub fn leaves(&self) -> Vec<CoreLeaf> {
        let mut out = Vec::new();
        for (_, field) in self.fields {
            field.push_leaves(&mut out);
        }
        out
    }
}

impl CoreField {
    /// The scalars this field flattens to, in order.
    pub fn leaves(&self) -> Vec<CoreLeaf> {
        let mut out = Vec::new();
        self.push_leaves(&mut out);
        out
    }

    /// Append the scalars this field flattens to onto `out`.
    fn push_leaves(&self, out: &mut Vec<CoreLeaf>) {
        match self {
            Self::Felt => out.push(CoreLeaf::Felt),
            Self::U8 => out.push(CoreLeaf::U8),
            Self::U16 => out.push(CoreLeaf::U16),
            Self::U32 => out.push(CoreLeaf::U32),
            Self::Record(name) => {
                for (_, field) in core_record(name).expect("core records nest core records").fields
                {
                    field.push_leaves(out);
                }
            }
        }
    }
}

/// The record `name` with `fields`; the table spells its single-field records with it.
const fn inner(name: &'static str, fields: &'static [(&'static str, CoreField)]) -> CoreRecord {
    CoreRecord { name, fields }
}

/// `inner: felt`.
const INNER_FELT: &[(&str, CoreField)] = &[("inner", CoreField::Record("felt"))];

/// `inner: word`.
const INNER_WORD: &[(&str, CoreField)] = &[("inner", CoreField::Record("word"))];

/// Every record of the SDK's core types, in `miden.wit` declaration order.
pub const CORE_RECORDS: &[CoreRecord] = &[
    inner("felt", &[("inner", CoreField::Felt)]),
    CoreRecord {
        name: "word",
        fields: &[
            ("a", CoreField::Record("felt")),
            ("b", CoreField::Record("felt")),
            ("c", CoreField::Record("felt")),
            ("d", CoreField::Record("felt")),
        ],
    },
    inner("digest", INNER_WORD),
    CoreRecord {
        name: "account-id",
        fields: &[("suffix", CoreField::Record("felt")), ("prefix", CoreField::Record("felt"))],
    },
    inner("recipient", INNER_WORD),
    inner("tag", &[("inner", CoreField::U32)]),
    inner("asset-id", INNER_WORD),
    CoreRecord {
        name: "asset",
        fields: &[("id", CoreField::Record("asset-id")), ("value", CoreField::Record("word"))],
    },
    inner("asset-amount", INNER_FELT),
    inner("nonce", INNER_FELT),
    inner("block-number", INNER_FELT),
    inner("account-hash", INNER_WORD),
    inner("block-hash", INNER_WORD),
    inner("storage-value", INNER_WORD),
    inner("storage-root", INNER_WORD),
    inner("account-code-root", INNER_WORD),
    inner("vault-commitment", INNER_WORD),
    inner("note-idx", &[("inner", CoreField::U16)]),
    inner("note-type", &[("inner", CoreField::U8)]),
    inner("note-execution-hint", INNER_FELT),
];

/// The core record named `name`, if the SDK's core types declare one.
pub fn core_record(name: &str) -> Option<&'static CoreRecord> {
    CORE_RECORDS.iter().find(|record| record.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every nested record is itself a core record, and the flattened sizes are the SDK's.
    #[test]
    fn nested_records_resolve() {
        for record in CORE_RECORDS {
            for (_, field) in record.fields {
                if let CoreField::Record(name) = field {
                    assert!(
                        core_record(name).is_some(),
                        "`{}` nests unknown `{name}`",
                        record.name
                    );
                }
            }
        }
        let felts = |name| core_record(name).unwrap().leaves().len();
        assert_eq!(felts("asset"), 8);
        assert_eq!(felts("account-id"), 2);
        assert_eq!(core_record("note-type").unwrap().leaves(), [CoreLeaf::U8]);
    }
}
