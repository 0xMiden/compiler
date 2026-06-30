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
    /// This is reserved by the API, but current driver helpers reject it when default boundary
    /// materialization would be required.
    Many(SmallVec<[Type; 2]>),
    /// The source value is removed.
    ///
    /// This is reserved by the API, but current driver helpers reject it when default boundary
    /// materialization would be required.
    Drop,
}

impl TypeConversion {
    /// Build a 1:1 type conversion result.
    #[inline]
    pub fn one(ty: Type) -> Self {
        Self::One(ty)
    }

    /// Build a 1:N type conversion result.
    ///
    /// The current production driver reserves this shape but rejects it in helpers that require
    /// 1:1 conversion.
    #[inline]
    pub fn many(types: impl IntoIterator<Item = Type>) -> Self {
        Self::Many(types.into_iter().collect())
    }

    /// Build a conversion result that drops the source value.
    ///
    /// The current production driver reserves this shape but rejects it in helpers that require
    /// 1:1 conversion.
    #[inline]
    pub const fn drop() -> Self {
        Self::Drop
    }
}

/// Converts source IR types and values to target IR types and values.
///
/// A type converter is attached to conversion patterns that need operand/result type remapping.
/// Conversion callbacks are tried in registration order. If no type callback handles a type, the
/// converter treats it as an identity conversion.
///
/// The initial driver supports 1:1 boundary materialization. `Many` and `Drop` results are
/// accepted by the data model for future ABI/aggregate-lowering work, but callers must use the
/// explicit `*_1_to_1` helpers when they need current driver compatibility.
#[derive(Clone, Default)]
pub struct TypeConverter {
    conversions: Vec<TypeConversionFn>,
    value_conversions: Vec<ValueConversionFn>,
    source_materializations: Vec<MaterializationFn>,
    target_materializations: Vec<MaterializationFn>,
}

impl TypeConverter {
    /// Create an empty converter that defaults to identity conversion.
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a type conversion callback.
    ///
    /// Return `Some` to handle the source type, or `None` to let later callbacks try. Callbacks are
    /// evaluated in insertion order.
    #[inline]
    pub fn add_conversion<F>(&mut self, callback: F) -> &mut Self
    where
        F: Fn(&Type) -> Option<TypeConversion> + 'static,
    {
        self.conversions.push(Rc::new(callback));
        self
    }

    /// Add a value-specific conversion callback.
    ///
    /// Value callbacks are checked before type-only callbacks and may inspect the value's defining
    /// operation, uses, attributes, or type.
    #[inline]
    pub fn add_value_conversion<F>(&mut self, callback: F) -> &mut Self
    where
        F: Fn(ValueRef) -> Option<TypeConversion> + 'static,
    {
        self.value_conversions.push(Rc::new(callback));
        self
    }

    /// Add a source materialization callback.
    ///
    /// Source materializations convert a replacement value back to the original source type when
    /// replacing an operation whose old result still has live users expecting that source type.
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

    /// Add a target materialization callback.
    ///
    /// Target materializations convert an existing operand value to the type expected by a
    /// conversion pattern. If no callback handles the request, the converter creates
    /// `builtin.unrealized_conversion_cast`.
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

    /// Return true when `ty` converts to itself.
    #[inline]
    pub fn is_legal_type(&self, ty: &Type) -> bool {
        matches!(self.convert_type(ty), Some(TypeConversion::One(converted)) if converted == *ty)
    }

    /// Convert `ty` and require a 1:1 result.
    ///
    /// This returns an error for `Many`, `Drop`, or an unhandled conversion.
    pub fn convert_type_1_to_1(&self, ty: &Type) -> Result<Type, Report> {
        self.require_1_to_1(self.convert_type(ty), || format!("type '{ty}'"))
    }

    /// Convert `value` and require a 1:1 result.
    ///
    /// Value-specific callbacks are considered before type callbacks.
    pub fn convert_value_1_to_1(&self, value: ValueRef) -> Result<Type, Report> {
        self.require_1_to_1(self.convert_value(value), || {
            format!("value '{}' of type '{}'", value.borrow(), value.borrow().ty())
        })
    }

    /// Materialize a conversion from target IR back to a source type.
    ///
    /// Registered source materializers are tried first. If none applies, an
    /// `builtin.unrealized_conversion_cast` is inserted and marked as framework-owned.
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

    /// Materialize a conversion from source IR to a target type.
    ///
    /// Registered target materializers are tried first. If none applies, an
    /// `builtin.unrealized_conversion_cast` is inserted and marked as framework-owned.
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
