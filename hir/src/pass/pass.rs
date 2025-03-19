use alloc::{boxed::Box, rc::Rc};
use core::{any::Any, fmt};

use super::*;
use crate::{Context, EntityMut, OperationName, OperationRef, Report};

/// A type-erased [Pass].
///
/// This is used to allow heterogenous passes to be operated on uniformly.
///
/// Semantically, an [OperationPass] behaves like a `Pass<Target = Operation>`.
#[allow(unused_variables)]
pub trait OperationPass {
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
    fn into_any(self: Box<Self>) -> Box<dyn Any>;
    fn name(&self) -> &'static str;
    fn argument(&self) -> &'static str {
        // NOTE: Could we compute an argument string from the type name?
        ""
    }
    fn description(&self) -> &'static str {
        ""
    }
    fn info(&self) -> PassInfo {
        PassInfo::lookup(self.argument()).expect("could not find pass information")
    }
    /// The name of the operation that this pass operates on, or `None` if this is a generic pass.
    fn target_name(&self, context: &Context) -> Option<OperationName>;
    fn initialize_options(&mut self, options: &str) -> Result<(), Report> {
        Ok(())
    }
    fn print_as_textual_pipeline(&self, f: &mut fmt::Formatter) -> fmt::Result;
    fn has_statistics(&self) -> bool {
        !self.statistics().is_empty()
    }
    fn statistics(&self) -> &[Box<dyn Statistic>];
    fn statistics_mut(&mut self) -> &mut [Box<dyn Statistic>];
    fn initialize(&mut self, context: Rc<Context>) -> Result<(), Report> {
        Ok(())
    }
    fn can_schedule_on(&self, name: &OperationName) -> bool;
    fn run_on_operation(
        &mut self,
        op: OperationRef,
        state: &mut PassExecutionState,
    ) -> Result<(), Report>;
    fn run_pipeline(
        &mut self,
        pipeline: &mut OpPassManager,
        op: OperationRef,
        state: &mut PassExecutionState,
    ) -> Result<(), Report>;
}

impl<P> OperationPass for P
where
    P: Pass + 'static,
{
    fn as_any(&self) -> &dyn Any {
        <P as Pass>::as_any(self)
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        <P as Pass>::as_any_mut(self)
    }

    fn into_any(self: Box<Self>) -> Box<dyn Any> {
        <P as Pass>::into_any(self)
    }

    fn name(&self) -> &'static str {
        <P as Pass>::name(self)
    }

    fn argument(&self) -> &'static str {
        <P as Pass>::argument(self)
    }

    fn description(&self) -> &'static str {
        <P as Pass>::description(self)
    }

    fn info(&self) -> PassInfo {
        <P as Pass>::info(self)
    }

    fn target_name(&self, context: &Context) -> Option<OperationName> {
        <P as Pass>::target_name(self, context)
    }

    fn initialize_options(&mut self, options: &str) -> Result<(), Report> {
        <P as Pass>::initialize_options(self, options)
    }

    fn print_as_textual_pipeline(&self, f: &mut fmt::Formatter) -> fmt::Result {
        <P as Pass>::print_as_textual_pipeline(self, f)
    }

    fn has_statistics(&self) -> bool {
        <P as Pass>::has_statistics(self)
    }

    fn statistics(&self) -> &[Box<dyn Statistic>] {
        <P as Pass>::statistics(self)
    }

    fn statistics_mut(&mut self) -> &mut [Box<dyn Statistic>] {
        <P as Pass>::statistics_mut(self)
    }

    fn initialize(&mut self, context: Rc<Context>) -> Result<(), Report> {
        <P as Pass>::initialize(self, context)
    }

    fn can_schedule_on(&self, name: &OperationName) -> bool {
        <P as Pass>::can_schedule_on(self, name)
    }

    fn run_on_operation(
        &mut self,
        mut op: OperationRef,
        state: &mut PassExecutionState,
    ) -> Result<(), Report> {
        let op = <<P as Pass>::Target as PassTarget>::into_target_mut(&mut op);
        <P as Pass>::run_on_operation(self, op, state)
    }

    fn run_pipeline(
        &mut self,
        pipeline: &mut OpPassManager,
        op: OperationRef,
        state: &mut PassExecutionState,
    ) -> Result<(), Report> {
        <P as Pass>::run_pipeline(self, pipeline, op, state)
    }
}

/// A compiler pass which operates on an [Operation] of some kind.
#[allow(unused_variables)]
pub trait Pass: Sized + Any {
    /// The concrete/trait type targeted by this pass.
    ///
    /// Calls to `get_operation` will return a reference of this type.
    type Target: ?Sized + PassTarget;

    /// Used for downcasting
    #[inline(always)]
    fn as_any(&self) -> &dyn Any {
        self as &dyn Any
    }

    /// Used for downcasting
    #[inline(always)]
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self as &mut dyn Any
    }

    /// Used for downcasting
    #[inline(always)]
    fn into_any(self: Box<Self>) -> Box<dyn Any> {
        self as Box<dyn Any>
    }

    /// The display name of this pass
    fn name(&self) -> &'static str;
    /// The command line option name used to control this pass
    fn argument(&self) -> &'static str {
        // NOTE: Could we compute an argument string from the type name or `self.name()`?
        ""
    }
    /// A description of what this pass does.
    fn description(&self) -> &'static str {
        ""
    }
    /// Obtain the underlying [PassInfo] object for this pass.
    fn info(&self) -> PassInfo {
        PassInfo::lookup(self.argument()).expect("pass is not currently registered")
    }
    /// The name of the operation that this pass operates on, or `None` if this is a generic pass.
    fn target_name(&self, context: &Context) -> Option<OperationName> {
        <<Self as Pass>::Target as PassTarget>::target_name(context)
    }
    /// If command-line options are provided for this pass, implementations must parse the raw
    /// options here, returning `Err` if parsing fails for some reason.
    ///
    /// By default, this is a no-op.
    fn initialize_options(&mut self, options: &str) -> Result<(), Report> {
        Ok(())
    }
    /// Prints out the pass in the textual representation of pipelines.
    ///
    /// If this is an adaptor pass, print its pass managers.
    fn print_as_textual_pipeline(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let argument = self.argument();
        if !argument.is_empty() {
            write!(f, "{argument}")
        } else {
            write!(f, "unknown<{}>", self.name())
        }
    }
    /// Returns true if this pass has associated statistics
    fn has_statistics(&self) -> bool {
        !self.statistics().is_empty()
    }
    /// Get pass statistics associated with this pass
    fn statistics(&self) -> &[Box<dyn Statistic>] {
        &[]
    }
    /// Get mutable access to the pass statistics associated with this pass
    fn statistics_mut(&mut self) -> &mut [Box<dyn Statistic>] {
        &mut []
    }
    /// Initialize any complex state necessary for running this pass.
    ///
    /// This hook should not rely on any state accessible during the execution of a pass. For
    /// example, `context`/`get_operation`/`get_analysis`/etc. should not be invoked within this
    /// hook.
    ///
    /// This method is invoked after all dependent dialects for the pipeline are loaded, and is not
    /// allowed to load any further dialects (override the `get_dependent_dialects()` hook for this
    /// purpose instead). Returns `Err` with a diagnostic if initialization fails, in which case the
    /// pass pipeline won't execute.
    fn initialize(&mut self, context: Rc<Context>) -> Result<(), Report> {
        Ok(())
    }
    /// Query if this pass can be scheduled to run on the given operation type.
    fn can_schedule_on(&self, name: &OperationName) -> bool;
    /// Run this pass on the current operation
    fn run_on_operation(
        &mut self,
        op: EntityMut<'_, Self::Target>,
        state: &mut PassExecutionState,
    ) -> Result<(), Report>;
    /// Schedule an arbitrary pass pipeline on the provided operation.
    ///
    /// This can be invoke any time in a pass to dynamic schedule more passes. The provided
    /// operation must be the current one or one nested below.
    fn run_pipeline(
        &mut self,
        pipeline: &mut OpPassManager,
        op: OperationRef,
        state: &mut PassExecutionState,
    ) -> Result<(), Report> {
        state.run_pipeline(pipeline, op)
    }
}

impl<P> Pass for Box<P>
where
    P: Pass,
{
    type Target = <P as Pass>::Target;

    fn as_any(&self) -> &dyn Any {
        (**self).as_any()
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        (**self).as_any_mut()
    }

    fn into_any(self: Box<Self>) -> Box<dyn Any> {
        let pass = Box::into_inner(self);
        <P as Pass>::into_any(pass)
    }

    #[inline]
    fn name(&self) -> &'static str {
        (**self).name()
    }

    #[inline]
    fn argument(&self) -> &'static str {
        (**self).argument()
    }

    #[inline]
    fn description(&self) -> &'static str {
        (**self).description()
    }

    #[inline]
    fn info(&self) -> PassInfo {
        (**self).info()
    }

    #[inline]
    fn target_name(&self, context: &Context) -> Option<OperationName> {
        (**self).target_name(context)
    }

    #[inline]
    fn initialize_options(&mut self, options: &str) -> Result<(), Report> {
        (**self).initialize_options(options)
    }

    #[inline]
    fn print_as_textual_pipeline(&self, f: &mut fmt::Formatter) -> fmt::Result {
        (**self).print_as_textual_pipeline(f)
    }

    #[inline]
    fn has_statistics(&self) -> bool {
        (**self).has_statistics()
    }

    #[inline]
    fn statistics(&self) -> &[Box<dyn Statistic>] {
        (**self).statistics()
    }

    #[inline]
    fn statistics_mut(&mut self) -> &mut [Box<dyn Statistic>] {
        (**self).statistics_mut()
    }

    #[inline]
    fn initialize(&mut self, context: Rc<Context>) -> Result<(), Report> {
        (**self).initialize(context)
    }

    #[inline]
    fn can_schedule_on(&self, name: &OperationName) -> bool {
        (**self).can_schedule_on(name)
    }

    #[inline]
    fn run_on_operation(
        &mut self,
        op: EntityMut<'_, Self::Target>,
        state: &mut PassExecutionState,
    ) -> Result<(), Report> {
        (**self).run_on_operation(op, state)
    }

    #[inline]
    fn run_pipeline(
        &mut self,
        pipeline: &mut OpPassManager,
        op: OperationRef,
        state: &mut PassExecutionState,
    ) -> Result<(), Report> {
        (**self).run_pipeline(pipeline, op, state)
    }
}

pub type DynamicPipelineExecutor =
    dyn FnMut(&mut OpPassManager, OperationRef) -> Result<(), Report>;

/// The state for a single execution of a pass. This provides a unified
/// interface for accessing and initializing necessary state for pass execution.
pub struct PassExecutionState {
    /// The operation being transformed
    op: OperationRef,
    context: Rc<Context>,
    analysis_manager: AnalysisManager,
    /// The set of preserved analyses for the current execution
    preserved_analyses: PreservedAnalyses,
    // Callback in the pass manager that allows one to schedule dynamic pipelines that will be
    // rooted at the provided operation.
    #[allow(unused)]
    pipeline_executor: Option<Box<DynamicPipelineExecutor>>,
}
impl PassExecutionState {
    pub fn new(
        op: OperationRef,
        context: Rc<Context>,
        analysis_manager: AnalysisManager,
        pipeline_executor: Option<Box<DynamicPipelineExecutor>>,
    ) -> Self {
        Self {
            op,
            context,
            analysis_manager,
            preserved_analyses: Default::default(),
            pipeline_executor,
        }
    }

    #[inline(always)]
    pub fn context(&self) -> Rc<Context> {
        self.context.clone()
    }

    #[inline(always)]
    pub const fn current_operation(&self) -> &OperationRef {
        &self.op
    }

    #[inline(always)]
    pub const fn analysis_manager(&self) -> &AnalysisManager {
        &self.analysis_manager
    }

    #[inline(always)]
    pub const fn preserved_analyses(&self) -> &PreservedAnalyses {
        &self.preserved_analyses
    }

    #[inline(always)]
    pub fn preserved_analyses_mut(&mut self) -> &mut PreservedAnalyses {
        &mut self.preserved_analyses
    }

    pub fn run_pipeline(
        &mut self,
        pipeline: &mut OpPassManager,
        op: OperationRef,
    ) -> Result<(), Report> {
        if let Some(pipeline_executor) = self.pipeline_executor.as_deref_mut() {
            pipeline_executor(pipeline, op)
        } else {
            Ok(())
        }
    }
}
