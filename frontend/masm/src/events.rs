use miden_assembly_syntax::ast::SystemEventNode;
use midenc_hir::Felt;

pub(crate) fn system_event_read_count(event: &SystemEventNode) -> usize {
    use SystemEventNode::*;

    match event {
        PushMapVal | PushMapValCount | PushMapValN0 | PushMapValN4 | PushMapValN8 | HasMapKey => 4,
        // The `DeferredEvaluate*` events read the digest word of a registered deferred node.
        DeferredEvaluate | DeferredEvaluateTag | DeferredEvaluatePayload => 4,
        PushMtNode | InsertMem => 6,
        // `[TAG, ptr, n_chunks]`: the payload itself is read from memory, not the stack.
        DeferredRegisterData => 6,
        InsertHdword => 8,
        InsertHdwordWithDomain => 9,
        // `[PAYLOAD_LO, PAYLOAD_HI, TAG]`, arranged as one Poseidon2 sponge input.
        DeferredRegister => 12,
        InsertHperm => 12,
        InsertHqword => 16,
    }
}

pub(crate) fn system_event_id(event: &SystemEventNode) -> Felt {
    let event: miden_core::events::SystemEvent = event.into();
    event.event_id().as_felt()
}
