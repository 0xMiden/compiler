mod analysis;
mod instrumentation;
mod manager;
#[allow(clippy::module_inception)]
mod pass;
pub mod registry;
mod specialization;
pub mod statistics;

use self::pass::PassExecutionState;
pub use self::{
    analysis::{Analysis, AnalysisManager, OperationAnalysis, PreservedAnalyses},
    instrumentation::{PassInstrumentation, PassInstrumentor, PipelineParentInfo},
    manager::{Nesting, OpPassManager, PassDisplayMode, PassManager},
    pass::{OperationPass, Pass},
    registry::{PassInfo, PassPipelineInfo},
    specialization::PassTarget,
    statistics::{PassStatistic, Statistic, StatisticValue},
};
