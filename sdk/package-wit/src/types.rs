//! Manifest types as WIT types: the mapping table, the flat value counts, and the type
//! declarations one interface collects.

use std::collections::BTreeSet;

use midenc_hir_type::{EnumRef, EnumType, StructRef, StructType, Type};

use crate::naming;

/// A manifest type seen from WIT.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mapped {
    /// The WIT type, as written in a signature.
    pub wit: String,
    /// The number of core Wasm values the type flattens to under the canonical ABI.
    pub values: usize,
    /// The number of operand stack elements those values occupy on Miden.
    pub felts: usize,
}

impl Mapped {
    /// A type that flattens to one core Wasm value occupying `felts` operand stack elements.
    fn scalar(wit: &str, felts: usize) -> Self {
        Self {
            wit: wit.to_owned(),
            values: 1,
            felts,
        }
    }
}

/// A type the interface declares itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decl {
    /// `record name { field: type, .. }`, fields in manifest order.
    Record(Vec<(String, String)>),
    /// `enum name { case, .. }`, cases in discriminant order.
    Enum(Vec<String>),
}

/// The types one interface needs: the core-types items it `use`s and the types it declares.
#[derive(Debug, Clone, Default)]
pub struct TypeSet {
    /// The core-types items used, by WIT name.
    pub core: BTreeSet<&'static str>,
    /// The declared types, by WIT name, in the order they were first needed (a record after the
    /// types of its fields).
    pub locals: Vec<(String, Decl)>,
}

/// A core-types item a manifest type maps to when its shape matches the item exactly.
struct CoreItem {
    /// The item's WIT name, which is also the kebab form of the manifest type name it replaces.
    name: &'static str,
    /// The shape check.
    matches: fn(&Type) -> bool,
    /// The operand stack elements (and core Wasm values) the item flattens to.
    felts: usize,
}

/// The core-types items a named manifest type can stand for.
///
/// A name alone is not enough: the manifest type must have the same flattened layout as the item,
/// because a WIT record lowers field by field in declaration order. The fields need not have the
/// item's own field types: the core `asset` is `{ id: asset-id, value: word }` with
/// `asset-id { inner: word }`, so it matches a manifest `{ id: word, value: word }`, and the core
/// `note-type` is a record `{ inner: u8 }`, which matches a `u8` C-like enum. A manifest type with
/// the right name but another layout becomes a local declaration instead.
const CORE_ITEMS: &[CoreItem] = &[
    CoreItem {
        name: "asset",
        matches: |ty| struct_shape(ty, &[("id", is_word), ("value", is_word)]),
        felts: 8,
    },
    CoreItem {
        name: "account-id",
        matches: |ty| struct_shape(ty, &[("prefix", Type::is_felt), ("suffix", Type::is_felt)]),
        felts: 2,
    },
    CoreItem {
        name: "note-type",
        matches: |ty| match ty {
            Type::Enum(EnumRef::Plain(en)) => {
                en.discriminant() == &Type::U8
                    && en.is_c_like()
                    && en
                        .variants()
                        .iter()
                        .map(|variant| &*variant.name)
                        .zip(en.discriminant_values())
                        .eq([("PRIVATE", 0), ("PUBLIC", 1)])
            }
            _ => false,
        },
        felts: 1,
    },
];

/// `[felt; 4]`, the core `word`.
fn is_word(ty: &Type) -> bool {
    matches!(ty, Type::Array(array) if array.element_type() == &Type::Felt && array.len() == 4)
}

/// A field of an expected struct shape: its name and a check of its type.
type FieldShape = (&'static str, fn(&Type) -> bool);

/// Whether `ty` is a plain struct with exactly these fields, in this order.
fn struct_shape(ty: &Type, fields: &[FieldShape]) -> bool {
    let Type::Struct(StructRef::Plain(st)) = ty else {
        return false;
    };
    st.fields().len() == fields.len()
        && st
            .fields()
            .iter()
            .zip(fields)
            .all(|(field, (name, check))| field.name.as_deref() == Some(*name) && check(&field.ty))
}

/// The manifest name of a named struct or enum, the hint a parameter is named after.
pub fn type_name(ty: &Type) -> Option<String> {
    match ty {
        Type::Struct(st) => st.name().map(|name| name.to_string()),
        Type::Enum(en) => Some(en.name().to_string()),
        _ => None,
    }
}

impl TypeSet {
    /// Whether the interface already has a type named `name`, used or declared.
    pub fn contains(&self, name: &str) -> bool {
        self.core.contains(name) || self.locals.iter().any(|(local, _)| local == name)
    }

    /// Map `ty`, recording the core items and declarations it needs. The error is the reason the
    /// type has no WIT form here.
    ///
    /// On an error the set may keep part of what the type needed, so a caller that must not see
    /// that maps on a clone, as `function` (in the crate root) does for each procedure.
    pub fn map(&mut self, ty: &Type) -> Result<Mapped, String> {
        match ty {
            Type::Felt => {
                self.use_core("felt")?;
                Ok(Mapped::scalar("felt", 1))
            }
            Type::I1 => Ok(Mapped::scalar("bool", 1)),
            Type::U8 => Ok(Mapped::scalar("u8", 1)),
            Type::I8 => Ok(Mapped::scalar("s8", 1)),
            Type::U16 => Ok(Mapped::scalar("u16", 1)),
            Type::I16 => Ok(Mapped::scalar("s16", 1)),
            Type::U32 => Ok(Mapped::scalar("u32", 1)),
            Type::I32 => Ok(Mapped::scalar("s32", 1)),
            // One core Wasm `i64`, but two operand stack elements on Miden.
            Type::U64 => Ok(Mapped::scalar("u64", 2)),
            Type::I64 => Ok(Mapped::scalar("s64", 2)),
            ty if is_word(ty) => {
                self.use_core("word")?;
                Ok(Mapped {
                    wit: "word".to_owned(),
                    values: 4,
                    felts: 4,
                })
            }
            Type::Struct(StructRef::Plain(st)) => self.map_struct(ty, st),
            Type::Enum(EnumRef::Plain(en)) => self.map_enum(ty, en),
            Type::Struct(StructRef::Rec(_)) | Type::Enum(EnumRef::Rec(_)) => Err(format!(
                "unsupported type `{}`: a recursive type has no WIT form",
                type_name(ty).unwrap_or_default()
            )),
            _ => Err(format!("unsupported type `{ty}`")),
        }
    }

    /// The core item `ty` stands for, recorded as used, when it matches one exactly.
    fn core_item(&mut self, wit_name: &str, ty: &Type) -> Result<Option<Mapped>, String> {
        let Some(item) = CORE_ITEMS.iter().find(|item| item.name == wit_name && (item.matches)(ty))
        else {
            return Ok(None);
        };
        self.use_core(item.name)?;
        Ok(Some(Mapped {
            wit: item.name.to_owned(),
            values: item.felts,
            felts: item.felts,
        }))
    }

    /// Map the named struct `ty` (`st`) to its core item, or else to a local record.
    fn map_struct(&mut self, ty: &Type, st: &StructType) -> Result<Mapped, String> {
        let Some(name) = st.name() else {
            return Err(format!("unsupported type `{ty}`: an anonymous struct has no WIT name"));
        };
        let wit_name = naming::kebab(naming::short_name(&name));
        if let Some(mapped) = self.core_item(&wit_name, ty)? {
            return Ok(mapped);
        }
        if st.fields().is_empty() {
            return Err(format!("unsupported type `{name}`: a WIT record needs a field"));
        }
        let mut fields = Vec::with_capacity(st.fields().len());
        let (mut values, mut felts) = (0, 0);
        for field in st.fields() {
            let Some(field_name) = &field.name else {
                return Err(format!("unsupported type `{name}`: a field has no name"));
            };
            let mapped = self.map(&field.ty)?;
            values += mapped.values;
            felts += mapped.felts;
            let ident = naming::ident(field_name)
                .map_err(|err| format!("unsupported type `{name}`: field {err}"))?;
            fields.push((field_name, ident, mapped.wit));
        }
        unique_idents(&name, "fields", fields.iter().map(|(from, to, _)| (&***from, to.as_str())))?;
        let fields = fields.into_iter().map(|(_, ident, wit)| (ident, wit)).collect();
        let wit = local_ident(&name)?;
        self.declare(wit_name, Decl::Record(fields))?;
        Ok(Mapped { wit, values, felts })
    }

    /// Map the enum `ty` (`en`) to its core item, or else to a local enum when it is C-like with
    /// contiguous discriminants from zero.
    fn map_enum(&mut self, ty: &Type, en: &EnumType) -> Result<Mapped, String> {
        let name = en.name();
        let wit_name = naming::kebab(naming::short_name(name));
        if let Some(mapped) = self.core_item(&wit_name, ty)? {
            return Ok(mapped);
        }
        if en.is_phantom() {
            return Err(format!("unsupported type `{name}`: an enum with no variants"));
        }
        if !en.is_c_like() {
            return Err(format!("unsupported type `{name}`: an enum with payloads"));
        }
        // The discriminant must fit the single 32-bit core value a WIT enum lowers to.
        if !matches!(
            en.discriminant(),
            Type::I1 | Type::U8 | Type::I8 | Type::U16 | Type::I16 | Type::U32 | Type::I32
        ) {
            return Err(format!(
                "unsupported type `{name}`: an enum with a `{}` discriminant",
                en.discriminant()
            ));
        }
        // A WIT enum lowers to the index of its case, so it carries the manifest's discriminant
        // only when the discriminants are exactly the variant indices.
        if !en.discriminant_values().zip(0u128..).all(|(value, index)| value == index) {
            return Err(format!(
                "unsupported type `{name}`: an enum with non-contiguous discriminants"
            ));
        }
        let cases = en
            .variants()
            .iter()
            .map(|variant| {
                naming::ident(&variant.name)
                    .map_err(|err| format!("unsupported type `{name}`: case {err}"))
            })
            .collect::<Result<Vec<String>, String>>()?;
        unique_idents(
            name,
            "cases",
            en.variants()
                .iter()
                .map(|variant| &*variant.name)
                .zip(cases.iter().map(String::as_str)),
        )?;
        let wit = local_ident(name)?;
        self.declare(wit_name, Decl::Enum(cases))?;
        Ok(Mapped::scalar(&wit, 1))
    }

    /// Record the core item `name` as used; an error when a local declaration already has its
    /// name.
    fn use_core(&mut self, name: &'static str) -> Result<(), String> {
        if self.locals.iter().any(|(local, _)| local == name) {
            return Err(conflict(name));
        }
        self.core.insert(name);
        Ok(())
    }

    /// Declare the local type `name`; declaring an identical type twice is a no-op, while a
    /// different type or a used core item of the same name is an error.
    fn declare(&mut self, name: String, decl: Decl) -> Result<(), String> {
        if self.core.contains(name.as_str()) {
            return Err(conflict(&name));
        }
        match self.locals.iter().find(|(local, _)| *local == name) {
            Some((_, existing)) if *existing == decl => Ok(()),
            Some(_) => Err(conflict(&name)),
            None => {
                self.locals.push((name, decl));
                Ok(())
            }
        }
    }
}

/// The WIT spelling of the local declaration of the manifest type `name`, or why it has none.
fn local_ident(name: &str) -> Result<String, String> {
    naming::ident(naming::short_name(name))
        .map_err(|err| format!("unsupported type `{name}`: {err}"))
}

/// Check that the members of the type `type_name` (its `kind`, e.g. "fields") keep distinct WIT
/// names; `members` pairs each manifest name with its WIT name.
fn unique_idents<'a>(
    type_name: &str,
    kind: &str,
    members: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> Result<(), String> {
    let mut seen: Vec<(&str, &str)> = Vec::new();
    for (from, to) in members {
        if let Some((other, _)) = seen.iter().find(|(_, seen)| *seen == to) {
            return Err(format!(
                "unsupported type `{type_name}`: {kind} `{other}` and `{from}` both have the WIT \
                 name `{to}`"
            ));
        }
        seen.push((from, to));
    }
    Ok(())
}

/// The reason two different types named `name` cannot share one interface.
fn conflict(name: &str) -> String {
    format!("conflicting definitions of type `{name}` in one interface")
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use midenc_hir_type::{ArrayType, Variant};

    use super::*;

    /// The manifest type of a word, `[felt; 4]`.
    fn word() -> Type {
        Type::from(ArrayType::new(Type::Felt, 4))
    }

    /// A named struct with `fields`.
    fn record(name: &str, fields: &[(&str, Type)]) -> Type {
        Type::from(StructType::named(
            Arc::from(name),
            fields.iter().map(|(name, ty)| (Arc::from(*name), ty.clone())),
        ))
    }

    /// A C-like enum with a `u8` discriminant and `variants`.
    fn c_enum(name: &str, variants: &[(&str, u128)]) -> Type {
        let variants = variants
            .iter()
            .map(|(name, value)| Variant::c_like(Arc::from(*name), Some(*value)));
        Type::from(EnumType::new(Arc::from(name), Type::U8, variants).unwrap())
    }

    #[test]
    fn core_items_need_the_exact_shape() {
        let mut set = TypeSet::default();
        let asset = record("Asset", &[("id", word()), ("value", word())]);
        assert_eq!(set.map(&asset).unwrap().wit, "asset");
        let id = record("AccountId", &[("prefix", Type::Felt), ("suffix", Type::Felt)]);
        assert_eq!(set.map(&id).unwrap().wit, "account-id");
        assert_eq!(set.core.iter().copied().collect::<Vec<_>>(), ["account-id", "asset"]);
        assert!(set.locals.is_empty());

        // Field order is part of the shape: the standards' `{suffix, prefix}` is a local record.
        let mut set = TypeSet::default();
        let swapped = record("AccountId", &[("suffix", Type::Felt), ("prefix", Type::Felt)]);
        let mapped = set.map(&swapped).unwrap();
        assert_eq!((mapped.wit.as_str(), mapped.felts), ("account-id", 2));
        assert_eq!(set.core.iter().copied().collect::<Vec<_>>(), ["felt"]);
        assert_eq!(
            set.locals,
            [(
                "account-id".to_owned(),
                Decl::Record(vec![
                    ("suffix".to_owned(), "felt".to_owned()),
                    ("prefix".to_owned(), "felt".to_owned())
                ])
            )]
        );
        // ...and the core `account-id` can no longer be used next to it.
        assert!(set.map(&id).unwrap_err().contains("conflicting definitions"));
    }

    #[test]
    fn enums_map_to_core_local_or_nothing() {
        let mut set = TypeSet::default();
        let note_type = c_enum("NoteType", &[("PRIVATE", 0), ("PUBLIC", 1)]);
        assert_eq!(set.map(&note_type).unwrap().wit, "note-type");
        assert!(set.core.contains("note-type"));

        let kind = c_enum("AuthKind", &[("AUTH_CONTROLLED", 0), ("OWNER_CONTROLLED", 1)]);
        assert_eq!(set.map(&kind).unwrap(), Mapped::scalar("auth-kind", 1));
        assert_eq!(
            set.locals,
            [(
                "auth-kind".to_owned(),
                Decl::Enum(vec!["auth-controlled".to_owned(), "owner-controlled".to_owned()])
            )]
        );

        let gaps = c_enum("Gaps", &[("A", 0), ("B", 2)]);
        assert!(set.map(&gaps).unwrap_err().contains("non-contiguous discriminants"));
        let note_type_gaps = c_enum("NoteType", &[("PRIVATE", 1), ("PUBLIC", 2)]);
        assert!(set.map(&note_type_gaps).unwrap_err().contains("non-contiguous"));
    }

    #[test]
    fn members_and_types_need_distinct_valid_wit_names() {
        let mut set = TypeSet::default();
        let clash = record("Clash", &[("fooBar", Type::Felt), ("foo_bar", Type::Felt)]);
        assert_eq!(
            set.map(&clash).unwrap_err(),
            "unsupported type `Clash`: fields `fooBar` and `foo_bar` both have the WIT name \
             `foo-bar`"
        );
        let empty = record("Empty", &[("__", Type::Felt)]);
        assert_eq!(
            set.map(&empty).unwrap_err(),
            "unsupported type `Empty`: field `__` has no WIT name (derived ``)"
        );
        let cases = c_enum("Slots", &[("SLOT_A", 0), ("SlotA", 1)]);
        assert_eq!(
            set.map(&cases).unwrap_err(),
            "unsupported type `Slots`: cases `SLOT_A` and `SlotA` both have the WIT name `slot-a`"
        );
        let digit_case = c_enum("Digits", &[("_1", 0)]);
        assert_eq!(
            set.map(&digit_case).unwrap_err(),
            "unsupported type `Digits`: case `_1` has no WIT name (derived `1`)"
        );
        let digit_type = record("_2", &[("x", Type::Felt)]);
        assert_eq!(
            set.map(&digit_type).unwrap_err(),
            "unsupported type `_2`: `_2` has no WIT name (derived `2`)"
        );
        assert!(set.locals.is_empty());
    }

    #[test]
    fn flat_counts_add_up() {
        let mut set = TypeSet::default();
        let pair = record("Pair", &[("lo", Type::U64), ("hi", word())]);
        let mapped = set.map(&pair).unwrap();
        assert_eq!((mapped.values, mapped.felts), (5, 6));
    }

    #[test]
    fn unsupported_types_name_themselves() {
        let mut set = TypeSet::default();
        assert_eq!(set.map(&Type::U128).unwrap_err(), "unsupported type `u128`");
        assert!(
            set.map(&Type::from(ArrayType::new(Type::Felt, 2)))
                .unwrap_err()
                .starts_with("unsupported type `[felt; 2]`")
        );
        let nested = record("Outer", &[("x", Type::F64)]);
        assert_eq!(set.map(&nested).unwrap_err(), "unsupported type `f64`");
    }
}
