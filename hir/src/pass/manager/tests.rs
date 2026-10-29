use alloc::{boxed::Box, format, rc::Rc, string::String, vec::Vec};
use core::cell::RefCell;

use super::*;
use crate::{
    RegionKindInterface, Symbol,
    diagnostics::Uri,
    dialects::{
        builtin::{Function, Module, World},
        test::TestDialect,
    },
    parse::{ParserConfig, parse_any},
    traits::{HasOnlyGraphRegion, NoTerminator},
};

type Events = Rc<RefCell<Vec<String>>>;

const TREE: &str = r#"builtin.world {
    builtin.function public extern("C") @root() { builtin.ret; };
    builtin.module public @outer {
        builtin.function public extern("C") @first() { builtin.ret; };
        builtin.module public @inner {
            builtin.function public extern("C") @deep() { builtin.ret; };
        };
        builtin.function public extern("C") @last() { builtin.ret; };
    };
};"#;

fn tree() -> (Rc<Context>, OperationRef) {
    let context = Rc::new(Context::default());
    let world = parse_any(ParserConfig::new(context.clone()), Uri::new("scheduling.hir"), TREE)
        .expect("valid namespace tree");
    (context, world)
}

struct RecordFunctions {
    stage: &'static str,
    events: Events,
}

impl Pass for RecordFunctions {
    type Target = Function;

    fn name(&self) -> &'static str {
        self.stage
    }

    fn target_name(&self, context: &Context) -> Option<OperationName> {
        Some(context.get_registered_name::<Function>())
    }

    fn can_schedule_on(&self, name: &OperationName) -> bool {
        name.is::<Function>()
    }

    fn run_on_operation(
        &mut self,
        op: EntityMut<'_, Function>,
        state: &mut PassExecutionState,
    ) -> Result<(), Report> {
        self.events.borrow_mut().push(format!("{}:{}", self.stage, Symbol::name(&*op)));
        state.set_post_pass_status(PostPassStatus::Unchanged);
        Ok(())
    }
}

fn record(stage: &'static str, events: &Events) -> Box<dyn OperationPass> {
    Box::new(RecordFunctions {
        stage,
        events: events.clone(),
    })
}

#[test]
fn incompatible_adjacent_adaptors_both_execute() {
    let (context, world) = tree();
    let events = Events::default();
    let mut pm = PassManager::on::<World>(context, Nesting::Explicit);
    pm.nest::<Function>().add_pass(record("typed", &events));
    pm.nest_any().add_pass(record("any", &events));
    pm.run(world).unwrap();
    assert_eq!(&*events.borrow(), &["typed:root", "any:root"]);
}

#[test]
fn trailing_adaptor_finalizes_its_nested_pipeline() {
    let (context, world) = tree();
    let events = Events::default();
    let mut pm = PassManager::on::<World>(context, Nesting::Explicit);
    {
        let mut modules = pm.nest::<Module>();
        modules.nest::<Function>().add_pass(record("a", &events));
        modules.nest::<Function>().add_pass(record("b", &events));
    }
    pm.run(world).unwrap();
    assert_eq!(&*events.borrow(), &["a:first", "b:first", "a:last", "b:last"]);
}

#[test]
fn immediate_and_implicit_nesting_stay_immediate() {
    for implicit in [false, true] {
        let (context, world) = tree();
        let events = Events::default();
        let mut pm = PassManager::on::<World>(context, Nesting::Implicit);
        if implicit {
            pm.add_pass(record("visit", &events));
        } else {
            pm.nest::<Function>().add_pass(record("visit", &events));
        }
        pm.run(world).unwrap();
        assert_eq!(&*events.borrow(), &["visit:root"]);
    }
}

#[test]
fn generic_before_typed_adaptors_keep_both_stages() {
    let (context, world) = tree();
    let events = Events::default();
    let mut pm = PassManager::on::<World>(context, Nesting::Explicit);
    pm.nest_any().add_pass(record("any", &events));
    pm.nest::<Function>().add_pass(record("typed", &events));
    pm.run(world).unwrap();
    assert_eq!(&*events.borrow(), &["any:root", "typed:root"]);
}

#[crate::derive::operation_trait]
trait ValidChildren {
    #[verifier]
    fn verify_children(op: &Operation, _context: &Context) -> Result<(), Report> {
        for region in op.regions().iter() {
            for block in region.body().iter() {
                if block.body().iter().any(|child| child.has_attribute("invalid")) {
                    return Err(Report::msg("container forbids invalid children"));
                }
            }
        }
        Ok(())
    }
}

#[crate::derive::operation(
    dialect = TestDialect,
    traits(NoTerminator, HasOnlyGraphRegion, IsolatedFromAbove, ValidChildren),
    implements(RegionKindInterface)
)]
pub struct RegionPair {
    #[region]
    first: crate::RegionRef,
    #[region]
    second: crate::RegionRef,
}

impl crate::RegionKindInterface for RegionPair {
    fn kind(&self) -> crate::RegionKind {
        crate::RegionKind::Graph
    }
}

#[crate::derive::operation(dialect = TestDialect, traits(IsolatedFromAbove))]
pub struct First {}
#[crate::derive::operation(dialect = TestDialect, traits(IsolatedFromAbove))]
pub struct Second {}

struct RecordOps(Events);
impl Pass for RecordOps {
    type Target = Operation;

    fn name(&self) -> &'static str {
        "record-ops"
    }

    fn can_schedule_on(&self, _name: &OperationName) -> bool {
        true
    }

    fn run_on_operation(
        &mut self,
        op: EntityMut<'_, Operation>,
        state: &mut PassExecutionState,
    ) -> Result<(), Report> {
        self.0.borrow_mut().push(op.name().to_string());
        state.set_post_pass_status(PostPassStatus::Unchanged);
        Ok(())
    }
}

#[test]
fn immediate_scheduling_visits_all_regions_in_order() {
    use crate::{Builder, BuilderExt, OpBuilder};
    let context = Rc::new(Context::default());
    let mut builder = OpBuilder::new(context.clone());
    let pair = builder.create::<RegionPair, ()>(crate::SourceSpan::UNKNOWN)().unwrap();
    let regions = pair
        .as_operation_ref()
        .borrow()
        .regions()
        .iter()
        .map(|r| r.as_region_ref())
        .collect::<Vec<_>>();
    assert_eq!(regions.len(), 2);
    builder.create_block(regions[0], None, &[]);
    builder.create::<First, ()>(crate::SourceSpan::UNKNOWN)().unwrap();
    builder.create_block(regions[1], None, &[]);
    builder.create::<Second, ()>(crate::SourceSpan::UNKNOWN)().unwrap();
    let events = Events::default();
    let mut pm = PassManager::on::<RegionPair>(context, Nesting::Explicit);
    pm.nest::<First>().add_pass(Box::new(RecordOps(events.clone())));
    pm.nest::<Second>().add_pass(Box::new(RecordOps(events.clone())));
    pm.run(pair.as_operation_ref()).unwrap();
    assert_eq!(&*events.borrow(), &["test.first", "test.second"]);
}

#[test]
fn recursive_stages_visit_each_function_once_and_keep_stage_order() {
    let (context, world) = tree();
    let events = Events::default();
    let mut pm = PassManager::on::<World>(context, Nesting::Explicit);
    pm.nest_recursive::<Function>().add_pass(record("a", &events));
    pm.nest::<Function>().add_pass(record("immediate", &events));
    pm.nest_recursive::<Function>().add_pass(record("b", &events));
    pm.run(world).unwrap();
    assert_eq!(
        &*events.borrow(),
        &[
            "a:root",
            "a:first",
            "a:deep",
            "a:last",
            "immediate:root",
            "b:root",
            "b:first",
            "b:deep",
            "b:last",
        ]
    );
}

#[test]
fn recursive_matches_delegate_their_subtree_to_the_nested_pipeline() {
    for recurse_inside in [false, true] {
        let (context, world) = tree();
        let events = Events::default();
        let mut pm = PassManager::on::<World>(context, Nesting::Explicit);
        {
            let mut modules = pm.nest_recursive::<Module>();
            if recurse_inside {
                modules.nest_recursive::<Function>().add_pass(record("visit", &events));
            } else {
                modules.nest::<Function>().add_pass(record("visit", &events));
            }
        }
        pm.run(world).unwrap();
        let expected = if recurse_inside {
            &["visit:first", "visit:deep", "visit:last"][..]
        } else {
            &["visit:first", "visit:last"][..]
        };
        assert_eq!(&*events.borrow(), expected);
    }
}

#[test]
fn recursive_scheduling_excludes_its_root() {
    let (context, world) = tree();
    let events = Events::default();
    let mut pm = PassManager::on::<World>(context, Nesting::Explicit);
    pm.nest_recursive::<World>().add_pass(Box::new(RecordOps(events.clone())));
    pm.run(world).unwrap();
    assert!(events.borrow().is_empty());
}

#[derive(Default)]
struct OperationIdentity(Option<OperationRef>);
impl crate::pass::Analysis for OperationIdentity {
    type Target = Operation;

    fn as_any(&self) -> &dyn core::any::Any {
        self
    }

    fn as_any_rc(self: Rc<Self>) -> Rc<dyn core::any::Any> {
        self
    }

    fn name(&self) -> &'static str {
        "operation-identity"
    }

    fn analyze(&mut self, op: &Operation, _am: AnalysisManager) -> Result<(), Report> {
        self.0 = Some(op.as_operation_ref());
        Ok(())
    }

    fn invalidate(&self, preserved: &mut crate::pass::PreservedAnalyses) -> bool {
        !preserved.is_preserved::<Self>()
    }
}

struct InspectAnalyses(Events);
impl Pass for InspectAnalyses {
    type Target = Operation;

    fn name(&self) -> &'static str {
        "inspect-analyses"
    }

    fn can_schedule_on(&self, _name: &OperationName) -> bool {
        true
    }

    fn run_on_operation(
        &mut self,
        op: EntityMut<'_, Operation>,
        state: &mut PassExecutionState,
    ) -> Result<(), Report> {
        let root = op.as_operation_ref();
        let is_world = op.is::<World>();
        drop(op);
        let am = state.analysis_manager();
        assert_eq!(am.get_analysis::<OperationIdentity>()?.0, Some(root));
        if is_world {
            // Cache analyses at every namespace depth before the recursive function traversal.
            let result = root.raw_prewalk::<Forward, _, _>(|op: OperationRef| {
                if op.borrow().is::<Module>()
                    && let Err(err) = am.get_child_analysis::<OperationIdentity>(op)
                {
                    return WalkResult::Break(err);
                }
                WalkResult::Continue(())
            });
            if let WalkResult::Break(err) = result {
                return Err(err);
            }
        } else {
            let mut next = root.borrow().parent_op();
            let mut count = 0;
            while let Some(parent) = next {
                let cached = am
                    .get_cached_parent_analysis::<OperationIdentity>(parent)
                    .expect("ancestor analysis remains accessible through recursive scheduling");
                assert_eq!(cached.0, Some(parent));
                count += 1;
                next = parent.borrow().parent_op();
            }
            let op = root.borrow();
            let function = op.downcast_ref::<Function>().unwrap();
            self.0.borrow_mut().push(format!("{}:{count}", Symbol::name(function)));
        }
        state.preserved_analyses_mut().preserve_all();
        state.set_post_pass_status(PostPassStatus::Unchanged);
        Ok(())
    }
}

#[test]
fn recursive_scheduling_preserves_analysis_ancestry() {
    let (context, world) = tree();
    let events = Events::default();
    let mut pm = PassManager::on::<World>(context, Nesting::Explicit);
    pm.add_pass(Box::new(InspectAnalyses(events.clone())));
    pm.nest_recursive::<Function>()
        .add_pass(Box::new(InspectAnalyses(events.clone())));
    pm.run(world).unwrap();
    assert_eq!(&*events.borrow(), &["root:1", "first:2", "deep:3", "last:2"]);
}

struct FailOnDeep;
impl Pass for FailOnDeep {
    type Target = Function;

    fn name(&self) -> &'static str {
        "fail-on-deep"
    }

    fn can_schedule_on(&self, name: &OperationName) -> bool {
        name.is::<Function>()
    }

    fn run_on_operation(
        &mut self,
        op: EntityMut<'_, Function>,
        state: &mut PassExecutionState,
    ) -> Result<(), Report> {
        if Symbol::name(&*op).as_str() == "deep" {
            Err(Report::msg("deep function rejected"))
        } else {
            state.set_post_pass_status(PostPassStatus::Unchanged);
            Ok(())
        }
    }
}

struct ObserveFailure(Events);
impl PassInstrumentation for ObserveFailure {
    fn run_before_pass(&mut self, pass: &dyn OperationPass, op: &OperationRef) {
        if pass.name() == "fail-on-deep" {
            let op = op.borrow();
            self.0
                .borrow_mut()
                .push(format!("before:{}", Symbol::name(op.downcast_ref::<Function>().unwrap())));
        }
    }

    fn run_after_pass_failed(&mut self, pass: &dyn OperationPass, op: &OperationRef) {
        if pass.name() == "fail-on-deep" {
            let op = op.borrow();
            self.0
                .borrow_mut()
                .push(format!("failed:{}", Symbol::name(op.downcast_ref::<Function>().unwrap())));
        }
    }
}

#[test]
fn recursive_failure_is_instrumented_and_stops_siblings_and_later_stages() {
    let (context, world) = tree();
    let events = Events::default();
    let mut pm = PassManager::on::<World>(context, Nesting::Explicit);
    pm.add_instrumentation(Box::new(ObserveFailure(events.clone())));
    pm.nest_recursive::<Function>().add_pass(Box::new(FailOnDeep));
    pm.nest_recursive::<Function>().add_pass(record("later", &events));
    assert!(pm.run(world).unwrap_err().to_string().contains("deep function rejected"));
    assert_eq!(
        &*events.borrow(),
        &["before:root", "before:first", "before:deep", "failed:deep"]
    );
}

#[test]
fn adjacent_recursive_pipelines_do_not_coalesce_into_per_function_order() {
    let (context, world) = tree();
    let events = Events::default();
    let mut pm = PassManager::on::<World>(context, Nesting::Explicit);
    pm.nest_recursive::<Function>().add_pass(record("a", &events));
    pm.nest_recursive::<Function>().add_pass(record("b", &events));
    pm.run(world).unwrap();
    assert_eq!(
        &*events.borrow(),
        &["a:root", "a:first", "a:deep", "a:last", "b:root", "b:first", "b:deep", "b:last",]
    );
}

struct InvalidateParent;
impl Pass for InvalidateParent {
    type Target = Operation;

    fn name(&self) -> &'static str {
        "invalidate-parent"
    }

    fn can_schedule_on(&self, name: &OperationName) -> bool {
        name.is::<First>()
    }

    fn run_on_operation(
        &mut self,
        mut op: EntityMut<'_, Operation>,
        state: &mut PassExecutionState,
    ) -> Result<(), Report> {
        let marker = op
            .context_rc()
            .create_attribute::<crate::dialects::builtin::attributes::BoolAttr, _>(true);
        op.set_attribute("invalid", marker);
        state.set_post_pass_status(PostPassStatus::Changed);
        Ok(())
    }
}

#[test]
fn recursive_scheduling_reverifies_unmatched_containers_after_child_changes() {
    use crate::{Builder, BuilderExt, OpBuilder};
    let context = Rc::new(Context::default());
    let mut builder = OpBuilder::new(context.clone());
    let root = builder.create::<RegionPair, ()>(crate::SourceSpan::UNKNOWN)().unwrap();
    let region = root.borrow().first().as_region_ref();
    builder.create_block(region, None, &[]);
    let container = builder.create::<RegionPair, ()>(crate::SourceSpan::UNKNOWN)().unwrap();
    let region = container.borrow().first().as_region_ref();
    builder.create_block(region, None, &[]);
    builder.create::<First, ()>(crate::SourceSpan::UNKNOWN)().unwrap();
    root.as_operation_ref().borrow().recursively_verify().unwrap();

    let later = Events::default();
    let mut pm = PassManager::on::<RegionPair>(context, Nesting::Explicit);
    pm.nest_recursive::<First>().add_pass(Box::new(InvalidateParent));
    pm.add_pass(Box::new(RecordOps(later.clone())));
    assert!(
        pm.run(root.as_operation_ref()).is_err(),
        "the intermediate container invariant must be checked"
    );
    assert!(later.borrow().is_empty(), "verification failure must stop later passes");
}
