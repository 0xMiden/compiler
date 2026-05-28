use alloc::{format, rc::Rc, vec::Vec};

use smallvec::SmallVec;

use super::rewriter::ConversionPatternRewriter;
use crate::{Report, SourceSpan, Type, ValueRef, dialects::builtin::UnrealizedConversionCast};

type TypeConversionFn = Rc<dyn Fn(&Type) -> Option<TypeConversion>>;
type ValueConversionFn = Rc<dyn Fn(ValueRef) -> Option<TypeConversion>>;
type MaterializationFn = Rc<
    dyn Fn(
        &mut ConversionPatternRewriter,
        ValueRef,
        Type,
        SourceSpan,
    ) -> Result<Option<ValueRef>, Report>,
>;

/// The result of converting one source type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TypeConversion {
    /// The source value is represented by one target value.
    One(Type),
    /// The source value is represented by multiple target values.
    ///
    /// This is reserved by the API, but the Phase 5 driver rejects it when default boundary
    /// materialization would be required.
    Many(SmallVec<[Type; 2]>),
    /// The source value is removed.
    ///
    /// This is reserved by the API, but the Phase 5 driver rejects it when default boundary
    /// materialization would be required.
    Drop,
}

impl TypeConversion {
    #[inline]
    pub fn one(ty: Type) -> Self {
        Self::One(ty)
    }

    #[inline]
    pub fn many(types: impl IntoIterator<Item = Type>) -> Self {
        Self::Many(types.into_iter().collect())
    }

    #[inline]
    pub const fn drop() -> Self {
        Self::Drop
    }
}

/// Converts source IR types and values to target IR types and values.
#[derive(Clone, Default)]
pub struct TypeConverter {
    conversions: Vec<TypeConversionFn>,
    value_conversions: Vec<ValueConversionFn>,
    source_materializations: Vec<MaterializationFn>,
    target_materializations: Vec<MaterializationFn>,
}

impl TypeConverter {
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    #[inline]
    pub fn add_conversion<F>(&mut self, callback: F) -> &mut Self
    where
        F: Fn(&Type) -> Option<TypeConversion> + 'static,
    {
        self.conversions.push(Rc::new(callback));
        self
    }

    #[inline]
    pub fn add_value_conversion<F>(&mut self, callback: F) -> &mut Self
    where
        F: Fn(ValueRef) -> Option<TypeConversion> + 'static,
    {
        self.value_conversions.push(Rc::new(callback));
        self
    }

    #[inline]
    pub fn add_source_materialization<F>(&mut self, callback: F) -> &mut Self
    where
        F: Fn(
                &mut ConversionPatternRewriter,
                ValueRef,
                Type,
                SourceSpan,
            ) -> Result<Option<ValueRef>, Report>
            + 'static,
    {
        self.source_materializations.push(Rc::new(callback));
        self
    }

    #[inline]
    pub fn add_target_materialization<F>(&mut self, callback: F) -> &mut Self
    where
        F: Fn(
                &mut ConversionPatternRewriter,
                ValueRef,
                Type,
                SourceSpan,
            ) -> Result<Option<ValueRef>, Report>
            + 'static,
    {
        self.target_materializations.push(Rc::new(callback));
        self
    }

    /// Convert a type.
    ///
    /// If no callback handles `ty`, it is treated as an identity conversion.
    pub fn convert_type(&self, ty: &Type) -> Option<TypeConversion> {
        self.conversions
            .iter()
            .find_map(|convert| convert(ty))
            .or_else(|| Some(TypeConversion::One(ty.clone())))
    }

    /// Convert a value, allowing value-specific callbacks to override type-only conversion.
    pub fn convert_value(&self, value: ValueRef) -> Option<TypeConversion> {
        self.value_conversions
            .iter()
            .find_map(|convert| convert(value))
            .or_else(|| self.convert_type(value.borrow().ty()))
    }

    #[inline]
    pub fn is_legal_type(&self, ty: &Type) -> bool {
        matches!(self.convert_type(ty), Some(TypeConversion::One(converted)) if converted == *ty)
    }

    pub fn convert_type_1_to_1(&self, ty: &Type) -> Result<Type, Report> {
        self.require_1_to_1(self.convert_type(ty), || format!("type '{ty}'"))
    }

    pub fn convert_value_1_to_1(&self, value: ValueRef) -> Result<Type, Report> {
        self.require_1_to_1(self.convert_value(value), || {
            format!("value '{}' of type '{}'", value.borrow(), value.borrow().ty())
        })
    }

    pub fn materialize_source_conversion(
        &self,
        rewriter: &mut ConversionPatternRewriter,
        value: ValueRef,
        ty: Type,
        span: SourceSpan,
    ) -> Result<ValueRef, Report> {
        self.materialize_conversion(
            &self.source_materializations,
            rewriter,
            value,
            ty,
            span,
            "source",
        )
    }

    pub fn materialize_target_conversion(
        &self,
        rewriter: &mut ConversionPatternRewriter,
        value: ValueRef,
        ty: Type,
        span: SourceSpan,
    ) -> Result<ValueRef, Report> {
        self.materialize_conversion(
            &self.target_materializations,
            rewriter,
            value,
            ty,
            span,
            "target",
        )
    }

    fn materialize_conversion(
        &self,
        callbacks: &[MaterializationFn],
        rewriter: &mut ConversionPatternRewriter,
        value: ValueRef,
        ty: Type,
        span: SourceSpan,
        kind: &'static str,
    ) -> Result<ValueRef, Report> {
        if *value.borrow().ty() == ty {
            return Ok(value);
        }

        for materialize in callbacks {
            if let Some(materialized) = materialize(rewriter, value, ty.clone(), span)? {
                return Ok(materialized);
            }
        }

        let op = rewriter.create_op::<UnrealizedConversionCast, _>(span, (value, ty.clone()))?;
        rewriter.mark_materialization_op(op.as_operation_ref());
        let result = op.borrow().result().as_value_ref();
        if *result.borrow().ty() != ty {
            return Err(Report::msg(format!(
                "{kind} materialization produced type '{}', expected '{ty}'",
                result.borrow().ty()
            )));
        }
        Ok(result)
    }

    fn require_1_to_1<F>(
        &self,
        conversion: Option<TypeConversion>,
        describe_source: F,
    ) -> Result<Type, Report>
    where
        F: FnOnce() -> alloc::string::String,
    {
        match conversion {
            Some(TypeConversion::One(ty)) => Ok(ty),
            Some(TypeConversion::Many(types)) => Err(Report::msg(format!(
                "only 1:1 type conversion is supported here; {} converted to {} values",
                describe_source(),
                types.len()
            ))),
            Some(TypeConversion::Drop) => Err(Report::msg(format!(
                "only 1:1 type conversion is supported here; {} was dropped",
                describe_source()
            ))),
            None => Err(Report::msg(format!("failed to convert {}", describe_source()))),
        }
    }
}

#[cfg(test)]
mod tests {
    use smallvec::smallvec;

    use super::*;
    use crate::Type;

    #[test]
    fn defaults_to_identity_conversion() {
        let converter = TypeConverter::new();

        assert_eq!(converter.convert_type_1_to_1(&Type::U32).unwrap(), Type::U32);
        assert!(converter.is_legal_type(&Type::U32));
    }

    #[test]
    fn applies_registered_type_conversion() {
        let mut converter = TypeConverter::new();
        converter.add_conversion(|ty| {
            if ty == &Type::U32 {
                Some(TypeConversion::One(Type::I32))
            } else {
                None
            }
        });

        assert_eq!(converter.convert_type_1_to_1(&Type::U32).unwrap(), Type::I32);
        assert!(!converter.is_legal_type(&Type::U32));
    }

    #[test]
    fn rejects_many_and_drop_for_1_to_1_queries() {
        let mut many = TypeConverter::new();
        many.add_conversion(|ty| {
            if ty == &Type::U32 {
                Some(TypeConversion::Many(smallvec![Type::I32, Type::I32]))
            } else {
                None
            }
        });
        assert!(format!("{}", many.convert_type_1_to_1(&Type::U32).unwrap_err()).contains("1:1"));

        let mut drop = TypeConverter::new();
        drop.add_conversion(|ty| {
            if ty == &Type::U32 {
                Some(TypeConversion::Drop)
            } else {
                None
            }
        });
        assert!(format!("{}", drop.convert_type_1_to_1(&Type::U32).unwrap_err()).contains("1:1"));
    }
}
