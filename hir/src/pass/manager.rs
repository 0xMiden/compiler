use alloc::{
    boxed::Box,
    collections::BTreeMap,
    format,
    rc::Rc,
    string::{String, ToString},
};

use compact_str::{CompactString, ToCompactString};
use midenc_session::{Options, diagnostics::Severity};
use smallvec::{SmallVec, smallvec};

use super::{
    AnalysisManager, OpFilter, OperationPass, Pass, PassExecutionState, PassInstrumentation,
    PassInstrumentor, PipelineParentInfo, Statistic,
};
use crate::{
    Context, EntityMut, OpPrintingFlags, OpRegistration, Operation, OperationName, OperationRef,
    Report,
    pass::{PostPassStatus, Print},
    traits::IsolatedFromAbove,
};

#[derive(Debug, Default, Copy, Clone, PartialEq, Eq)]
pub enum Nesting {
    Implicit,
    #[default]
    Explicit,
}

#[derive(Debug, Default, Copy, Clone, PartialEq, Eq)]
pub enum PassDisplayMode {
    List,
    #[default]
    Pipeline,
}

#[derive(Default, Debug)]
pub struct IRPrintingConfig {
    pub print_module_scope: bool,
    pub print_after_only_on_failure: bool,
    // NOTE: Taken from the Options struct
    pub print_ir_after_all: bool,
    pub print_ir_after_pass: SmallVec<[String; 1]>,
    pub print_ir_after_modified: bool,
    pub print_ir_filters: SmallVec<[OpFilter; 1]>,
    pub flags: OpPrintingFlags,
}

impl TryFrom<&Options> for IRPrintingConfig {
    type Error = Report;

    fn try_from(options: &Options) -> Result<Self, Self::Error> {
        let pass_filters = options.print_ir_after_pass.clone();

        if options.print_ir_after_all && !pass_filters.is_empty() {
            return Err(Report::msg(
                "Flags `print_ir_after_all` and `print_ir_after_pass` are mutually exclusive. \
                 Please select only one."
                    .to_string(),
            ));
        };

        let print_ir_filters = options
            .print_ir_filters
            .iter()
            .cloned()
            .map(OpFilter::from)
            .filter(|filter| !matches!(filter, OpFilter::All))
            .collect();

        Ok(Self {
            print_ir_after_all: options.print_ir_after_all,
            print_ir_after_pass: pass_filters.into(),
            print_ir_after_modified: options.print_ir_after_modified,
            print_ir_filters,
            ..Default::default()
        })
    }
}

/// The main pass manager and pipeline builder
pub struct PassManager {
    context: Rc<Context>,
    /// The underlying pass manager
    pm: OpPassManager,
    /// A manager for pass instrumentation
    instrumentor: Rc<PassInstrumentor>,
    /// An optional crash reproducer generator, if this pass manager is setup to
    /// generate reproducers.
    ///crash_reproducer_generator: Rc<PassCrashReproducerGenerator>,
    /// Indicates whether to print pass statistics
    statistics: Option<PassDisplayMode>,
    /// Indicates whether or not pass timing is enabled
    timing: bool,
    /// Indicates whether or not to run verification between passes
    verification: bool,
}

impl PassManager {
    /// Create a new pass manager under the given context with a specific nesting style. The created
    /// pass manager can schedule operations that match `name`.
    pub fn new(context: Rc<Context>, name: impl AsRef<str>, nesting: Nesting) -> Self {
        let pm = OpPassManager::new(name.as_ref(), nesting, context.clone());
        Self {
            context,
            pm,
            instrumentor: Default::default(),
            statistics: None,
            timing: false,
            verification: true,
        }
    }

    /// Create a new pass manager under the given context with a specific nesting style.
    ///
    /// The created pass manager can schedule operations that match type `T`.
    pub fn on<T: OpRegistration>(context: Rc<Context>, nesting: Nesting) -> Self {
        Self::new(context, <T as OpRegistration>::full_name(), nesting)
    }

    /// Run the passes within this manager on the provided operation. The
    /// specified operation must have the same name as the one provided the pass
    /// manager on construction.
    pub fn run(&mut self, op: OperationRef) -> Result<(), Report> {
        use crate::Spanned;

        let op_name = op.borrow().name();
        let anchor = self.pm.name();
        if let Some(anchor) = anchor
            && anchor != &op_name
        {
            return Err(self
                .context
                .diagnostics()
                .diagnostic(Severity::Error)
                .with_message("failed to construct pass manager")
                .with_primary_label(
                    op.borrow().span(),
                    format!("can't run '{anchor}' pass manager on '{op_name}'"),
                )
                .into_report());
        }

        // Register all dialects for the current pipeline.
        /*
        let dependent_dialects = self.get_dependent_dialects();
        self.context.append_dialect_registry(dependent_dialects);
        for dialect_name in dependent_dialects.names() {
            self.context.get_or_register_dialect(dialect_name);
        }
        */

        // Before running, make sure to finalize the pipeline pass list.
        self.pm.finalize_pass_list()?;

        // Run pass initialization
        self.pm.initialize()?;

        // Construct a top level analysis manager for the pipeline.
        let analysis_manager = AnalysisManager::new(op, Some(self.instrumentor.clone()));

        // If reproducer generation is enabled, run the pass manager with crash handling enabled.
        /*
        let result = if self.crash_reproducer_generator.is_some() {
            self.run_with_crash_recovery(op, analysis_manager);
        } else {
            self.run_passes(op, analysis_manager);
        }
        */
        let result = self.run_passes(op, analysis_manager);

        // Dump all of the pass statistics if necessary.
        #[cfg(feature = "std")]
        if self.statistics.is_some() {
            let mut output = alloc::string::String::new();
            self.dump_statistics(&mut output).map_err(Report::msg)?;
            std::println!("{output}");
        }

        result
    }

    fn run_passes(
        &mut self,
        op: OperationRef,
        analysis_manager: AnalysisManager,
    ) -> Result<(), Report> {
        OpToOpPassAdaptor::run_pipeline(
            &mut self.pm,
            op,
            analysis_manager,
            self.verification,
            Some(self.instrumentor.clone()),
            Some(&PipelineParentInfo { pass: None }),
        )
    }

    #[inline]
    pub fn context(&self) -> Rc<Context> {
        self.context.clone()
    }

    /// Runs the verifier after each individual pass.
    pub fn enable_verifier(&mut self, yes: bool) -> &mut Self {
        self.verification = yes;
        self
    }

    pub fn add_instrumentation(&mut self, pi: Box<dyn PassInstrumentation>) -> &mut Self {
        self.instrumentor.add_instrumentation(pi);
        self
    }

    pub fn enable_ir_printing(mut self, config: IRPrintingConfig) -> Self {
        let print = Print::new(&config);

        if let Some(print) = print {
            let print = Box::new(print);
            self.add_instrumentation(print);
        }
        self
    }

    pub fn enable_timing(&mut self, yes: bool) -> &mut Self {
        self.timing = yes;
        self
    }

    pub fn enable_statistics(&mut self, mode: Option<PassDisplayMode>) -> &mut Self {
        self.statistics = mode;
        self
    }

    fn dump_statistics(&mut self, out: &mut dyn core::fmt::Write) -> core::fmt::Result {
        self.pm.print_statistics(out, self.statistics.unwrap_or_default())
    }

    pub fn print_as_textual_pipeline(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.pm.print_as_textual_pipeline(f)
    }

    pub fn nest<T: OpRegistration>(&mut self) -> NestedOpPassManager<'_> {
        self.pm.nest::<T>()
    }

    pub fn nest_pass_manager(&mut self, nested: OpPassManager) -> NestedOpPassManager<'_> {
        self.pm.nest_pass_manager(nested)
    }

    /// Nest a new op-specific pass manager (for the op with the given name), under this pass manager.
    pub fn nest_with_type(&mut self, nested_name: &str) -> NestedOpPassManager<'_> {
        self.pm.nest_with_type(nested_name)
    }

    /// Nest a new op-agnostic ("any") pass manager under this pass manager.
    pub fn nest_any(&mut self) -> NestedOpPassManager<'_> {
        self.pm.nest_any()
    }

    pub fn add_pass(&mut self, pass: Box<dyn OperationPass>) {
        self.pm.add_pass(pass)
    }

    pub fn add_nested_pass<T: OpRegistration>(&mut self, pass: Box<dyn OperationPass>) {
        self.pm.add_nested_pass::<T>(pass)
    }
}

/// This class represents a pass manager that runs passes on either a specific
/// operation type, or any isolated operation. This pass manager can not be run
/// on an operation directly, but must be run either as part of a top-level
/// `PassManager`(e.g. when constructed via `nest` calls), or dynamically within
/// a pass by using the `Pass::runPipeline` API.
pub struct OpPassManager {
    /// The current context
    context: Rc<Context>,
    /// The name of the operation that passes of this pass manager operate on
    name: Option<OperationName>,
    /// The set of passes to run as part of this pass manager
    passes: SmallVec<[Box<dyn OperationPass>; 8]>,
    /// Control the implicit nesting of passes that mismatch the name set for this manager
    nesting: Nesting,
}

impl OpPassManager {
    pub const ANY: &str = "any";

    /// Construct a new op-agnostic ("any") pass manager with the given operation
    /// type and nesting behavior. This is the same as invoking:
    /// `OpPassManager(OpPassManager::ANY, nesting)`.
    pub fn any(nesting: Nesting, context: Rc<Context>) -> Self {
        Self {
            context,
            name: None,
            passes: Default::default(),
            nesting,
        }
    }

    pub fn new(name: &str, nesting: Nesting, context: Rc<Context>) -> Self {
        if name == Self::ANY {
            return Self::any(nesting, context);
        }

        let (dialect_name, opcode) = name.split_once('.').expect(
            "invalid operation name: expected format `<dialect>.<name>`, but missing `<dialect>.`",
        );
        let dialect_name = crate::interner::Symbol::intern(dialect_name);
        let dialect = context.get_registered_dialect(dialect_name);
        let ops = dialect.registered_ops();
        let name =
            ops.iter()
                .find(|name| name.name().as_str() == opcode)
                .cloned()
                .unwrap_or_else(|| {
                    panic!(
                        "invalid operation name: found dialect '{dialect_name}', but no operation \
                         called '{opcode}' is registered to that dialect"
                    )
                });
        Self {
            context,
            name: Some(name),
            passes: Default::default(),
            nesting,
        }
    }

    pub fn on<T: OpRegistration>(nesting: Nesting, context: Rc<Context>) -> Self {
        let dialect_name = <T as OpRegistration>::dialect_name();
        let opcode = <T as OpRegistration>::name();
        let dialect = context.get_registered_dialect(dialect_name);
        let name = dialect
            .registered_ops()
            .iter()
            .find(|n| n.name() == opcode)
            .cloned()
            .unwrap_or_else(|| {
                panic!(
                    "invalid operation name: found dialect '{dialect_name}', but no operation \
                     called '{opcode}' is registered to that dialect"
                );
            });
        Self {
            context,
            name: Some(name),
            passes: Default::default(),
            nesting,
        }
    }

    pub fn for_operation(name: OperationName, nesting: Nesting, context: Rc<Context>) -> Self {
        Self {
            context,
            name: Some(name),
            passes: Default::default(),
            nesting,
        }
    }

    pub fn context(&self) -> Rc<Context> {
        self.context.clone()
    }

    pub fn passes(&self) -> &[Box<dyn OperationPass>] {
        &self.passes
    }

    pub fn passes_mut(&mut self) -> &mut [Box<dyn OperationPass>] {
        &mut self.passes
    }

    pub fn is_empty(&self) -> bool {
        self.passes.is_empty()
    }

    pub fn len(&self) -> usize {
        self.passes.len()
    }

    pub fn is_op_agnostic(&self) -> bool {
        self.name.is_none()
    }

    pub fn clear(&mut self) {
        self.passes.clear();
    }

    /// Nest a new op-specific pass manager (for the op with the given name), under this pass manager.
    pub fn nest_with_type(&mut self, nested_name: &str) -> NestedOpPassManager<'_> {
        self.nest_pass_manager(Self::new(nested_name, self.nesting, self.context.clone()))
    }

    pub fn nest<T: OpRegistration>(&mut self) -> NestedOpPassManager<'_> {
        self.nest_pass_manager(Self::on::<T>(self.nesting, self.context.clone()))
    }

    /// Nest a new op-agnostic ("any") pass manager under this pass manager.
    pub fn nest_any(&mut self) -> NestedOpPassManager<'_> {
        self.nest_pass_manager(Self::any(self.nesting, self.context.clone()))
    }

    fn nest_for(&mut self, nested_name: OperationName) -> NestedOpPassManager<'_> {
        self.nest_pass_manager(Self::for_operation(nested_name, self.nesting, self.context.clone()))
    }

    pub fn add_pass(&mut self, pass: Box<dyn OperationPass>) {
        // If this pass runs on a different operation than this pass manager, then implicitly
        // nest a pass manager for this operation if enabled.
        let pass_op_name = pass.target_name(&self.context);
        if let Some(pass_op_name) = pass_op_name
            && self.name.as_ref().is_some_and(|name| name != &pass_op_name)
        {
            if matches!(self.nesting, Nesting::Implicit) {
                let mut nested = self.nest_for(pass_op_name);
                nested.add_pass(pass);
                return;
            }
            panic!(
                "cannot add pass '{}' restricted to '{pass_op_name}' to a pass manager intended \
                 to run on '{}', did you intend to nest?",
                pass.name(),
                self.name().unwrap(),
            );
        }

        self.passes.push(pass);
    }

    pub fn add_nested_pass<T: OpRegistration>(&mut self, pass: Box<dyn OperationPass>) {
        let mut nested = self.nest::<T>();
        nested.add_pass(pass);
    }

    pub fn finalize_pass_list(&mut self) -> Result<(), Report> {
        let finalize_adaptor = |adaptor: &mut OpToOpPassAdaptor| -> Result<(), Report> {
            for pm in adaptor.pass_managers_mut() {
                pm.finalize_pass_list()?;
            }

            Ok(())
        };

        // Walk the pass list and merge adjacent adaptors.
        let num_passes = self.passes.len();
        let passes = core::mem::replace(&mut self.passes, SmallVec::with_capacity(num_passes));
        let prev_adaptor = None::<Box<OpToOpPassAdaptor>>;
        let (_, prev_adaptor) = passes.into_iter().try_fold(
            (&mut self.passes, prev_adaptor),
            |(passes, prev), mut pass| {
                // Is this pass an adaptor?
                match pass.as_any_mut().downcast_mut::<OpToOpPassAdaptor>() {
                    // Yes, merge it into the previous one if present, otherwise use this as the
                    // first adaptor in a potential chain of them
                    Some(adaptor) => match prev {
                        Some(mut prev_adaptor) => {
                            if adaptor.try_merge_into(&mut prev_adaptor) {
                                Ok::<_, Report>((passes, Some(prev_adaptor)))
                            } else {
                                let current =
                                    pass.into_any().downcast::<OpToOpPassAdaptor>().unwrap();
                                Ok((passes, Some(current)))
                            }
                        }
                        None => {
                            let current = pass.into_any().downcast::<OpToOpPassAdaptor>().unwrap();
                            Ok((passes, Some(current)))
                        }
                    },
                    // This pass isn't an adaptor, but if we have one, we need to finalize it
                    None => {
                        match prev {
                            Some(mut prev_adaptor) => {
                                finalize_adaptor(&mut prev_adaptor)?;
                                passes.push(prev_adaptor as Box<dyn OperationPass>);
                                passes.push(pass);
                            }
                            None => {
                                passes.push(pass);
                            }
                        }
                        Ok((passes, None))
                    }
                }
            },
        )?;

        if let Some(prev_adaptor) = prev_adaptor {
            self.passes.push(prev_adaptor);
        }

        // If this is a op-agnostic pass manager, there is nothing left to do.
        match self.name.as_ref() {
            None => Ok(()),
            // Otherwise, verify that all of the passes are valid for the current operation anchor.
            Some(name) => {
                for pass in self.passes.iter() {
                    if !pass.can_schedule_on(name) {
                        return Err(self
                            .context
                            .diagnostics()
                            .diagnostic(Severity::Error)
                            .with_message(format!(
                                "unable to schedule pass '{}' on pass manager intended for \
                                 '{name}'",
                                pass.name()
                            ))
                            .into_report());
                    }
                }

                Ok(())
            }
        }
    }

    pub fn name(&self) -> Option<&OperationName> {
        self.name.as_ref()
    }

    pub fn set_nesting(&mut self, nesting: Nesting) {
        self.nesting = nesting;
    }

    pub fn nesting(&self) -> Nesting {
        self.nesting
    }

    /// Indicate if this pass manager can be scheduled on the given operation
    pub fn can_schedule_on(&self, name: &OperationName) -> bool {
        // If this pass manager is op-specific, we simply check if the provided operation name
        // is the same as this one.
        if let Some(op_name) = self.name() {
            return op_name == name;
        }

        // Otherwise, this is an op-agnostic pass manager. Check that the operation can be
        // scheduled on all passes within the manager.
        if !name.implements::<dyn IsolatedFromAbove>() {
            return false;
        }
        self.passes.iter().all(|pass| pass.can_schedule_on(name))
    }

    fn initialize(&mut self) -> Result<(), Report> {
        for pass in self.passes.iter_mut() {
            // If this pass isn't an adaptor, directly initialize it
            if let Some(adaptor) = pass.as_any_mut().downcast_mut::<OpToOpPassAdaptor>() {
                for pm in adaptor.pass_managers_mut() {
                    pm.initialize()?;
                }
            } else {
                pass.initialize(self.context.clone())?;
            }
        }

        Ok(())
    }

    #[allow(unused)]
    fn merge_into(&mut self, rhs: &mut Self) {
        assert_eq!(self.name, rhs.name, "merging unrelated pass managers");
        for pass in self.passes.drain(..) {
            rhs.passes.push(pass);
        }
    }

    pub fn nest_pass_manager(&mut self, nested: Self) -> NestedOpPassManager<'_> {
        let adaptor = Box::new(OpToOpPassAdaptor::new(nested));
        NestedOpPassManager {
            parent: self,
            nested: Some(adaptor),
        }
    }

    /// Prints out the passes of the pass manager as the textual representation of pipelines.
    pub fn print_as_textual_pipeline(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        if let Some(anchor) = self.name() {
            write!(f, "{anchor}(")?;
        } else {
            f.write_str("any(")?;
        }
        for (i, pass) in self.passes().iter().enumerate() {
            if i > 0 {
                f.write_str(",")?;
            }
            pass.print_as_textual_pipeline(f)?;
        }
        f.write_str(")")
    }

    pub fn print_statistics(
        &self,
        out: &mut dyn core::fmt::Write,
        display_mode: PassDisplayMode,
    ) -> core::fmt::Result {
        const PASS_STATS_DESCRIPTION: &str = "... Pass statistics report ...";

        // Print the stats header.
        writeln!(out, "=={:-<73}==", "")?;
        // Figure out how many spaces for the description name.
        let padding = 80usize.saturating_sub(PASS_STATS_DESCRIPTION.len());
        writeln!(out, "{PASS_STATS_DESCRIPTION: <padding$}")?;
        writeln!(out, "=={:-<73}==", "")?;

        // Defer to a specialized printer for each display mode.
        match display_mode {
            PassDisplayMode::List => self.print_statistics_as_list(out),
            PassDisplayMode::Pipeline => self.print_statistics_as_pipeline(out),
        }
    }

    fn add_stats(
        pass: &dyn OperationPass,
        merged_stats: &mut BTreeMap<&str, SmallVec<[Box<dyn Statistic>; 4]>>,
    ) {
        use alloc::collections::btree_map::Entry;

        if let Some(adaptor) = pass.as_any().downcast_ref::<OpToOpPassAdaptor>() {
            // Recursively add each of the children.
            for pass_manager in adaptor.pass_managers() {
                for pass in pass_manager.passes() {
                    Self::add_stats(&**pass, merged_stats);
                }
            }
        } else {
            // If this is not an adaptor, add the stats to the list if there are any.
            if !pass.has_statistics() {
                return;
            }
            let statistics = SmallVec::<[Box<dyn Statistic>; 4]>::from_iter(
                pass.statistics().iter().map(|stat| Statistic::clone(&**stat)),
            );
            match merged_stats.entry(pass.name()) {
                Entry::Vacant(entry) => {
                    entry.insert(statistics);
                }
                Entry::Occupied(mut entry) => {
                    let prev_stats = entry.get_mut();
                    assert_eq!(prev_stats.len(), statistics.len());
                    for (index, mut stat) in statistics.into_iter().enumerate() {
                        let _ = prev_stats[index].try_merge(&mut stat);
                    }
                }
            }
        }
    }

    /// Print the statistics results in a list form, where each pass is sorted by name.
    fn print_statistics_as_list(&self, out: &mut dyn core::fmt::Write) -> core::fmt::Result {
        let mut merged_stats = BTreeMap::<&str, SmallVec<[Box<dyn Statistic>; 4]>>::default();
        for pass in self.passes.iter() {
            Self::add_stats(&**pass, &mut merged_stats);
        }

        // Print the timing information sequentially.
        for (pass, stats) in merged_stats.iter() {
            self.print_pass_entry(out, 2, pass, stats)?;
        }

        Ok(())
    }

    fn print_statistics_as_pipeline(&self, _out: &mut dyn core::fmt::Write) -> core::fmt::Result {
        todo!()
    }

    fn print_pass_entry(
        &self,
        out: &mut dyn core::fmt::Write,
        indent: usize,
        pass: &str,
        stats: &[Box<dyn Statistic>],
    ) -> core::fmt::Result {
        use core::fmt::Write;

        writeln!(out, "{pass: <indent$}")?;
        if stats.is_empty() {
            return Ok(());
        }

        // Collect the largest name and value length from each of the statistics.

        struct Rendered<'a> {
            name: &'a str,
            description: &'a str,
            value: compact_str::CompactString,
        }

        let mut largest_name = 0usize;
        let mut largest_value = 0usize;
        let mut rendered_stats = SmallVec::<[Rendered; 4]>::default();
        for stat in stats {
            let mut value = compact_str::CompactString::default();
            let doc = stat.pretty_print();
            write!(&mut value, "{doc}")?;
            let name = stat.name();
            largest_name = core::cmp::max(largest_name, name.len());
            largest_value = core::cmp::max(largest_value, value.len());
            rendered_stats.push(Rendered {
                name,
                description: stat.description(),
                value,
            });
        }

        // Sort the statistics by name.
        rendered_stats.sort_by(|a, b| a.name.cmp(b.name));

        // Print statistics
        for stat in rendered_stats {
            write!(out, "{: <1$} (S) ", "", indent)?;
            write!(out, "{: <1$} ", &stat.value, largest_value)?;
            write!(out, "{: <1$}", &stat.name, largest_name)?;
            if stat.description.is_empty() {
                out.write_char('\n')?;
            } else {
                writeln!(out, " - {}", &stat.description)?;
            }
        }

        Ok(())
    }
}

pub struct NestedOpPassManager<'parent> {
    parent: &'parent mut OpPassManager,
    nested: Option<Box<OpToOpPassAdaptor>>,
}

impl core::ops::Deref for NestedOpPassManager<'_> {
    type Target = OpPassManager;

    fn deref(&self) -> &Self::Target {
        &self.nested.as_deref().unwrap().pass_managers()[0]
    }
}
impl core::ops::DerefMut for NestedOpPassManager<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.nested.as_deref_mut().unwrap().pass_managers_mut()[0]
    }
}

impl Drop for NestedOpPassManager<'_> {
    fn drop(&mut self) {
        self.parent.add_pass(self.nested.take().unwrap() as Box<dyn OperationPass>);
    }
}

pub struct OpToOpPassAdaptor {
    pms: SmallVec<[OpPassManager; 1]>,
}
impl OpToOpPassAdaptor {
    pub fn new(pm: OpPassManager) -> Self {
        Self { pms: smallvec![pm] }
    }

    pub fn name(&self) -> CompactString {
        use core::fmt::Write;

        let mut name = CompactString::default();
        let names =
            crate::formatter::DisplayValues::new(self.pms.iter().map(|pm| match pm.name() {
                None => alloc::borrow::Cow::Borrowed(OpPassManager::ANY),
                Some(name) => alloc::borrow::Cow::Owned(name.to_string()),
            }));
        write!(&mut name, "Pipeline Collection: [{names}]").unwrap();
        name
    }

    /// Try to merge the current pass adaptor into 'rhs'.
    ///
    /// This will try to append the pass managers of this adaptor into those within `rhs`, or return
    /// failure if merging isn't possible. The main situation in which merging is not possible is if
    /// one of the adaptors has an `any` pipeline that is not compatible with a pass manager in the
    /// other adaptor. For example, if this adaptor has a `hir.function` pipeline and `rhs` has an
    /// `any` pipeline that operates on a FunctionOpInterface. In this situation the pipelines have
    /// a conflict (they both want to run on the same operations), so we can't merge.
    pub fn try_merge_into(&mut self, rhs: &mut Self) -> bool {
        // Functor used to detect if the given generic pass manager will have a potential schedule
        // conflict with the given `pms`.
        let has_schedule_conflict_with = |generic_pm: &OpPassManager, pms: &[OpPassManager]| {
            pms.iter().any(|pm| {
                // If this is a non-generic pass manager, a conflict will arise if a non-generic
                // pass manager's operation name can be scheduled on the generic passmanager.
                if let Some(name) = pm.name() {
                    generic_pm.can_schedule_on(name)
                } else {
                    // Otherwise, this is a generic pass manager. We current can't determine when
                    // generic pass managers can be merged, so conservatively assume they conflict.
                    true
                }
            })
        };

        // Check that if either adaptor has a generic pass manager, that pm is compatible within any
        // non-generic pass managers.
        //
        // Check the current adaptor.
        let lhs_generic = self.pass_managers().iter().find(|pm| pm.is_op_agnostic());
        if lhs_generic.is_some_and(|pm| has_schedule_conflict_with(pm, rhs.pass_managers())) {
            return false;
        }

        // Check the rhs adaptor.
        let rhs_generic = self.pass_managers().iter().find(|pm| pm.is_op_agnostic());
        if rhs_generic.is_some_and(|pm| has_schedule_conflict_with(pm, self.pass_managers())) {
            return false;
        }

        for mut pm in self.pms.drain(..) {
            // If an existing pass manager exists, then merge the given pass manager into it.
            if let Some(existing) =
                rhs.pass_managers_mut().iter_mut().find(|rpm| pm.name() == rpm.name())
            {
                pm.merge_into(existing);
            } else {
                // Otherwise, add the given pass manager to the list.
                rhs.pms.push(pm);
            }
        }

        // After coalescing, sort the pass managers within rhs by name.
        rhs.pms.sort_by(|lhs, rhs| {
            use core::cmp::Ordering;
            // Order op-specific pass managers first and op-agnostic pass managers last.
            match (lhs.name(), rhs.name()) {
                (None, None) => Ordering::Equal,
                (None, Some(_)) => Ordering::Greater,
                (Some(_), None) => Ordering::Less,
                (Some(lhs), Some(rhs)) => lhs.cmp(rhs),
            }
        });

        true
    }

    pub fn pass_managers(&self) -> &[OpPassManager] {
        &self.pms
    }

    pub fn pass_managers_mut(&mut self) -> &mut [OpPassManager] {
        &mut self.pms
    }

    /// Run the given operation and analysis manager on a provided op pass manager.
    fn run_pipeline(
        pm: &mut OpPassManager,
        op: OperationRef,
        analysis_manager: AnalysisManager,
        verify: bool,
        instrumentor: Option<Rc<PassInstrumentor>>,
        parent_info: Option<&PipelineParentInfo>,
    ) -> Result<(), Report> {
        if verify {
            // We run an initial recursive verification, since this is the first verification done
            // to the operations
            Self::verify(&op, true)?;
        }

        assert!(
            instrumentor.is_none() || parent_info.is_some(),
            "expected parent info if instrumentor is provided"
        );

        // Clear out any computed operation analyses on exit.
        //
        // These analyses won't be used anymore in this pipeline, and this helps reduce the
        // current working set of memory. If preserving these analyses becomes important in the
        // future, we can re-evaluate.
        let _clear = analysis_manager.defer_clear();

        // Run the pipeline over the provided operation.
        let mut op_name = None;
        if let Some(instrumentor) = instrumentor.as_deref() {
            op_name = pm.name().cloned();
            instrumentor.run_before_pipeline(op_name.as_ref(), parent_info.as_ref().unwrap(), op);
        }

        for pass in pm.passes_mut() {
            Self::run(&mut **pass, op, analysis_manager.clone(), verify)?;
        }

        if let Some(instrumentor) = instrumentor.as_deref() {
            instrumentor.run_after_pipeline(op_name.as_ref(), parent_info.as_ref().unwrap());
        }

        Ok(())
    }

    /// Run the given operation and analysis manager on a single pass.
    fn run(
        pass: &mut dyn OperationPass,
        op: OperationRef,
        analysis_manager: AnalysisManager,
        verify: bool,
    ) -> Result<(), Report> {
        use crate::Spanned;

        let (op_name, span, context) = {
            let op = op.borrow();
            (op.name(), op.span(), op.context_rc())
        };
        if !op_name.implements::<dyn IsolatedFromAbove>() {
            return Err(context
                .diagnostics()
                .diagnostic(Severity::Error)
                .with_message("failed to execute pass")
                .with_primary_label(
                    span,
                    "trying to schedule a pass on an operation which does not implement \
                     `IsolatedFromAbove`",
                )
                .into_report());
        }
        if !pass.can_schedule_on(&op_name) {
            return Err(context
                .diagnostics()
                .diagnostic(Severity::Error)
                .with_message("failed to execute pass")
                .with_primary_label(span, "trying to schedule a pass on an unsupported operation")
                .into_report());
        }

        // Initialize the pass state with a callback for the pass to dynamically execute a pipeline
        // on the currently visited operation.
        let pi = analysis_manager.pass_instrumentor();
        let parent_info = PipelineParentInfo {
            pass: Some(pass.name().to_compact_string()),
        };
        let callback_op = op;
        let callback_analysis_manager = analysis_manager.clone();
        let pipeline_callback: Box<super::pass::DynamicPipelineExecutor> = Box::new(
            move |pipeline: &mut OpPassManager, root: OperationRef| -> Result<(), Report> {
                let pi = callback_analysis_manager.pass_instrumentor();
                let op = callback_op.borrow();
                let context = op.context_rc();
                let root_op = root.borrow();
                if !root_op.is_ancestor_of(&op) {
                    return Err(context
                        .diagnostics()
                        .diagnostic(Severity::Error)
                        .with_message("failed to execute pass")
                        .with_primary_label(
                            root_op.span(),
                            "trying to schedule a dynamic pass pipeline on an operation that \
                             isn't nested under the current operation the pass is processing",
                        )
                        .into_report());
                }
                assert!(pipeline.can_schedule_on(&root_op.name()));
                // Before running, finalize the passes held by the pipeline
                pipeline.finalize_pass_list()?;

                // Initialize the user-provided pipeline and execute the pipeline
                pipeline.initialize()?;

                let nested_am = if root == callback_op {
                    callback_analysis_manager.clone()
                } else {
                    callback_analysis_manager.nest(root)
                };
                Self::run_pipeline(pipeline, root, nested_am, verify, pi, Some(&parent_info))
            },
        );

        let mut execution_state = PassExecutionState::new(
            op,
            context.clone(),
            analysis_manager.clone(),
            Some(pipeline_callback),
        );

        // Instrument before the pass has run
        if let Some(instrumentor) = pi.as_deref() {
            instrumentor.run_before_pass(pass, &op);
        }

        let mut result =
            if let Some(adaptor) = pass.as_any_mut().downcast_mut::<OpToOpPassAdaptor>() {
                adaptor.run_on_operation(op, &mut execution_state, verify)
            } else {
                pass.run_on_operation(op, &mut execution_state)
            };

        // Invalidate any non-preserved analyses
        analysis_manager.invalidate(execution_state.preserved_analyses_mut());

        // When `verify == true`, we run the verifier (unless the pass failed)
        if result.is_ok() && verify {
            // If the pass is an adaptor pass, we don't run the verifier recursively because the
            // nested operations should have already been verified after nested passes had run
            let run_verifier_recursively = !pass.as_any().is::<OpToOpPassAdaptor>();

            // Reduce compile time by avoiding running the verifier if the pass didn't change the
            // IR since the last time the verifier was run:
            //
            // * If the pass said that it preserved all analyses then it can't have permuted the IR
            let run_verifier_now = !execution_state.preserved_analyses().is_all();

            if run_verifier_now
                && let Err(verification_result) = Self::verify(&op, run_verifier_recursively)
            {
                result = result.map_err(|_| verification_result);
            }
        }

        if let Some(instrumentor) = pi.as_deref() {
            if result.is_err() {
                instrumentor.run_after_pass_failed(pass, &op);
            } else {
                instrumentor.run_after_pass(pass, &op, &execution_state);
            }
        }

        // Return the pass result
        result.map(|_| ())
    }

    fn verify(op: &OperationRef, verify_recursively: bool) -> Result<(), Report> {
        let op = op.borrow();
        if verify_recursively {
            op.recursively_verify()
        } else {
            op.verify()
        }
    }

    fn run_on_operation(
        &mut self,
        op: OperationRef,
        state: &mut PassExecutionState,
        verify: bool,
    ) -> Result<(), Report> {
        let analysis_manager = state.analysis_manager();
        let instrumentor = analysis_manager.pass_instrumentor();
        let parent_info = PipelineParentInfo {
            pass: Some(self.name()),
        };

        // Collection region refs so we aren't holding borrows during pass execution
        let mut next_region = op.borrow().regions().back().as_pointer();
        while let Some(region) = next_region.take() {
            next_region = region.next();
            let mut next_block = region.borrow().body().front().as_pointer();
            while let Some(block) = next_block.take() {
                next_block = block.next();
                let mut next_op = block.borrow().front();
                while let Some(op) = next_op.take() {
                    next_op = op.next();
                    let op_name = op.borrow().name();
                    if let Some(manager) =
                        self.pms.iter_mut().find(|pm| pm.can_schedule_on(&op_name))
                    {
                        let am = analysis_manager.nest(op);
                        Self::run_pipeline(
                            manager,
                            op,
                            am,
                            verify,
                            instrumentor.clone(),
                            Some(&parent_info),
                        )?;
                    }
                }
            }
        }
        state.set_post_pass_status(PostPassStatus::Unchanged);
        Ok(())
    }
}

impl Pass for OpToOpPassAdaptor {
    type Target = Operation;

    fn name(&self) -> &'static str {
        crate::interner::Symbol::intern(self.name()).as_str()
    }

    #[inline(always)]
    fn target_name(&self, _context: &Context) -> Option<OperationName> {
        None
    }

    fn print_as_textual_pipeline(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        let pms = self.pass_managers();
        for (i, pm) in pms.iter().enumerate() {
            if i > 0 {
                f.write_str(",")?;
            }
            pm.print_as_textual_pipeline(f)?;
        }

        Ok(())
    }

    #[inline(always)]
    fn can_schedule_on(&self, _name: &OperationName) -> bool {
        true
    }

    fn run_on_operation(
        &mut self,
        _op: EntityMut<'_, Operation>,
        _state: &mut PassExecutionState,
    ) -> Result<(), Report> {
        unreachable!("unexpected call to `Pass::run_on_operation` for OpToOpPassAdaptor")
    }
}
