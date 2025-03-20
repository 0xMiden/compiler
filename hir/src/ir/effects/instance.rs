use alloc::{boxed::Box, rc::Rc};
use core::fmt;

use super::{DefaultResource, Effect, Resource};
use crate::{
    interner, AttributeSet, AttributeValue, BlockArgument, BlockArgumentRef, EntityRef, OpOperand,
    OpOperandImpl, OpResult, OpResultRef, SymbolRef, Value, ValueRef,
};

#[derive(Clone)]
pub struct EffectInstance<T> {
    /// The specific effect being applied
    effect: T,
    /// The resource that the given value resides in
    resource: Rc<dyn Resource>,
    /// The [Symbol], [OpOperand], [OpResult], or [BlockArgument] that the effect applies to.
    value: Option<EffectValue>,
    /// Additional parameters of the effect instance.
    parameters: AttributeSet,
    /// The stage the side effect happens in.
    ///
    /// Side effects with a lower stage happen earlier than those with a higher stage.
    stage: u8,
    /// Indicates whether this side effect acts on every single value of the resource
    effect_on_full_region: bool,
}

impl<T> EffectInstance<T> {
    pub fn new(effect: T) -> Self {
        Self::new_with_resource(effect, DefaultResource)
    }

    pub fn new_for_value(effect: T, value: impl Into<EffectValue>) -> Self {
        Self::new_for_value_with_resource(effect, value, DefaultResource)
    }
}

impl<T> EffectInstance<T> {
    pub fn new_with_resource(effect: T, resource: impl Resource) -> Self {
        Self {
            effect,
            resource: Rc::new(resource),
            parameters: AttributeSet::new(),
            value: None,
            stage: 0,
            effect_on_full_region: false,
        }
    }

    #[inline]
    pub fn new_for_value_with_resource(
        effect: T,
        value: impl Into<EffectValue>,
        resource: impl Resource,
    ) -> Self {
        Self {
            effect,
            resource: Rc::new(resource),
            parameters: Default::default(),
            value: Some(value.into()),
            stage: 0,
            effect_on_full_region: false,
        }
    }

    #[inline(always)]
    pub fn with_parameter(
        mut self,
        name: impl Into<interner::Symbol>,
        value: impl AttributeValue,
    ) -> Self {
        self.parameters.insert(name, Some(value));
        self
    }

    #[inline(always)]
    pub fn with_stage(mut self, stage: u8) -> Self {
        self.stage = stage;
        self
    }

    #[inline(always)]
    pub fn with_effect_on_full_region(mut self, yes: bool) -> Self {
        self.effect_on_full_region = yes;
        self
    }

    /// Get the effect being applied
    #[inline]
    pub fn effect(&self) -> &T {
        &self.effect
    }

    /// Get the resource that the effect applies to
    #[inline]
    pub fn resource(&self) -> &dyn Resource {
        self.resource.as_ref()
    }

    /// Get the parameters of the effect.
    #[inline]
    pub const fn parameters(&self) -> &AttributeSet {
        &self.parameters
    }

    /// Get the stage at which the effect happens.
    #[inline]
    pub const fn stage(&self) -> u8 {
        self.stage
    }

    /// Returns whether this efffect acts on every single value of the resource.
    #[inline]
    pub const fn is_effect_on_full_region(&self) -> bool {
        self.effect_on_full_region
    }

    /// Get the value the effect is being applied on, or `None` if there isn't a known value
    /// being affected.
    pub fn value(&self) -> Option<ValueRef> {
        match self.value.as_ref()? {
            EffectValue::Result(res) => Some(*res as ValueRef),
            EffectValue::BlockArgument(arg) => Some(*arg as ValueRef),
            EffectValue::Operand(operand) => Some(operand.borrow().as_value_ref()),
            _ => None,
        }
    }

    /// Get the value the effect is being applied on, or `None` if there isn't a known value
    /// being affected.
    #[allow(unused)]
    fn effect_value(&self) -> Option<&EffectValue> {
        self.value.as_ref()
    }

    /// Get the value the effect is being applied on, if it is of the specified type, or `None` if
    /// there isn't a known value being affected.
    pub fn value_of_kind<'a, 'b: 'a, V>(&'b self) -> Option<EntityRef<'a, V>>
    where
        V: Value,
        EntityRef<'a, V>: TryFrom<&'b EffectValue>,
    {
        self.value.as_ref().and_then(|value| value.try_as_ref())
    }

    /// Get the symbol reference the effect is applied on, or `None` if there isn't a known symbol
    /// being affected.
    pub fn symbol(&self) -> Option<SymbolRef> {
        match self.value.as_ref()? {
            EffectValue::Symbol(symbol_use) => Some(*symbol_use),
            _ => None,
        }
    }
}

impl<T: Effect> fmt::Debug for EffectInstance<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EffectInstance")
            .field("effect", &self.effect)
            .field("resource", &self.resource)
            .field("value", &self.value)
            .field("parameters", &self.parameters)
            .field("stage", &self.stage)
            .field("effect_on_full_region", &self.effect_on_full_region)
            .finish()
    }
}

#[derive(PartialEq, Eq)]
pub enum EffectValue {
    Attribute(Box<dyn AttributeValue>),
    Symbol(SymbolRef),
    Operand(OpOperand),
    Result(OpResultRef),
    BlockArgument(BlockArgumentRef),
}
impl Clone for EffectValue {
    fn clone(&self) -> Self {
        match self {
            Self::Attribute(attr) => Self::Attribute(attr.clone_value()),
            Self::Symbol(value) => Self::Symbol(*value),
            Self::Operand(value) => Self::Operand(*value),
            Self::Result(value) => Self::Result(*value),
            Self::BlockArgument(value) => Self::BlockArgument(*value),
        }
    }
}
impl fmt::Debug for EffectValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Attribute(attr) => f.debug_tuple("Attribute").field(attr).finish(),
            Self::Symbol(symbol_use) => f
                .debug_tuple("Symbol")
                .field_with(|f| {
                    let symbol = symbol_use.borrow();
                    write!(f, "{}", &symbol.path())
                })
                .finish(),
            Self::Operand(operand) => {
                let value = operand.borrow().as_value_ref();
                f.debug_tuple("Operand").field(&value).finish()
            }
            Self::Result(result) => {
                let value = *result as ValueRef;
                f.debug_tuple("Result").field(&value).finish()
            }
            Self::BlockArgument(arg) => {
                let value = *arg as ValueRef;
                f.debug_tuple("BlockArgument").field(&value).finish()
            }
        }
    }
}

impl From<Box<dyn AttributeValue>> for EffectValue {
    fn from(value: Box<dyn AttributeValue>) -> Self {
        Self::Attribute(value)
    }
}

impl From<SymbolRef> for EffectValue {
    fn from(value: SymbolRef) -> Self {
        Self::Symbol(value)
    }
}

impl From<OpOperand> for EffectValue {
    fn from(value: OpOperand) -> Self {
        Self::Operand(value)
    }
}

impl From<OpResultRef> for EffectValue {
    fn from(value: OpResultRef) -> Self {
        Self::Result(value)
    }
}

impl From<BlockArgumentRef> for EffectValue {
    fn from(value: BlockArgumentRef) -> Self {
        Self::BlockArgument(value)
    }
}

impl From<ValueRef> for EffectValue {
    fn from(value: ValueRef) -> Self {
        let value = value.borrow();
        if let Some(result) = value.downcast_ref::<OpResult>() {
            Self::Result(result.as_op_result_ref())
        } else {
            let arg = value.downcast_ref::<BlockArgument>().unwrap();
            Self::BlockArgument(arg.as_block_argument_ref())
        }
    }
}

impl EffectValue {
    pub fn try_as_ref<'a, 'b: 'a, V>(&'b self) -> Option<EntityRef<'a, V>>
    where
        V: Value,
        EntityRef<'a, V>: TryFrom<&'b Self>,
    {
        TryFrom::try_from(self).ok()
    }
}

impl<'a> core::convert::TryFrom<&'a EffectValue> for EntityRef<'a, OpOperandImpl> {
    type Error = ();

    fn try_from(value: &'a EffectValue) -> Result<Self, Self::Error> {
        match value {
            EffectValue::Operand(operand) => Ok(operand.borrow()),
            _ => Err(()),
        }
    }
}

impl<'a> core::convert::TryFrom<&'a EffectValue> for EntityRef<'a, BlockArgument> {
    type Error = ();

    fn try_from(value: &'a EffectValue) -> Result<Self, Self::Error> {
        match value {
            EffectValue::BlockArgument(operand) => Ok(operand.borrow()),
            _ => Err(()),
        }
    }
}

impl<'a> core::convert::TryFrom<&'a EffectValue> for EntityRef<'a, OpResult> {
    type Error = ();

    fn try_from(value: &'a EffectValue) -> Result<Self, Self::Error> {
        match value {
            EffectValue::Result(operand) => Ok(operand.borrow()),
            _ => Err(()),
        }
    }
}
