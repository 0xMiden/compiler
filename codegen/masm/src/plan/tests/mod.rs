mod abi;
mod effects;
mod layouts;

use alloc::{collections::BTreeMap, vec, vec::Vec};

use super::{actions::*, *};

impl SelectedLowering {
    fn fixture(kind: LoweringKind, source: SourceStepId) -> Self {
        Self {
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
            selected: SelectedProgram {
                origin: ProcedureOrigin::Generated {
                    definition: GeneratedDefinitionId(0),
                    parameters: Vec::new(),
                },
                values: BTreeMap::new(),
                segments: vec![SelectedSegment {
                    id: SegmentId(0),
                    parameters: Vec::new(),
                    units: Vec::new(),
                }],
                edges: Vec::new(),
                structure: ControlTree::Segment(SegmentId(0)),
                abi: ProcedureAbi::new(CallingConvention::SameContext, 0, 0).unwrap(),
                source_map: BTreeMap::new(),
            },
            contracts: BTreeMap::new(),
            segment_actions: BTreeMap::from([(SegmentId(0), actions)]),
            edge_actions: BTreeMap::new(),
            spill_slots: Vec::new(),
            lowering_bindings: BTreeMap::new(),
        }
    }
}
