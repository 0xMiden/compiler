use smallvec::SmallVec;

use crate::{
    adt::{SmallDenseMap, SmallSet},
    dataflow::analyses::{scheduling::*, LivenessAnalysis},
    dialects::builtin::Function,
    dominance::DominanceInfo,
    effects::*,
    pass::{Pass, PassExecutionState},
    traits::Terminator,
    Block, BlockRef, EntityMut, Op, Operation, ProgramPoint, Report, ValueRef,
};

pub struct InstructionScheduler;

impl Pass for InstructionScheduler {
    type Target = Function;

    fn name(&self) -> &'static str {
        "scheduler"
    }

    fn argument(&self) -> &'static str {
        "scheduler"
    }

    fn can_schedule_on(&self, _name: &crate::OperationName) -> bool {
        true
    }

    fn run_on_operation(
        &mut self,
        op: EntityMut<'_, Self::Target>,
        state: &mut PassExecutionState,
    ) -> Result<(), Report> {
        log::debug!(target: "scheduler", "optimizing instruction schedule for {}", op.as_operation());

        // First, push down all constants to their uses by materializing copies of those operations
        //
        // We do this because it avoids maintaining huge live ranges for values that are always
        // cheaper to emit just-in-time when generating code for the Miden VM. This improves the
        // quality

        let mut operation = op.as_operation_ref();
        let entry_block = op.entry_block();
        drop(op);

        let dominfo = state.analysis_manager().get_analysis::<DominanceInfo>()?;
        let liveness = state.analysis_manager().get_analysis_for::<LivenessAnalysis, Function>()?;

        let depgraph = build_dependency_graph(entry_block, &operation.borrow(), &liveness);
        dbg!(&depgraph);
        let treegraph = OrderedTreeGraph::new(&depgraph)
            .expect("unable to topologically sort treegraph for block");
        dbg!(&treegraph);
        //let mut blockq = SmallVec::<[BlockRef; 8]>::from_slice(self.domtree.cfg_postorder());
        //block_scheduler.schedule(schedule);

        self.rewrite(&mut operation.borrow_mut(), &dominfo, &liveness)
    }
}

impl InstructionScheduler {
    fn rewrite(
        &mut self,
        op: &mut Operation,
        dominfo: &DominanceInfo,
        liveness: &LivenessAnalysis,
    ) -> Result<(), Report> {
        todo!()
    }
}

fn build_dependency_graph(
    block_id: BlockRef,
    op: &Operation,
    liveness: &LivenessAnalysis,
) -> DependencyGraph {
    let mut graph = DependencyGraph::default();

    // This set represents the values which are guaranteed to be materialized for an instruction
    let mut materialized_args = SmallSet::<ValueRef, 4>::default();
    // This map represents values used as block arguments, and the successors which use them
    let mut block_arg_uses = SmallDenseMap::<ValueRef, SmallSet<BlockRef, 2>>::default();

    // For each instruction, record it and it's arguments/results in the graph
    let block = block_id.borrow();
    for inst in block.body().iter() {
        materialized_args.clear();
        block_arg_uses.clear();

        let inst_index = inst.get_or_compute_order();
        let inst_id = inst.as_operation_ref();
        let node_id = graph.add_node(Node::Inst {
            op: inst_id,
            pos: inst_index as u16,
        });

        let pp = ProgramPoint::before(&*inst);
        for arg in inst.operands().group(0).iter().copied() {
            let value = arg.borrow().as_value_ref();
            materialized_args.insert(value);
            let arg_node = ArgumentNode::Direct(arg);
            graph.add_data_dependency(node_id, arg_node, value, pp);
        }

        // Ensure all result nodes are added to the graph, otherwise unused results will not be
        // present in the graph which will cause problems when we check for those results later
        for (result_idx, result) in inst.results().iter().copied().enumerate() {
            let result_node = Node::Result {
                value: result,
                index: result_idx as u8,
            };
            let result_node_id = graph.add_node(result_node);
            graph.add_dependency(result_node_id, node_id);
        }

        match inst.num_successors() {
            0 => {}
            1 => {
                // Add edges representing these data dependencies in later blocks
                for arg in inst.successor(0).arguments.into_iter().copied() {
                    let value = arg.borrow().as_value_ref();
                    let arg_node = ArgumentNode::Indirect(arg);
                    graph.add_data_dependency(node_id, arg_node, value, pp);
                }
            }
            _ => {
                // Preprocess the arguments which are used so we can determine materialization
                // requirements
                for succ in inst.successor_iter() {
                    for arg in succ.arguments.iter() {
                        let arg = arg.borrow().as_value_ref();
                        block_arg_uses
                            .entry(arg)
                            .or_insert_with(Default::default)
                            .insert(succ.successor());
                    }
                }

                // For each successor, check if we should implicitly require an argument along that
                // edge due to liveness analysis indicating that it is used
                // somewhere downstream. We only consider block arguments passed to
                // at least one other successor, and which are not already explicitly
                // provided to this successor.
                let materialization_threshold = inst.num_successors();
                // Finally, add edges to the dependency graph representing the nature of each
                // argument
                for succ in inst.successor_iter() {
                    for arg in succ.arguments.iter().copied() {
                        let value = arg.borrow().as_value_ref();
                        let is_conditionally_materialized =
                            block_arg_uses[&value].len() < materialization_threshold;
                        let must_materialize =
                            materialized_args.contains(&value) || !is_conditionally_materialized;
                        let arg_node = if must_materialize {
                            ArgumentNode::Indirect(arg)
                        } else {
                            ArgumentNode::Conditional(arg)
                        };
                        graph.add_data_dependency(node_id, arg_node, value, pp);
                    }
                }
            }
        }
    }

    // HACK: If there are any instruction nodes with no predecessors, with the exception of the
    // block terminator, then we must add a control dependency to the graph to reflect the fact
    // that the instruction must have been placed in this block intentionally. However, we are
    // free to schedule the instruction as we see fit to avoid de-optimizing the normal
    // instruction schedule unintentionally.
    //
    // We also avoid adding control dependencies for instructions without side effects that are not
    // live beyond the current block, as those are dead code and should be eliminated by DCE anyway
    //
    // The actual scheduling decision for the instruction is deferred to `analyze_inst`, where we
    // treat the instruction similarly to argument materialization, and either make it a
    // pre-requisite of the instruction or execute it in the post-execution phase depending on
    // the terminator type
    assign_effect_dependencies(&mut graph, &block, liveness);

    // Eliminate dead code as indicated by the state of the dependency graph
    //dce(&mut graph, block_id, function, liveness);

    graph
}

/// This function performs two primary tasks:
///
/// 1. Ensure that there are edges in the graph between instructions that must not be reordered
///    past each other during scheduling due to effects on a shared resource or global resources.
///    An obvious example is loads and stores: you might have two independent expression trees that
///    read and write to the same memory location - but if the order in which the corresponding
///    loads and stores are scheduled changes, it can change the behavior of the program. Other
///    examples include function calls with side effects and inline assembly.
///
/// 2. Ensure that even if an instruction has no predecessors, it still gets scheduled if it has
///    side effects. This can happen if there are no other effectful instructions in the same block.
///    We add an edge from the block terminator to these instructions, which will guarantee that
///    they are executed before leaving the block.
///
/// This step is essential for selecting a correct instruction schedule, as effect-free instructions
/// can be freely reordered, ideal for optimization, whereas reordering instructions with effects
/// must be done very conservatively. For example, the IR allows us to represent effects against
/// a specific symbol (e.g. global variable), however there are instructions that allow one to
/// materialize a pointer to that symbol as an SSA value, and then pass that around. Thus,
/// reordering two operations around each other, one that affects the symbol and one that affects a
/// value representing a pointer to that symbol, is not safe, even though initially it may appear
/// that they affect different areas of memory.
///
/// NOTE: This function only assigns control dependencies for instructions _with_ side effects. An
/// instruction with no dependents, and no side effects, is treated as dead code, since by
/// definition its effects cannot be visible.
fn assign_effect_dependencies(
    graph: &mut DependencyGraph,
    block: &Block,
    liveness: &LivenessAnalysis,
) {
    let terminator = {
        let op = block.terminator().unwrap();
        let index = op.borrow().get_or_compute_order();
        Node::Inst {
            op,
            pos: index as u16,
        }
    };

    let mut reads: Vec<(Node, Option<ValueRef>)> = vec![];
    let mut writes: Vec<(Node, Option<ValueRef>)> = vec![];

    let mut handle_effects = |op: &Operation,
                              node: Node,
                              interface: &dyn EffectOpInterface<MemoryEffect>,
                              graph: &mut DependencyGraph| {
        if interface.has_no_effect() {
            return;
        }

        for effect in interface.effects() {
            if matches!(effect.effect(), MemoryEffect::Read | MemoryEffect::Write) {
                // Look for any stores to either the entire heap or the same value

                // If the effect applies to a specific value, look for the last store of that value
                let value = effect.value();
                if let Some(value) = value {
                    if let Some(last_store) = writes.iter().find_map(|(prev_store, written_to)| {
                        if written_to.is_none_or(|v| v == value) {
                            Some(*prev_store)
                        } else {
                            None
                        }
                    }) {
                        // Only add the dependency if there is no path from this instruction
                        // to that one
                        if !graph.is_reachable_from(node, last_store) {
                            graph.add_dependency(node, last_store);
                        }
                    }
                } else if let Some(_symbol) = effect.symbol() {
                    // If the effect applies to a specific symbol, look for the last store of that
                    // symbol
                    //
                    // TODO(pauls): Use this to narrow the scope for reads/writes of a global
                    // variable

                    // For now we just look for the last write to any value
                    if let Some((last_store, _)) = writes.last() {
                        // Only add the dependency if there is no path from this instruction
                        // to that one
                        if !graph.is_reachable_from(node, *last_store) {
                            graph.add_dependency(node, *last_store);
                        }
                    }
                } else {
                    // This is an unscoped read/write, so order with regard to any other write
                    if let Some((last_store, _)) = writes.last() {
                        // Only add the dependency if there is no path from this instruction
                        // to that one
                        if !graph.is_reachable_from(node, *last_store) {
                            graph.add_dependency(node, *last_store);
                        }
                    }
                }

                if matches!(effect.effect(), MemoryEffect::Read) {
                    reads.push((node, value));
                    continue;
                }

                // This is a write effect: have there been any loads observed?
                if let Some(value) = value {
                    if let Some(last_read) = reads.iter().find_map(|(prev_read, read_from)| {
                        if read_from.is_none_or(|v| v == value) {
                            Some(*prev_read)
                        } else {
                            None
                        }
                    }) {
                        // Only add the dependency if there is no path from this instruction
                        // to that one
                        if !graph.is_reachable_from(node, last_read) {
                            graph.add_dependency(node, last_read);
                        }
                    }
                } else if let Some(_symbol) = effect.symbol() {
                    // If the effect applies to a specific symbol, look for the last read of that
                    // symbol
                    //
                    // TODO(pauls): Use this to narrow the scope for reads/writes of a global
                    // variable

                    // For now we just look for the last read to any value
                    if let Some((last_read, _)) = reads.last() {
                        // Only add the dependency if there is no path from this instruction
                        // to that one
                        if !graph.is_reachable_from(node, *last_read) {
                            graph.add_dependency(node, *last_read);
                        }
                    }
                } else {
                    // This is an unscoped write, so order with regard to any other read
                    if let Some((last_read, _)) = reads.last() {
                        // Only add the dependency if there is no path from this instruction
                        // to that one
                        if !graph.is_reachable_from(node, *last_read) {
                            graph.add_dependency(node, *last_read);
                        }
                    }
                }

                writes.push((node, value));
            } else {
                // We treat other memory effects as global read/writes
                if let Some((last_write, _)) = writes.last() {
                    if !graph.is_reachable_from(node, *last_write) {
                        graph.add_dependency(node, *last_write);
                    }
                }
                if let Some((last_read, _)) = reads.last() {
                    if !graph.is_reachable_from(node, *last_read) {
                        graph.add_dependency(node, *last_read);
                    }
                }
                reads.push((node, None));
                writes.push((node, None));
            }
        }
    };

    for op in block.body().iter() {
        // Skip the block terminator
        if op.implements::<dyn Terminator>() {
            continue;
        }

        let inst_index = op.get_or_compute_order();
        let node = Node::Inst {
            op: op.as_operation_ref(),
            pos: inst_index as u16,
        };

        // Does this instruction have memory effects?
        //
        // If it reads memory, ensure that there is an edge in the graph from the last observed
        // store. In effect, this makes the read dependent on the most recent write, even if there
        // is no direct connection between the two instructions otherwise.
        //
        // If it writes memory, ensure that there is an edge in the graph from the last observed
        // load. In effect, this makes the write dependent on the most recent read, even if there
        // is no direct connection between the two instructions otherwise.
        //
        // If it both reads and writes, ensure there are edges to both the last load and store.
        let has_side_effects = !op.is_memory_effect_free();
        if let Some(memory_effects) = op.as_trait::<dyn EffectOpInterface<MemoryEffect>>() {
            handle_effects(&op, node, memory_effects, graph)
        }

        // At this point, we want to handle adding a control dependency from the terminator to this
        // instruction, if there are no other nodes on which to attach one, and if the instruction
        // requires one.

        // Skip instructions with transitive dependents on at least one result, or a direct
        // dependent
        let has_dependents = graph.predecessors(node).any(|pred| {
            if pred.dependent.is_result() {
                graph.num_predecessors(pred.dependent) > 0
            } else {
                true
            }
        });
        if has_dependents {
            continue;
        }

        // Instructions with no side effects require a control dependency if at least
        // one result is live after the end of the current block. We add the dependency
        // to the instruction results if present, otherwise to the instruction itself.
        let mut live_results = SmallVec::<[Node; 2]>::default();
        for pred in graph.predecessors(node) {
            match pred.dependent {
                Node::Result { value, .. } => {
                    let is_live_after =
                        liveness.is_live_at_end(value as ValueRef, block.as_block_ref());
                    if is_live_after {
                        live_results.push(pred.dependent);
                    }
                }
                _ => continue,
            }
        }

        let has_live_results = !live_results.is_empty();
        for result_node in live_results.into_iter() {
            graph.add_dependency(terminator, result_node);
        }

        // Instructions with side effects but no live results require a control dependency
        if has_side_effects && !has_live_results {
            // Only add one if there is no other transitive dependency that accomplishes
            // the same goal
            if !graph.is_reachable_from(terminator, node) {
                graph.add_dependency(terminator, node);
            }
            continue;
        }
    }
}

/*
fn dce(
    graph: &mut DependencyGraph,
    block_id: hir::Block,
    function: &hir::Function,
    liveness: &LivenessAnalysis,
) {
    // Perform dead-code elimination
    //
    // Find all instruction nodes in the graph, and if none of the instruction results
    // are used, or are live beyond it's containing block; and the instruction has no
    // side-effects, then remove all of the nodes related to that instruction, continuing
    // until there are no more nodes to process.
    let mut worklist = VecDeque::<(hir::Inst, NodeId)>::from_iter(
        function.dfg.block_insts(block_id).enumerate().map(|(i, inst)| {
            (
                inst,
                Node::Inst {
                    id: inst,
                    pos: i as u16,
                }
                .into(),
            )
        }),
    );
    let mut remove_nodes = Vec::<NodeId>::default();
    while let Some((inst, inst_node)) = worklist.pop_front() {
        // If the instruction is not dead at this point, leave it alone
        if !is_dead_instruction(inst, block_id, function, liveness, graph) {
            continue;
        }
        let inst_block = function.dfg.insts[inst].block;
        let inst_args = function.dfg.inst_args(inst);
        let branch_info = function.dfg.analyze_branch(inst);
        // Visit the immediate successors of the instruction node in the dependency graph,
        // these by construction may only be Argument or BlockArgument nodes.
        for succ in graph.successors(inst_node) {
            let dependency_node_id = succ.dependency;
            // For each argument, remove the edge from instruction to argument, and from
            // argument to the item it references. If the argument references an instruction
            // result in the same block, add that instruction back to the worklist to check
            // again in case we have made it dead
            match succ.dependency.into() {
                Node::Argument(ArgumentNode::Direct { index, .. }) => {
                    let value = inst_args[index as usize];
                    match function.dfg.value_data(value) {
                        hir::ValueData::Inst {
                            inst: value_inst, ..
                        } => {
                            let value_inst = *value_inst;
                            let value_inst_block = function.dfg.insts[value_inst].block;
                            if value_inst_block == inst_block {
                                let pos = function
                                    .dfg
                                    .block_insts(inst_block)
                                    .position(|id| id == value_inst)
                                    .unwrap();
                                // Check `value_inst` later to see if it has been made dead
                                worklist.push_back((
                                    value_inst,
                                    Node::Inst {
                                        id: value_inst,
                                        pos: pos as u16,
                                    }
                                    .into(),
                                ));
                            }
                        }
                        hir::ValueData::Param { .. } => {}
                    }
                }
                Node::Argument(
                    ArgumentNode::Indirect {
                        successor, index, ..
                    }
                    | ArgumentNode::Conditional {
                        successor, index, ..
                    },
                ) => {
                    let successor = successor as usize;
                    let index = index as usize;
                    let value = match &branch_info {
                        BranchInfo::SingleDest(succ) => {
                            assert_eq!(successor, 0);
                            succ.args[index]
                        }
                        BranchInfo::MultiDest(ref succs) => succs[successor].args[index],
                        BranchInfo::NotABranch => unreachable!(
                            "indirect/conditional arguments are only valid as successors of a \
                             branch instruction"
                        ),
                    };
                    match function.dfg.value_data(value) {
                        hir::ValueData::Inst {
                            inst: value_inst, ..
                        } => {
                            let value_inst = *value_inst;
                            let value_inst_block = function.dfg.insts[value_inst].block;
                            if value_inst_block == inst_block {
                                let pos = function
                                    .dfg
                                    .block_insts(inst_block)
                                    .position(|id| id == value_inst)
                                    .unwrap();
                                // Check `value_inst` later to see if it has been made dead
                                worklist.push_back((
                                    value_inst,
                                    Node::Inst {
                                        id: value_inst,
                                        pos: pos as u16,
                                    }
                                    .into(),
                                ));
                            }
                        }
                        hir::ValueData::Param { .. } => {}
                    }
                }
                // This is a control dependency added intentionally, skip it
                Node::Inst { .. } => continue,
                // No other node types are possible
                Node::Result { .. } | Node::Stack(_) => {
                    unreachable!("invalid successor for instruction node")
                }
            }
            remove_nodes.push(dependency_node_id);
        }

        // Remove all of the result nodes because the instruction is going away
        for pred in graph.predecessors(inst_node) {
            remove_nodes.push(pred.dependent);
        }

        // Remove the instruction last
        remove_nodes.push(inst_node);

        // All of the nodes to be removed are queued, so remove them now before we proceed
        for remove_id in remove_nodes.iter().copied() {
            graph.remove_node(remove_id);
        }
    }
}
 */

/*
fn is_dead_instruction(
    op: &Operation,
    block_id: BlockRef,
    liveness: &LivenessAnalysis,
    graph: &DependencyGraph,
) -> bool {
    if op.is_trivially_dead() {
        return true;
    }

    let is_live = op.results().iter().copied().enumerate().any(|(result_idx, result)| {
        let result_node = Node::Result {
            value: result,
            index: result_idx as u8,
        };
        if graph.num_predecessors(result_node) > 0 {
            return true;
        }
        liveness.is_live_at_end(result as ValueRef, block_id)
    });

    !is_live && op.is_memory_effect_free()
}
*/
