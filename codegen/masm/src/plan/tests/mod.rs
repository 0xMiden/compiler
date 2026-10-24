mod abi;
mod effects;
mod layouts;
pub(crate) mod lowering_units;
pub(crate) mod pipeline;

use alloc::{collections::BTreeMap, vec::Vec};

use super::{actions::*, *};

impl SelectedLowering {
    fn fixture(kind: LoweringKind, source: SourceStepId) -> Self {
        Self {
            address_results: Vec::new(),
            id: SelectedStepId(source.0),
            kind,
            source,
            semantic_inputs: Vec::new(),
            stack_inputs: Vec::new(),
            stored_inputs: Vec::new(),
            outputs: Vec::new(),
            effects: LoweringEffects::default(),
            scratch: Vec::new(),
            scratch_effects: Vec::new(),
        }
    }
}
impl ProcedurePlan {
    fn fixture(actions: Vec<PlanAction>) -> Self {
        Self {
            debug_locations: Default::default(),
            decisions: Default::default(),
            selected: crate::verify::tests::empty_program(),
            contracts: BTreeMap::new(),
            segment_actions: BTreeMap::from([(SegmentId(0), actions)]),
            edge_actions: BTreeMap::new(),
            spill_slots: Vec::new(),
            lowering_bindings: BTreeMap::new(),
        }
    }
}
