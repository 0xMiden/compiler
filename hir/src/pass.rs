mod analysis;
mod instrumentation;
mod manager;
#[allow(clippy::module_inception)]
mod pass;
pub mod registry;
mod specialization;
pub mod statistics;

use alloc::{borrow::Cow, string::String};

pub use self::{
    analysis::{Analysis, AnalysisManager, OperationAnalysis, PreservedAnalyses},
    instrumentation::{PassInstrumentation, PassInstrumentor, PipelineParentInfo},
    manager::{IRPrintingConfig, Nesting, OpPassManager, PassDisplayMode, PassManager},
    pass::{OperationPass, Pass, PassExecutionState, PostPassStatus},
    registry::{PassInfo, PassPipelineInfo},
    specialization::PassTarget,
    statistics::{PassStatistic, Statistic, StatisticValue},
};
use crate::{EntityRef, Operation, OperationName, OperationRef, SmallVec, TraceTarget};

/// Handles IR printing, based on the [`IRPrintingConfig`] passed in
/// [Print::new]. Currently, this struct is managed by the [`PassManager`]'s [`PassInstrumentor`],
/// which calls the Print struct via its [`PassInstrumentation`] trait implementation.
///
/// The configuration passed by [`IRPrintingConfig`] controls *when* the IR gets displayed, rather
/// than *how*. The display format itself depends on the `Display` implementation done by each
/// [`Operation`].
///
/// [`Print::selected_passes`] controls which passes are selected to be printable. This means that
/// those selected passes will run all the configured filters; which will determine whether
/// the pass displays the IR or not. The available options are [`SelectedPasses::All`] to enable all
/// the passes and [`SelectedPasses::Just`] to enable a select set of passes.
///
/// The filters that run on the selected passes are:
/// - [`Print::only_when_modified`] will only print the IR if said pass modified the IR.
///
/// - [`Print::op_filter`] will only display a specific subset of operations.
#[derive(Default)]
pub struct Print {
    selected_passes: Option<SelectedPasses>,
    filters: SmallVec<[OpFilter; 1]>,
    only_when_modified: bool,
}

/// Which passes are enabled for IR printing.
#[derive(Debug)]
enum SelectedPasses {
    /// Enable all passes for IR Printing.
    All,
    /// Just select a subset of passes for IR printing.
    Just(SmallVec<[String; 1]>),
}

#[derive(Default, Debug, Clone)]
pub enum OpFilter {
    /// Print all operations
    #[default]
    All,
    /// Print any `Symbol` operation, optionally filtering by symbols whose name contains a given
    /// string.
    ///
    /// See [`Print::with_symbol_filter`] for more details.
    Symbol(Option<Cow<'static, str>>),
    /// Print only operations of the given type
    ///
    /// NOTE: Currently marked as `dead_code` since it is not configured via the CLI.
    ///
    /// See [`Print::with_symbol_filter`] for more details.
    Type {
        dialect: crate::interner::Symbol,
        op: crate::interner::Symbol,
    },
}

impl From<midenc_session::IrFilter> for OpFilter {
    fn from(value: midenc_session::IrFilter) -> Self {
        use midenc_session::IrFilter;

        match value {
            IrFilter::Any => OpFilter::All,
            IrFilter::Symbol(pattern) => OpFilter::Symbol(pattern),
            IrFilter::Op { dialect, op } => OpFilter::Type { dialect, op },
        }
    }
}

impl Print {
    pub fn new(config: &IRPrintingConfig) -> Option<Self> {
        if config.print_ir_after_all
            || !config.print_ir_after_pass.is_empty()
            || config.print_ir_after_modified
        {
            Some(Self::default().with_pass_filter(config).with_symbol_filter(config))
        } else {
            None
        }
    }

    pub fn with_type_filter<T: crate::OpRegistration>(mut self) -> Self {
        let dialect = <T as crate::OpRegistration>::dialect_name();
        let op = <T as crate::OpRegistration>::name();
        self.filters.push(OpFilter::Type { dialect, op });
        self
    }

    /// Configure which operations are printed. This is set via the different variants present in
    /// [`OpFilter`].
    fn with_symbol_filter(mut self, config: &IRPrintingConfig) -> Self {
        self.filters.extend(config.print_ir_filters.iter().cloned());
        self
    }

    fn with_pass_filter(mut self, config: &IRPrintingConfig) -> Self {
        let is_ir_filter_set = if config.print_ir_after_all {
            self.selected_passes = Some(SelectedPasses::All);
            true
        } else if !config.print_ir_after_pass.is_empty() {
            self.selected_passes = Some(SelectedPasses::Just(config.print_ir_after_pass.clone()));
            true
        } else {
            false
        };

        if config.print_ir_after_modified {
            self.only_when_modified = true;
            // NOTE: If the user specified the "print after modification" flag, but didn't specify
            // any IR pass filter flag; then we assume that the desired behavior is to set the "all
            // pass" filter.
            if !is_ir_filter_set {
                self.selected_passes = Some(SelectedPasses::All);
            }
        };

        self
    }

    pub fn print_ir(&self, op: EntityRef<'_, Operation>, topic: &'static str, phase: &str) {
        let target = TraceTarget::category("pass").with_topic(topic);
        // Determine if any filter applies to `op`, and print accordingly
        if self.filters.is_empty() {
            let name = op.name();
            if let Some(sym) = op.as_symbol() {
                log::trace!(target: &target, symbol = sym.name().as_str(), dialect = name.dialect().as_str(), op = name.name().as_str(); "{phase}: {op}");
            } else {
                log::trace!(target: &target, dialect = name.dialect().as_str(), op = name.name().as_str(); "{phase}: {op}");
            }
            return;
        }

        for filter in self.filters.iter() {
            match filter {
                OpFilter::All => {
                    let name = op.name();
                    if let Some(sym) = op.as_symbol() {
                        log::trace!(target: &target, symbol = sym.name().as_str(), dialect = name.dialect().as_str(), op = name.name().as_str(); "{phase}: {op}");
                    } else {
                        log::trace!(target: &target, dialect = name.dialect().as_str(), op = name.name().as_str(); "{phase}: {op}");
                    }
                    break;
                }
                OpFilter::Type {
                    dialect,
                    op: op_name,
                } => {
                    let name = op.name();
                    if name.dialect() == *dialect && name.name() == *op_name {
                        log::trace!(target: &target, dialect = dialect.as_str(), op = op_name.as_str(); "{phase}: {op}");
                        break;
                    }
                }
                OpFilter::Symbol(None) => {
                    if let Some(sym) = op.as_symbol() {
                        let name = op.name();
                        log::trace!(target: &target, symbol = sym.name().as_str(), dialect = name.dialect().as_str(), op = name.name().as_str(); "{phase}: {}", sym.as_symbol_operation());
                        break;
                    }
                }
                OpFilter::Symbol(Some(filter)) => {
                    if let Some(sym) =
                        op.as_symbol().filter(|sym| sym.name().as_str().contains(filter.as_ref()))
                    {
                        let name = op.name();
                        log::trace!(target: &target, symbol = sym.name().as_str(), dialect = name.dialect().as_str(), op = name.name().as_str(); "{phase}: {}", sym.as_symbol_operation());
                        break;
                    }
                }
            }
        }
    }

    fn pass_filter(&self, pass: &dyn OperationPass) -> bool {
        match &self.selected_passes {
            Some(SelectedPasses::All) => true,
            Some(SelectedPasses::Just(passes)) => passes.iter().any(|p| pass.name() == *p),
            None => false,
        }
    }

    fn should_print(&self, pass: &dyn OperationPass, ir_changed: &PostPassStatus) -> bool {
        let pass_filter = self.pass_filter(pass);

        // Always print, unless "only_when_modified" has been set and there have not been changes.
        let modification_filter =
            !matches!(ir_changed, PostPassStatus::Unchanged if self.only_when_modified);

        pass_filter && modification_filter
    }
}

impl PassInstrumentation for Print {
    fn run_before_pipeline(
        &mut self,
        _name: Option<&OperationName>,
        _parent_info: &PipelineParentInfo,
        op: OperationRef,
    ) {
        if !self.only_when_modified {
            return;
        }

        let op = op.borrow();
        self.print_ir(op, "pipeline", "before");
    }

    fn run_before_pass(&mut self, pass: &dyn OperationPass, op: &OperationRef) {
        if self.only_when_modified {
            return;
        }
        if self.pass_filter(pass) {
            let op = op.borrow();
            self.print_ir(op, pass.name(), "before");
        }
    }

    fn run_after_pass(
        &mut self,
        pass: &dyn OperationPass,
        op: &OperationRef,
        post_execution_state: &PassExecutionState,
    ) {
        let changed = post_execution_state.post_pass_status();

        if self.should_print(pass, changed) {
            let op = op.borrow();
            self.print_ir(op, pass.name(), "after");
        }
    }
}
