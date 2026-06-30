use alloc::{collections::BTreeMap, rc::Rc, vec::Vec};
use core::{
    any::TypeId,
    ptr::{DynMetadata, Pointee},
};

use super::DynamicLegalityResult;
use crate::{
    Context, DialectRegistration, FxHashMap, OpRegistration, Operation, OperationName, Report,
    interner::Symbol,
};

type DynamicLegalityFn = Rc<dyn Fn(&Operation) -> DynamicLegalityResult>;

/// Describes whether an operation instance is legal for a conversion target.
pub enum Legality {
    /// The operation is statically legal.
    Legal,
    /// The operation is statically illegal.
    Illegal,
    /// The target has no rule for the operation.
    Unknown,
    /// A dynamic legality predicate accepted this operation instance.
    DynamicLegal,
    /// A dynamic legality predicate rejected this operation instance.
    ///
    /// The optional reason is intended for user-facing conversion diagnostics.
    DynamicIllegal { reason: Option<Report> },
}

impl Legality {
    /// Return true when this result permits the operation to remain in converted IR.
    #[inline]
    pub const fn is_legal(&self) -> bool {
        matches!(self, Self::Legal | Self::DynamicLegal)
    }

    /// Return true when this result explicitly rejects the operation.
    #[inline]
    pub const fn is_illegal(&self) -> bool {
        matches!(self, Self::Illegal | Self::DynamicIllegal { .. })
    }

    /// Return true when no target rule matched the operation.
    #[inline]
    pub const fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown)
    }
}

/// Describes the best legality answer available from operation metadata alone.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum StaticLegality {
    /// The operation name is accepted without inspecting a concrete operation instance.
    Legal,
    /// The operation name is rejected without inspecting a concrete operation instance.
    Illegal,
    /// No matching rule is known for the operation name.
    Unknown,
    /// A dynamic predicate must inspect each operation instance to decide legality.
    Dynamic,
}

/// Policy used when an operation has no op, dialect, or interface rule.
#[derive(Clone)]
pub enum UnknownOpPolicy {
    /// Treat otherwise-unmatched operations as legal.
    Legal,
    /// Treat otherwise-unmatched operations as illegal.
    Illegal,
    /// Decide legality for otherwise-unmatched operations with a callback.
    Dynamic(DynamicLegalityFn),
}

impl UnknownOpPolicy {
    #[inline]
    fn static_legality(&self) -> StaticLegality {
        match self {
            Self::Legal => StaticLegality::Legal,
            Self::Illegal => StaticLegality::Illegal,
            Self::Dynamic(_) => StaticLegality::Dynamic,
        }
    }

    #[inline]
    fn evaluate(&self, op: &Operation) -> Legality {
        match self {
            Self::Legal => Legality::Legal,
            Self::Illegal => Legality::Illegal,
            Self::Dynamic(callback) => evaluate_dynamic(callback(op)),
        }
    }
}

/// A legality rule for a dialect, operation, or interface.
#[derive(Clone)]
pub enum LegalityRule {
    /// Matching operations are legal.
    Legal,
    /// Matching operations are illegal.
    Illegal,
    /// Matching operations are checked by a callback.
    Dynamic(DynamicLegalityFn),
}

impl LegalityRule {
    #[inline]
    fn static_legality(&self) -> StaticLegality {
        match self {
            Self::Legal => StaticLegality::Legal,
            Self::Illegal => StaticLegality::Illegal,
            Self::Dynamic(_) => StaticLegality::Dynamic,
        }
    }

    #[inline]
    fn evaluate(&self, op: &Operation) -> Legality {
        match self {
            Self::Legal => Legality::Legal,
            Self::Illegal => Legality::Illegal,
            Self::Dynamic(callback) => evaluate_dynamic(callback(op)),
        }
    }
}

/// Legality rule attached to an operation interface.
///
/// Interface rules are evaluated after exact operation rules and dialect rules. They are useful
/// for broad predicates such as "all operations implementing this lowering interface are legal",
/// but targets that need exceptions should use explicit operation or dialect rules for those
/// exceptions because those rules take precedence.
#[derive(Clone)]
pub struct InterfaceLegalityRule {
    trait_id: TypeId,
    rule: LegalityRule,
}

/// Rule controlling whether an operation's nested regions are skipped by conversion.
#[derive(Clone)]
pub enum RecursiveLegalityRule {
    /// If the operation itself is legal, all nested operations are considered legal.
    Legal,
    /// If the operation itself is legal, the callback decides whether nested operations are
    /// considered legal for this instance.
    Dynamic(DynamicLegalityFn),
}

impl RecursiveLegalityRule {
    #[inline]
    fn evaluate(&self, op: &Operation) -> bool {
        match self {
            Self::Legal => true,
            Self::Dynamic(callback) => callback(op).is_legal(),
        }
    }
}

/// Defines the set of operations accepted by a dialect conversion.
///
/// A target answers legality queries using this precedence:
///
/// 1. Exact operation rule.
/// 2. Dialect rule.
/// 3. Interface rule, in registration order.
/// 4. Unknown operation policy.
///
/// Conversion targets are owned by concrete passes. The generic driver consumes a target together
/// with a set of conversion patterns and requires every visited operation to become legal, except
/// for operations nested under recursively legal operations.
pub struct ConversionTarget {
    context: Rc<Context>,
    unknown_op_policy: UnknownOpPolicy,
    dialect_actions: FxHashMap<Symbol, LegalityRule>,
    op_actions: BTreeMap<OperationName, LegalityRule>,
    interface_actions: Vec<InterfaceLegalityRule>,
    recursive_legality: BTreeMap<OperationName, RecursiveLegalityRule>,
}

impl ConversionTarget {
    /// Create a target whose default unknown-operation policy is illegal.
    pub fn new(context: Rc<Context>) -> Self {
        Self {
            context,
            unknown_op_policy: UnknownOpPolicy::Illegal,
            dialect_actions: Default::default(),
            op_actions: Default::default(),
            interface_actions: Default::default(),
            recursive_legality: Default::default(),
        }
    }

    /// Return the context used to resolve dialect and operation registrations.
    #[inline]
    pub fn context(&self) -> Rc<Context> {
        Rc::clone(&self.context)
    }

    /// Set the fallback policy used when no operation, dialect, or interface rule matches.
    pub fn set_unknown_op_policy(&mut self, policy: UnknownOpPolicy) -> &mut Self {
        self.unknown_op_policy = policy;
        self
    }

    /// Mark every operation in dialect `D` legal unless a more-specific operation rule overrides
    /// it.
    pub fn add_legal_dialect<D: DialectRegistration>(&mut self) -> &mut Self {
        self.set_dialect_rule::<D>(LegalityRule::Legal)
    }

    /// Mark every operation in dialect `D` illegal unless a more-specific operation rule overrides
    /// it.
    pub fn add_illegal_dialect<D: DialectRegistration>(&mut self) -> &mut Self {
        self.set_dialect_rule::<D>(LegalityRule::Illegal)
    }

    /// Mark operations in dialect `D` legal or illegal according to `callback`.
    ///
    /// The callback is evaluated for each matching operation instance, so it may inspect traits,
    /// attributes, operand/result types, regions, or any other operation metadata.
    pub fn add_dynamically_legal_dialect<D, F>(&mut self, callback: F) -> &mut Self
    where
        D: DialectRegistration,
        F: Fn(&Operation) -> DynamicLegalityResult + 'static,
    {
        self.set_dialect_rule::<D>(LegalityRule::Dynamic(Rc::new(callback)))
    }

    /// Mark operations in dialect `D` dynamically legal when their operation registration
    /// implements `Trait`.
    pub fn add_dynamically_legal_dialect_if_op_interface<D, Trait>(&mut self) -> &mut Self
    where
        D: DialectRegistration,
        Trait: ?Sized + Pointee<Metadata = DynMetadata<Trait>> + 'static,
    {
        self.add_dynamically_legal_dialect::<D, _>(|op| {
            DynamicLegalityResult::legal_if(op.implements::<Trait>())
        })
    }

    /// Mark operation `Op` legal.
    pub fn add_legal_op<Op: OpRegistration>(&mut self) -> &mut Self {
        self.set_op_rule::<Op>(LegalityRule::Legal)
    }

    /// Mark operation `Op` illegal.
    pub fn add_illegal_op<Op: OpRegistration>(&mut self) -> &mut Self {
        self.set_op_rule::<Op>(LegalityRule::Illegal)
    }

    /// Mark operation `Op` legal or illegal according to `callback`.
    ///
    /// This is the most precise legality hook and takes precedence over dialect and interface
    /// rules.
    pub fn add_dynamically_legal_op<Op, F>(&mut self, callback: F) -> &mut Self
    where
        Op: OpRegistration,
        F: Fn(&Operation) -> DynamicLegalityResult + 'static,
    {
        self.set_op_rule::<Op>(LegalityRule::Dynamic(Rc::new(callback)))
    }

    /// Mark all operations whose registration implements `Trait` legal.
    ///
    /// This is intentionally broad. Prefer operation or dialect rules when the target needs
    /// explicit exceptions.
    pub fn add_legal_op_interface<Trait>(&mut self) -> &mut Self
    where
        Trait: ?Sized + Pointee<Metadata = DynMetadata<Trait>> + 'static,
    {
        self.set_interface_rule::<Trait>(LegalityRule::Legal)
    }

    /// Mark all operations whose registration implements `Trait` dynamically legal according to
    /// `callback`.
    pub fn add_dynamically_legal_op_interface<Trait, F>(&mut self, callback: F) -> &mut Self
    where
        Trait: ?Sized + Pointee<Metadata = DynMetadata<Trait>> + 'static,
        F: Fn(&Operation) -> DynamicLegalityResult + 'static,
    {
        self.set_interface_rule::<Trait>(LegalityRule::Dynamic(Rc::new(callback)))
    }

    /// Mark operation `Op` recursively legal.
    ///
    /// Recursive legality only applies when `Op` itself is legal. When it applies, conversion
    /// skips all nested operations under that operation.
    pub fn mark_op_recursively_legal<Op: OpRegistration>(&mut self) -> &mut Self {
        let name = self.registered_op_name::<Op>();
        self.recursive_legality.insert(name, RecursiveLegalityRule::Legal);
        self
    }

    /// Mark operation `Op` recursively legal when `callback` accepts a concrete instance.
    ///
    /// The operation itself must still be legal for recursive legality to apply.
    pub fn mark_op_recursively_legal_if<Op, F>(&mut self, callback: F) -> &mut Self
    where
        Op: OpRegistration,
        F: Fn(&Operation) -> DynamicLegalityResult + 'static,
    {
        let name = self.registered_op_name::<Op>();
        self.recursive_legality
            .insert(name, RecursiveLegalityRule::Dynamic(Rc::new(callback)));
        self
    }

    /// Evaluate operation-instance legality using the target precedence rules.
    pub fn legality(&self, op: &Operation) -> Legality {
        let name = op.name();
        if let Some(rule) = self.op_actions.get(&name) {
            return rule.evaluate(op);
        }

        if let Some(rule) = self.dialect_actions.get(&name.dialect()) {
            return rule.evaluate(op);
        }

        for interface in self.interface_actions.iter() {
            if name.implements_trait_id(&interface.trait_id) {
                return interface.rule.evaluate(op);
            }
        }

        self.unknown_op_policy.evaluate(op)
    }

    /// Evaluate operation-name legality without inspecting a concrete operation instance.
    ///
    /// Dynamic rules return [`StaticLegality::Dynamic`]. This is used by legalization path
    /// discovery to decide which generated operations may be terminal targets.
    pub fn static_legality(&self, name: &OperationName) -> StaticLegality {
        if let Some(rule) = self.op_actions.get(name) {
            return rule.static_legality();
        }

        if let Some(rule) = self.dialect_actions.get(&name.dialect()) {
            return rule.static_legality();
        }

        for interface in self.interface_actions.iter() {
            if name.implements_trait_id(&interface.trait_id) {
                return interface.rule.static_legality();
            }
        }

        self.unknown_op_policy.static_legality()
    }

    /// Return true when `op` is legal for this target.
    #[inline]
    pub fn is_legal(&self, op: &Operation) -> bool {
        self.legality(op).is_legal()
    }

    /// Return true when `op` is legal and its nested operations may be skipped.
    pub fn is_recursively_legal(&self, op: &Operation) -> bool {
        let name = op.name();
        let Some(rule) = self.recursive_legality.get(&name) else {
            return false;
        };
        self.is_legal(op) && rule.evaluate(op)
    }

    fn set_dialect_rule<D: DialectRegistration>(&mut self, rule: LegalityRule) -> &mut Self {
        self.context.get_or_register_dialect::<D>();
        self.dialect_actions.insert(Symbol::intern(D::NAMESPACE), rule);
        self
    }

    fn set_op_rule<Op: OpRegistration>(&mut self, rule: LegalityRule) -> &mut Self {
        let name = self.registered_op_name::<Op>();
        self.op_actions.insert(name, rule);
        self
    }

    fn set_interface_rule<Trait>(&mut self, rule: LegalityRule) -> &mut Self
    where
        Trait: ?Sized + Pointee<Metadata = DynMetadata<Trait>> + 'static,
    {
        let trait_id = TypeId::of::<Trait>();
        if let Some(interface) = self
            .interface_actions
            .iter_mut()
            .find(|interface| interface.trait_id == trait_id)
        {
            interface.rule = rule;
        } else {
            self.interface_actions.push(InterfaceLegalityRule { trait_id, rule });
        }
        self
    }

    fn registered_op_name<Op: OpRegistration>(&self) -> OperationName {
        self.context
            .get_or_register_dialect::<<Op as OpRegistration>::Dialect>()
            .expect_registered_name::<Op>()
    }
}

fn evaluate_dynamic(result: DynamicLegalityResult) -> Legality {
    match result {
        DynamicLegalityResult::Legal => Legality::DynamicLegal,
        DynamicLegalityResult::Illegal { reason } => Legality::DynamicIllegal { reason },
    }
}

#[cfg(test)]
mod tests {
    use alloc::rc::Rc;

    use crate::{
        Context, OpRegistration, OperationRef, Report,
        conversion::{
            ConversionTarget, DynamicLegalityResult, Legality, StaticLegality, UnknownOpPolicy,
        },
        dialects::test::{Add, Constant, TestDialect},
        traits::{Commutative, ConstantLike},
    };

    fn detached_op<Op>(context: &Rc<Context>) -> OperationRef
    where
        Op: OpRegistration,
    {
        context
            .get_or_register_dialect::<<Op as OpRegistration>::Dialect>()
            .expect_registered_name::<Op>()
            .alloc_default(context.clone())
    }

    #[test]
    fn default_unknown_policy_is_illegal() {
        let context = Rc::new(Context::default());
        let target = ConversionTarget::new(context.clone());
        let op = detached_op::<Constant>(&context);

        assert!(matches!(target.legality(&op.borrow()), Legality::Illegal));
    }

    #[test]
    fn unknown_policy_can_mark_unknown_ops_legal_or_dynamic() {
        let context = Rc::new(Context::default());
        let op = detached_op::<Constant>(&context);

        let mut target = ConversionTarget::new(context.clone());
        target.set_unknown_op_policy(UnknownOpPolicy::Legal);
        assert!(target.is_legal(&op.borrow()));

        target.set_unknown_op_policy(UnknownOpPolicy::Dynamic(Rc::new(|_| {
            DynamicLegalityResult::illegal_with_reason(Report::msg("not legal"))
        })));
        assert!(matches!(
            target.legality(&op.borrow()),
            Legality::DynamicIllegal { reason: Some(_) }
        ));
    }

    #[test]
    fn operation_rule_overrides_dialect_rule() {
        let context = Rc::new(Context::default());
        let mut target = ConversionTarget::new(context.clone());
        target.add_legal_dialect::<TestDialect>().add_illegal_op::<Constant>();

        let constant = detached_op::<Constant>(&context);
        let add = detached_op::<Add>(&context);

        assert!(matches!(target.legality(&constant.borrow()), Legality::Illegal));
        assert!(matches!(target.legality(&add.borrow()), Legality::Legal));
    }

    #[test]
    fn dialect_rule_overrides_interface_rule() {
        let context = Rc::new(Context::default());
        let mut target = ConversionTarget::new(context.clone());
        target
            .add_legal_op_interface::<dyn ConstantLike>()
            .add_illegal_dialect::<TestDialect>();

        let constant = detached_op::<Constant>(&context);

        assert!(matches!(target.legality(&constant.borrow()), Legality::Illegal));
    }

    #[test]
    fn dialect_can_be_dynamic_by_operation_interface() {
        let context = Rc::new(Context::default());
        let mut target = ConversionTarget::new(context.clone());
        target.add_dynamically_legal_dialect_if_op_interface::<TestDialect, dyn ConstantLike>();

        let constant = detached_op::<Constant>(&context);
        let add = detached_op::<Add>(&context);

        assert!(matches!(target.legality(&constant.borrow()), Legality::DynamicLegal));
        assert!(matches!(target.legality(&add.borrow()), Legality::DynamicIllegal { .. }));
    }

    #[test]
    fn interface_rule_applies_when_no_op_or_dialect_rule_exists() {
        let context = Rc::new(Context::default());
        let mut target = ConversionTarget::new(context.clone());
        target.add_legal_op_interface::<dyn Commutative>();

        let add = detached_op::<Add>(&context);
        let constant = detached_op::<Constant>(&context);

        assert!(matches!(target.legality(&add.borrow()), Legality::Legal));
        assert!(matches!(target.legality(&constant.borrow()), Legality::Illegal));
    }

    #[test]
    fn static_legality_reports_metadata_only_result() {
        let context = Rc::new(Context::default());
        let mut target = ConversionTarget::new(context.clone());
        target.add_dynamically_legal_op::<Constant, _>(|_| DynamicLegalityResult::legal());

        let constant = context
            .get_or_register_dialect::<TestDialect>()
            .expect_registered_name::<Constant>();

        assert_eq!(target.static_legality(&constant), StaticLegality::Dynamic);
    }

    #[test]
    fn recursive_legality_requires_operation_legality() {
        let context = Rc::new(Context::default());
        let mut target = ConversionTarget::new(context.clone());
        target.mark_op_recursively_legal::<Constant>();

        let constant = detached_op::<Constant>(&context);
        assert!(!target.is_recursively_legal(&constant.borrow()));

        target.add_legal_op::<Constant>();
        assert!(target.is_recursively_legal(&constant.borrow()));
    }

    #[test]
    fn dynamic_recursive_legality_uses_callback() {
        let context = Rc::new(Context::default());
        let mut target = ConversionTarget::new(context.clone());
        target
            .add_legal_op::<Constant>()
            .mark_op_recursively_legal_if::<Constant, _>(|_| DynamicLegalityResult::illegal());

        let constant = detached_op::<Constant>(&context);
        assert!(!target.is_recursively_legal(&constant.borrow()));
    }
}
