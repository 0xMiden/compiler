//! Manifest types as WIT types: the mapping table, the flat value counts, and the type
//! declarations one interface collects.

use std::collections::BTreeSet;

use midenc_frontend_wasm_metadata::namespace::CORE_TYPES_INTERFACE_ID;
use midenc_hir_type::{EnumRef, EnumType, StructRef, StructType, Type};
use midenc_package_interface::abi::{WasmScalar, flatten_type, is_word};

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
    pub core: BTreeSet<String>,
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

/// The manifest name of a named struct or enum, the hint a parameter is named after; for a type of
/// the SDK's core types (see [`sdk_core_item`]), the name of the core item.
pub fn type_name(ty: &Type) -> Option<String> {
    let name = match ty {
        Type::Struct(st) => st.name()?.to_string(),
        Type::Enum(en) => en.name().to_string(),
        _ => return None,
    };
    Some(sdk_core_item(&name).map(str::to_owned).unwrap_or(name))
}

/// The core-types item a manifest type named `<CORE_TYPES_INTERFACE_ID>/<item>` stands for, e.g.
/// `asset` for `miden:base/core-types@1.0.0/asset`: a Rust-built component's manifest names the
/// SDK types it uses by their component-model id.
fn sdk_core_item(name: &str) -> Option<&str> {
    name.strip_prefix(CORE_TYPES_INTERFACE_ID)?.strip_prefix('/')
}

impl TypeSet {
    /// The names of the types the interface uses or declares.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.core
            .iter()
            .map(String::as_str)
            .chain(self.locals.iter().map(|(name, _)| name.as_str()))
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
            // One core Wasm `i64`, but two operand stack elements on Miden. `function` (in the
            // crate root) leaves out a procedure with such a parameter or result: no binding
            // exercises the limb order of a call to a MASM callee yet.
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

    /// The mapping of the struct or enum `ty` named `name` when the name alone decides it: a type
    /// of the SDK's core types (see [`sdk_core_item`]) is that core item, and a name that is not a
    /// Miden identifier path names a type from elsewhere, which is unsupported. `None` for a Miden
    /// identifier path.
    fn map_by_name(&mut self, ty: &Type, name: &str) -> Result<Option<Mapped>, String> {
        if let Some(item) = sdk_core_item(name) {
            if !naming::is_valid(item) {
                return Err(format!("unsupported type `{name}`: `{item}` is no WIT name"));
            }
            // The layout is the SDK's own by construction, so the manifest type is counted as it
            // is rather than checked against the item's shape.
            let flat = flatten_type(ty).map_err(|ty| format!("unsupported type `{ty}`"))?;
            let felts = flat
                .iter()
                .map(|flat| match flat.scalar {
                    WasmScalar::I64 | WasmScalar::U64 => 2,
                    _ => 1,
                })
                .sum();
            self.use_core(item)?;
            return Ok(Some(Mapped {
                wit: item.to_owned(),
                values: flat.len(),
                felts,
            }));
        }
        if name.split("::").any(|segment| segment.contains([':', '/', '@', '.'])) {
            return Err(format!(
                "unsupported type `{name}`: the name is neither a Miden identifier path nor a \
                 type of the SDK's `{CORE_TYPES_INTERFACE_ID}`"
            ));
        }
        Ok(None)
    }

    /// Map the named struct `ty` (`st`) to its core item, or else to a local record.
    fn map_struct(&mut self, ty: &Type, st: &StructType) -> Result<Mapped, String> {
        let Some(name) = st.name() else {
            return Err(format!("unsupported type `{ty}`: an anonymous struct has no WIT name"));
        };
        if let Some(mapped) = self.map_by_name(ty, &name)? {
            return Ok(mapped);
        }
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
            let ident = naming::rust_ident(field_name)
                .map_err(|err| format!("unsupported type `{name}`: field {err}"))?;
            fields.push((field_name, ident, mapped.wit));
        }
        // The Rust (snake case) field names are unique when the WIT (kebab case) ones are.
        unique_idents(
            &name,
            "fields",
            "WIT",
            fields.iter().map(|(from, to, _)| (&***from, to.as_str())),
        )?;
        let fields = fields.into_iter().map(|(_, ident, wit)| (ident, wit)).collect();
        let wit = local_ident(&name)?;
        self.declare(wit_name, Decl::Record(fields))?;
        Ok(Mapped { wit, values, felts })
    }

    /// Map the enum `ty` (`en`) to its core item, or else to a local enum when it is C-like with
    /// contiguous discriminants from zero.
    fn map_enum(&mut self, ty: &Type, en: &EnumType) -> Result<Mapped, String> {
        let name = en.name();
        if let Some(mapped) = self.map_by_name(ty, name)? {
            return Ok(mapped);
        }
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
        // wit-bindgen spells the cases in upper camel case, which can merge cases that are
        // distinct in WIT (`slot1`, `slot-1`), so both spellings must be unique.
        let mut cases = Vec::with_capacity(en.variants().len());
        let mut rust_cases = Vec::with_capacity(en.variants().len());
        for variant in en.variants() {
            let case = naming::ident(&variant.name)
                .map_err(|err| format!("unsupported type `{name}`: case {err}"))?;
            let rust = naming::rust_case_ident(&variant.name, &case)
                .map_err(|err| format!("unsupported type `{name}`: case {err}"))?;
            cases.push(case);
            rust_cases.push(rust);
        }
        let variant_names = || en.variants().iter().map(|variant| &*variant.name);
        unique_idents(name, "cases", "WIT", variant_names().zip(cases.iter().map(String::as_str)))?;
        unique_idents(
            name,
            "cases",
            "Rust",
            variant_names().zip(rust_cases.iter().map(String::as_str)),
        )?;
        let wit = local_ident(name)?;
        self.declare(wit_name, Decl::Enum(cases))?;
        Ok(Mapped::scalar(&wit, 1))
    }

    /// Record the core item `name` as used; an error when a local declaration already has its
    /// name or its Rust spelling.
    fn use_core(&mut self, name: &str) -> Result<(), String> {
        if self.locals.iter().any(|(local, _)| local == name) {
            return Err(conflict(name));
        }
        self.check_rust_name(name)?;
        self.core.insert(name.to_owned());
        Ok(())
    }

    /// Declare the local type `name`; declaring an identical type twice is a no-op, while a
    /// different type or a used core item of the same name, or another type of the same Rust
    /// spelling, is an error.
    fn declare(&mut self, name: String, decl: Decl) -> Result<(), String> {
        if self.core.contains(name.as_str()) {
            return Err(conflict(&name));
        }
        match self.locals.iter().find(|(local, _)| *local == name) {
            Some((_, existing)) if *existing == decl => Ok(()),
            Some(_) => Err(conflict(&name)),
            None => {
                self.check_rust_name(&name)?;
                self.locals.push((name, decl));
                Ok(())
            }
        }
    }

    /// An error when a type of another WIT name in the set has the Rust spelling of the type
    /// `name` (see [`naming::rust_type_name`]): `slot1` and `slot-1` are both `Slot1`.
    fn check_rust_name(&self, name: &str) -> Result<(), String> {
        let rust = naming::rust_type_name(name);
        match self
            .names()
            .find(|other| *other != name && naming::rust_type_name(other) == rust)
        {
            Some(other) => Err(format!(
                "types `{other}` and `{name}` in one interface both have the Rust name `{rust}`"
            )),
            None => Ok(()),
        }
    }
}

/// The Rust prelude types the SDK's foreign procedure call bindings leave unqualified, so a local
/// type of the same Rust spelling would be taken for them.
const PRELUDE_TYPES: &[&str] = &["Option", "Result", "String", "Vec"];

/// The WIT spelling of the local declaration of the manifest type `name`, or why it has none: it
/// has no WIT spelling, or its Rust spelling (see [`naming::rust_type_ident`]) is `Self`,
/// `Guest_` or one of the [`PRELUDE_TYPES`].
fn local_ident(name: &str) -> Result<String, String> {
    let short = naming::short_name(name);
    let ident = naming::ident(short).map_err(|err| format!("unsupported type `{name}`: {err}"))?;
    let rust = naming::rust_type_ident(short, &ident)
        .map_err(|err| format!("unsupported type `{name}`: {err}"))?;
    if PRELUDE_TYPES.contains(&rust.as_str()) {
        return Err(format!(
            "unsupported type `{name}`: its Rust name `{rust}` clashes with the Rust prelude's \
             `{rust}`, which the SDK's foreign procedure call bindings leave unqualified"
        ));
    }
    // The SDK's foreign procedure call bindings name a dependency's types in plain upper camel
    // case, so they would miss the type wit-bindgen renames.
    if rust == "Guest_" {
        return Err(format!(
            "unsupported type `{name}`: wit-bindgen renames it to `Guest_` in the generated \
             bindings, which the SDK's foreign procedure call bindings do not follow"
        ));
    }
    Ok(ident)
}

/// Check that the members of the type `type_name` (its `kind`, e.g. "fields") keep distinct names
/// in the `form` ("WIT" or "Rust"); `members` pairs each manifest name with its name in that form.
fn unique_idents<'a>(
    type_name: &str,
    kind: &str,
    form: &str,
    members: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> Result<(), String> {
    let mut seen: Vec<(&str, &str)> = Vec::new();
    for (from, to) in members {
        if let Some((other, _)) = seen.iter().find(|(_, seen)| *seen == to) {
            return Err(format!(
                "unsupported type `{type_name}`: {kind} `{other}` and `{from}` both have the \
                 {form} name `{to}`"
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

    /// A type maps to a core item only with the exact core shape, field order included.
    #[test]
    fn core_items_need_the_exact_shape() {
        let mut set = TypeSet::default();
        let asset = record("Asset", &[("id", word()), ("value", word())]);
        assert_eq!(set.map(&asset).unwrap().wit, "asset");
        let id = record("AccountId", &[("prefix", Type::Felt), ("suffix", Type::Felt)]);
        assert_eq!(set.map(&id).unwrap().wit, "account-id");
        assert_eq!(set.core.iter().collect::<Vec<_>>(), ["account-id", "asset"]);
        assert!(set.locals.is_empty());

        // Field order is part of the shape: the standards' `{suffix, prefix}` is a local record.
        let mut set = TypeSet::default();
        let swapped = record("AccountId", &[("suffix", Type::Felt), ("prefix", Type::Felt)]);
        let mapped = set.map(&swapped).unwrap();
        assert_eq!((mapped.wit.as_str(), mapped.felts), ("account-id", 2));
        assert_eq!(set.core.iter().collect::<Vec<_>>(), ["felt"]);
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

    /// Enums map to the core enum, a local enum, or nothing when discriminants have gaps.
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

    /// Types with clashing, invalid or unusable type, field or case names are unsupported.
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
        let keyword_field = record("Seed", &[("gen", Type::Felt)]);
        assert_eq!(
            set.map(&keyword_field).unwrap_err(),
            "unsupported type `Seed`: field `gen` would be the Rust keyword `gen` in the \
             generated bindings, which wit-bindgen does not escape"
        );
        let self_case = c_enum("Mode", &[("SELF", 0), ("OTHER", 1)]);
        assert_eq!(
            set.map(&self_case).unwrap_err(),
            "unsupported type `Mode`: case `SELF` would be the Rust keyword `Self` in the \
             generated bindings, which wit-bindgen does not escape"
        );
        let rust_cases = c_enum("Slots", &[("SLOT1", 0), ("SLOT_1", 1)]);
        assert_eq!(
            set.map(&rust_cases).unwrap_err(),
            "unsupported type `Slots`: cases `SLOT1` and `SLOT_1` both have the Rust name `Slot1`"
        );
        let guest = record("Guest", &[("x", Type::Felt)]);
        assert_eq!(
            set.map(&guest).unwrap_err(),
            "unsupported type `Guest`: wit-bindgen renames it to `Guest_` in the generated \
             bindings, which the SDK's foreign procedure call bindings do not follow"
        );
        let self_type = record("Self_", &[("x", Type::Felt)]);
        assert_eq!(
            set.map(&self_type).unwrap_err(),
            "unsupported type `Self_`: `Self_` would be the Rust keyword `Self` in the generated \
             bindings, which wit-bindgen does not escape"
        );
        let digit_type = record("_2", &[("x", Type::Felt)]);
        assert_eq!(
            set.map(&digit_type).unwrap_err(),
            "unsupported type `_2`: `_2` has no WIT name (derived `2`)"
        );
        assert!(set.locals.is_empty());
    }

    /// Two local types whose Rust names coincide cannot share one interface.
    #[test]
    fn local_types_need_distinct_rust_names() {
        let mut set = TypeSet::default();
        let slot1 = record("Slot1", &[("x", Type::U32)]);
        let slot_1 = record("Slot_1", &[("x", Type::U32)]);
        assert_eq!(set.map(&slot1).unwrap().wit, "slot1");
        assert_eq!(
            set.map(&slot_1).unwrap_err(),
            "types `slot1` and `slot-1` in one interface both have the Rust name `Slot1`"
        );
        assert_eq!(set.locals.len(), 1);
    }

    /// A record's flat value and stack element counts are the sums over its fields.
    #[test]
    fn flat_counts_add_up() {
        let mut set = TypeSet::default();
        let pair = record("Pair", &[("lo", Type::U64), ("hi", word())]);
        let mapped = set.map(&pair).unwrap();
        assert_eq!((mapped.values, mapped.felts), (5, 6));
    }

    /// The error for an unsupported type names the innermost unsupported type.
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

    /// The name of a type of the SDK's core types, as a Rust-built component's manifest spells it.
    fn sdk_name(item: &str) -> String {
        format!("{CORE_TYPES_INTERFACE_ID}/{item}")
    }

    /// The Rust-built layout of the core `felt`: `{ inner: felt }`.
    fn sdk_felt() -> Type {
        record(&sdk_name("felt"), &[("inner", Type::Felt)])
    }

    /// The Rust-built layout of the core `word`: four `felt` records.
    fn sdk_word() -> Type {
        let felt = sdk_felt();
        record(
            &sdk_name("word"),
            &[("a", felt.clone()), ("b", felt.clone()), ("c", felt.clone()), ("d", felt)],
        )
    }

    /// Types a Rust-built component names by their core-types id map to those core items.
    #[test]
    fn sdk_core_type_ids_map_to_core_items() {
        let mut set = TypeSet::default();
        let asset_id = record(&sdk_name("asset-id"), &[("inner", sdk_word())]);
        let asset = record(&sdk_name("asset"), &[("id", asset_id), ("value", sdk_word())]);
        assert_eq!(
            set.map(&asset).unwrap(),
            Mapped {
                wit: "asset".to_owned(),
                values: 8,
                felts: 8
            }
        );
        assert_eq!(
            set.map(&sdk_word()).unwrap(),
            Mapped {
                wit: "word".to_owned(),
                values: 4,
                felts: 4
            }
        );
        assert_eq!(set.map(&sdk_felt()).unwrap(), Mapped::scalar("felt", 1));
        let note_type = record(&sdk_name("note-type"), &[("inner", Type::U8)]);
        assert_eq!(set.map(&note_type).unwrap(), Mapped::scalar("note-type", 1));
        let tag = record(&sdk_name("tag"), &[("inner", Type::U32)]);
        assert_eq!(set.map(&tag).unwrap(), Mapped::scalar("tag", 1));
        let note_idx = record(&sdk_name("note-idx"), &[("inner", Type::U16)]);
        assert_eq!(set.map(&note_idx).unwrap(), Mapped::scalar("note-idx", 1));
        // Only the items themselves are used, not the items their fields are.
        assert_eq!(
            set.core.iter().collect::<Vec<_>>(),
            ["asset", "felt", "note-idx", "note-type", "tag", "word"]
        );
        assert!(set.locals.is_empty());
        assert_eq!(type_name(&asset).as_deref(), Some("asset"));
    }

    /// A type named by the id of a WIT interface other than the SDK's core types is unsupported.
    #[test]
    fn foreign_wit_type_ids_are_unsupported() {
        let mut set = TypeSet::default();
        let thing = record("other:pkg/iface@1.0.0/thing", &[("x", Type::Felt)]);
        assert_eq!(
            set.map(&thing).unwrap_err(),
            "unsupported type `other:pkg/iface@1.0.0/thing`: the name is neither a Miden \
             identifier path nor a type of the SDK's `miden:base/core-types@1.0.0`"
        );
        assert!(set.core.is_empty() && set.locals.is_empty());
    }

    /// Local types whose Rust names are the prelude types the FPI bindings leave unqualified are
    /// unsupported.
    #[test]
    fn local_types_named_like_prelude_types_are_unsupported() {
        let mut set = TypeSet::default();
        for name in ["Result", "Option", "String", "Vec"] {
            let ty = record(name, &[("x", Type::Felt)]);
            assert_eq!(
                set.map(&ty).unwrap_err(),
                format!(
                    "unsupported type `{name}`: its Rust name `{name}` clashes with the Rust \
                     prelude's `{name}`, which the SDK's foreign procedure call bindings leave \
                     unqualified"
                )
            );
        }
        assert!(set.locals.is_empty());
    }
}
