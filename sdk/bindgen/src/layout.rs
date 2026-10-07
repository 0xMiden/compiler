//! Which Miden types have a Rust form, and the `#[repr]` of each generated struct.
//!
//! MASM code reads and writes generated structs through pointers, so a struct's Rust layout must
//! be its HIR layout: the same field offsets, size and alignment. The two disagree by default
//! wherever the Rust form of a field is aligned differently from its HIR type: HIR aligns `u64`
//! and `i64` to 4 where wasm32 aligns them to 8, and the SDK's `Word` is 16-byte aligned where HIR
//! aligns `[felt; 4]` to 4. Each struct is therefore written in the first of these forms whose
//! wasm32 layout is its HIR layout:
//!
//! 1. its own `repr` with its `[felt; 4]`s (alone or in arrays) as `Word`;
//! 2. its own `repr` with them as `[Felt; 4]`;
//! 3. `#[repr(C, packed(4))]` with them as `[Felt; 4]`, which places a 64-bit field at a multiple
//!    of 4 as HIR does. Rust allows it unless a field holds a `#[repr(align)]` type: a struct
//!    with `Word` fields, or an `@align` struct.
//!
//! The one difference allowed is the alignment a `Word` brings: a struct holding one is 16-byte
//! aligned in Rust, as the SDK's own types are, so a struct holding *it* must keep it at a
//! multiple of 16 for their offsets to agree. A struct none of the forms fits has no Rust form,
//! and neither has anything that holds it, points at it, or takes it.

use miden_assembly_syntax::ast::types::{EnumType, StructType, Type, TypeRepr};
use midenc_package_interface::abi::is_word;

use crate::{names, types::field_name};

/// The `#[repr]` and `Word` choice a generated struct is written with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StructForm {
    /// The representation, as HIR names it: `Default` is `#[repr(C)]`.
    pub(crate) repr: TypeRepr,
    /// Whether its `[felt; 4]`s, alone or in arrays, are `Word` rather than `[Felt; 4]`.
    pub(crate) words: bool,
}

/// The wasm32 layout of a type's Rust form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Layout {
    size: usize,
    align: usize,
    /// Whether it holds a `#[repr(align)]` type by value (a `Word`, an `@align` struct), which
    /// Rust does not allow in a packed struct.
    aligned: bool,
    /// Whether it holds a `Word` by value, whose alignment may exceed the HIR alignment.
    word: bool,
}

impl Layout {
    const fn scalar(size: usize) -> Self {
        Self {
            size,
            align: size,
            aligned: false,
            word: false,
        }
    }
}

/// Whether `ty` has a Rust form, and why not if it has none.
///
/// A struct or enum without one produces no code; neither does a type export, struct field or
/// signature that holds it or points at it.
pub(crate) fn check(ty: &Type) -> Result<(), String> {
    layout(ty, true, &mut Vec::new()).map(|_| ())
}

/// The form a struct is written in; see the module doc. A struct none of the forms fits, or one of
/// whose fields has no Rust form, has none.
pub(crate) fn struct_form(st: &StructType) -> Result<StructForm, String> {
    struct_layout(st, &mut Vec::new()).map(|(form, _)| form)
}

/// The integer type the C-like enum `en` is represented by. An enum with no variants, with
/// variants that carry values, with a discriminant that is not an integer or with two variants of
/// one Rust name has no Rust form.
pub(crate) fn enum_repr(en: &EnumType) -> Result<&Type, String> {
    let name = en.name();
    if en.variants().is_empty() {
        return Err(format!("enum `{name}` has no variants to represent"));
    }
    if !en.is_c_like() {
        return Err(format!(
            "enum `{name}` has variants that carry values; only an enum of plain discriminants \
             has a Rust form"
        ));
    }
    let discriminant = en.discriminant();
    if !matches!(
        discriminant,
        Type::I8 | Type::U8 | Type::I16 | Type::U16 | Type::I32 | Type::U32 | Type::I64 | Type::U64
    ) {
        return Err(format!(
            "enum `{name}` has a `{discriminant}` discriminant, which no Rust `repr` holds"
        ));
    }
    let variants = en
        .variants()
        .iter()
        .map(|v| (v.name.to_string(), names::variant_ident(&v.name)));
    if let Some((first, second, ident)) = names::duplicate(variants) {
        return Err(format!("variants `{first}` and `{second}` would both be named `{ident}`"));
    }
    Ok(discriminant)
}

/// The layout of `ty`'s Rust form, with a `[felt; 4]` as `Word` if `words`.
///
/// `visiting` holds the structs whose layout is being computed, outermost first: a struct can only
/// refer back to one of them through a pointer, whose pointee is then known to be checked.
fn layout(ty: &Type, words: bool, visiting: &mut Vec<Type>) -> Result<Layout, String> {
    Ok(match ty {
        Type::I1 | Type::I8 | Type::U8 => Layout::scalar(1),
        Type::I16 | Type::U16 => Layout::scalar(2),
        Type::I32 | Type::U32 | Type::Felt => Layout::scalar(4),
        Type::I64 | Type::U64 => Layout::scalar(8),
        Type::I128 | Type::U128 => Layout::scalar(16),
        Type::Ptr(ptr) => {
            // The pointee is written out (`ElementPtr<T>`, `*mut T`), so it needs a Rust form.
            if !visiting.contains(ptr.pointee()) {
                layout(ptr.pointee(), true, visiting)?;
            }
            Layout::scalar(4)
        }
        Type::Array(_) if words && is_word(ty) => Layout {
            size: 16,
            align: 16,
            aligned: true,
            word: true,
        },
        Type::Array(array) => {
            let element = layout(&array.ty, words, visiting)?;
            Layout {
                size: element.size * array.len,
                ..element
            }
        }
        Type::Struct(st) => {
            visiting.push(ty.clone());
            let result = struct_layout(&st.get(), visiting);
            visiting.pop();
            result?.1
        }
        Type::Enum(en) => layout(enum_repr(&en.get())?, words, visiting)?,
        Type::Unknown
        | Type::Never
        | Type::Variadic
        | Type::U256
        | Type::F64
        | Type::List(_)
        | Type::Function(_) => return Err(format!("type `{ty}` has no Rust form in a binding")),
    })
}

/// The form of `st` and the layout it has in that form.
fn struct_layout(
    st: &StructType,
    visiting: &mut Vec<Type>,
) -> Result<(StructForm, Layout), String> {
    let fields = st.fields().iter().enumerate();
    let idents = fields.clone().map(|(index, field)| {
        let name = field_name(field, index);
        let ident = names::item_ident(&name);
        (name, ident)
    });
    if let Some((first, second, ident)) = names::duplicate(idents) {
        return Err(format!("fields `{first}` and `{second}` would both be named `{ident}`"));
    }

    let offsets: Vec<usize> = st.fields().iter().map(|field| field.offset as usize).collect();
    let (size, align) = (st.size(), st.min_alignment());
    let forms = [(st.repr(), true), (st.repr(), false), (TypeRepr::packed(4), false)];
    for (repr, words) in forms {
        let layouts = fields
            .clone()
            .map(|(index, field)| {
                layout(&field.ty, words, visiting)
                    .map_err(|reason| format!("field `{}`: {reason}", field_name(field, index)))
            })
            .collect::<Result<Vec<_>, String>>()?;
        let Some((rust_offsets, rust)) = rust_layout(repr, &layouts) else {
            continue;
        };
        let same_alignment = rust.align == align || (rust.word && rust.align > align);
        if rust_offsets == offsets && rust.size == size && same_alignment {
            return Ok((StructForm { repr, words }, rust));
        }
    }
    Err(format!(
        "no Rust struct has its layout (field offsets {offsets:?}, size {size}, alignment {align})"
    ))
}

/// The field offsets and the layout of a Rust struct with `repr` whose fields have `fields`
/// layouts, as rustc lays it out on wasm32; `None` if rustc rejects the struct.
fn rust_layout(repr: TypeRepr, fields: &[Layout]) -> Option<(Vec<usize>, Layout)> {
    let aligned = fields.iter().any(|field| field.aligned);
    let word = fields.iter().any(|field| field.word);
    // The cap on a field's alignment, and the struct's least alignment.
    let (cap, least) = match repr {
        TypeRepr::Transparent => {
            // One field may have a size or an alignment other than 1, and is the struct's layout.
            let mut nontrivial = fields.iter().filter(|field| field.size != 0 || field.align != 1);
            let only = nontrivial.next();
            if nontrivial.next().is_some() {
                return None;
            }
            let empty = Layout {
                size: 0,
                ..Layout::scalar(1)
            };
            return Some((vec![0; fields.len()], only.copied().unwrap_or(empty)));
        }
        TypeRepr::Packed(align) if aligned || !align.is_power_of_two() => return None,
        TypeRepr::Packed(align) => (usize::from(align.get()), 1),
        TypeRepr::Align(align) if !align.is_power_of_two() => return None,
        TypeRepr::Align(align) => (usize::MAX, usize::from(align.get())),
        TypeRepr::Default => (usize::MAX, 1),
    };
    let mut offset = 0usize;
    let mut align = least;
    let offsets = fields
        .iter()
        .map(|field| {
            let field_align = field.align.min(cap);
            offset = offset.next_multiple_of(field_align);
            let at = offset;
            offset += field.size;
            align = align.max(field_align);
            at
        })
        .collect();
    let layout = Layout {
        size: offset.next_multiple_of(align),
        align,
        aligned: aligned || matches!(repr, TypeRepr::Align(_)),
        word,
    };
    Some((offsets, layout))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use miden_assembly_syntax::ast::types::{ArrayType, PointerType, Variant};

    use super::*;

    fn word() -> Type {
        Type::from(ArrayType::new(Type::Felt, 4))
    }

    fn form(repr: TypeRepr, words: bool) -> Result<StructForm, String> {
        Ok(StructForm { repr, words })
    }

    /// A `[felt; 4]` is `Word` only where the struct keeps it at a multiple of 16 and its own size
    /// is one: otherwise `Word`'s alignment would move it, or pad the struct.
    #[test]
    fn word_fields_become_word_only_at_aligned_offsets() {
        let felt = Type::Felt;
        let default = TypeRepr::Default;
        let aligned = StructType::new([word(), word()]);
        assert_eq!(struct_form(&aligned), form(default, true));
        let unaligned = StructType::new([felt.clone(), word()]);
        assert_eq!(struct_form(&unaligned), form(default, false));
        let trailing = StructType::new([word(), felt.clone()]);
        assert_eq!(struct_form(&trailing), form(default, false), "size 20 is not a multiple of 16");

        // An array of words is as many word fields.
        let words = Type::from(ArrayType::new(word(), 2));
        assert_eq!(struct_form(&StructType::new([words.clone()])), form(default, true));
        assert_eq!(struct_form(&StructType::new([felt, words])), form(default, false));

        // Rust rejects a 16-byte aligned field in a packed struct, wherever it sits.
        let packed = StructType::new_with_repr(TypeRepr::packed(1), [word(), word()]);
        assert_eq!(packed.size(), 32);
        assert_eq!(struct_form(&packed), form(TypeRepr::packed(1), false));
    }

    #[test]
    fn a_struct_without_words_keeps_its_repr() {
        let odd = StructType::new([Type::U8, Type::Felt, Type::Felt]);
        assert_eq!(struct_form(&odd), form(TypeRepr::Default, true));
        // `[u32; 4]` and `[felt; 3]` are not words.
        let not_words = [
            Type::Felt,
            Type::from(ArrayType::new(Type::U32, 4)),
            Type::from(ArrayType::new(Type::Felt, 3)),
        ];
        assert_eq!(struct_form(&StructType::new(not_words)), form(TypeRepr::Default, true));
    }

    /// A struct whose generated type has `Word` fields is 16-byte aligned in Rust, where HIR
    /// aligns it to 4: a struct holding it must keep it at a multiple of 16, and cannot be packed.
    #[test]
    fn a_struct_with_word_fields_is_a_word_field_of_its_holder() {
        let with_words = Type::from(StructType::new([word()]));
        let holder = StructType::new([with_words.clone(), word()]);
        assert_eq!(struct_form(&holder), form(TypeRepr::Default, true));
        let array = StructType::new([Type::from(ArrayType::new(with_words.clone(), 2))]);
        assert_eq!(struct_form(&array), form(TypeRepr::Default, true));
        let misplaced = StructType::new([Type::Felt, with_words]);
        assert_eq!(
            struct_form(&misplaced),
            Err("no Rust struct has its layout (field offsets [0, 4], size 20, alignment 4)".into())
        );

        // A struct whose words are `[Felt; 4]` is 4-byte aligned like its HIR form.
        let unaligned = Type::from(StructType::new([Type::Felt, word()]));
        let holder = StructType::new([Type::Felt, unaligned]);
        assert_eq!(struct_form(&holder), form(TypeRepr::Default, true));
    }

    /// HIR aligns a 64-bit integer to 4, wasm32 to 8: a struct where that moves a field or pads
    /// the struct is packed to 4, and a struct whose HIR alignment is 4 is packed even when
    /// nothing moves.
    #[test]
    fn a_struct_with_a_64_bit_field_is_packed_to_four_where_rust_would_move_it() {
        let default = TypeRepr::Default;
        let packed = TypeRepr::packed(4);
        assert_eq!(struct_form(&StructType::new([Type::U32, Type::U64])), form(packed, false));
        assert_eq!(struct_form(&StructType::new([Type::U64, Type::U32])), form(packed, false));
        assert_eq!(struct_form(&StructType::new([Type::U64])), form(packed, false));
        // Nested, the holder sees the packed struct's alignment, 4.
        let stamp = Type::from(StructType::new([Type::U32, Type::I64]));
        assert_eq!(struct_form(&StructType::new([Type::U32, stamp])), form(default, true));
        // An enum with a 64-bit discriminant is a 64-bit field.
        let variants = [Variant::c_like("A".into(), Some(0))];
        let big = EnumType::new("Big".into(), Type::U64, variants).unwrap();
        let holder = StructType::new([Type::U8, Type::from(big)]);
        assert_eq!(struct_form(&holder), form(packed, false));

        // A `[felt; 4]` is `[Felt; 4]` in a packed struct: `Word` is `#[repr(align(16))]`, which
        // Rust does not allow there. A struct whose `Word` fields stay `Word` cannot be packed.
        let stamped = StructType::new([word(), Type::U64]);
        assert_eq!(struct_form(&stamped), form(packed, false));
        let with_words = Type::from(StructType::new([word()]));
        let err = struct_form(&StructType::new([with_words, Type::U64])).unwrap_err();
        assert!(err.starts_with("no Rust struct has its layout (field offsets [0, 16]"), "{err}");
    }

    /// HIR's `@align` and `@packed` where Rust cannot follow: a packed struct holding an aligned
    /// one (which rustc rejects), and alignments the reprs give differently.
    #[test]
    fn reprs_rust_cannot_mirror_have_no_rust_form() {
        let aligned = Type::from(StructType::new_with_repr(TypeRepr::align(16), [Type::Felt]));
        let packed = StructType::new_with_repr(TypeRepr::packed(1), [Type::U8, aligned]);
        assert!(struct_form(&packed).is_err());
        // `@align(2)` below the natural alignment: HIR's alignment is 2, Rust's 4, so a struct
        // holding it would place it differently.
        let low = StructType::new_with_repr(TypeRepr::align(2), [Type::Felt]);
        assert!(struct_form(&low).is_err());
        // `@packed(8)` above the natural alignment: HIR's alignment is 8, Rust's 4.
        let high = StructType::new_with_repr(TypeRepr::packed(8), [Type::Felt]);
        assert!(struct_form(&high).is_err());
        // Plain reprs stay as they are.
        let spaced = StructType::new_with_repr(TypeRepr::align(16), [Type::Felt, Type::Felt]);
        assert_eq!(struct_form(&spaced), form(TypeRepr::align(16), true));
        let bytes = StructType::new_with_repr(TypeRepr::packed(1), [Type::U8, Type::Felt]);
        assert_eq!(struct_form(&bytes), form(TypeRepr::packed(1), true));
        let wrapper = StructType::new_with_repr(TypeRepr::Transparent, [Type::Felt]);
        assert_eq!(struct_form(&wrapper), form(TypeRepr::Transparent, true));
    }

    #[test]
    fn types_with_no_rust_form_are_rejected_wherever_they_are() {
        assert_eq!(check(&Type::U256), Err("type `u256` has no Rust form in a binding".into()));
        let holder = Type::from(StructType::new([(Arc::from("x"), Type::U256)]));
        assert_eq!(
            check(&holder),
            Err("field `x`: type `u256` has no Rust form in a binding".into())
        );
        let pointer = Type::Ptr(Arc::new(PointerType::new(holder)));
        assert!(check(&pointer).is_err(), "a pointee is written out too");
        assert!(check(&Type::from(ArrayType::new(Type::U256, 2))).is_err());

        // A 128-bit integer has a Rust form in memory: sixteen bytes, 16-aligned on both sides.
        let wide = layout(&Type::U128, true, &mut Vec::new()).unwrap();
        assert_eq!((wide.size, wide.align), (16, 16));

        // Fields of one Rust name.
        let twins =
            StructType::new([(Arc::from("a-b"), Type::Felt), (Arc::from("a_b"), Type::Felt)]);
        assert_eq!(
            struct_form(&twins),
            Err("fields `a-b` and `a_b` would both be named `a_b`".into())
        );
    }
}
