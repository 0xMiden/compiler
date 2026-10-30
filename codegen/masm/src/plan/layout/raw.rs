//! Bounded compatibility adapter for midenc-hir-type's aggregate remainder-splitting bugs.
//!
//! Project lanes from the original layout, never from a reconstructed remainder. This is only
//! called after ValueLayout validates the entire value fits in sixteen bytes. See
//! tasks/fix-hir-type-aggregate-splitting.md for upstream repair and removal criteria.
use alloc::vec::Vec;

use midenc_hir::{ArrayType, NameAndType, PointerType, StructType, Type, TypeRepr};

pub(super) fn parts(ty: &Type) -> Vec<Type> {
    let end = extent(ty);
    (0..end)
        .step_by(4)
        .map(|start| project(ty, start, end.min(start + 4)))
        .collect()
}

/// Raw splitting historically elides trailing struct padding, but keeps complete small types.
/// Interior padding still occupies its original byte positions and can span entire lanes.
fn extent(ty: &Type) -> usize {
    if ty.size_in_bytes() <= 4 {
        return ty.size_in_bytes();
    }
    match ty {
        Type::Struct(structure) => structure
            .get()
            .fields()
            .iter()
            .filter(|field| field.ty.size_in_bytes() != 0)
            .map(|field| field.offset as usize + extent(&field.ty))
            .max()
            .unwrap_or(0),
        Type::Array(array) => {
            stride(array.element_type()) * (array.len() - 1) + extent(array.element_type())
        }
        _ => ty.size_in_bytes(),
    }
}

fn stride(ty: &Type) -> usize {
    let alignment = ty.min_alignment();
    ty.size_in_bytes().div_ceil(alignment) * alignment
}

fn bytes(len: usize) -> Type {
    if len == 1 {
        Type::U8
    } else {
        Type::from(ArrayType::new(Type::U8, len))
    }
}

/// A lane fragment retains its byte position and optional source field name.
struct Fragment {
    offset: usize,
    value: NameAndType,
}

fn project(ty: &Type, start: usize, end: usize) -> Type {
    let part = project_fragment(ty, start, end);
    // A recursive fragment may occupy less than a lane. Constructor-introduced padding must
    // never enlarge it past its original interval before an enclosing group is reconstructed.
    debug_assert!((1..=end - start).contains(&part.size_in_bytes()), "{ty:?}: {start}..{end}");
    part
}

fn project_fragment(ty: &Type, start: usize, end: usize) -> Type {
    if start == 0 && end == ty.size_in_bytes() {
        return ty.clone();
    }
    match ty {
        Type::Struct(structure) => {
            let structure = structure.get();
            let fragments = structure
                .fields()
                .iter()
                .filter_map(|field| {
                    let offset = field.offset as usize;
                    let lo = start.max(offset);
                    let hi = end.min(offset + field.ty.size_in_bytes());
                    (lo < hi).then(|| Fragment {
                        offset: lo - start,
                        value: NameAndType {
                            name: if lo == offset && hi == offset + field.ty.size_in_bytes() {
                                field.name.clone()
                            } else {
                                None
                            },
                            ty: project(&field.ty, lo - offset, hi - offset),
                        },
                    })
                })
                .collect();
            combine(fragments, structure.repr(), end - start)
        }
        Type::Array(array) => {
            let element = array.element_type();
            let stride = stride(element);
            // Retain array lane types when the slice is a whole, contiguous run of elements.
            if start.is_multiple_of(stride)
                && (end - start).is_multiple_of(stride)
                && stride == element.size_in_bytes()
            {
                let count = (end - start) / stride;
                return if count == 1 {
                    element.clone()
                } else {
                    Type::from(ArrayType::new(element.clone(), count))
                };
            }
            let fragments = (start / stride..=((end - 1) / stride))
                .filter_map(|index| {
                    let offset = index * stride;
                    let lo = start.max(offset);
                    let hi = end.min(offset + element.size_in_bytes());
                    (lo < hi).then(|| Fragment {
                        offset: lo - start,
                        value: project(element, lo - offset, hi - offset).into(),
                    })
                })
                .collect();
            combine(fragments, TypeRepr::packed(1), end - start)
        }
        Type::List(element) => {
            let representation = Type::from(StructType::new([
                Type::U32,
                Type::from(PointerType::new(element.as_ref().clone())),
            ]));
            project(&representation, start, end)
        }
        _ => bytes(end - start),
    }
}

fn combine(fragments: Vec<Fragment>, repr: TypeRepr, width: usize) -> Type {
    if fragments.is_empty() {
        return bytes(width);
    }
    if fragments.len() == 1 && fragments[0].offset == 0 {
        return fragments.into_iter().next().unwrap().value.ty;
    }
    // Preserve the existing field-group Type when its public constructor reproduces all offsets.
    // Groups whose alignment padding exceeds this slice use packed, explicit padding below.
    if !matches!(repr, TypeRepr::Transparent) && repr.min_alignment().is_none_or(|align| align <= 4)
    {
        let candidate = StructType::new_with_repr(
            repr,
            fragments.iter().map(|fragment| NameAndType {
                name: fragment.value.name.clone(),
                ty: fragment.value.ty.clone(),
            }),
        );
        if candidate.size() <= width
            && candidate
                .fields()
                .iter()
                .zip(&fragments)
                .all(|(field, fragment)| field.offset as usize == fragment.offset)
        {
            return Type::from(candidate);
        }
    }
    let mut fields = Vec::<NameAndType>::new();
    let mut offset = 0;
    for fragment in fragments {
        if fragment.offset > offset {
            fields.push(bytes(fragment.offset - offset).into());
        }
        offset = fragment.offset + fragment.value.ty.size_in_bytes();
        fields.push(fragment.value);
    }
    Type::from(StructType::new_with_repr(TypeRepr::packed(1), fields))
}
