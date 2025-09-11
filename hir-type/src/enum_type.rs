use smallvec::SmallVec;

use super::Type;

/// This represents an enum 'sum' type.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct EnumType {
    /// The computed (max) size of this enum type.
    pub(crate) size: u32,

    /// The integer type for the discriminant.  Will be the smallest integer possible to handle all
    /// variant cases.
    pub(crate) discriminant_ty: Type,

    /// Offset to the case payload, based on discriminant and payload alignment.
    pub(crate) payload_offs32: u32,

    /// The variant case types, in the original order specified
    pub(crate) variants: SmallVec<[Option<Type>; 2]>,
}

impl EnumType {
    pub fn new<I: IntoIterator<Item = Option<Type>>>(
        variant_tys: I,
        discriminant_ty: Type,
        payload_offs32: u32,
    ) -> Self {
        let variants: SmallVec<[Option<Type>; 2]> = variant_tys.into_iter().collect();

        let max_variant_size: u32 = variants
            .iter()
            .fold(0, |sz, ty| sz.max(ty.as_ref().map(|ty| ty.size_in_bytes()).unwrap_or(0)))
            .try_into()
            .expect("Invalid variant type: size is larger than 2^32 bytes.");

        EnumType {
            size: payload_offs32 + max_variant_size,
            discriminant_ty,
            payload_offs32,
            variants,
        }
    }

    pub fn discriminant_ty(&self) -> &Type {
        &self.discriminant_ty
    }

    pub fn payload_offs(&self) -> u32 {
        self.payload_offs32
    }

    pub fn variants(&self) -> &[Option<Type>] {
        self.variants.as_slice()
    }

    pub fn min_alignment(&self) -> usize {
        self.discriminant_ty
            .min_alignment()
            .max(self.variants.iter().fold(0, |align, opt_ty| {
                opt_ty.as_ref().map(|ty| align.max(ty.min_alignment())).unwrap_or(align)
            }))
    }
}

impl miden_formatting::prettier::PrettyPrint for EnumType {
    fn render(&self) -> miden_formatting::prettier::Document {
        use miden_formatting::prettier::*;

        let single_line =
            self.variants.iter().enumerate().fold(Document::Empty, |doc, (i, variant)| {
                let var_name = const_text("case") + display(i);
                let var_ty = variant
                    .as_ref()
                    .map(|ty| const_text("(") + ty.render() + const_text(")"))
                    .unwrap_or(Document::Empty);

                if i > 0 {
                    doc + const_text(", ") + var_name + var_ty
                } else {
                    var_name + var_ty
                }
            });

        let multi_line = indent(4,
            self.variants.iter().enumerate().fold(Document::Empty, |doc, (i, variant)| {
                let var_name = const_text("case") + display(i);
                let var_ty = variant
                    .as_ref()
                    .map(|ty| const_text("(") + ty.render() + const_text(")"))
                    .unwrap_or(Document::Empty);

                doc + nl() + var_name + var_ty + const_text(",")
            }));

        let body = const_text("{") + (single_line | multi_line) + const_text("}");

        const_text("enum ") + body
    }
}
