mod analysis;
mod instrumentation;
mod manager;
#[allow(clippy::module_inception)]
mod pass;
pub mod registry;
mod specialization;
pub mod statistics;

pub use self::{
    analysis::{Analysis, AnalysisManager, OperationAnalysis, PreservedAnalyses},
    instrumentation::{PassInstrumentation, PassInstrumentor, PipelineParentInfo},
    manager::{Nesting, OpPassManager, PassDisplayMode, PassManager},
    pass::{OperationPass, Pass, PassExecutionState},
    registry::{PassInfo, PassPipelineInfo},
    specialization::PassTarget,
    statistics::{PassStatistic, Statistic, StatisticValue},
};

/// A `Pass` which prints IR it is run on, based on provided configuration.
#[derive(Default)]
pub struct Print {
    filter: OpFilter,
    target: Option<compact_str::CompactString>,
}

#[derive(Default, Clone, Copy)]
pub enum OpFilter {
    /// Print all operations
    #[default]
    All,
    /// Print any `Symbol` operation, optionally filtering by symbols whose name contains a given
    /// string.
    Symbol(Option<&'static str>),
    /// Print only operations of the given type
    Type {
        dialect: crate::interner::Symbol,
        op: crate::interner::Symbol,
    },
}

impl Print {
    /// Create a printer that prints any operation
    pub fn any() -> Self {
        Self {
            filter: OpFilter::All,
            target: None,
        }
    }

    /// Create a printer that only prints operations of type `T`
    pub fn only<T: crate::OpRegistration>() -> Self {
        let dialect = <T as crate::OpRegistration>::dialect_name();
        let op = <T as crate::OpRegistration>::name();
        Self {
            filter: OpFilter::Type { dialect, op },
            target: None,
        }
    }

    /// Create a printer that only prints `Symbol` operations containing `name`
    pub fn symbol_matching(name: &'static str) -> Self {
        Self {
            filter: OpFilter::Symbol(Some(name)),
            target: None,
        }
    }

    /// Specify the `log` target to write the IR output to.
    ///
    /// By default, the target is `printer`, unless the op is a `Symbol`, in which case it is the
    /// `Symbol` name.
    pub fn with_target(mut self, target: impl AsRef<str>) -> Self {
        let target = compact_str::CompactString::new(target.as_ref());
        self.target = Some(target);
        self
    }
}

impl Pass for Print {
    type Target = crate::Operation;

    fn name(&self) -> &'static str {
        "print"
    }

    fn can_schedule_on(&self, _name: &crate::OperationName) -> bool {
        true
    }

    fn run_on_operation(
        &mut self,
        op: crate::EntityMut<'_, Self::Target>,
        _state: &mut PassExecutionState,
    ) -> Result<(), crate::Report> {
        let op = op.into_entity_ref();
        match self.filter {
            OpFilter::All => {
                let target = self.target.as_deref().unwrap_or("printer");
                log::trace!(target: target, "{op}");
            }
            OpFilter::Type {
                dialect,
                op: op_name,
            } => {
                let name = op.name();
                if name.dialect() == dialect && name.name() == op_name {
                    let target = self.target.as_deref().unwrap_or("printer");
                    log::trace!(target: target, "{op}");
                }
            }
            OpFilter::Symbol(None) => {
                if let Some(sym) = op.as_symbol() {
                    let name = sym.name().as_str();
                    let target = self.target.as_deref().unwrap_or(name);
                    log::trace!(target: target, "{}", sym.as_symbol_operation());
                }
            }
            OpFilter::Symbol(Some(filter)) => {
                if let Some(sym) = op.as_symbol().filter(|sym| sym.name().as_str().contains(filter))
                {
                    let target = self.target.as_deref().unwrap_or(filter);
                    log::trace!(target: target, "{}", sym.as_symbol_operation());
                }
            }
        }
        Ok(())
    }
}
