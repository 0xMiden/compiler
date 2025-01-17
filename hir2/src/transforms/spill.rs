use crate::{
    dataflow::analyses::SpillAnalysis,
    dialects::builtin::{Function, FunctionRef},
    pass::{Pass, PassExecutionState},
    EntityMut, Report,
};

pub struct InsertSpills;

impl Pass for InsertSpills {
    type Target = Function;

    fn name(&self) -> &'static str {
        "insert-spills"
    }

    fn argument(&self) -> &'static str {
        "insert-spills"
    }

    fn can_schedule_on(&self, _name: &crate::OperationName) -> bool {
        true
    }

    fn run_on_operation(
        &mut self,
        op: EntityMut<'_, Self::Target>,
        state: &mut PassExecutionState,
    ) -> Result<(), Report> {
        // Temporary workaround to allow us to run analysis without aliasing
        //
        // TODO: Not sure how to address this in general. We re-borrow things in various utilities
        // and analyses pretty regularly, so any mutable borrow makes the risk of triggering an
        // aliasing assertion too likely
        let mut func_ref = unsafe { FunctionRef::from_raw(&*op) };
        drop(op);

        let spills = state.analysis_manager().get_analysis_for::<SpillAnalysis, Function>()?;

        // Place spills and reloads, rewrite IR to ensure live ranges we aimed to split are actually
        // split.
        let mut op = func_ref.borrow_mut();
        self.rewrite(&mut op, &spills)
    }
}

impl InsertSpills {
    fn rewrite(
        &mut self,
        _function: &mut Function,
        spill_analysis: &SpillAnalysis,
    ) -> Result<(), Report> {
        assert!(!spill_analysis.has_spills(), "temporarily not supporting spills");

        Ok(())
    }
}
