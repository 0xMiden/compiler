# Unified stack allocator design

Status: approved design. This document specifies the resulting architecture. Required observable behavior is recorded in the acceptance document, historical integration states in the appendix, and implementation sequencing and completion gates in the implementation plan.

The MASM backend will own one physical allocation plan for each emitted procedure. That plan determines the ordered operand stack, compiler-managed local storage, operand scheduling, control-flow transfers, and call preparation. An independent checker validates the plan and its instruction expansion before assembly. HIR remains semantic SSA: a physical reload does not create a new HIR definition, and a wide region interface does not need to be narrowed.

The hard machine constraint is that an instruction may access only the top sixteen field elements through indexed stack operations. Total stack depth may exceed sixteen. Correctness therefore depends on the sequence of accesses, including temporary instructions inside a lowering, rather than a bound on the number of live values or operation operands.

## Architecture and trust boundary

Final HIR or a registered generated-procedure definition supplies the semantic input. Selection constructs the checked graph; allocation chooses physical locations and actions; expansion produces annotated MASM; independent verification admits the procedure for assembly. Packaging does not add executable instructions after verification.

| Boundary | Guarantee or trusted premise | Details |
| --- | --- | --- |
| Semantic input and selection | HIR identities, effects, control alternatives, and generated stages determine the required coverage and bindings | [Allocation input](#allocation-input-and-control-flow) |
| Physical planning | One plan owns the operand stack, frame, edge transfers, and call preparation; legal accesses address only the top sixteen lanes | [Machine invariants](#required-invariants), [allocation and scheduling](#allocation-and-scheduling) |
| Independent checking | The checker checks source correspondence, independently derives liveness and token facts, and replays expanded instructions and physical frame accesses | [Verification boundaries](#verification-boundaries), [shared definitions](#shared-definitions-and-independent-implementation) |
| Trusted semantics | The shared selector, target effects, lane layouts, ABIs, registered lowering semantics, and external summaries remain trusted; physical checking does not prove arithmetic or memory algorithms | [Result publication and trust](#result-publication-and-trust) |
| Acceptance evidence | VM/differential execution tests semantic behavior; mutation tests exercise rejection of invalid plans and expansions | [Required observable behavior](unified-stack-allocator-acceptance.md) |

The detailed acceptance catalogue lives in the [acceptance document](unified-stack-allocator-acceptance.md). Earlier review boundaries are retained as [historical integration states](#appendix-historical-integration-states); they do not authorize alternate production paths.

## Scope and decisions

The design covers all HIR bodies the backend currently emits, including component functions, supporting modules, standalone functions, generated adapters, and global initializers. It preserves the existing supported value representations and ABIs. It supports wide SCF interfaces, legal temporary overflow, and the full sixteen-input FPI payload. Arbitrarily large aggregate SSA values, new calling conventions, and unlimited procedure frames are outside this change.

The final architecture has these decisions:

- Physical allocation occurs after HIR optimization, CFG-to-SCF conversion, and representation legalization. No HIR rewrite follows construction of the allocation input.
- Allocation operates on raw felt lanes, with value layouts retaining their semantic grouping and limb order.
- Every control-flow entry has one location contract fixed before its body is planned. Single-predecessor entries inherit state, ordinary joins choose from actual incoming states, and loop headers are fixed before their cyclic bodies. Every incoming edge establishes the contract through checked simultaneous bindings and any necessary physical transfers.
- Instruction selection supplies executable lowering units with explicit stack and stored-input requirements. Lowering units expose all raw effects, including temporary effects, and distinguish internal stages from actual allocation boundaries.
- Heuristic schedules improve cost. A deterministic storage-based transition establishes that a supported request can be realized without illegal stack accesses.
- A release-build verifier checks allocation and raw expansion. Assembly does not accept an unverified plan.
- The two HIR spill passes, spill-driven SSA reconstruction, and region narrowing are removed. There is no permanent legacy allocator, compatibility adapter, or feature flag.

## Existing backend and the boundary being changed

The source baseline is compiler commit `62d5241d6aa2a25eae7b766eda196bc364d8295a`, with PR 1461 reviewed at `bcbb13cc047e3597571a3b42c4e15f87121d3ff8`. Relative source links below are repository links, not proposed new files.

| Existing component | Current responsibility | Resulting responsibility |
| --- | --- | --- |
| Legacy spill analysis | Bounds resident sets, selects spills and reloads, reconciles control-flow residency | Removed from physical allocation; backend graph liveness and the physical planner replace it |
| Legacy spill materialization and SSA reconstruction | Insert spill/reload operations, reconstruct SSA, convert them to locals | Removed; physical actions reference semantic values without rewriting HIR |
| PR 1461 `NarrowRegionOps` | Rebuild wide SCF operations with local-backed columns | Removed or omitted if not landed; edge location assignment handles wide interfaces |
| Legacy `BlockEmitter` | Schedules operands, drops values, handles casts, recursively manages stack state and debug locations | Split into physical planning, verified instruction expansion, and serialization |
| [Operand movement solver](../../codegen/masm/src/opt/operands) | Searches copy/permutation schedules; cannot introduce temporary storage | Optional bounded optimization within the planner; failure selects the deterministic transition |
| Legacy lowering callbacks and [instruction algorithms](../../codegen/masm/src/emit) | Select and emit code, with some temporary effects hidden from the modeled stack | Select lowering units with explicit requirements, effects, and source provenance |
| Legacy branch utilities | Adopt one branch's stack and realign another; assert loop agreement | Emit the structured skeleton and explicit edge transfers to fixed contracts |
| [`MasmFunctionBuilder`](../../codegen/masm/src/lower/component.rs) | Fix frame size from HIR locals before emission | Finalize the frame after backend spill slot and scratch allocation |
| [Legalization](../../codegen/masm/src/legalization.rs) | Representation/ABI checks, including combined operand-window limits | Retain representation and actual ABI checks; replace scheduler-imposed limits with lowering unit legality |

Before CFG-to-SCF processes a cycle containing the region entry, it makes initial activation explicit with a fresh predecessor-free entry block. The new block preserves the region argument types, order and spans and forwards those arguments to the original entry; existing backedges retain their original operands. A single-block self-loop still requires lifting. Dominance is invalidated after entry normalization.

CFG-to-SCF has no algorithmic dependency on spill insertion. Removing the pre-lift spill pass changes its input shapes, but not its semantic contract. Conversion can leave residual conditional branches to distinct return-like exit blocks; the backend must continue to handle these edges. See [`cfg_to_scf`](../../hir-transform/src/cfg_to_scf.rs).

## Machine state and invariants

### Values and raw lanes

`ValueId` identifies a HIR SSA value or a selected temporary passed between lowering units. `Layout(value)` is its ordered raw representation, with lane zero nearest stack top. `Atom(value, lane)` identifies the bits of one lane in the current dynamic instance of that value. A stack entry is an occurrence of an atom, not merely membership in a resident set. Multiple occurrences may have the same atom. Raw temporaries internal to a lowering unit are tracked by verifier tokens; they do not require new values in the selected program.

Layouts use the existing `to_raw_parts()` convention in [`Operand`](../../codegen/masm/src/stack.rs). The initial supported domain remains the existing nonempty representations of at most four lanes per ordinary operand. Wider tuples of such values are supported. The allocator does not claim to complete the currently incomplete aggregate load/store lowering. Representation-neutral casts and split/join operations have explicit lane-alias maps; they cannot silently reverse limbs or change width.

A spill slot is a compiler-assigned location in the current procedure activation’s local frame that preserves a value lane independently of the operand stack. Spill slots store raw field elements. They never apply integer masks, sign extension, byte-pointer conversion, or aggregate memory-layout transformations. Source-level local accesses retain their existing typed semantics and remain separate from compiler spill slots.

### Owned stack and protected tail

Stack notation is top first:

```text
[owned occurrences | protected caller tail | bottom zero padding]
```

The planner knows every owned occurrence, including occurrences below depth fifteen. Metadata inspection can inspect any known depth. An emitted instruction accessing an existing element must satisfy that instruction's actual index restrictions; metadata inspection is not an emitted access.

The protected tail is an uninterpreted sequence belonging to the caller. For `exec`/`dynexec` entry it starts immediately below the declared arguments, including unknown lanes still inside the addressable window as well as overflow. Modeling only the arguments and treating the remaining addressable lanes as disposable is invalid. Ordinary allocation actions must not read, rearrange, overwrite, or drain the tail. A lowering unit additionally preserves the suffix beneath its declared inputs, even when that suffix contains owned values that another lowering unit will use later.

The VM keeps physical depth at least sixteen. A pop at the floor introduces a bottom zero. Consequently, logical owned depth and physical VM depth are different quantities. Same-context transitions preserve the protected sequence but may append bottom padding. Contracts permit this padding difference; they do not permit differences in owned occurrences or protected values.

Let `n` be owned depth, `t` protected-tail length, and `b` bottom-padding count. Physical height is `n + t + b`. For a primitive consuming `d` owned lanes, apply the floor rule in actual primitive execution order:

```text
n' = n - d
b' = max(b, 16 - t - n', 0)
```

A push increases `n` and leaves `b` unchanged. Consequently, between explicit normalization operations, `b_out <= max(b_entry, max(0, 16 - t))`. Repeating a loop with a fixed owned contract and protected tail cannot create unbounded padding. Temporary pops inside instruction expansions participate in this calculation.

The checker's finite padding domain is a set of `(tail_length_class, padding_count)` possibilities: tail classes `0..15` and `>=16`, and padding counts `0..16`. The actual tail identity and symbolic length are retained separately. Unclassified input suffix elements remain protected; only established floor-generated zeros are padding. At selected-program joins, union padding possibilities while requiring the exact owned-stack and required spill-slot contract after simultaneous parameter binding. Internal branches use the token-merging rules specified below, under the same padding model. Calls summarize this bounded relation rather than assuming a fixed physical stack delta. Stack-depth-observing helpers require explicit contracts; ordinary lowering units cannot inspect the abstract padding or protected tail.

At a context-return boundary, padding abstraction alone is insufficient. The emitted procedure must return with physical depth exactly sixteen, with results in the prescribed positions. The existing context-return cleanup is retained as an explicitly checked ABI operation. It must never be applied to an ordinary `exec` return whose overflow can contain caller-owned values.

### Canonical source signatures and target call capacity

Wasm canonical ABI classification counts flattened core values, independently of their Miden lane widths. A flattened `u64` counts as one source parameter and occupies two felts. Classify the complete source parameter list using the standard sixteen-value limit; append a canonical lowering's result pointer after parameter classification, so a core wrapper may have seventeen parameters. Do not invent a tuple signature merely because the direct source signature expands beyond sixteen felts.

Same-context wrappers may receive wider argument stacks. They perform canonical argument loading and result storage in their own context, then construct the actual target invocation. Enforce the target ABI's sixteen-felt payload limit on that invocation, after separating wrapper-only operands. An output pointer used to store returned results is not a target argument. The allocator and physical verifier still check every indexed access and the complete same-context suffix contract.

For FPI, the six metadata lanes remain separate from the actual payload. Thirteen canonical values comprising six metadata felts, six `u64` values and one payload felt occupy nineteen wrapper lanes but only thirteen payload lanes, and are supported. For dyncall, the four target-root values participate in source classification but not the target argument budget. A source signature containing four root values and sixteen payload values therefore uses a canonical tuple; the same-context wrapper loads the complete tuple, separates the root, and passes the sixteen payload felts to the target. Tuple loading preserves canonical layouts and typed scalar carriers before any invocation-specific representation conversion.

This separation does not introduce a new cross-context transport ABI. Ordinary imports or exports whose actual target payload exceeds sixteen felts remain diagnosed, as do tuple transports unsupported by their target boundary. Source-to-VM tests must exercise validated component signatures, canonical tuple and result-pointer variants, distinct multi-lane values, and genuine target overflow; direct HIR wrapper tests alone do not establish source compatibility.

### Required invariants

1. **Legal access.** Every primitive's reads, writes, permutations, and word accesses are legal at the point where it executes. The bound counts felt lanes, including temporary lanes.
2. **Correct consumption.** Each lowering unit receives the declared stack-input order and multiplicity, and each stored-input binding contains its required current atom. Its result lanes have the declared layout. A later phase's operands are not considered dead after an earlier phase consumes a related value.
3. **Recoverability.** Every live atom is available in an owned occurrence, a currently valid spill slot, or an explicitly allowed literal/alias reconstruction. No transition destroys its last recoverable representation.
4. **Valid storage.** A load reads a spill slot containing the current required atom. A write invalidates the old content. Spill slot validity is a path fact, not a historical “this value was spilled” flag.
5. **Simultaneous edges.** Edge assignments observe all source values before overwriting any destination needed by another assignment.
6. **Entry agreement.** Every executable incoming edge establishes the destination's exact owned stack, required storage contents, and address provenance after simultaneous binding. Physical bottom padding is governed separately by the floor rule.
7. **Loop rebinding.** Header parameters denote the next iteration after a backedge. Stale occurrences and spill slots from the previous iteration cannot satisfy the new contract merely because their static `ValueId` is the same.
8. **Tail and ABI preservation.** Allocation preserves the protected tail; call lowering units enforce their actual context, clobber, padding, and return-depth rules.
9. **Closed physical plan.** All spill slot addresses, scratch addresses, result bindings, and raw instruction effects are determined before serialization. No emitter makes additional allocation decisions.
10. **Resource legality.** Frames and addresses satisfy target limits. Optimization failure is recoverable; an invalid lowering unit, unavailable semantic value, or frame exhaustion is diagnosed explicitly.

A dead occurrence can remain buried until a legal cleanup point. It remains represented as dead storage in the physical state. It is never forgotten while still present on the runtime stack.

## Allocation input and control flow

### Selected program

Instruction selection creates a codegen-owned `SelectedProgram`, independent of HIR mutation. It contains a structured control-flow skeleton, straight-line segments of selected lowering units, and an execution-graph projection of that skeleton. This is a small backend representation, not another general-purpose optimization dialect.

A **lowering unit** is one selected instance of a target lowering, with explicit semantic uses and definitions, physical input/output requirements, and storage requirements. It expands without invoking allocation internally, but can contain branches, loops, calls, and traps. A **registered lowering definition** specifies its selection, expansion, and checking rules; a `SelectedLowering` is an instance of that definition; its **expansion** is the emitted MASM body. One HIR operation may select several units when it needs allocation between phases. Internal stages of one unit do not create those boundaries.

The skeleton determines where the final MASM `if`, `while`, and switch-dispatch code occurs. The graph determines reachability, liveness, and edge transfers. They reference the same segments and edges. Selection can introduce temporary values and segments for phased calls and dispatch; these are included before liveness is computed.

Ordinary operation order and effects remain fixed. The allocator does not move loads, calls, traps, or other HIR operations across each other. Rematerialization is limited initially to exact literals and representation aliases. Existing HIR optimization retains responsibility for semantic motion.

Pointer constants carry their complete pointer type, including the outer byte/element address space and the nested pointee type. Parsing, folding, materialization and result inference preserve that type; final selection reconstructs the same complete type when checking source authority. A pointer constant changes no address bits or memory behavior merely by retaining its declared type. Constructors take the complete pointer type so a full result type cannot accidentally become an extra pointee layer.

Frontend return construction preserves the complete declared result tuple in source order. For Wasm, an explicit return takes exactly the declared number of operands from the stack suffix; unrelated lower operands are not results. A reachable implicit function exit returns its complete validated exit tuple. Unreachable exits do not synthesize a return from an invalid stack state. Selection independently checks the resulting HIR tuple against the function signature; allocation never repairs a truncated or mismatched source return.

### Final emission identities

The backend determines final emitted module and procedure owners before selection, using the target and existing namespace eligibility rules. An immutable, finite map contains only definitions the component actually emits; it includes generated owners and validates destination uniqueness and collisions. Declaration-only external siblings, raw external helpers and digest targets retain their identities, even when their spelling overlaps an emitted namespace. Authored component namespaces retain their existing eligibility rules. Already-namespaced source dependencies use their supplied namespace, not the enclosing compilation target.

Canonical HIR identities remain authoritative for calling conventions, effects, kernel ownership and restricted helpers such as `truncate_stack`. Selection and source-correspondence checking use the same final emission-owner map to form targets and import destinations. Source and local alias names retain their spelling. Generated roots, initializers and tables use the same map, so final invocation identities enter verification together with the instructions that use them.

Packaging assembles these final identities without rewriting verified instructions or reconstructing procedure bodies. `--emit=masm` therefore shows the final namespace. Supplying an incompatible later target is diagnosed; changing the target requires lowering again. There is no general post-verification relocation mechanism or implicit prefix-based retargeting.

### Generated procedure definitions

`ProcedureOrigin` is either a final HIR procedure or a registered generated-procedure definition with explicit parameters. Generated definitions cover executable main, initialization sequencing and fragments, entry/ABI adapters, and the optional test harness. They specify required stages, source bindings, control flow, effects, and entry/return conventions. `SourceStepId` identifies an HIR operation or a stage of such a definition; generated bodies do not invent HIR provenance.

A generated definition produces a `SelectedProgram` through the same allocation, frame-layout, expansion, and verification boundary as an HIR body. The coverage checker compares its selected stages and bindings with the registered definition and its parameters. A builder cannot declare its own output correct by attaching a coverage annotation. Initialization order, entrypoint target, invocation convention, test-harness control flow, and return normalization are explicit obligations.

Artifact construction may package and link verified bodies but cannot append executable instructions afterward. In particular, the current raw-MASM construction of main and the test harness in [artifact generation](../../codegen/masm/src/artifact.rs) moves behind this boundary. The executable root's entry/exit contract is explicit; any helper procedure it invokes must use an opcode compatible with that helper's convention.

### Graph construction

A segment ends at a selected branch, region entry/exit, call phase boundary requiring allocation, or terminal operation. Each edge records simultaneous bindings `destination parameter <- source value` and its control alternative. Captures remain references to dominating values; they do not require a second allocation mechanism.

| HIR construct | Execution graph |
| --- | --- |
| `scf.if` | Dispatch to arms; each yielding arm transfers to a continuation whose parameters are the operation results |
| Missing `else` | Identity edge to the continuation; valid only with no results |
| `scf.index_switch` | Selected dispatch tree with case/default edges, followed by yield-to-continuation bindings |
| `scf.while` entry | Init operands bind the before-region parameters |
| `scf.condition`, true | Forwarded values bind the after-region parameters |
| `scf.condition`, false | Forwarded values bind the while-result continuation parameters |
| After-region `scf.yield` | Yielded values bind the before-region parameters on the backedge |
| Residual `cf` terminal dispatch | Explicit edges to the supported terminal exit blocks |
| Return or unconditional trap | Terminal edge; no normal successor contract |

The before and after parameter groups of a while are independent. Their arity and types need not match one another. Each individual binding must match its destination layout.

A selected loop retains a fixed cyclic entry segment and a structured before-region subtree. The subtree starts at that entry and may contain nested conditionals, loops, and lowering phases with allocation boundaries. Its normal frontier reaches the condition site, determined by the common source of the checked true/false edges. Predicate demand belongs to that condition site; induction, header contracts, and backedges belong to the fixed cyclic entry. Allocation, demand/reconstruction analysis, expansion, and independent graph projection all traverse the entire before subtree. Each iteration executes that subtree before evaluating the condition. A straight-line before-region is the single-segment case of this representation.

Trapping before-region arms have no normal continuation. If the entire before-region traps, selection represents only that executable path and retains source coverage for the omitted condition, after-region, and unreachable tail; it does not invent a predicate use. Logical return markers cannot bypass later executable code or a physical while's continuation-flag consumption. Final source normalization must expose supported terminal exits; unsupported mixed return/normal fallthrough or returns inside a physical loop are diagnosed explicitly.

An index switch with no explicit cases selects its default region directly, preserving its captures, bindings, results, and source coverage without generating comparisons. Nonempty switches retain the existing sorted case/region pairing and linear or guarded interval/binary dispatch policy.

The graph builder validates source/destination ownership, arities, lane layouts, region terminators, and structured control-flow correspondence. It handles supported region operations explicitly. Merely implementing `RegionBranchOpInterface` does not make an unknown operation lowerable; generic interface verification is not currently sufficient to establish these facts.

The existing while emission shape can remain: an outer `while.true`, before-region code, a conditional selecting after-region code or exit, and the synthetic continuation flag. Synthetic flags are physical control temporaries, not another sixteen-element budget charged against all forwarded values. Edge repairs execute on the selected edge after its predicate has been consumed, except repairs common to all successors that are explicitly proved safe to share.

## Analyses

### Reachability and liveness

The selected graph is immutable during analysis and allocation. Reachability follows all structurally executable alternatives; an edge is excluded only when selection has established its impossibility, such as an unconditional trap. Analysis correctness does not depend on speculative branch probabilities or stale HIR executability facts.

Backend liveness uses dense bitsets of semantic value identities. A live value initially means all its representation lanes are needed; lane-specific aliases are normalized through the layout table. Selected temporaries passed between lowering units and operands used in later phases are ordinary values in this analysis.

Every value is published as a complete representation at an allocation boundary. `OutputPublication::Stack` requires the complete top-first output prefix. `OutputPublication::Aliases` instead publishes complete semantic names through source-checked `Reconstruction::Alias` lane mappings; it does not require a physical result prefix. A selected unit defining a value publishes all its layout lanes exactly once at that unit's normal completion; separate units cannot publish different lanes of the same value. Segment parameters likewise introduce complete values through checked simultaneous bindings. Cross-unit partial computations use explicit scalar or typed temporary values and an explicit alias/join completion unit when a complete result is needed. This keeps dense semantic-value demand meaningful at every boundary; it does not restrict raw partial stages inside one lowering unit.

Let `P(S)` be the parameters of successor segment `S`, and `bind_e(p)` the source supplied to parameter `p` on edge `e: B -> S`:

```text
edge_live(e) = (live_in(S) - P(S))
             union { bind_e(p) | p in live_in(S) intersect P(S) }

live_out(B) = union edge_live(e), for all outgoing edges e

live_before(I) = semantic_uses(I) union (live_after(I) - definitions(I))
```

Solve backward to the least fixed point using a predecessor worklist. Reverse scans then record liveness at lowering unit boundaries. Branch predicates are semantic uses. Edge operands contribute through live destination parameters, not through a blanket use of every forwarded operand. Debug-only descriptions do not create semantic uses. An alias-producing cast still has a semantic dependency on the bits it aliases.

The existing HIR `LivenessAnalysis` remains available to HIR optimizations. It is not authoritative for physical allocation: it combines HIR region traversal, executability, and next-use penalties that do not describe selected call phases or synthetic control flow.

### Representation-only publication

Selection authorizes `Aliases` only for transparent representation aliases, split/join, and unpack stages. Their existing checked reconstruction maps are the sole lane-map authority. Every source lane belongs to a declared semantic input whose definition precedes the alias stage. Observable operations retain stack publication even when some results equal their inputs.

An alias unit has no stack or stored inputs, scratch, scratch effects, address results, or source effects, and returns normally. It retains an empty lowering body and its source/proof boundaries. Preparation checks recoverability of all semantic inputs and live-through values without emitting movement, loads, stores, or fallback normalization, even for values buried below the addressable window. A source may exist solely in a spill slot or through an established literal/alias reconstruction.

Completion enables all output reconstructions simultaneously, preserving physical occurrences, cells, and padding. Stale semantic facts about redefined values are invalidated as usual; forgetting a fact performs no memory write. Loop entries establish only the current dynamic scope, so a prior iteration's output name cannot supply its own new definition.

Independent replay resolves all output tokens from incoming semantic names before rebinding any output. Each source must have an actual physical representation or an independently checked reconstruction; name presence alone is insufficient. Replay requires unchanged stack, memory cells, spill contents, padding, scratch-content identities and execution obligations. Complete-value, dominance and declared lane-mapping checks still apply. This allows source and alias to remain live together without a duplicate until a consumer actually requests two physical occurrences.

Acceptance includes buried, spill-only, reconstructed-only and chained aliases; split/join/unpack lane order across joins/backedges; rejection of unavailable sources, incomplete or incorrect maps, unauthorized publication, effects/storage requirements and physical mutation; and transparent casts whose alias boundary emits no stack operations.

### Next use and cost

Correctness depends only on liveness. Scheduling priority uses a separate deterministic next-use computation over the selected graph: each lowering unit step has a positive cost, a use has distance zero, predecessor transfer adds that cost, and alternative paths take the minimum finite distance. Parameter bindings map distances back to their edge sources. Unused values have infinite distance. Loop membership is computed from graph SCCs; a tie favors retaining a value used in the current loop over a value used only after leaving it.

Distances are saturated integers. They can be computed by a reverse relaxation worklist initialized from uses. Ties use stable value and lane IDs. The heuristic is deliberately simple and inspectable; it is not a claim of globally optimal spilling.

### Entry contracts

Each segment has one entry contract, fixed before planning its body. The contract specifies its exact owned stack, required valid spill-slot and spanning-scratch contents, address provenance, checked lane aliases, and tail policy. Every incoming edge must establish it. Choosing a contract need not precede planning its predecessor bodies: those bodies depend on liveness and next use, not on a successor's final layout.

Procedure entry starts from its ABI-defined state. The rules below govern reachable internal entries; an unreachable segment requires no physical plan.

The planner records each completed predecessor's outgoing state before edge repair. At a conditional, this is the state after predicate consumption on the corresponding alternative. A join waits for all reachable forward incoming states; it does not select whichever state happens to be visited first. Loop headers use the separate rule below. Removing the selected structured loop backedges must leave an acyclic planning graph; selection validates this property. Nested loops follow the same rule recursively. This permits each segment body to be planned once, without global provisional allocation or repeated contract revision.

#### Inherited entries

A nonheader segment with one reachable predecessor inherits the incoming owned stack and valid spill-slot facts for live values, translated through its checked parameter bindings. Dead or buried stack occurrences remain physically represented; there is no compulsory normalization or sixteen-lane truncation. Ordinary preparation can remove them when useful. Facts for dead slot contents may be forgotten without a runtime operation.

Bindings do not imply extra physical copies merely because the destination uses different SSA names. When one source atom supplies multiple destination atoms, the contract can record their equality and let them share a representation. These aliases arise only from checked bindings or existing representation-alias rules; they are not an unrestricted value-equivalence analysis. If a later lowering unit or a chosen contract needs two distinct stack positions, it must materialize two occurrences. A single occurrence cannot fill two positions in an ordered stack layout.

#### Delayed selection at ordinary joins

After all predecessor body plans are complete:

1. Translate their actual outgoing states into the destination's parameter namespace without emitting repairs. Record the source-side evidence for every binding, including duplicate bindings. Captures retain their identities. Only alias equalities proved on every incoming edge are eligible for the join contract.
2. If all incoming owned stacks already agree, adopt that stack with the common valid spill facts only when every live atom is recoverable from this proposed common contract, including its eligible aliases and permitted reconstructions. Per-path recoverability in different slots is insufficient; if a live atom has no common representation, continue to candidate construction and repair. Physical bottom-padding possibilities are unioned separately. An already agreed wider stack need not be narrowed to sixteen.
3. Otherwise build the three candidate layouts below, attach their required spill facts, and deduplicate identical contracts.
4. Trial incremental repair from each immutable incoming state to each candidate. Also trial preparation of the successor's first lowering unit from that candidate. An empty segment contributes zero preparation cost. Trials do not commit actions, storage requests, or successor body plans.
5. Choose the minimum-cost successfully constructed candidate, commit its edge repairs, freeze its contract, and plan the successor body once. Candidate failure does not damage the incoming snapshots. An unavailable live value is an invariant error. Final frame capacity is checked after physical slot assignment over the completed plan; local scoring does not claim to predict the final coloring or perform global frame-capacity search.

The candidate set is fixed and small:

| Candidate | Construction |
| --- | --- |
| Next-use layout | Rank live complete values by next use, then stable ID; place values while their combined width fits sixteen lanes |
| Common-residency layout | Prefer complete values addressable on every incoming edge. Order them by their maximum incoming raw depth, then next use and stable ID, which preserves unanimously agreed relative ordering. Fill remaining capacity from the next-use ranking |
| Incoming layout | Filter each incoming stack to one occurrence per live complete value and take layouts fitting sixteen lanes. Group identical layouts and choose the group with greatest incoming weight; break ties by stable edge ID |

These candidates use sixteen as a selection heuristic, not a total-depth invariant. ABI layouts, inherited states, agreed join states, and lowering unit preparations may be wider. Captures and parameters participate in the same candidate construction. A candidate assigns a required spill slot to each live lane without a resident or permitted reconstructed representation; therefore a wide interface remains representable. The all-spill-slot contract is the conservative limiting case of this policy.

The score is `sum(weight(edge) * repair_cost(edge, candidate)) + sum(weight(edge)) * first_preparation_cost(candidate)`. Costs use the scheduling estimate of expanded instructions, including stored-input establishment, stack loads, and stores. Weights default to one; existing execution-frequency metadata may supply weights without introducing a new profiling analysis. Equal scores use the candidate order in the table, then stable IDs. This accounts for an immediate shuffle or reload in the successor without looking ahead through its body or recursively optimizing downstream contracts.

#### Preserved spill-slot facts

A resident value may also have a required valid spill copy. At an ordinary join, retain such a fact when the same abstract slot contains the corresponding current atom on every incoming edge after binding. This is an intersection of proven slot contents, not a union of previous stores. Different slots on different edges do not establish a common fact. Additional facts may be forgotten; no path-specific fact becomes a successor assumption.

Prefer a common existing slot for a required nonresident atom. Otherwise use its deterministic canonical spill-slot ID, creating that ID lazily. Captures reuse their canonical IDs across segments; loop parameters have stable header slot IDs. Stable IDs identify storage, not permanent contents. If several common slots contain an atom, prefer its canonical slot, then the smallest stable slot ID, to avoid retaining unnecessary duplicate storage obligations.

Canonical slot keys derive from stable semantic value/lane IDs, and temporary keys from stable lowering unit or edge IDs and local indices. Tie-breaking does not use predecessor visitation order, global allocation counters, or speculative-trial order.

For example, when both arms load `x` from the same slot, the join can retain both its stack occurrence and its valid spill copy. A later eviction can drop the stack occurrence without storing it again. Slot facts included in the contract remain liveness obligations during physical slot reuse.

#### Loop-header selection

Loop headers are fixed after their forward-entry states are known and before planning their cyclic bodies. Forward incoming edges are repaired to that contract; backedges are repaired after their bodies are planned. A later backedge never causes header revision or body replanning.

Forward-entry equality alone does not establish a loop invariant. This initial policy does not add aliases between distinct loop parameters at the header, even if they receive the same initial value: their backedge sources may differ. Selection-established representation aliases and invariant-capture identities remain available. Ordinary joins inside the loop can still preserve equalities proved by all their actual incoming states.

Exact scratch-address facts are retained for live captures and directly identity-carried parameters. Common forward-entry facts supply the base case. A parameter retains its fact only when every cyclic incoming edge binds it to the same old-source parameter; checked SSA definitions prevent its redefinition inside the loop. Inspect bindings simultaneously, without following aliases or permutations. One changing binding excludes the parameter even if another backedge preserves it. This bounded proof does not broaden cached spill-copy eligibility or infer invariance from equal initial addresses.

Choose resident complete values using static loop demand: values used in the cyclic body take priority over values used only after leaving it. Within that class, use next-use distance restricted to the loop; within the exit-only class, use ordinary next use. Prefer an existing forward-entry order when priorities tie, then stable IDs. Fit the selected values into the ordinary sixteen-lane header target. This prioritizes repeated loop work over making the first entry free, without forecasting an unplanned backedge. Nested-loop uses participate in the containing loop's demand; no trip-count inference is required.

Nonresident loop-invariant captures receive spill slots established on forward entry. Their contents are required at the header and preserved through the body and backedges. Resident captures also retain spill facts already established on every forward entry. The compiler does not create speculative spill copies for every resident invariant. A spill first introduced inside a loop may still repeat; eliminating all such stores is outside this initial policy.

An invariant spill fact is established inductively: the entry edge supplies it, the body preserves it, and each backedge proves it still holds. It is not intersected with an initially unknown backedge and discarded. Calls preserve these slots under the existing caller-local ABI assumption. Physical coloring must respect their loop-spanning lifetimes.

Changing loop parameters obey the same fixed slot assignments but require the new incoming source token on every backedge. An old iteration's contents are insufficient. Identity transfers can omit a store when the slot already contains the actual required source atom; a stable parameter ID alone cannot establish that equality.

## Physical plan representation

The following types specify responsibilities; field/container choices can follow existing Rust conventions.

```rust
struct SelectedProgram {
    origin: ProcedureOrigin,       // final HIR or registered generated definition
    values: ValueTable,             // HIR origin, type, raw layout, alias maps
    segments: Vec<SelectedSegment>, // selected lowering units, in semantic order
    edges: Vec<TransferEdge>,       // simultaneous parameter bindings
    structure: ControlTree,         // MASM-compatible structured skeleton
    abi: ProcedureAbi,
}

struct EntryContract {
    stack_top_first: Vec<Atom>,
    required_spill_slots: Vec<(SpillSlotId, Atom)>,
    required_scratch: Vec<ScratchCellFact>, // initialization and selected content relations
    address_facts: Vec<AddressFact>,       // object, range, and offset provenance
    aliases: LaneAliasTable,        // equalities established by every incoming edge
    tail: TailPolicy,
}

struct AllocatorState {
    stack_top_first: Vec<Occurrence>,
    spill_slots: Map<SpillSlotId, Atom>, // currently valid contents only
    aliases: LaneAliasTable,
    tail: TailState,
}

enum PlanAction {
    Dup { depth: u8 },
    Swap { depth: u8 },
    SwapWord { word: u8 },          // complete owned words, indices 1..=3
    MoveUp { depth: u8 },
    MoveDown { depth: u8 },
    Drop,
    PushLiteral(Felt),
    StoreLane(SpillSlotId),         // consumes raw top lane
    LoadLane(SpillSlotId),          // pushes raw spill slot contents
    RunLowering { step: SelectedStepId }, // exact selected instance and phase
    DebugLocation(DebugLocationId),
}

struct SelectedLowering {
    kind: LoweringKind,                 // registered expansion and checking rules
    source: SourceStepId,             // HIR operation or generated-definition stage
    semantic_inputs: Vec<ValueId>,
    stack_inputs: Vec<Atom>,          // top first, with required multiplicity
    stored_inputs: Vec<StoredInput>,
    outputs: Vec<Atom>,              // complete semantic definitions
    publication: OutputPublication, // Stack prefix or semantic-only Aliases
    effects: LoweringEffects,
    scratch: Vec<ScratchRequirement>,
    scratch_effects: Vec<ScratchEffect>, // checked content dependencies, reads, and writes
    address_results: Vec<AddressFact>, // scalar outputs equal declared scratch addresses
}

enum OutputPublication {
    Stack,
    Aliases,
}

struct StoredInput {
    id: StoredInputId,                // local to this selected lowering unit
    atom: Atom,
}

struct LoweringBindings {
    input_order: InputOrder, // Canonical or a source-authorized whole-operand reversal
    stored_inputs: Map<StoredInputId, SpillSlotId>,
    scratch: Map<ScratchId, ScratchAllocation>,
}

struct ProcedurePlan {
    selected: SelectedProgram,
    contracts: Vec<EntryContract>,
    segment_actions: Vec<Vec<PlanAction>>,
    edge_actions: Vec<Vec<PlanAction>>,
    spill_slots: SpillSlotTable,
    lowering_bindings: Map<SelectedStepId, LoweringBindings>,
    semantic_storage_reads: Map<SelectedStepId, Vec<SpillSlotId>>,
    source_map: SourceMap,
}

struct VerifiedProcedure {
    frame: FinalFrameLayout,
    body: VerifiedMasmBody,
    debug_locations: DebugLocationTable,
    invoked: InvocationSet,
}
```

Depths in primitive actions count raw lanes. Value grouping is metadata used to propose schedules, not a restriction on fallback operations. `RunLowering` references the selected operation; it cannot provide a caller-authored list of “expected values” that the checker blindly trusts.

Selected segments declare `ContentId` parameters separately from `ValueId` parameters; edges carry a separate list of `ContentBinding { destination, source }`. Each content ID has one selected definition or parameter owner. Incoming content bindings are complete and simultaneous, permit repeated sources, and must refer to definitions available at the source. The verifier substitutes them into physical scratch demands from one source snapshot before introducing destination names. Scratch demand propagates backward independently of SSA operand liveness.

Unit source-memory permissions are compositional: `SourceMemoryEffects { read, write }` may permit both accesses. Every actual source-memory access, including direct source-local instructions, requires its corresponding permission. A primitive memory contract describes at most one contiguous read footprint and one fixed-width contiguous write region, with explicit input/output token references for the stored values. Reads are fixed-width or use the closed footprints of EvalCircuit, MemToMap, DeferredRegisterData and PrintLn. Source permissions do not grant compiler-storage access.

Edge binding is not an executable arbitrary `Rename` instruction. The checker first proves the target locations contain the edge's source values, then applies the simultaneous destination names at the boundary. Alias maps are likewise justified by selection rules.

Source-checked `Reconstruction::Literal` and `Reconstruction::Alias` declarations constrain actual lane identities at publication and incoming parameter binding. Fresh hypotheses preserve literal identities and acyclic alias relations only for definitions dominating that entry; a previous loop instance cannot establish a current alias. Recoverability may use those identities to reconstruct a literal or share an equal representation. A reconstruction declaration does not exempt emitted instructions from identity checks or replace an actual required physical store.

Contract aliases describe equality of current lane tokens. They allow multiple semantic names to use one recoverable representation, but do not change physical stack multiplicity. Rebinding or redefining a value invalidates facts about its old dynamic instance, including aliases; only checked new bindings can establish replacement equalities.

`SelectedLowering.address_results` declares scratch-address provenance published at normal unit completion. Each fact names a scalar output of that unit, a declared scratch object, an exact element offset, and an accessible range. Selection checks the declaration against the registered source definition and validates output ownership, uniqueness, and the range with checked arithmetic. An address that survives the unit boundary requires a spanning scratch object; publishing an address into unit-local workspace is invalid. Scratch-content/output equality is a different relation and cannot substitute for an address-result declaration.

Allocator completion invalidates facts for old dynamic definitions before publishing these declared address facts. Entry inheritance, joins, and simultaneous edge binding carry only established current facts. The allocator does not inspect expansion instructions to discover addresses or import the verifier's token transfers. An address-result declaration does not initialize the object, permit a memory access, or relax pointer-escape restrictions.

`SpillSlotId` is an abstract spill slot for one raw lane. A scratch descriptor records size, alignment, contiguity, address space, declared uses and lifetime, and whether it is unit-local or spans lowering units. Scratch cannot alias a live spill slot. All scratch objects are declared before frame assignment; lowering unit expansion cannot discover additional storage. Source-declared HIR locals use their existing identifiers and occupy a disjoint frame range. Spill slot allocation does not mutate `Function::locals()`.

Spill slots use the existing procedure-local memory and calling convention. On invocation, the VM advances the frame pointer to allocate sufficient space for the callee’s declared locals. The existing ABI assumes that a procedure does not write into its caller’s local memory unless explicitly arranged, such as by passing a pointer to a caller local. Ordinary lowering does not expose compiler spill-slot addresses to source operations or callees. Generic event handlers may observe all generated physical state, including those addresses, and may return them through advice. Subsequent source and callee accesses, including advice-derived addresses, remain subject to the existing source-memory ABI; the verifier does not prove numerical disjointness for unknown addresses. Spill contents survive same-context calls under that ABI assumption. This is an accepted premise of the design, not an open validation question or a new memory-protection mechanism; malicious writes are outside scope. The allocator and verifier still enforce correct frame offsets, slot reuse, and separation from source locals and reserved scratch.

## Allocation and scheduling

### Lowering unit requests

Each selected lowering unit declares semantic uses and definitions independently of their required physical locations. Its interface specifies an ordered stack-input prefix, stored-input requirements, output layout, allowed aliases, memory/call/trap effects, control behavior, and declared scratch. The registered lowering unit kind determines the expansion and checking rules; a plan cannot supply arbitrary expected values or trusted effects to exempt itself from verification.

Call effects belong to the unit's ordered, source-owned call-site contracts; there is no duplicate unit-wide call-effect field or aggregate. A unit may contain calls with different signatures. Selection reconstructs each site's target, invocation family, ABI, path, arguments, result relations and borrow obligations from the registered source definition and keeps that evidence private in the checked selected program. The physical verifier checks every invocation against its exact registered site, including static occurrence/path checks and replayed stack, selector, suffix, window and publication obligations. Removing the redundant aggregate does not authorize arbitrary calls or effects. Registered context/root stack truncation remains a separate normalization contract with its existing target and return-boundary restrictions. As with other trusted summaries, matching a registered call contract does not independently prove the callee's algorithm or true external signature; semantic conformance tests cover that trusted boundary.

A stored input requires a particular current atom in a valid spill slot at lowering unit entry. It is not a source-local identifier, a new SSA definition, or permission for the emitter to allocate storage. The allocator binds each requirement to a slot, reusing existing valid contents where possible. Equal atoms can share one slot through checked aliases; distinct stored-input IDs do not mandate distinct slots. A semantic operand may occur both in the required stack prefix and in stored inputs, and may have repeated physical uses.

The allocator establishes stored inputs before arranging the stack prefix. If a required stored atom exists only below the addressable window, the existing storage fallback can expose and store it while preserving other pending inputs. All bound slot contents remain required during subsequent stack preparation. A literal satisfies a stored-input requirement only after being written to a bound slot; choosing an immediate instead would require a lowering unit variant that explicitly permits it. Establishing requirements consists of physical allocation actions, not semantic execution of the operation.

Expansion receives resolved input and scratch bindings plus an instruction sink. It cannot query or mutate the allocator's location map, invent a spill slot, or introduce an undeclared storage read. Stored-input slots are read-only within the lowering unit; mutable temporary storage uses declared scratch. The lowering unit preserves the stack suffix beneath its declared stack inputs, even when that suffix includes owned live values. Temporary pushes may exceed sixteen total elements; every actual indexed access must remain legal.

#### Internal stages and allocation boundaries

An internal lowering unit stage is part of one uninterrupted expansion. It can have branches, loops, temporary lanes, bound-slot reads, and intrinsic calls. There is no allocator invocation, entry-contract normalization, or intermediate SSA result publication between its stages. “Uninterrupted” concerns allocation; it does not promise atomic memory effects or prohibit traps.

An allocation boundary is explicitly selected between separate lowering unit steps. A multi-phase operation uses such steps only when allocation between them is useful. Their temporary values and cross-phase scratch lifetimes are present in the selected program before liveness runs; later phases' operands remain live through earlier phases. Each boundary must have a complete physical contract. No phase can assume that a partially prepared stack remains in place unless that boundary actually specifies it.

Each phase publishes complete temporary or result values. For example, phases that compute the two limbs of a `u64` independently publish two scalar temporaries; a later alias/join unit declares their semantic dependencies and publishes both lanes of the `u64` together. The selected-program checker rejects missing, repeated or out-of-range output lanes and a value split across unit publications. Internal raw stages have no intermediate semantic publication and remain unrestricted by this boundary rule.

The byte `memmove` and FPI examples below each use one lowering unit with internal stages. In particular, FPI uses sixteen stack inputs and six stored inputs at full width, then builds its twenty-two-lane call layout internally. It needs neither a fictitious intermediate payload value nor a general facility for locking a prepared prefix across separately allocated phases. Indirect-call or dispatch selection may still introduce genuine allocation boundaries where useful.

Pure arithmetic and existing small scalar memory lowering units can usually retain their instruction sequences. `InstOpEmitter`'s result binding on destructor is replaced with explicit completion of a lowering unit: verify the result shape, then bind its output identities. A partial or failing lowering unit cannot silently publish results.

#### Arithmetic overflow contracts

Felt `add`, `sub`, and `mul` interpret Checked and Overflowing against canonical integers in `0..p−1`, where `p = 2^64 − 2^32 + 1`. Checked traps when the mathematical result leaves that range. Overflowing publishes the field result together with an `i1` flag indicating modular reduction or subtraction underflow. The flag precedes the complete typed result in the HIR result tuple. Wrapping retains native field arithmetic; Unchecked retains its existing precondition and does not acquire a new guarantee for out-of-range mathematical results.

For unsigned 128-bit operations, Checked traps outside `0..2^128−1`; for signed 128-bit operations, it traps outside `−2^127..2^127−1`. Overflowing returns the low 128 result bits together with the corresponding unsigned or signed overflow flag. Wrapping returns the low 128 bits. Source-valid Unchecked operations may use the wrapping algorithm: the existing arithmetic builder contract makes overflow undefined, so this does not define a result for an invalid Unchecked input. Signed overflow must not be inferred from unsigned carry alone.

The registered definition validates the exact operand types, result tuple, and overflow property together. A one-result operation carrying Overflowing is invalid; lowering cannot invent a second semantic result. Publication requires the actual raw result lanes, including an emitted flag where required, rather than a flag present only in the emitter's type model. The physical verifier checks those effects and publication boundaries; targeted VM tests establish the arithmetic values, limb order, flags, and trap conditions, including canonical field boundaries and signed extrema.

The overflow corrections cover Felt and signed/unsigned 128-bit add, subtract, and multiply. They do not expand support to the existing unimplemented signed division, remainder, divmod, or arithmetic-right-shift operations. Selection must reject unsupported operations and type/property combinations explicitly rather than certify an unavailable algorithm.

### Safe storage writes

Every planner-generated compiler-storage write must preserve the old contents if a pending input, live-after value, retained contract fact, bound stored input, or unfinished edge transfer still requires them. This rule applies to stored-input preparation, selective-eviction backing stores, full normalization, and edge repair. A canonical slot ID is a destination preference, never evidence that the cell is empty or contains its namesake value.

`safe_spill_slot(a, needed)` reuses a suitable valid copy or chooses a destination whose old contents are redundant or unneeded. If the preferred slot holds the only needed copy of another atom, choose a fresh abstract slot. For a mandatory destination fixed by a contract, first preserve endangered old contents in another valid location or a fresh snapshot slot, then perform the write. Fresh snapshot selection does not recursively overwrite another required source. Update slot facts after the actual write; merely allocating or renaming a slot initializes nothing.

For example, with stack `[a]`, canonical slot `slot(a)` containing the only copy of needed `b`, and both atoms required, normalization must store `a` elsewhere or preserve `b` before overwriting. It cannot blindly store `a` into `slot(a)`. Final coloring must also obey interference; the physical verifier detects overwrites through aliased final offsets independently of these abstract write checks.

### Straight-line allocation

At each lowering unit step:

1. Determine all semantic inputs, required stored atoms, repeated stack occurrences, live-after values, and future-phase requirements from the selected program and liveness. Stored operands contribute semantic uses even though they do not occupy the input prefix.
2. Establish the stored-input bindings on a trial state, then propose stack preparation using the scheduling tiers below. A source occurrence may be consumed if another representation preserves every required future use. Preserve every bound stored-input fact throughout preparation.
3. Simulate the proposal on a private state copy. Accept only a complete transition satisfying the stack prefix, stored-input bindings, and recoverability invariants. An unsuccessful proposal does not mutate the committed plan.
4. Commit the first successful tier's complete proposal. If the earlier tiers do not apply or succeed, use full normalization while preserving the stored-input bindings. Trial actions, lowering unit bindings, spill-slot contents, and storage requests are committed together; failed trials leave none of them behind.
5. Expand/execute the lowering unit's declared physical effect. Bind results explicitly. Discard invalid spill slot facts and mark dead occurrences. Remove accessible dead occurrences when profitable; inaccessible dead lanes stay accounted for until normalization.

Dead-occurrence cleanup is a bounded preparation candidate for the next non-alias unit, after its debug observations. First construct the ordinary preparation. If it has nonzero estimated cost, try removing consecutive dead owned lanes from the incoming top, stopping at the first needed lane, sixteen lanes, or the ordinary preparation's cost budget. Demand includes semantic and stored inputs, alias reconstruction dependencies, and live-after lanes; compare semantic identities rather than raw value IDs. Do not remove a useful copy merely because it is backed elsewhere, move interior lanes, or touch the unknown caller suffix.

Run the same scheduling tiers from that cleaned state and compare the complete expanded preparation estimates, including every cleanup drop. Choose cleanup only when it is no more expensive, preferring it on ties; retain the original on failure. Do not credit possible future drops: a trap may prevent them from executing. Zero-cost preparations and alias-only publication remain unchanged. This bounds immediate estimated preparation cost, not total VM cycles or MAST bytes, which also depend on later choices and block packing. The recorded actions remain the sole physical authority; final edge cleanup is unchanged.

Preparation diagnostics record the actual selected construction. `Stack`, `Loads`, `Selective`, and `Normalization` identify the four scheduling tiers. `Aliases` identifies publication with unchanged physical state, and edge `Transfer` is reported separately; neither may inherit a default normalization label. Each committed unit or control preparation records its allocation-action count, expanded cost estimate, spill loads, and spill stores. Edge transfers have their own totals. These observations do not grant verification authority, change scheduling decisions, or imply runtime execution frequency.

The old `Move`/`Copy` distinction becomes a scheduling choice derived from recoverability. A live-after input need not be duplicated if consuming it leaves a valid spill slot. Multiple uses in one input tuple require distinct physical occurrences, even when they denote one atom.

Allocation chooses operand sources, preservation, loads, and evictions. The existing bounded shuffle solver remains an optimization for resident copies and permutations, with widths and duplicate handling checked against raw lanes. It does not choose spill slots or perform eviction. Its `NoSolution` selects another construction, not a diagnostic or an immediate full drain. It cannot rename different atoms as equivalent to manufacture a solution.

Individual shuffle tactics must declare their feasibility limits before entering preparation loops. In particular, `LinearStackWindow` applies only when the required raw operand width fits sixteen lanes: its copy preparation reserves addressable space for the complete tuple. Wider tuples may still use other valid proposals or full normalization. A solver budget charged between tactic invocations cannot bound a nonterminating loop inside a tactic.

Word swaps are explicit physical actions. `SwapWord { word }` exchanges the top four lanes with lanes `4*word..4*word+4` for word indices 1–3, preserving limb order, occurrence identity and all intervening/deeper lanes. Both words must be owned and addressable; caller suffixes and floor padding cannot participate. Expansion emits exactly the planned word instruction, and replay independently checks correspondence and ownership.

Grouped swaps use this action for complete aligned four-lane endpoints; an adjacent move of two four-lane groups is the same permutation. Stack scheduling also tries each of the three single-word swaps from the unchanged incoming trial, allowing mixed/partial layouts that grouped search cannot model. Every candidate must satisfy the existing ordered-prefix and survivor checks. Compare expanded VM-operation costs, retaining the grouped candidate on ties. This is a bounded addition to the existing tier, not a broader search or a post-expansion rewrite. Word duplication is not needed: its four VM operations do not improve upon four aligned scalar copies.

The current solver models grouped operands. Use it only when the candidate can be represented faithfully in that model, including aliases and copies. A split raw layout can bypass it: tiers one and two may decline, while tier three has a deterministic raw-lane permutation. The allocator's raw representation must not inherit a whole-value grouping restriction merely to satisfy the optional solver.

### Source-authorized operand order

`UnitEvidence.input_symmetry` is reconstructed from the source operation, operand types and overflow mode, then sealed by selection. Its default is `InputSymmetry::Ordered`. `CommutativePair { lanes_per_operand }` permits exchanging exactly two complete, equally typed operands while preserving each operand's limb order. Selection checks positive width, complete canonical lane groups, exactly their semantic inputs, and ordinary stack result publication. Repeated operands are legal. This permission changes physical consumption order; it does not rename source values, change liveness, or authorize arbitrary lane permutations.

The arithmetic selector grants this permission to the supported Add/Mul forms, including checked and overflowing integer variants, bitwise and boolean operations, equality/inequality, and supported Min/Max forms. The backend can lower pointer equality, but current HIR Eq/Neq require integral operands; this optimization preserves that source restriction and makes no pointer-source success claim. Subtraction, ordered comparisons, shifts and other noncommutative operations remain ordered. Unsupported type/operation combinations remain unsupported. AssertEq and extension-field groups retain their existing ordered contracts. The arithmetic semantics and commutativity of this explicit source whitelist remain trusted lowering knowledge, validated through VM tests; physical replay does not prove algebraic equivalence.

For a permitted pair, prepare canonical and reversed input views independently from the same incoming state and allocation counters. Each view follows the existing scheduling tiers and validates its own complete prefix and preservation obligations. Compare the expanded costs of the complete successful preparations; canonical wins ties. A failed trial contributes no actions, storage requests or identities to the other. If only one order succeeds, use it; if neither succeeds, report the canonical failure. This same preparation routine supplies the first-unit cost during entry-contract selection.

`LoweringBindings.input_order` records the winning order, even when preparation emits no instructions, and survives final frame resolution. The canonical selected input tuple stays immutable. Expansion checks that the sealed source permits the recorded order. Independent replay derives the required prefix from the sealed permission and checks actual incoming tokens before executing the lowering. Neither an allocator-supplied tuple nor a successful allocator simulation establishes those identities.

Exact helper-call obligations referring to canonical source inputs are projected through the same complete-operand exchange. Apply the map to referenced atoms, not to the helper argument vector: wide bitwise lowerings can call helpers with interleaved source lanes. Literal and unrelated atom obligations, helper targets, paths and occurrences, ABI rules, and result publications retain their original checks. Shared code defines only the mathematical permutation; source authorization, allocation simulation and verifier token replay remain separate responsibilities.

Acceptance includes both physical orders, zero-action reversal, repeated operands, complete wide limbs and flags, checked traps, live original names used by subsequent noncommutative operations, normalization, canonical ties, first-unit join costs and exact multi-helper projections. Malformed group permissions, unauthorized reversal, wrong physical prefixes and forged helper arguments must reject independently of numerical VM tests.

### Scheduling tiers

Try these tiers in order:

1. **Stack-only scheduling.** Reuse existing occurrences through legal copies and permutations. A checked successful schedule is accepted even when total owned depth exceeds sixteen.
2. **Loads and shuffles without eviction.** Materialize spill-slot or literal operands and schedule the resulting stack, optionally gathering resident sources before the loads. Existing unrelated occurrences remain on the stack.
3. **Bounded preparation with selective eviction.** Reserve an active prefix, evict only enough occurrences to fit the prepared operands and retained active values, then materialize and permute them. Deeper occurrences remain untouched.
4. **Full normalization.** Drain and reconstruct the known owned prefix using the deterministic transition below.

Being in a spill slot is an ordinary operand source, not a reason to select tier four. Loading a slot leaves its contents valid. A value may remain represented both on the stack and in its spill slot; dropping its stack occurrence does not invalidate the slot. These facts survive within a segment until a write or semantic rebinding invalidates them. Contract boundaries retain their existing explicit-fact rules.

### Loads and shuffles without eviction

Tier two tries two fixed candidates on independent state copies. In the first, materialize one source for each distinct input atom absent from the addressable prefix, then invoke the bounded shuffle solver. In the second, first gather one occurrence of each required resident atom to the top using only moves, then materialize the absent sources and invoke the solver. Gather sources and materialize absent atoms in reverse order of their first appearance in the input tuple, with stable occurrence selection. Gathering introduces no copies and no height increase. Each absent source must have a valid spill slot or permitted reconstruction. The solver supplies remaining multiplicity and live-after copies, subject to actual access checks.

Each candidate uses a fixed solver budget. Choose the successful candidate with the smaller estimated expanded instruction count, including its loads and gathering moves, breaking ties in favor of the first. This is a scheduling cost estimate, not a correctness condition. Neither candidate evicts occurrences. A failed access, unsatisfied preservation obligation, or exhausted solver causes that trial to be discarded. There is no unbounded retry or search over load orders.

For example, with top-first notation and last-use inputs `a` and `b`:

```text
input stack: [x0, x1, ..., x14, a | deeper values]
required:    [a, b]                 b is in a valid spill slot

moveup.15
load_slot b
swap.1

prepared:    [a, b, x0, x1, ..., x14 | deeper values]
```

Loading first would bury `a` at depth sixteen. Gathering it first makes every access legal without spilling any `x` value. Total depth grows, which is permitted. If `a` is live afterward, a valid spill copy or an additional stack occurrence must preserve it; for this example, duplicating `a` at the top before loading `b` is also legal.

### Bounded preparation with selective eviction

Tier three operates on raw lanes. Let `A` be the first `min(owned_depth, 16)` owned occurrences and `S` the remaining owned suffix. The protected caller tail is separate and unchanged. The boundary between `A` and `S` is fixed for this proposal: removing an occurrence from `A` does not recruit a newly exposed occurrence from `S`. A value layout may cross the boundary; its lanes retain their identities and prescribed operand order.

This tier applies to a stack-input tuple `I` of at most sixteen lanes when every input atom has a source in `A`, a valid spill slot, or a permitted reconstruction. An atom available only in `S` makes this tier inapplicable. A wider stack-input tuple also makes this tier inapplicable, without making the selected lowering unit illegal; other checked schedules and full normalization can establish legal wider prefixes. Stored-input requirements do not count toward `len(I)`, and expansion can construct a wider internal layout, as FPI does.

The algorithm is:

1. **Assign input occurrences.** For each input atom, assign existing occurrences in `A` to its requested positions, in stable depth order, up to the requested multiplicity. Pin those occurrences against eviction. Unassigned input positions will be loaded, reconstructed, or duplicated from a pinned source. Multiplicity counts raw physical occurrences, not distinct SSA values.
2. **Construct the retention set.** Initially retain every unassigned occurrence of `A` after the input tuple. For every live-after atom whose assigned input occurrences would be its last representation, add one planned survivor copy unless another retained occurrence, `S`, a valid spill slot, or a permitted reconstruction already preserves it. A planned copy is a reservation; it has not yet been pushed.
3. **Select evictions until the target fits.** Let `R` be the retained occurrences and planned survivor copies. While `len(I) + len(R) > 16`, select one member of `R` for removal and perform step four before selecting another. Prefer dead or redundant occurrences, then occurrences recoverable from a valid spill slot or literal, then occurrences requiring a new spill store. Within each class, prefer the farthest next use, then stable atom/occurrence IDs. Recompute recoverability after each choice so two individually redundant copies cannot both be discarded without preserving their value.
4. **Preserve and remove each selected victim.** For an actual active occurrence, issue `moveup` followed by a drop if another representation suffices, or a raw store if this is the last representation needed afterward. For a planned survivor copy, cancel the reservation; if necessary, establish a spill copy from its pinned input source first. The latter can use `dup.d; store_lane(slot)`, which leaves the source occurrence on the stack. This handles a sixteen-lane input tuple whose inputs are also live afterward: preservation moves to spill slots rather than requiring a seventeenth retained lane.
5. **Materialize missing occurrences.** After all removals and backing stores, fill unassigned input positions and surviving copy reservations in stable target order. Prefer duplicating an available active source, otherwise load its valid spill slot or reconstruct its literal. Each materialization increases active occupancy by one. All sources remain in the active prefix or memory throughout this phase.
6. **Permute to the target.** The exact target is `I` followed by `R`, preserving the original relative order of retained actual occurrences and placing planned survivor copies afterward in stable atom order. Ask the bounded solver to improve the permutation. If it declines, visit the target from bottom to top and `moveup` its corresponding, not-yet-placed occurrence. Assign distinct occurrence IDs to duplicate lanes. Already placed occurrences form the prepared prefix and are not selected again. Every move addresses a lane within the active prefix.

Victim redundancy and live-after recoverability are evaluated after the lowering unit consumes its assigned input occurrences. A pinned input is a materialization source, but is not by itself a surviving representation. For example, if one `a` is pinned as an input and another `a` is retained, evicting the retained copy must store it when `a` is live afterward and no other representation survives.

Evicting a live value can establish a spill copy that also preserves its input occurrences; re-evaluate planned survivor reservations accordingly. Canceling a now-unnecessary reservation requires no runtime drop. Once `len(I) + len(R) <= 16`, no further eviction is required by this construction. Optional dead-value cleanup remains a separate cost choice.

### Preparation invariants and progress

All tiers establish the exact input prefix and preserve every live-after atom. Tier three additionally maintains:

- **Fixed suffix.** `S` and the protected caller tail retain their identities and order. Slot loads, backing stores, and permutations never index into either sequence.
- **Source availability.** Every unmaterialized input has a pinned active source, a valid spill slot, or a permitted reconstruction. Eviction cannot remove a pinned source.
- **Preservation before destruction.** Canceling a survivor reservation or removing an occurrence cannot destroy the last representation needed after the lowering unit. A valid spill copy is sufficient even if the only stack occurrence is consumed.
- **Reserved capacity.** Before materialization, actual active occurrences plus all outstanding materializations equal the chosen target size, at most sixteen. Each completed materialization preserves that bound. Eviction and backing stores also begin and end with at most sixteen active occurrences.
- **Legal temporary accesses.** Raw address-generation and store/load temporaries may exceed sixteen total lanes. Their actual indexed accesses are checked in execution order; the capacity invariant is not imposed on intermediate primitive states or total stack depth.
- **Atomic proposal.** An incomplete proposal publishes neither actions nor location facts. Final independent verification checks the committed actions after physical slot assignment and expansion.

The input tuple itself fits sixteen, so removing retention reservations must eventually make the target fit, provided required backing storage fits the procedure frame. Each removal decreases the target size. The finite materialization list then fills that target, and the final permutation places one occurrence per step. Thus bounded preparation does not depend on shuffle-search success. An unavailable input source selects full normalization; a missing semantic value or actual frame exhaustion remains an invariant/resource diagnostic, not an infinite retry.

### Full normalization

The conservative transition can replace the entire known owned prefix. Its desired output is either a lowering unit input prefix or an exact destination contract. It never drains the protected tail.

```text
needed = source atoms required by the destination stack and slot obligations
         union all pending lowering unit stack inputs and stored-input atoms
         union atoms live after the transition

while the owned prefix is nonempty:
    a = its top atom
    if a is needed and has neither a valid spill slot nor a permitted literal reconstruction:
        store_top_lane(safe_spill_slot(a, needed))
    else:
        drop_top_lane()

establish required slot contents from preserved sources or permitted literals
    using simultaneous-copy rules for edge assignments

for a in reverse(desired_stack_top_first):
    push its literal, or load its valid spill slot

check required spill slots and exact desired prefix
```

Resolve edge requirements to source atoms before this transition. When normalization is used while establishing a lowering unit's stored inputs, its preservation set includes the entire pending stack-input tuple, even when those inputs have no use after the lowering unit. Literal reconstruction permits dropping an occurrence during the drain; it does not satisfy a stored-input requirement. Establishing that requirement still emits a literal push followed by a store. Preserve already established stored-input bindings throughout the transition.

When preparing an ordinary lowering unit, the conservative desired prefix contains only its stack inputs; live-through values remain in spill slots. When establishing an entry contract, it contains exactly the contract's resident atoms. This makes fallback progress independent of how many operands were buried and avoids repeatedly shuffling a growing prepared prefix.

Raw lane stores expand to `locaddr.k; mem_store`; raw lane loads expand to `locaddr.k; mem_load`. MASM `mem_store` consumes the original top lane after consuming its address. `locaddr` uses only top temporaries, so it can legally run while other values spill into the VM overflow table. These paths introduce no helper calls and no typed-value normalization.

The loop strictly decreases the number of owned occurrences. Reconstruction is a finite sequence of loads/pushes. Thus the transition terminates and uses no inaccessible indexed operation, provided all required atoms were recoverable initially, the destination lowering unit is valid, and the procedure frame fits. Missing input data is a compiler invariant error, not a reason to manufacture a value or retry indefinitely.

### Edge transfers and loop parameters

Transfers execute only on their selected control-flow edge. A conditional first establishes its predicate and common source state; its taken arm performs the corresponding repair after predicate consumption. A yield repairs the edge to the containing operation's continuation. A loop's initial edge and backedge use the same header contract.

Edge repair is incremental. An already satisfied contract emits no runtime actions. An already valid required spill slot emits no store. Changing semantic parameter names or retaining additional proved spill facts is not itself a reason to move values.

For each edge, construct its source-valued target by substituting all destination parameters with their incoming source atoms simultaneously. Keep that snapshot until every transfer is complete; do not relabel mutable state one parameter at a time. Alias claims are checked against this same snapshot. The target lists exact physical stack positions as well as required slot contents, so duplicate sources may still require multiple physical occurrences.

The repair planner proceeds as follows on a trial state:

1. Compare the source-valued target with actual stack occurrences and valid slot contents. Remove already satisfied slot assignments. If the exact owned stack, required slots, and alias obligations are all satisfied, finish with only checked semantic binding.
2. For remaining slot assignments, protect source tokens against destination overwrites. Existing occurrences or unaffected valid slots can supply protection. When an old slot is about to be overwritten, preserve any readers that still need its contents, including readers used to build the destination stack. Protection is established by legal copies or temporary spill slots before the destructive write.
3. Execute acyclic slot assignments whose destination is no longer needed as an old source. For a cycle, save one old destination lane in a temporary slot, redirect its readers, and continue. One raw-lane scratch cell breaks a scalar assignment cycle; additional source protection may be needed for stack outputs. A required slot can be filled from an accessible stack occurrence, another slot, or permitted reconstruction. Repeated destination slots with incompatible required contents are invalid; repeated source atoms are allowed.
4. Establish the exact target stack with legal resident moves, slot loads, literal reconstruction, and selective removal of unwanted occurrences. Preserve still-needed sources before removing them. Use the same raw actions, bounded permutation, and access checks as operand preparation, but with the edge's exact target and pending-transfer obligations rather than an operation's consumed inputs. Full normalization remains the fallback when the bounded repair cannot expose or remove the necessary occurrences legally.
5. Check every target position, required slot, spanning-scratch fact, address fact, and alias against the original simultaneous binding. Then introduce the destination names together and retain exactly the contract's declared facts and aliases. Do not replan the predecessor body or revise a frozen destination contract.

The low-level transfer machinery is shared with operand preparation; its completion condition differs. Operand preparation may leave retained active occurrences and a deeper owned suffix below its inputs. Edge repair may retain those occurrences only if they are explicitly part of the destination contract. Otherwise they must be removed legally. A preserved caller tail is never part of that cleanup, and floor-generated bottom padding is governed separately.

Trial repair may preserve all endangered sources in temporary slots instead of using minimal cycle handling. This changes cost, not semantics. An unsuccessful incremental trial is discarded before full normalization starts from the original incoming state. Required slots, temporary snapshots, and stack sources all participate in spill-slot liveness and final physical verification.

At a backedge, this process kills the old header parameter versions before publishing the new versions. For example, `(a, b) <- (b, a)` cannot be implemented by storing `b` over `a` and subsequently loading that overwritten cell for `b`.

The same freshness rule applies to every semantic definition executed inside a loop: locations and aliases recorded for an earlier dynamic instance of its static result ID cannot establish the new result. At a checked edge binding, substitute the source tokens simultaneously into the destination contract and discard all other facts. Static `ValueId` equality alone never proves current spill slot contents. Inherited entries explicitly carry forward their proved facts in the new contract; they do not bypass this check.

### Procedure frame layout

Spill slots are created lazily when a contract or action needs them. They are not allocated for every SSA definition up front. Complete the physical action graph, including edge repairs and temporary snapshots, using abstract lane spill slots before assigning physical offsets. Storage reuse is a separate deterministic pass; it does not reschedule actions or revise frozen contracts.

#### Stored-content lifetimes

Semantic liveness, stored-content liveness, and physical storage are distinct. A value can remain live on the stack after its last required reload, allowing that stored copy's cell to be reused. An abstract `SpillSlotId` identifies a storage request, not permanent contents or a dedicated physical cell. Repeated stores to the same abstract slot begin new content lifetimes.

Compute storage liveness backward over the completed action graph:

```text
                           reads        overwrites
LoadLane(s)                {s}          {}
StoreLane(s)               {}           {s}
RequireSlot(s, atom)       {s}          {}
RunLowering(bindings)      bound slots  {}

live_out(action) = union live_in(successor)
live_in(action)  = reads(action)
                   union (live_out(action) - overwrites(action))
```

`RequireSlot` denotes a pseudo-read at a contract boundary, not an emitted instruction. Every retained contract spill fact contributes such a read, including a cached copy of a resident value retained to avoid a later store. “Optional” means contract selection may omit that fact before successor planning. Once retained, it is a storage obligation; physical coloring cannot silently discard it while downstream actions rely on it.

A store kills the old contents' lifetime, while token verification separately establishes what the new contents are. Contract entry, semantic parameter rebinding, and result publication are not memory definitions. Explicit actions must establish required contents. Use a bitset predecessor worklist to a least fixed point; no path predicates, numerical lifetime intervals, or per-iteration slot IDs are required.

At a branch, union successor requirements. Slots stored and used only in different arms can share storage when their contents are not needed outside those arms. If both contents are established before the branch and needed by different successors, they coexist before the branch and interfere. Coloring does not move either store into an arm to improve reuse.

At a loop, future reads propagate through backedges. An invariant spill copy stays live around the cycle. A changing header slot's old contents are killed only by an actual overwrite, and the repaired edge must then satisfy the header's pseudo-read with the next iteration's token. Edge-copy snapshot temporaries use the same equations, including reads needed to construct stack outputs. The fixed point computes storage demand; it does not certify correct iteration identity.

#### Whole-unit reservations and scalar coloring

`RunLowering` reads every slot in its stored-input bindings and does not define spill-slot contents: bound stored inputs are read-only, and mutable temporaries use declared scratch. Reserve the bound contents throughout the entire unit, including internal branches, loops, and calls. Copies needed after the unit also remain live across it. A semantic input's consumption at entry does not end its storage reservation, and the last internal prefix load in FPI does not release its bound slot before the call completes. The declarations are available before physical coloring; allocation does not need internal instruction liveness.

Add interference between distinct simultaneously live spill slots, between each store destination and every other slot live after that store, and between distinct simultaneously required contract slots. Whole-unit reservations participate as simultaneous obligations throughout `RunLowering`. In particular:

```text
store A
store B       // B is never read
load A
```

`A` and `B` must interfere: a dead store can still destroy live contents. Distinct slots are not coalesced merely because their values happen to agree at one point. Requirements already bound to the same abstract slot naturally share it without an additional coalescing analysis.

Deterministic greedy coloring, ordered by descending interference degree then stable slot ID, assigns the first available color. Each color occupies one felt cell. Noninterfering slots may share a color across segments, arms, loop regions, and successive lowering units. No optimal coloring or slot splitting is required.

#### Scratch workspace

Sized scratch stays separate from scalar coloring. Each object retains its complete contiguous range and declared alignment; a four-lane digest accessed through one address requires four contiguous, word-aligned cells. Validate that descriptor alignments are positive powers of two in element units. Scratch descriptors distinguish two storage classes:

- **Unit-local scratch:** all accesses and address uses belong to one `RunLowering` expansion. Lay out that unit's objects in stable ID order with checked alignment, giving them nonoverlapping ranges and reserving every object for the whole unit. There is no internal-stage reuse analysis.
- **Scratch spanning units:** contents or addresses are needed across allocation boundaries, including initialization or address preparation outside the consuming expansion. Declare the selected lifetime and uses explicitly, and give each such object a dedicated aligned range, initially without reuse between these objects.

All unit-local layouts overlay one reusable workspace. Its alignment is the maximum required unit-local alignment; its size is the maximum per-unit layout size. With no unit-local objects, its size is zero and alignment is one. Each object address is the workspace base plus its unit's relative offset. Sequential units and units on different control-flow arms reuse the workspace without variable-sized interference coloring. Nested emission helpers within one unit contribute to that same unit's layout; they are not separate workspace owners.

A synchronous callee can borrow unit-local scratch when its registered call contract guarantees that it does not retain the pointer. The caller reserves the workspace through the call. If a pointer or contents must remain available after the unit, the object belongs to the spanning class. Nonretention establishes lifetime, not preservation of contents: borrowed ranges also need the memory-effect rules below. This uses the accepted caller-local ABI premise and explicit borrowing contracts, without adding adversarial-memory analysis.

Reusing a range does not establish initialization for its new object. Each unit invocation, including repeated execution inside a loop, must initialize local scratch before a required read. The verifier checks object ownership and definite initialization as well as actual cell contents; it cannot use a previous workspace owner's contents as an undeclared input. No runtime clearing is required. Spanning objects retain only facts established by their declared accesses and checked control-flow contracts.

#### Scratch dependencies across boundaries

Each spanning object has explicit selected content dependencies, reads, and writes. `ScratchCellFact` identifies an object and cell offset and requires either definite initialization or a particular selected content token. Exact identity is required when semantics depend on the contents, such as the procedure root read by `dyncall`; mere initialization is insufficient. `AddressFact` associates a selected pointer token with its object, accessible range, and offset. All facts are checked after final frame assignment, rather than supplied as unchecked annotations.

The selected program declares definitions and uses of these frame-content identities before allocation. A unit that stages a root establishes a checked relation between its input lanes and the object's cells; dispatch consumes that explicit storage dependency. These dependencies propagate backward over the selected graph to determine required boundary facts. They are separate from ordinary SSA operand liveness: a later scratch read does not force an unnecessary stack or scalar-spill copy, and scratch does not become an implicit source for arbitrary operand scheduling. An ordinary stack operand still needs the explicit materialization required by its selected lowering.

Entry contracts carry the required scratch contents and address provenance alongside their spill-slot facts. Forward entries, branch joins, loop backedges, and unit boundaries establish them using simultaneous source-side bindings before introducing destination names. At a join, retain only facts established on every incoming normal edge. At a loop, establish invariant and carried content relations on initialization and every backedge, without accepting a prior iteration merely because an object or value ID is unchanged. The verifier initializes each segment from these checked requirements; unlisted facts are unavailable.

Scratch objects have fixed identities and locations across their selected lifetime. Ordinary edge repair does not relocate them or invent content definitions. Required facts must follow from the registered lowering's actual writes, selected storage transfers, or preserved contents; otherwise verification fails. If branches produce different logical contents needed afterward, selection exposes the corresponding content parameter and simultaneous incoming bindings. Publication of a content definition is restricted to the registered unit's declared scratch outputs and checked physical writes; it cannot bless unrelated cells or bypass source correspondence.

Borrowed-range summaries declare read requirements and possible writes at object/range granularity. Read-only ranges preserve their tokens. A possible write invalidates prior identities; a cell is definitely initialized afterward only if it was initialized and all allowed effects preserve initialization, or an explicit definite-write summary establishes initialization. Definite outputs receive fresh tokens or the exact relation specified by the registered summary. Possible writes alone do not initialize previously uninitialized cells. Unknown or undeclared borrowed effects are rejected. Passing a pointer grants only the declared access, and the checker applies these effects before evaluating subsequent reads or boundary obligations.

#### Final layout and checking

The frame has this order, inserting alignment padding where needed:

```text
existing source locals
scalar spill cells, one per color
scratch objects spanning lowering units
reusable unit-local scratch workspace
final frame-alignment padding
```

Source locals keep their existing order and element offsets. Compiler scratch ranges are disjoint from source locals and scalar spill cells. The final layout drives `locaddr`, debug offsets, and the procedure declaration together. Use checked arithmetic for size, alignment, and range calculations; validate the complete frame before narrowing offsets to instruction operands. The assembler 0.35 bound is 65,532 elements, accounting for four-element alignment. Its constant is currently private; use a named backend bound derived from the target word size, with an assembly boundary test, unless the dependency exposes it. Do not substitute `u16::MAX`. A resource diagnostic reports source locals, scalar colors, spanning scratch, workspace, and padding contributions.

After physical assignment, the verifier tracks contents by actual frame offset. A write replaces the cell's token regardless of its abstract slot name; an old abstract fact cannot survive a write through another colored slot. It checks required contents at loads and boundaries, frame bounds and alignment, object ownership, storage separation, and loop/edge token identities. The checker need not reproduce the coloring heuristic: independently derived obligations and actual instruction replay determine whether reuse was safe. Diagnostics likewise use the final offsets and current verified contents rather than stale abstract slot facts.

No promise of optimal coloring or support beyond the frame limit is made. Scalar reuse and the unit-local workspace are part of the final implementation; this design does not require one permanent physical cell for every intermediate value or every unit-local scratch object.

## Calls and procedure boundaries

Call effects are selected from explicit ABI descriptors carrying both the target calling convention and its argument/result layout. Invocation-family compatibility is mandatory: `exec`/`dynexec` must target a same-context convention, and `call`/`dyncall` must target a context-call convention. Cross-family invocation is undefined behavior in either direction; it is not legalized by an empty protected tail, a matching arity, or an ownership-transfer annotation. Known mismatches are diagnosed. Indirect-call type metadata and checks include the required convention; unresolved external targets use the declared convention as an ABI obligation.

| Call form | Required behavior |
| --- | --- |
| `exec` / `dynexec` | Same-context ABI consumes declared arguments and produces declared results while preserving the entire suffix immediately below the arguments, including its addressable and overflow lanes, under the floor-padding rule |
| `exec_indirect` | Bounds/type/convention checks and index-to-address conversion precede `dynexec`; the address is consumed before callee argument use |
| `call` / `dyncall` | Caller-save ABI: all sixteen exposed lanes are clobbered except declared results on return; only the caller's overflow below that window is preserved/restored by the VM |
| `syscall` | Use the same caller-save sixteen-lane stack policy as `call`/`dyncall`, including zero-padding, nonresult clobbering, preserved caller overflow and result cleanup; retain its distinct kernel-target and root-memory semantics |
| FPI executor | Same-context `exec` with twenty-two inputs and sixteen outputs; six prefix lanes precede the fixed sixteen-lane payload |

Direct-call argument extensions are explicit selected conversions before dispatch. Validate the actual operand width against the resolved parameter width; equal types need no conversion. Zero-extension admits unsigned widening. Sign-extension preserves the source bit-pattern semantics of the existing lowering, including unsigned sources: an eight-bit pattern `255` sign-extended to sixteen bits becomes `0xffff` (signed −1), rather than numeric `255`. A signed source may be no wider than its signed destination; an unsigned source must be strictly narrower for a differing-type direct-call sign extension. Narrow results extend only through the destination width and retain canonical zeroes above that representation. Indirect calls retain their existing exact-type argument contract; extension attributes on equal types are no-ops.

MASM lifting expresses lossless I1/U8/U16/U32-to-Felt adaptations as explicit numeric `hir.cast` operations before final selection. Each preserves one raw lane and its input identity, without emitting an instruction. The shared cast implementation and selected conversion domain agree on all four source types; Felt's nominal bit width must not introduce an extra limb. Other unresolved adaptations are not implicitly accepted by this rule. General `hir.cast` evaluation remains an existing evaluator limitation; compiler and VM coverage does not imply evaluator support.

Canonical Wasm source signatures follow [canonical source classification and target call capacity](#canonical-source-signatures-and-target-call-capacity). Classify parameters by flattened core-value count, then append any wrapper-only result pointer. Separately check the actual target invocation against its felt payload budget after removing FPI metadata or the dynamic root and wrapper-only operands. A wide same-context wrapper is not itself a target payload overflow. Core signature validation precedes access to flattened argument slices.

Dynamic selector evidence has two closed forms. `FrameRoot` requires the exact assigned frame address and four current stored root lanes, retaining the existing provenance and stale-content checks. `CheckedTableSlot` requires the exact scalar address temporary produced by a registered function-table lookup stage, together with its source/stage identity and resolved table, signature, and convention obligations. Source checking reconstructs this rule from the actual indirect-call operation and resolved table/link layout; a candidate cannot authorize an arbitrary source pointer by attaching the declaration. Known incompatible table targets are diagnosed. The lookup stage must dominate dispatch, and source coverage rejects missing/reordered phases or undeclared intervening effects.

The independent physical verifier proves that dispatch consumes the checked lookup temporary's actual token, along with its argument, storage, and suffix obligations. Bounds checks, null/signature-tag checks, and index-to-slot arithmetic remain trusted lowering semantics, covered by VM execution tests; token verification does not prove arbitrary source-memory root contents or arithmetic correctness. The allocation-free lookup construction is shared with existing emission. An allocation boundary may separate lookup from dispatch: the address temporary and later arguments remain explicit semantic uses, and a stored address binding permits preparation of all sixteen argument lanes before the transient selector is loaded. Root staging across a boundary similarly declares spanning scratch content definitions and subsequent reads; no implicit liveness or storage dependency is permitted.

#### Caller-save context calls

The stack policy in this section applies to `call`, `dyncall`, and `syscall`. A syscall has a separate ABI descriptor: its target must be a kernel procedure and it executes in the root memory context. Sharing the caller-save stack policy does not give it ordinary call fresh-memory semantics or same-context `exec` suffix preservation.

Kernel entry classification requires the exact root `$kernel` module and an exported procedure. Currently both HIR `Public` and `Internal` emit as MASM `Public`, so both receive the Kernel ABI in that module. `Private` procedures retain their declared ordinary calling convention and cannot be syscall targets. When emission adopts MASM `pub(package)` for HIR `Internal`, classification must change with it: internal and private procedures must never receive the Kernel ABI or be targeted by `syscall`.

For `a` argument lanes and `r` result lanes, each at most sixteen, prepare and check this layout in top-first notation:

```text
before call: [arg0, ..., arg(a-1), zero repeated (16-a) | S]
after call:  [result0, ..., result(r-1), arbitrary (16-r) | S]
after cleanup: [result0, ..., result(r-1) | S]
```

`S` contains all retained caller-owned occurrences followed by the protected caller tail and any bottom padding. It starts below the entire exposed window at invocation. Inputs needed after the call must have another representation in `S` or valid spill slots. The caller cannot rely on a callee preserving any nonresult window lane, even if it received a zero or a duplicate argument there. Result identities are introduced only for the signature's result positions; other returned lanes have arbitrary contents with no caller-value identity.

The nonargument input positions must contain explicit zeros. This is an information-isolation obligation as well as stack preparation: no unrelated caller value, including a dead value, may be exposed as incidental padding. No callee inspection or trusted implementation permits omitting this preparation. The verifier checks actual zeros and that every retained caller occurrence lies below the window or in valid storage.

A `dyncall` address is additional transient state consumed before the VM exposes the sixteen-lane window. Check the window after that consumption, and check the staged root's exact contents and provenance. On normal return, remove the `16-r` unspecified lanes without destroying results or `S`; use repeated `drop` for `r = 0`, repeated `swap.1; drop` for `r = 1`, and repeated `movup.r; drop` for `2 <= r <= 15`; for `r = 16` no such cleanup is needed. Apply the ordinary floor-padding rules during cleanup. Do not assume those unspecified returned lanes are still zero.

#### Procedure entry and return

A same-context procedure begins with its argument prefix above an opaque protected suffix of unknown length and contents. Its verifier proves suffix preservation for every legal suffix, even when some of it is addressable. It may append only floor-induced bottom padding; it may not truncate overflow or treat unknown incoming lanes as owned scratch.

A context-call procedure sees exactly sixteen lanes, owns that exposed window, and has no access to the caller's hidden overflow. Only its argument positions are semantic inputs; the other positions are arbitrary disposable contents at procedure entry, even though compiler-generated callers zero them. It returns exactly sixteen lanes with declared results in the prescribed positions; callers assume nothing about the other returned contents.

#### Program root and executable entrypoints

A MASM program's `begin` block starts in a fresh root context. The VM supplies program inputs padded with zeros to exactly sixteen operand-stack elements. This root-entry guarantee is distinct from the calling convention of an ordinary procedure or an external context-call export. A valid HIR program entrypoint must have a convention compatible with `exec`; an incompatible target is rejected, not invoked with `call` or silently adapted.

The registered generated-main definition has this shape:

```text
begin entry: P = the VM-provided sixteen input/padding lanes
    root-context initialization
    optional test-harness setup
    exec selected program entrypoint
    exec truncate_stack
begin return: exactly sixteen lanes
```

The generated prelude is transparent to the operand stack: the selected entrypoint receives exactly `P`, in the original order and at the original depth. During initialization and harness setup, treat `P` as a protected input window; temporary values and helper arguments live above it. Check that each prelude stage preserves this window and that the prelude restores the exact entry stack before invoking the user procedure. The prelude's intended memory effects are not erased: initialization, harness writes, and entrypoint execution all occur in the same root context, with initialization performed exactly once as specified by the generated definition.

The user's entrypoint follows the ordinary same-context contract, preserving the suffix below its declared arguments. Its return may leave floor-induced padding below the meaningful state. Root main then invokes `truncate_stack` to preserve the top sixteen lanes and remove overflow before returning from `begin`. Do not require the entrypoint to predict or undo VM floor padding, or replace this cleanup with a proof that the call must finish at depth sixteen. This root cleanup is justified by ownership of the root context; it is not a permission for an ordinary `exec` callee to truncate its caller's suffix.

[Generated main](../../codegen/masm/src/artifact.rs) retains its `exec` invocation and existing root-memory sequencing. The implementation must validate the selected program entrypoint's convention and preserve prelude transparency rather than infer program semantics from canonical export lowering. Context-call exports and their return cleanup remain separate. Any explicitly selected adapter has its own convention and uses compatible invocations internally; no adapter or annotation authorizes a cross-family invocation of its target.

FPI selection uses one lowering unit whose six prefix values are required stored inputs and whose payload is the required stack prefix. Allocation establishes the stored inputs first, then prepares the payload. Expansion pads it to sixteen, loads the prefix in reverse order, and invokes the executor without an intervening allocation boundary. The resulting twenty-two-lane layout is legal because the executor consumes the prefix before needing the buried payload lanes. In the final HIR form, the six prefix values are ordinary semantic operands; frontend-created prefix locals and `Local2Reg` pinning cease to be allocation requirements. Protocol lookup, account switching, and executor semantics remain unchanged.

For indirect calls, replace the current `selector width + argument width <= 16` legalization policy with the actual selected call contract. The current supported callee argument ABI remains bounded at sixteen. An address/index or staged root may temporarily coexist with that payload. This does not authorize arbitrary wider callee signatures or implicit widening conversions while the address occupies stack top.

The existing fixed `dyncall` root scratch must be represented as a declared scratch object, disjoint from scalar spills and other simultaneously reserved objects. It uses the workspace only when all initialization and address uses are inside one lowering unit; otherwise it is scratch spanning units. Its current argument that scheduling emits no memory instructions no longer applies. Independently colored scalar spill slots cannot substitute for the root's required contiguous, aligned range.

External procedures and shared intrinsics require convention-specific ABI/effect summaries for their stack and control effects. Same-context summaries promise suffix preservation and cannot observe or depend on incidental bottom padding. Context-call summaries assume the entire exposed window is clobbered except declared results; caller overflow preservation comes from the VM, not trust in the callee. Helpers that inspect stack depth or intentionally truncate overflow have dedicated summaries and restricted invocation sites. Preservation of caller-local memory follows the existing ABI assumption described above; it does not require a new per-callee proof.

In particular, recognize `miden::core::sys::truncate_stack` by its resolved identity even when called through the [raw SDK import](../../sdk/stdlib-sys/src/raw/core.rs). It preserves the top sixteen and destroys all overflow. Its use is reserved for compiler-registered context-call or kernel return cleanup and generated program-root cleanup, where ownership of the discarded overflow is established. Authored direct and indirect calls receive `AbiMismatch`, including a final call before a source return: that position alone does not prove ownership of the overflow. A zero-argument source signature does not justify a suffix-preserving summary. Supporting authored cleanup would require a separately checked return-pattern contract.

## Lowering units and verification

### Lowering unit expansion

A lowering unit is an executable target lowering with a declared stack/storage input and output boundary. Its implementation may use existing arithmetic, pointer, memory, and intrinsic emission code after removing allocation decisions from that code. It emits explicit raw MASM instructions or structured sub-bodies. Stored reads refer to declared input IDs resolved through checked bindings; scratch accesses refer to declared objects. It cannot directly change the planner's value-location map or allocate an undisclosed spill slot.

The raw verifier has instruction-effect rules for the emitted MASM subset, including immediate expansion, word operations, conditional drops/swaps, calls, and internal control flow. Unmodeled instructions are rejected. Rules check actual accesses and preservation, not merely net stack delta. A word load that overwrites existing lanes must have explicit owned scratch/padding for those lanes.

Result identities can be introduced at normal lowering unit completion after checking the independently registered contract and raw shape. The semantic correctness of arithmetic and memory algorithms, their termination and trap conditions, and trusted external procedures remains in the trusted lowering base, exercised by VM/differential tests. The checker proves physical allocation and stack effects; a stack-correct expansion is not by itself proof of the operation's complete semantics.

### Fixed-stack intrinsics

Closed instruction contracts describe the actual raw accesses of fixed-stack intrinsics, including assembler-inserted padding and cleanup. `Hash` and `HMerge` consume four and eight lanes respectively and produce four lanes; `HPerm` replaces twelve lanes. Their internal temporary accesses and minimum-depth behavior are checked, while cryptographic output values and lane order require VM tests against the pinned cryptographic implementation. `UnpackWord` maps exactly `array<Felt, 4>` to four Felt results with identical lane tokens and no executable instruction.

Source `Caller` has no operands and produces a four-lane word. Raw MASM `Caller` overwrites the existing top four lanes; it does not push a word. Its lowering therefore emits `PadW; Caller`, establishing owned output space before the overwrite and preserving the entire incoming suffix. The raw target rule remains a four-lane overwrite. The result is ZERO outside a syscall and the actual caller hash inside one, as defined by the pinned VM; no extra context-dependent trap is introduced.

`AdvicePop` consumes one value from the actual advice provider and pushes one Felt, trapping on exhaustion without publishing a result. `Clk` pushes the actual VM clock value; its contract does not promise a compiler-independent instruction count. These closed operations retain source execution order. Their target rules grant no arbitrary host-event observation or source/compiler-memory access.

`AdviceLoadWord` replaces exactly four raw operand-stack slots with one advice word. Each of its four source operands must have a nonempty one-lane representation; mixed types, pointers and packed values remain valid within that domain. Wide, zero-sized and unrepresented operands are diagnosed. Source verification obtains the raw representation before querying size, and final selection independently checks the source tuple and exactly four Felt results. The shared emitter validates the complete input window before changing its typed stack or emitted body, so a rejected direct invocation leaves that helper's state unchanged.

Raw `AdvLoadW` is a trapping four-lane overwrite, with no physical pop/push floor steps or operand-memory effect. It consumes exactly one advice word and preserves the entire suffix. Fewer than four advice elements trap without successful result publication; the contract does not promise advice-provider rollback. Actual advice order and consumption require VM checks in addition to physical verification.

Fixed intrinsics use the same physical-verification boundary as other trusted lowerings. There is no exact instruction-sequence contract or generic output-freshness requirement. Removing or replacing a native instruction can leave a physically valid expansion while changing results, advice consumption or traps. Such semantic mutations are tested by VM execution against independent expected values and provider behavior; physical mutation tests require rejection only when an actual checked binding, access, preservation or publication relation is violated.

Merkle intrinsics require exact Felt tuples. In top-first order, `MTreeGet` maps `[depth,index,root]` to `[old_node,root]`, with the four root lanes retaining their input identities. `MTreeSet` maps `[depth,index,old_root,new_node]` to `[old_node,new_root]`; these eight result lanes are computed, including when a particular no-op update happens to preserve values. `MTreeMerge` maps two ordered root words to one computed root word. `MTreeVerify` preserves all ten lanes of `[node,depth,index,root]` and retains the source diagnostic. Its identity results do not remove its semantic assertion; missing assertions are detected by invalid-proof VM tests.

All four Merkle operations may trap. Their closed target rules account for actual assembler temporaries, including embedded event-ID pushes/drops and `MTreeSet`'s transient eighteen owned lanes reached through legal indexed accesses. Reserved Merkle events are handled by the VM using the declared input window; they grant no arbitrary host-event or operand-memory permission. Get and Set temporarily require advice-stack capacity for a word despite preserving preexisting advice on success. Set preserves the old tree while adding the updated path; Merge accepts arbitrary child values without requiring prior tree roots and may fail the provider's storage budget. Preserve existing lookup, range, diagnostic and update ordering without promising rollback on traps. Provider semantics, root computation and word order require actual VM tests; exact retained-input relations and physical suffix preservation remain independently verified.

`HornerBase` and `HornerExt` each require sixteen Felt inputs and results. They preserve lanes zero through thirteen and compute the two extension-accumulator lanes at positions fourteen and fifteen. The only operand-memory access is a native word read through lane thirteen; the word contains `[alpha_low,alpha_high,0,0]`. Their contracts permit source reads, retain normal compiler-storage provenance checks, and model no stack-height change. Address, alignment and evaluation-word validation can trap. Polynomial evaluation and accumulator order remain semantic VM-test obligations.

`MemStream` requires thirteen Felt inputs and results. It reads eight consecutive native elements through lane twelve, replaces lanes zero through seven with those elements, preserves lanes eight through eleven, and returns the original pointer plus eight in lane twelve. The closed read contract covers all eight cells, retains compiler-storage provenance and range checks, may trap, and has no stack-height change. Runtime alignment and numeric address validity remain VM preconditions; a physically safe misaligned read can verify and then trap. Both word reads precede operand output updates. A successful final range beginning at `0xffff_fff8` returns the Felt value `2^32`; a subsequent read at that value traps. VM tests check word order, pointer advancement and runtime traps rather than requiring a new affine-pointer or alignment proof.

`AdvicePipe` requires thirteen Felt inputs and results. It consumes two advice words, writes their eight elements consecutively through input twelve, returns those same elements at outputs zero through seven, retains lanes eight through eleven, and returns the pointer plus eight. Its closed contract has one eight-cell write from `Output(0)` through `Output(7)`, source WRITE permission, may-trap, and no height change. Advice exhaustion with fewer than eight elements consumes none. Once a complete double word is obtained, a later memory failure may consume that advice and may leave the first word written; operand outputs are updated only after both writes succeed.

`CryptoStream` requires fourteen Felt inputs and results. It reads eight plaintext cells through input twelve, adds inputs zero through seven lane-wise in the field, writes the eight resulting ciphertext elements through input thirteen, and returns those same eight values. It retains lanes eight through eleven and advances both pointers by eight. Its contract combines an eight-cell read and an eight-cell write from `Output(0)` through `Output(7)`, source READ_WRITE permission, may-trap, and no height change. The VM checks both complete numeric ranges and rejects exact or partial overlap before word accesses; adjacent ranges are allowed. Subsequent alignment or resource failures can trap, and a later write failure may leave the first word committed. Outputs publish only on success.

Both operations keep Felt native-address operands and field pointer arithmetic. Their ordinary units declare no compiler scratch or stored inputs, and source-memory permission does not authorize private compiler storage. Numerical results, pointer advancement, advice order and failure-side effects require VM oracles; the physical checker establishes stored/output identity and memory authority using the primitive contract above.

`EvalCircuit` requires three Felt inputs and results, in top-first order `[ptr,num_read,num_eval]`. All three results retain their input identities and the complete suffix is preserved. Its unit has source READ permission, may trap, and has no write, scratch, stored input or stack-height change. The closed counted-read footprint covers `2*num_read + num_eval` consecutive native cells beginning at `ptr`; it produces no memory-valued output. The VM first rejects a count sum above 32768, then requires positive even `num_read` and positive `num_eval` divisible by four, before any memory access. The maximum valid footprint is 65532 cells. Counts remain runtime Felt operands; selection does not require constants or introduce a narrower pointer type.

The VM reads the quadratic-extension inputs as words and then decodes/evaluates consecutive scalar gate encodings, requiring the final circuit value to be zero. An invalid encoding or unresolved wire can trap after a read prefix. Physical verification checks the full possible successful footprint under the source/compiler rules below, rather than evaluating the circuit or changing runtime validation order. The VM checks the supplied encoding; callers relying on a particular circuit remain responsible for authenticating it. Numerical evaluation, access failures and the traced execution limit of 32 circuit witnesses remain trusted VM behavior with execution tests; the witness limit is not imposed on ordinary untraced execution.

`FriExt2Fold4` requires seventeen Felt inputs and sixteen results. The seventeenth input is an explicit owned operand, moved into output fifteen by the native instruction's single physical pop; it is never borrowed from the protected suffix or floor padding. Output eleven retains input eight. All other outputs are computed, with no operand-memory effect. The three occurrences of the updated pointer may carry distinct opaque verifier tokens: their numerical equality is part of the trusted native algorithm and VM tests, and no physical storage or alias obligation depends on unifying them. Full normalization can prepare the seventeen-lane tuple by top-of-stack reloads without indexed access beyond fifteen. Invalid cosets, zero domain powers and inconsistent previous-fold values trap before the stack transition.

`LogDeferred` requires twelve Felt inputs and results. It reads the statement word at lanes four through seven and the VM's previous deferred root, computes the twelve-lane permutation state for their AND node, and advances the deferred state. It does not observe additional operand-stack lanes, operand memory, advice or arbitrary host handlers. Its closed rule produces twelve computed lanes, may trap, and has no stack-height change. Preserve the VM's registered/true-statement prerequisites and state limits without promising rollback. VM tests compare all output lanes and the stored deferred root across successive invocations; physical verification continues to check the declared window and suffix, not prove the deferred-state algorithm.

### Fixed-window system events

A finite target-owned system-event descriptor binds the resolved VM event ID to its complete observed payload window. `SystemEvent` selection requires that exact ID, exactly N Felt operands and N Felt results, and an input/result identity relation for every lane. The payload operands are semantic SSA uses; incidental allocator stack contents cannot supply them. Non-Felt immediates, unknown IDs and mismatched tuples receive diagnostics. The closed fixed-window set is:

| VM variants | Payload lanes N | Non-stack behavior |
| --- | --- | --- |
| MapValueToStack, MapValueCountToStack, MapValueToStackN0/N4/N8, HasMapKey | 4 each | Query the advice map and push the specified values/count/padding/presence to advice |
| MerkleNodeToStack | 6 | Query the Merkle store and push the node |
| HdwordToMap, MerkleNodeMerge | 8 each | Insert the hashed payload into the advice map, or extend the Merkle store |
| HdwordToMapWithDomain | 9 | Insert the payload using its explicit hash domain |
| HqwordToMap | 16 | Hash and insert all four payload words |
| HpermToMap, DeferredRegister | 12 each | Insert the original permutation rate under the computed key, or register the stack-resident deferred node |
| DeferredEvaluate, DeferredEvaluateTag, DeferredEvaluatePayload | 4 each | Evaluate the deferred node and push the specified canonical components |
| Ext2Inv | 2 | Push extension-inverse advice |
| U32Clz, U32Ctz, U32Clo, U32Cto, ILog2 | 1 each | Push the corresponding numeric advice |

The pinned VM's `DeferredRegister` rejects framework `AND` nodes with `InvalidTag`: its Join path uses a constructor reserved for precompile tags. This lowering preserves that existing runtime behavior. An exact-ID regression records the rejection; successful canonicalization and empty-payload tests use a supported precompile predicate and built-in TRUE. The dependency defect remains a separate follow-up rather than a compiler-side repair or an exception to physical verification.

These 22 variants preserve the entire operand stack and do not read or write operand memory. Actual provider/deferred effects, error conditions and ordering remain the pinned VM semantics, without a rollback promise. Their definitions use the existing immediate-event emitter. Its actual expansion is `Push(ID); Emit; Drop`: at observation, payload lane i is at depth i+1. `HqwordToMap` therefore reads payload at depth sixteen while the ID is present; this VM-supported overflow observation does not impose a sixteen-total-lane source limit. Physical rules model the temporary height and preserve all payload and suffix identities.

Target effects carry `observed_inputs`, a list of `Observation::Value(input_index)` or `Observation::SourceRangeBounds` roles, empty by default. Each of the 22 non-memory system-event rules names the whole `0..N` payload with strict Value observations. Compiler frame-start/frame-end events retain their empty payload contract. Plans cannot author or weaken these target-owned roles.

Before an observed primitive transitions, independent replay checks the actual input tokens. A Value observation rejects any compiler-storage may-origin, including mixed or unresolved provenance and pointers dropped before normal completion. Source-local-only origins retain the existing ABI assumption. Unobserved suffix tokens remain protected. Existing selected-graph and internal-loop origin fixed points rerun these checks after newly discovered provenance; no separate origin solver is introduced.

#### Memory-backed system events

`MemToMap` and `DeferredRegisterData` each require six Felt operands and results, preserving all six actual input identities and the suffix under the same `Push(ID); Emit; Drop` expansion. Both declare source READ, may trap, and have no operand-memory writes, scratch or stored inputs. They extend the finite system-event descriptor to 24 variants without changing the HIR operation inventory.

`MemToMap` observes `[key0,key1,key2,key3,start,end]`: keys are strict Value observations; start/end have one paired SourceRangeBounds role tied to `MemoryRead::SourceRange { start_input:4, end_input:5 }`. The pair carries no duplicate indices or planner-supplied proof. Replay checks all strict observations before deriving private, ephemeral range evidence from the actual tokens:

- If neither bound has compiler may-origin, retain the existing ordinary/source-memory ABI and all known local address facts.
- If either has compiler may-origin, both must have exact offsets in the current verified frame and satisfy `0 <= start <= end <= source_extent`. Use checked subtraction for width. Missing exact facts reject even if both tokens are equal or a literal happens to equal a frame address.
- Exact compiler or mixed-origin addresses satisfying this spatial proof are permitted as bounds. This proves source containment, not source derivation. Every origin remains intact afterward.

The source extent is the declared whole source-local prefix, including existing holes under its ABI; compiler alignment padding, spills and scratch do not enlarge it. Empty `[L,L)` is permitted at extent L, including exact `[0,0)` with no source locals when an actual frame cell allows constructing the address. A truly zero-sized frame still cannot invent a Locaddr token. Empty pairs above L reject. This permission never relabels tokens, initializes cells or authorizes returning, source-storing, strictly observing, or passing a compiler pointer through an unauthorized call. Positive source fixtures consume/drop the endpoint before returning unrelated data. Each later use and fixed-point replay derives its own evidence; no permission persists in tokens or edge contracts.

The possible read is the whole native half-open `[start,end)` interval, independent of provider lookup, equality-scan short circuits or available budget. Canonical literal bounds outside U32, reversed literal endpoints, or known equality establish no read only after observation checks. Exact frame offsets are relative spatial facts, not absolute U32 literals. Possible nonempty reads require source READ and check every known local boundary; ordinary unresolved source ranges retain the ABI. Empty/no-read paths require no fictitious first-cell access or memory permission. The source-only resolver must not interpret an exclusive compiler-origin endpoint as a compiler cell read. No loaded-value tokens are needed. Both VM endpoints fit U32, so the largest possible width is U32_MAX and cell U32_MAX is never copied.

`DeferredRegisterData` observes `[tag0,tag1,tag2,tag3,ptr,n_chunks]` with six strict Value roles and the closed `MemoryRead::DeferredRegisterData` footprint. It has no bounds exception. The actual VM checks positive U32 count, tag decode/shape, `4+8*n <= 1<<20`, U32 pointer, four-cell pointer alignment and checked `ptr+8*n <= U32_MAX`, then reads `8*n` current-context cells (missing cells are zero) before constructing/registering/evaluating the node. Maximum count is 131071 and width is 1048568 cells. Unknown source counts retain the ABI; known source-local crossings reject. These direct host reads are scalar accesses, so the physical checker introduces no additional word-alignment restriction on relative frame offsets.

After all six strict observations pass, the closed footprint resolver recognizes exactly these universal pre-read failures from canonical literal facts:

| Established predicate | Footprint |
| --- | --- |
| Count is zero, exceeds U32, or exceeds 131071 | NoRead |
| Reserved tag ID is 0, regardless of arguments | NoRead: canonical TRUE and malformed reserved forms both fail before reads |
| Reserved tag ID is 1 or 2 with any known nonzero argument | NoRead: malformed framework tag |
| Tag ID is 1 and known count differs from 1 | NoRead: malformed tag or wrong AND count |
| Literal pointer exceeds U32 or is not divisible by four | NoRead |
| Known valid count and U32/aligned literal pointer have checked end above U32_MAX | NoRead |
| Otherwise, count is known valid | PossibleRead of `8*n` cells |
| Otherwise, count is unknown | PossibleRead with unresolved width under the source ABI |

No installed-registry, payload, remaining-budget or minimum-span deduction is added. A universal later pre-read predicate may establish NoRead while earlier fields are unknown, but replay does not choose the VM's first error or reorder guards. Relative frame offsets cannot stand in for absolute pointer literals or alignment facts. Canonical AND with count one remains PossibleRead: the pinned handler reads eight cells before the known framework-constructor `InvalidTag` failure. Wrong-count AND fails before reading. Preserve both behaviors and the dependency follow-up. NoRead exempts only footprint checking, not observation, aliases, traps, source effect occurrence or suffix preservation.

Validate all role/read references before indexing. Reject duplicate or overlapping roles, a pair without SourceRange, SourceRange without exactly one pair, identical start/end indices, operand-memory writes or Memory-valued outputs for a paired source range. Strict roles plus paired indices partition the consumed payload exactly once. Equal values at distinct indices are valid. Target-table tests establish the exact key/bound assignment; generic structural validation cannot infer handler semantics.

The finite target rule's completeness and the handler's adherence to its observation window remain trusted and VM-tested. The bounds exception additionally trusts the closed MemToMap handler not to retain either bound's bit pattern. Physical replay proves the declared role and source-interval obligations; it does not confine the host's ProcessorState API or interpret advice/deferred algorithms. A physically valid omitted event, wrong same-width ID or wrong payload computation needs semantic execution tests. Unchanged memory alone cannot prove that host reads did not occur: pinned handler ordering and concrete VM errors/provider outcomes support those acceptance checks.

This contract supplies no implicit payload dependencies for generic `EmitEvent`/`EmitEventImm`, even if their ID names a system event. Their source contract and host-specific `TraceEvent` remain separate required design decisions.

#### Generic events observe generated physical state

`EmitEvent(id) -> id` and `EmitEventImm(ID) -> ()` keep their existing HIR interfaces. They expose the actual generated processor state to VM event dispatch: addressable stack, overflow, source and compiler memory, contexts, clock and other state available through the read-only processor API. No source-MASM snapshot, implicit payload uses, stable stack positions or identical observations across allocation choices are promised. Existing conservative HIR effect barriers retain event occurrence and effect order without freezing pure computations.

The backend does not interpret the ID to reconstruct a known operation or duplicate a handler's algorithm. Both generic forms use the same contract for any Felt ID. The VM owns dispatch, provider/deferred effects, host effects and errors. Dedicated `SystemEvent` and `PrintLn` operations retain their explicit operands and existing closed contracts. Frontend selection of the appropriate HIR operation is a separate responsibility; changing raw-event disassembly is outside this PR series.

Source-correspondence checking seals a private generic-event category into the checked unit evidence. Only the two registered generic source definitions can establish it. A candidate plan, numerical event ID, emitted instruction or ordinary source READ permission cannot grant this capability. Dynamic source evidence includes the exact selected ID atom; replay requires that token at `Emit` and preserves its result alias. Immediate source selection requires a Felt attribute and no operands/results, diagnosing malformed input before emission.

Replay resolves the matching generic source category before ordinary target lookup. Dynamic `Emit` preserves its one ID lane; immediate emission has no semantic input/output lanes and models the assembler's transient `Push(ID); Emit; Drop`. Both preserve the remaining operand stack and operand memory and may trap. Full-state observation is a distinct capability, not an unbounded memory footprint or a scan that demands initialized compiler cells. It leaves token identities, provenance, storage initialization and ownership unchanged. It grants no permission to subsequent loads, stores, returns, calls, or instructions from another source unit. Without matching source evidence, the existing strict target rules apply.

Handlers can export compiler data or addresses through advice. Later opaque advice values retain the existing source-memory ABI; there is no universal pointer-nonescape or confidentiality guarantee at generic events. Known compiler provenance still receives all existing checks. Actual operand-memory mutation remains outside the handler API and this contract.

Physical verification checks source authority, ID identity, aliases, floor transitions, suffix preservation and isolation of this capability. It does not prove handler algorithms, immediate numerical intent, or event occurrence inside an otherwise physically legal expansion. Actual VM/host tests check omission, duplication, wrong IDs, dispatch/errors, provider effects, ordering and generated-state observations. The pinned VM's preservation of operand stack and operand memory is a trusted, execution-tested target premise.

#### PrintLn contract and source-memory boundary

PrintLn uses the existing source-memory ABI for its decoded byte addresses. Both its pointer and length are strict Value observations: reject any compiler-storage may-origin, including mixed or unresolved provenance, before zero-length or invalid-input no-read shortcuts. Preserve original token identities and every origin; this operation has no SourceRangeBounds permission.

The current exact `FrameAddress(offset)` fact describes a native element address `frame_base + offset`. PrintLn interprets its raw pointer bits as a byte address and reads starting at `floor(pointer / 4)`. Neither the unchanged native offset nor `offset / 4` establishes that decoded address relative to the frame base. Transparent pointer/integer casts preserve bits, not an address-space conversion proof.

Where a valid byte-aware frame fact is unavailable, decoded source-only addresses remain unresolved source accesses under the existing ABI, even when the original token has an exact native FrameAddress. Possible reads require source READ permission. Canonical byte-pointer literals may establish absolute native addresses and footprint arithmetic, but do not establish frame-relative containment. The verifier does not claim independent containment of this decoded range within the source-local prefix, require constant runtime inputs, or invent a source frame large enough for the worst case. This boundary adds no scaled address identity or new provenance solver; compiler-origin observation restrictions remain strict. The closed target footprint is `MemoryRead::PrintLn { address, length }`, with two distinct raw input references. Validate both references and their strict Value observation roles before indexing; forbid operand-memory writes and Memory-valued outputs. The source operation requires exactly default byte `ptr<u8>` and U32 inputs and no results. Its unit has source READ and may-trap effects, with no scratch or stored inputs. Reuse ordinary immediate Emit: its primitive retains both inputs under `Push(ID); Emit; Drop`, after which the unit drops the two operands. Source occurrence and order remain required even when no result is produced.

The pinned debugger handler checks pointer U32, length conversion to host usize, then length at most 524288 bytes. For positive length it reads the current-context native interval starting at `pointer / 4`, of width `ceil((pointer % 4 + length) / 4)`. Each cell contributes its low 32 canonical bits as four little-endian bytes; select the requested byte slice and decode UTF-8. Zero length reads no cell and logs an empty string after the guards. A missing VM memory word or invalid UTF-8 produces a warning and continues execution, as do failed address/length/cap checks. An unwritten sibling of an allocated four-cell word is available as zero and is not a missing-word case. Successful output is an Info record at `stdout`; decode warnings are Warn records at `executor`. No operand-memory write or advice mutation occurs. Ordinary DefaultHost lacks this handler and may trap on dispatch, so decoder warnings do not justify removing the may-trap effect.

There is no byte-end-U32 guard. A valid starting byte pointer may read across that byte boundary after native conversion; the cap limits the largest footprint to 131073 native cells. After strict observations, canonical pointer above U32_MAX, canonical length above 524288, or zero length establishes NoRead. These shortcuts remove only footprint permission checking, not the event or its continuation/cleanup/observation obligations. Known valid literals determine absolute native footprint arithmetic; they never become frame offsets. Otherwise require source READ under the agreed ABI, retaining all original token/origin facts and any representable footprint facts. Do not invent a count-constancy restriction, relative-address proof, guard reorder or runtime check. Possible reads conservatively cover the whole successful span even when a concrete handler execution stops early at missing memory or invalid UTF-8.

Preserve the debugger's actual behavior in this conversion. The evaluator's differing UTF-8 errors, missing-cell zero defaults, strict U32 packed cells, lack of a PrintLn cap and smaller byte-address range remain a recorded follow-up; passing backend tests do not claim cross-evaluator equivalence. Handler adherence to the declared observation/read contract and logging semantics remains trusted and tested through the actual pinned Executor-installed handler, without copying its decoder. These host reads are not a new AIR memory-proof guarantee.

Tests isolate the process-global logger in a child invocation of one exact unit-test case, installing its collector before any fixture. The parent must verify nonzero matrix completion, not merely subprocess success; normal and release gates both execute that matrix. Capture actual output/warnings synchronously with execution and clear capture between cases, including unwind. Check handler memory/advice preservation at the event boundary so procedure prologues, epilogues and spill traffic do not contaminate the oracle; account for frame events separately. Preserve full stack/suffix and minimum-depth behavior, default/forced scheduling and live-through operands. Compiler-origin pointer or length must reject before every NoRead shortcut, including late graph/internal-loop origins. Physically valid event omission, wrong ID or wrong payload requires semantic execution evidence.

### Example: byte `memmove`

The existing [copy lowering](../../codegen/masm/src/emit/mem.rs) handles range checks, an alignment-dependent intrinsic fast path, and a directional scalar loop. For a byte-pointer `memmove`, select one lowering unit:

```text
semantic inputs: src, dst, count
stack entry:    [src, dst, count | S]
stored inputs: none
normal exit:   [S]
effects:       source-memory reads, destination-memory writes, may trap
```

`S` is the entire suffix beneath the three inputs, including any owned live values and the protected caller tail. The allocator preserves another representation of each input needed afterward before invoking the lowering unit. The lowering unit uses stack temporaries and registered intrinsic calls, without adding allocation boundaries or publishing intermediate HIR results.

Its internal stages retain the existing algorithm:

1. Check that the byte length and both end addresses fit the supported address space. On a normal path this leaves `[src, dst, count | S]`; failure traps.
2. Test whether `src`, `dst`, and `count` are divisible by four.
3. In the aligned arm, convert to element units and invoke `::intrinsics::mem::memmove_elements` with `[count / 4, src / 4, dst / 4 | S]`. Its registered physical contract consumes those three inputs and preserves `S`.
4. In the general arm, select descending traversal when `dst > src`, otherwise ascending traversal. Normalize both paths to the scalar-loop state below.
5. After the scalar loop, drop its five state lanes. Both normal arms finish at `[S]`; the source operation has no SSA results.

The scalar loop has explicit raw-state contracts:

```text
ascending initialization:  [0, src, dst, count, 1 | S]
descending initialization: [count - 1, src, dst, u32::MAX, u32::MAX | S]

body entry: [i, src, dst, end, step | S]
body exit:  [continue, i_next, src, dst, end, step | S]
```

The body computes the two current byte addresses, loads and stores a byte, advances `i` by wrapping addition with `step`, and tests against `end`. `while.true` consumes the condition. An initial condition handles the zero-count path without executing the body. The internal loop checker verifies initialization, one iteration, the backedge contract, and exit cleanup; it does not unroll a runtime count.

The verifier checks the lowering unit's source operand identities and order, every raw temporary access, unchanged loop-state lanes and suffix, branch agreement, intrinsic stack contracts, and complete cleanup. Branches start from the actual incoming state; loop bodies start from their inductively checked raw contracts. The current emitter's manually reconstructed typed stack and fresh typed stacks for branch emission are not accepted as evidence about the actual instruction effects.

The memory algorithm remains trusted: the sufficiency of range checks, correctness of pointer/unit conversions, overlap direction, byte extraction and insertion, termination, and the intrinsic's copying semantics require execution/differential coverage. The physical verifier need not reject a comparison changed in a way that selects the wrong copy direction while leaving every stack access legal. Source-memory operations follow the program's existing memory semantics; compiler spill-slot addressing and scratch separation remain checked allocation obligations.

### Typed memory fill and subword accesses

`MemSet` retains the backend's existing supported value domain: nonzero values of at most four bytes represented by one raw lane, including small aggregate, enum, and function values, plus the supported wider integer scalars. It repeats values at successive `size_in_bytes` positions in both byte and native element address spaces. Address calculations preserve the full native domain, using an element/offset pair or an exact wide byte position converted to that pair for each store; a native address must not be narrowed through a `u32` byte pointer. For supported value size `size`, count `count`, and raw pointer `ptr`, the complete range is:

```text
length = count * size
start  = Byte: ptr       Element: 4 * ptr
limit  = Byte: 0xffff_fffc
         Element: 2^34
require start <= limit && length <= limit - start
```

For sub-element values, this is a packed fill, not a loop of element-addressed typed stores. For example, filling four `u8` values at element address `E` fills the four bytes of cell `E`; four `Store` operations at element addresses `E` through `E + 3` update the low byte of four different cells. Byte-space `MemSet` rejects any fill touching the last four bytes because of its exclusive endpoint `0xffff_fffc`; `Store` and `MemCpy` do not inherit that restriction. These are existing operation-specific contracts, not a uniform pointer-stepping rule.

Evaluate these bounds without narrowing or arithmetic overflow, before any write and even when count is zero. The limit is an exclusive endpoint; the native one-past endpoint need not be materialized as an address. Range failures leave memory unchanged. The unit does not promise transactional memory effects.

Typed accesses touch only the cells containing the requested bytes. In particular, U16 loads and stores at offsets `0..=2` access one element; only offset `3` crosses into the next. An untouched full-width Felt neighbor is neither inspected nor changed, and a within-cell access at the final native element does not require `addr + 1`. Preserve existing offset validation, unaffected bits of the u32 view of a touched cell, and caller stack suffixes. A store never assumes that a touched cell holds a u32: a cell may still hold a felt at or above 2^32 from an earlier store (a reused stack slot), and every read-modify-write store keeps only the low 32 bits of such a cell before merging (`u32cast` after the load). Loads keep their byte-view requirement: a sub-word or unaligned load from such a cell traps, while an aligned whole-cell load returns the felt unchanged. These helper semantics require actual VM tests; generic token verification does not prove a trusted helper's memory footprint.

Packed subword stores write the declared bit footprint and preserve the bits outside it in the u32 view of the touched cells. The shared store owner uses the one-cell path when `8 * byte_offset + size_in_bits <= 32`; otherwise it merges through the two-cell window helpers (`load_rmw_window` to read, `store_sw` to write back). This includes 24-bit values and packed boolean-array widths such as 9, 17, and 25 bits. Immediate-address crossings use that same owner. Compile-time alignment checks must cover the full native address domain without narrowing through a `u32` byte pointer. Existing type-specific dispatch and input validation remain unchanged: the sub-32-bit mask helper retains its already-truncated-input precondition, while whole-cell stores are not given a new blanket 32-bit validation rule. Wrappers around Felt retain their existing storage behavior. This does not add large-aggregate support or an aggregate evaluator representation.

### Example: full-width FPI

The [FPI lowering unit](../../codegen/masm/src/lower/units/fpi.rs) pads the payload, loads six stored prefix atoms in reverse order, and emits the executor call. Semantic operands supply the prefix and payload independently of storage:

```text
semantic inputs:
    account_suffix, account_prefix, root0, root1, root2, root3,
    payload0, ..., payload15

stack entry:    [payload0, ..., payload15 | S]
stored inputs: account_suffix, account_prefix, root0, root1, root2, root3
normal exit:   [result0, ..., result15 | S]
invocation:    exec ::miden::protocol::tx::execute_foreign_procedure
call contract: 22 input lanes -> 16 output lanes, with preserved suffix
```

All twenty-two operands contribute semantic uses. The allocator first establishes valid slots for the six stored requirements, then prepares the sixteen payload occurrences while preserving those slots. A prefix atom may also occur in the payload; both physical requirements must hold even though they refer to one semantic value. Prefix values needed later can remain in their slots. Live-after payload values likewise require surviving occurrences or slots.

Expansion performs this uninterrupted sequence:

```text
[payload0, ..., payload15 | S]

load stored root3
load stored root2
load stored root1
load stored root0
load stored account_prefix
load stored account_suffix

[account_suffix, account_prefix, root0, root1, root2, root3,
 payload0, ..., payload15 | S]

exec execute_foreign_procedure

[result0, ..., result15 | S]
```

Each bound-slot load expands to raw address generation and a load at the top. The lowering unit never needs to index the buried payload during prefix construction. The protocol executor stages the six prefix lanes into its own locals before accessing the deepest payload lane. This explains the legal twenty-two-lane handoff; the executor's implementation is still covered by its trusted call summary rather than independently proved by the compiler's allocation verifier.

A full payload needs no padding. For fewer inputs, append zero lanes after the real payload and before loading the prefix, using the existing legal padding sequence. The [frontend wrapper conversion](../../frontend/wasm/src/component/lower_imports.rs) changes wrapper order, account prefix then suffix, into executor order, account suffix then prefix. Final HIR operands and the registered selection rule retain that semantic ordering without frontend-owned prefix locals.

The verifier checks all stored-input contents and bindings at entry, payload order and multiplicity, actual padding zeros, reverse prefix load order, every raw temporary access, the exact call-input layout and resolved executor identity, and normal-return result publication. It checks preservation of live-through representations and `S` using the registered executor effect summary. It rejects stale slots, swapped prefix lanes, an illegal indexed payload access, or premature result binding. Prefix staging creates no intermediate payload result and performs no second allocation inside the lowering unit.

Protocol behavior, authorization, account switching, foreign-procedure execution, and the executor's promised effects remain trusted. The allocator verifier also starts from final HIR; it does not prove that frontend wrapper conversion was semantically correct before reaching HIR. Integration tests must exercise that conversion as well as the call itself.

### Verification boundaries

Verification is mandatory in normal compilation and has two layers:

1. **Selected-program and allocation checking.** Check source correspondence, input identities, liveness obligations, edge bindings, location validity, frame offsets, and source provenance against final HIR or registered generated definitions and the selected lowering definitions.
2. **Expanded instruction checking.** Symbolically execute the actual expanded MASM body with raw lane and memory tokens, including temporary instructions, to establish the physical effects asserted by the plan.

The checker is separate from allocator transitions and solver tactics. Shared definitions specify the target and supported lowerings; independently computed verifier state establishes whether the emitted program satisfies them. The sharing boundary is specified below.

Source correspondence is checked by reconstructing selection from the current HIR or registered generated definition and comparing it with the candidate. This uses the same trusted selector implementation that produced the candidate: it detects candidate mutation and intervening HIR changes, but is not an independent proof of instruction selection. The physical verifier independently derives liveness and token facts and replays actual instructions. Replacing reselection with a captured snapshot would change which source state is authoritative and requires a separate design decision.

Registered lowering construction also supplies some call-site and trap metadata. Checking emitted code against that metadata establishes consistency with the declared contract; it does not independently prove the constructor's source semantics.

The selected-program checker requires exactly one semantic coverage record for each HIR operation/region, or for each required stage of the registered generated-procedure definition. A record names its ordered lowering units, stack and stored-input bindings, scratch content dependencies, temporaries, result publication, effects, and control alternatives. Aliases and debug operations may have no executable phase; phased calls may have several. The checker validates the record against the registered selection rule and source operands, or the generated definition and its parameters, rather than trusting provenance annotations. Each effectful unit required by the source coverage executes once per applicable selected path; phases execute in order and publish results only at the specified completion boundary. Internal stages such as a copy loop can perform many primitive memory effects while implementing that invocation; their complete algorithmic semantics remain in the trusted lowering base. The checker verifies the graph projection against the structured skeleton. Before sealing selection, each If and loop true/false alternative must also carry the corresponding True/False edge kind, so redundant labels cannot contradict executable alternatives or suppress replay of a taken repair. This catches omitted captures, missing edges, swapped bindings, or unsupported terminators before physical checking.

The verifier independently recomputes semantic liveness and required scratch content dependencies from this checked graph and registered phase uses/definitions, including stored operands. It verifies each stored-input binding against the selected source atom at entry, establishes required scratch contents and provenance, and checks every actual read through its final physical address. It does not accept the allocator's liveness tables, slot-validity claims, or recoverability claims as proof. Next-use scores and solver costs are irrelevant to verification.

### Symbolic state and token identities

For each segment, initialize symbolic state from its entry contract, execute its actions and lowering unit expansions, then execute each outgoing edge's actual repair. Entry contracts are proof hypotheses, not facts established by declaration. The actual function entry must satisfy the first contract from the ABI, and every incoming edge must establish its destination contract.

At an edge, compare each destination location against the corresponding source-side atom from the simultaneous binding. Only then introduce destination parameter names. This induction proves loop correctness without unrolling; a stale previous-iteration parameter cannot pass the source-side equality check.

The checker maintains:

- Ordered raw tokens for every owned stack occurrence, plus an opaque protected tail and the floor-padding abstraction.
- Actual contents of source-independent compiler frame cells, identified by final offsets.
- Unit-local scratch ownership, required spanning-object contents, and symbolic memory-address provenance for compiler storage.
- Current semantic definitions/aliases and required live values.
- Call-context state and whether the current exit requires exact depth sixteen.

Every read requires a valid current token. Every destructive action checks surviving live-value availability at the declared allocation boundary. A lowering unit is responsible for its own in-progress temporary lifetimes; the boundary checker does not demand that a consumed input survive after its specified last phase.

Semantic names and verifier tokens are distinct. The verifier owns the mapping from each current `Atom` to its token; allocator annotations cannot establish that mapping. A conceptual token representation is:

```text
Input(scope, atom)                   checked incoming value
Literal(felt)                       exact constant
Computed(scope, instruction, lane)  opaque primitive or summarized call result
Join(join, ordered_incoming_tokens) branch-dependent value
LoopParam(loop_scope, parameter)    arbitrary current carried value
```

Scope identifies a symbolic verification context, not a runtime iteration counter. At a definition, a fresh binding replaces the old semantic instance; static instruction or `ValueId` equality never identifies different dynamic instances. Constants can share canonical tokens, and checked aliases can associate several semantic names with one token. Equality of types, widths, or initial values is not otherwise proof of identity.

Duplication creates another occurrence of the same token; permutations move tokens without changing them. A store writes the actual source token to the addressed physical cell, invalidating its old contents. A load returns that cell's current token. Uninitialized or not definitely initialized storage is a separate unavailable state, never a wildcard token. Every compiler-storage load used by verified code requires an initialized cell and the identity required by the binding. A generic handler may observe absent or stale physical cells; that observation neither initializes storage nor establishes a usable value binding.

Arithmetic normally produces fresh opaque result tokens. Source-memory loads likewise produce fresh tokens unless a registered rule proves a stronger relation. The checker does not infer algebraic equalities or assume that two loads from the same source address are equal. Compiler-address operations have exact rules sufficient to resolve the final frame offset and accessed range; an unresolved compiler-storage address is rejected. These rules preserve the existing ABI premise for source memory and same-context calls rather than introducing whole-program alias analysis.

Exact source-local address offsets are retained where the arithmetic rules resolve them, including existing checks on known accesses and crossings. Otherwise, source-local-only arithmetic produces fresh opaque tokens carrying the conservative source-local origin. It does not require a new dynamic source-local bounds proof. Any compiler-storage origin, including a mixture with source-local origins, retains the strict compiler-address rules and cannot be erased or relabeled as source memory.

The existing typed operand stack, where an intermediate can carry only a type, is not the verifier state. The checker accounts for every raw lane and actual frame cell even when typed emission helpers omit or reconstruct their own bookkeeping.

Closed target observation roles are checked before the primitive transition and all no-read shortcuts. Value roles reject compiler may-origins; SourceRangeBounds derives the exact paired source-interval evidence specified above and is inseparable from its SourceRange footprint. Source READ permission cannot waive either check.

### Primitive memory replay

Closed target definitions attach the following memory contract to a native instruction:

```text
MemoryEffects { read: Option<MemoryRead>, write: Option<MemoryWrite> }
MemoryRead = Fixed(MemoryRegion)
           | Circuit { address_input, read_count_input, eval_count_input }
           | SourceRange { start_input, end_input }
           | DeferredRegisterData { tag_inputs[4], address_input, chunks_input }
           | PrintLn { address_input, length_input }
MemoryWrite { region: MemoryRegion, values: Vec<WriteValue> }
WriteValue = Input(index) | Output(index)
```

Each fixed region is one fixed-width contiguous interval of native cells. Circuit describes EvalCircuit's counted footprint; SourceRange, DeferredRegisterData and PrintLn describe the closed source-only event footprints above. These are not an arithmetic-expression language or a general access program. Writes remain fixed-width. Input and output indices refer to the instruction's raw lanes, not SSA publication or a new allocation boundary. A write owns its ordered data references, in increasing cell-address order; there is no detached parallel write-source list. Plans cannot supply these closed target rules. A default contract has no memory access.

Validate references before replay: every input, indirect-address and counted-read operand index is within the consumed prefix, every output reference is within the result vector, every fixed region has nonzero width, and write-source count equals write width. A memory-derived output requires a fixed read region and a lane within that region. Circuit, SourceRange, DeferredRegisterData and PrintLn reads are footprint-only: a Memory-valued output referring to one is invalid. In particular, a write cannot provide its own initial read or bootstrap a memory output without a read. Invalid metadata returns an error rather than indexing or panicking.

A counted extent resolves to a known width, an unresolved width, or a provable pre-memory trap. Interpret known counts as canonical Felt values and apply the VM's count guards before computing the footprint with checked wide arithmetic. Invalid counts must not wrap or narrow into a fictional read. Known spans use checked range-size arithmetic accommodating U32_MAX cells; fixed write widths and lane indices need not widen. Unknown counts do not by themselves reject a source operation.

Preserve the existing source-memory ABI assumption at this boundary:

| Address and extent evidence | Required check |
| --- | --- |
| Ordinary source address, known or runtime counts | Require source READ permission; no new count-constancy or pointer-type restriction |
| Exact source-local address and known extent | Retain its base/origin and check the complete range, rejecting known crossings into compiler storage |
| Source-local-only origin with unresolved extent or offset | Retain all known address facts and the source-local origin; require source READ under the existing ABI assumption, without a dynamic bounds proof or worst-case local allocation |
| Unresolved address with any compiler origin, including mixed origins | Reject; source permissions cannot erase or relabel compiler provenance |
| Exact compiler address with unresolved extent | Reject the unresolved compiler range |
| Exact compiler address and known extent | Require full declared ownership and definite initialization of every accessed cell |

A known invalid count can establish a pre-memory trap rather than an accessed interval. This does not exempt unrelated publication or ABI checks. The ordinary EvalCircuit unit declares no compiler storage, so its source-read permission alone cannot authorize a compiler read. The verifier does not add runtime guards or an up-front whole-range VM check; actual alignment, address failures and guard ordering remain unchanged.

Replay the normal-success transition in this order:

1. Check the complete read footprint and its source/compiler permissions, ownership and initialization, whether or not any read value is returned. For fixed reads, capture only the lanes requested by Memory-valued output expressions before any write; repeated references to a lane share its token. Compiler reads still check every accessed cell. Source reads create fresh tokens only for requested lanes. A footprint-only read produces no loaded-value vector or per-cell fresh source tokens; unresolved source extents use the ABI boundary above, while unresolved compiler extents reject before traversal.
2. Construct the output token vector once, using the existing input, memory, literal, computed and conditional rules. Retain conservative address provenance and the existing restrictions on compiler-address computation.
3. Independently check the complete destination range, including source write permission, read-only stored inputs, compiler ownership and source/compiler boundaries. Resolve every write source against the captured inputs or constructed outputs. Check compiler-address escapes on these actual tokens, including output-backed values.
4. Write those exact tokens to the known compiler cells, marking exactly those cells written and initialized. Preserve other cells and the captured read snapshot. Source-memory writes retain their existing abstraction and cannot initialize private compiler storage. Apply the existing checked stack/floor transition with the same output vector.

Consequently, a returned lane and its stored copy can have the same physical identity without proving the lane's numerical value. Existing scalar stores name inputs; retained word stores may name their corresponding outputs, preserving the established LE/BE cell order. The representation introduces no general affine-pointer exception or arbitrary access program. SourceRangeBounds is limited to its closed, nonretaining source-interval contract above.

This is a successful-path model, not an atomicity guarantee or a trace of every failing sub-access. Native advice consumption, runtime validation order and partial side effects on traps remain part of the trusted instruction behavior and require VM tests. No ordinary result is published on a trapping path.

### Internal branch merging

Execute each arm from a copy of the same actual incoming symbolic state after applying the branch instruction's predicate consumption. Inspect the emitted instructions, not a claimed arm summary. Check both arms conservatively unless a supported exact rule proves a path impossible. Nonreturning paths contribute no normal continuation state.

At the join, require compatible raw stack shape under the floor-padding model, exact protected suffix preservation, and all required storage facts. Merge normal incoming states in a fixed predecessor order:

1. Preserve a token unchanged when every incoming path has that token at the location.
2. For differing local values, construct a `Join` token keyed by the join and the complete ordered incoming-token vector. Intern the same vector across stack positions and initialized frame cells, preserving equalities established on every path.
3. A cell unavailable on any incoming path is not definitely initialized afterward. Do not turn an unavailable marker into a readable join token.
4. Check any required semantic identities against the resulting facts. A join token does not equal an arbitrary incoming token and cannot satisfy a preserved-value requirement merely by acquiring its name.

For example:

```text
then: [a, a | S]      scratch[k] = a
else: [b, b | S]      scratch[k] = b
join: [j, j | S]      scratch[k] = j

j = Join(this_join, [a, b])
```

The three locations remain equal because all have the same incoming-token vector. Neither `j == a` nor `j == b` follows. Taking the union of predecessor equalities would be unsound; allocating an unrelated token per merged location would lose proved equalities unnecessarily. An external spill-slot obligation for `a` would fail in this example. Stored-input slots remain read-only, regardless of what a subsequent merge could prove.

These joins describe internal data flow, not new allocation contracts or SSA result publication. At selected-program edges, the separately specified simultaneous source bindings and destination contracts still govern semantic renaming.

### Internal loop invariants

Each internal loop has a raw-state contract describing invariant lanes and cells, carried local parameters, and its preserved suffix. The registered lowering definition supplies the contract schema; instantiated annotations propose bindings that the verifier must check. A contract cannot generalize a protected suffix or required external value into an unconstrained local parameter.

Conceptually, an invariant position is `Exact(token)`, while a changing local position is `Carry(parameter)`. Repeated uses of one carried parameter assert equality between those positions. Separate parameters imply no equality, even if their initial values happen to match. Address provenance and storage initialization requirements are part of the contract where subsequent accesses need them.

Verification is inductive:

1. **Initialization.** Match the actual forward-entry state against the contract using simultaneous substitution for all carried parameters. Establish every required equality, initialized cell, invariant token, and suffix position.
2. **Iteration.** Instantiate carried parameters with fresh symbolic tokens, distinct except for explicitly asserted equalities. Execute one actual body from this arbitrary state, checking nested control flow recursively. Invariants retain their incoming tokens.
3. **Backedges.** Account for the loop instruction's consumed continuation predicate. Simultaneously match each resulting backedge state against the contract: invariant positions must still contain their exact tokens, and all occurrences of a carried parameter must agree on its next token. Every backedge must establish the contract, including any assumed aliases.
4. **Exits.** Derive the normal continuation from the proven contract with arbitrary current carried values, including the zero-iteration path. Explicit exiting paths must establish their exit contract. Do not expose the particular result of the one symbolically executed iteration as the final runtime result.

The contract accounts for every stack lane and every frame fact exposed at the normal loop continuation. Required external facts remain explicit invariants; scratch facts outside the invariant or carried obligations become unavailable. In particular, initialization observed only in the checked body does not establish initialization after a possible zero-iteration exit. Exit parameters are scoped to the summarized continuation, not identified with body-only computed tokens.

For the scalar `memmove` loop:

```text
body entry: [I, SRC, DST, END, STEP | S]
body exit:  [condition, NEXT, SRC, DST, END, STEP | S]
```

`I` is carried. `SRC`, `DST`, `END`, `STEP`, and `S` are invariant during the loop. `END` and `STEP` can be join tokens from direction selection and still be exact invariants afterward. The checker verifies the transport of `NEXT`, but not that its arithmetic advances in the intended direction, that the condition is correct, or that the loop terminates.

If the contract requires both `stack[0] = I` and `scratch[k] = I`, advancing only the stack to `NEXT` leaves the cell containing the old token and fails the backedge check. Equal initial contents alone cannot justify assuming this equality on subsequent iterations. Nested loop scopes and simultaneous rebinding prevent a token for a previous dynamic definition from being accepted merely because its static instruction or SSA name recurs.

These contracts bound verification without unrolling or accumulating expressions across iterations. They are proof boundaries within one uninterrupted lowering unit; they do not invoke allocation, perform repair, or publish intermediate semantic results. A failed invariant is a verification error, not permission to weaken an external preservation obligation.

### Result publication and trust

Only the registered lowering definition can authorize semantic result publication, at its declared normal completion boundary. For `Stack`, verify the actual input bindings, physical output prefix, storage permissions and contents, suffix preservation, and registered output relations; resolve output tokens from their declared physical result positions. For `Aliases`, resolve output tokens through the checked lane maps against incoming semantic names, independently prove source recoverability, and require unchanged physical state. Both forms bind every lane of each new semantic value simultaneously after these checks. Publication does not replace or globally rename tokens, so proved stack/storage equalities remain intact. Alias-producing lowerings must establish their specified input/result equality rather than introduce an unrelated result token.

Semantic result positions refer to the entire result tuple, not the addressable stack window. Call publications, borrowed call outputs, output equalities, and scratch/output relations use `u32` positions, matching ABI result counts. Verification converts positions to host indices with checked conversion and bounds checks. Same-context results may exceed sixteen or 256 lanes; only indexed stack instructions are restricted to depths zero through fifteen. Generated definitions allocate disjoint value-ID ranges using checked arithmetic derived from their actual result counts, and diagnose ID-domain exhaustion before narrowing. Generated root cleanup retains the top sixteen lanes after a wider entrypoint return.

For each declared address result, the verifier independently proves that the actual output token is the declared object's final frame address plus its element offset, with the declared accessible range within that object's assigned allocation. It checks this relation before publication, using tokens derived from actual instructions. Wrong literals, objects, offsets or ranges, missing address generation, and stale dynamic results cannot be certified by the declaration. The allocator and verifier share the immutable address-result definition and source contract, but not transfer implementations or conclusions about the emitted address.

Publication cannot relabel a preserved suffix lane, validate changed spill-slot contents, establish an internal invariant, or turn a branch join into an expected input identity. Trapping or nonreturning paths publish no normal results. Intermediate phases may define only their selected temporary results; later phases' inputs and final result publication remain governed by the checked selected program.

The registered rule asserting that these output lanes implement the HIR operation is a trusted semantic boundary. Generic token checking does not prove arithmetic, memory algorithms, termination, or trap conditions. Even a physically well-formed arithmetic lowering with incorrectly ordered result limbs can pass unless a registered output relation checks that order; semantic VM/differential tests cover that gap. This differs from allocation copying or permuting already identified lanes, where exact token order is checked. Trusted external call summaries likewise authorize only their declared effects and results.

Checking that a required effectful unit occurs in the selected graph and reaches its proper boundary does not prove that its trusted implementation performs the source effect. In particular, an opaque result may retain an incoming token when no registered relation forbids it. A removed `AdvLoadW` or assertion-only native can therefore escape physical checking if all physical obligations still hold. Semantic execution tests must detect the resulting incorrect values, missing provider effects or missing traps. Exact call-site checks remain necessary for their allocation-facing ABI, clobber, input and result obligations; they do not imply exact instruction correspondence for all lowerings.

### Shared definitions and independent implementation

The final implementation keeps the same scratch-declaration domain in production and test builds, including opaque content identities, possible writes and synchronous nonretaining borrows. Current HIR source rules do not construct every supported declaration. Exactly three variant-level, production-only expected dead-code lints document that fact for `ContentRequirement::Content` and `ScratchEffect::{PossibleWrite, Borrow}`; their allocation and verifier handlers remain active. This does not authorize a module-wide allowance, a test-only verifier domain or a fabricated producer. Fixture-only constructors and concrete VM oracles are compiled only for tests.


The implementation shares target specifications and general infrastructure, but independently computes the evidence used to accept the physical plan:

| Shared definitions or infrastructure | Independent verifier responsibilities |
| --- | --- |
| Immutable IDs, lane layouts, structured MASM syntax | Own token environment, raw stack, frame contents, and symbolic scopes |
| Declarative instruction effects and ABI definitions | Apply those rules to actual emitted instructions, including every temporary access |
| Trusted selector and source-registration rules, registered lowering definitions, physical requirements, and semantic summaries | Check source correspondence, instantiated input bindings, storage accesses, and allowed publication |
| Generic collections, bitsets, and worklist utilities | Recompute liveness from checked uses, definitions, and edges; do not reuse allocator liveness results |
| Final frame-layout data and source-location formats | Validate offsets and ranges, replay physical cell reads/writes, and detect invalid reuse |
| Diagnostic rendering | Branch merging, loop induction, edge substitution, and recoverability checks |

The verifier must not call the allocator's mutating stack simulation, slot-validity machinery, transfer planner, solver tactics, or contract-satisfaction predicate. It must not accept scheduler-produced equality, liveness, or recoverability conclusions as proof. Shared immutable selected-program data is checked against its declared HIR or registered generated origin before it supplies verification premises. Actual instruction and frame-cell execution remains necessary even when the abstract plan claims the correct effect.

Shared instruction-effect tables, lane-layout rules, ABI summaries, and registered semantic publication rules are an explicit trusted foundation. A wrong target rule can compromise both allocation and checking; independence from scheduling does not remove that risk. Keep these definitions small and declarative, and validate target stack, word-operation, frame-address, and call effects against VM execution. Copying the same table into two implementations is not itself independent evidence. Mutation tests separately establish that the checker rejects bad plans and expansions.

Dominance is derived from the checked graph's indexed predecessors in reverse postorder, with immediate-dominator tree intervals answering ancestry queries. This relation and the immutable definition index are computed once per graph verification, independently of allocator conclusions. Entry hypotheses name only independently required live values, parameters, outgoing binding sources, physical-contract facts, and their recursive source-alias dependencies. Origin maps retain only nonempty frame-origin facts; later branch and loop evidence still accumulates monotonically and triggers replay. These storage choices do not weaken edge binding, reconstruction, or provenance checks.

### Procedure exits

For context-call returns, the checker accepts only a normalization sequence or trusted helper contract establishing physical depth sixteen and the required result prefix. Program-root return uses the registered `truncate_stack` cleanup preserving the top sixteen lanes after the user entrypoint returns. For ordinary `exec` returns, it checks prefix/suffix preservation and permits only floor-induced bottom zeros. It does not confuse an abstractly empty owned stack with a physically empty VM stack.

### Finalization and optimizations

The complete order is:

```text
final HIR or registered generated-procedure definition
  -> selected lowering units and structured graph
  -> liveness, next use, and structured backedge classification
  -> one-pass body planning with inherited entries, delayed joins, and fixed loop headers
  -> completed contracts, incremental edge repairs, and abstract spill slots
  -> spill slot reuse and final frame layout
  -> lowering/action expansion to annotated MASM
  -> physical peephole optimization
  -> independent verification
  -> VerifiedProcedure
  -> MASM serialization and assembly
```

Peepholes run before final verification and preserve source/lowering boundaries or provide a checked mapping for moved annotations. They cannot erase the evidence needed to establish lowering unit inputs/outputs. No unchecked stack-changing rewrite follows `VerifiedProcedure`. Serialization performs no scheduling, result rebinding, or hidden local allocation.

Executable anchoring is explicit in annotated MASM and checked before `VerifiedProcedure`. A closed anchor form admits only canonical `nop` anchors and the existing inert debug decorators. Before verification, canonicalization places exactly one `nop` after every maximal consecutive group of variable-location decorators. Canonical `nop` anchors do not split groups: derive groups while ignoring those anchors and nonserialized proof boundaries. Ordinary selected instructions, including an ordinary `nop`, remain group barriers; inline-call and inline-clear decorators end a group, so its `nop` precedes any inline-chain change. Each structured body is handled independently. A body with no executable instruction, control operation, or group anchor receives the existing single empty-body `nop`; a group anchor already satisfies that requirement.

This uniform rule gives every variable row a concrete following operation, including at body endpoints, before calls and control flow, and beside instructions that assemble to no operations. It avoids duplicating assembler inlining or basic-block construction. A `nop` preserves stack and memory contents but adds a VM cycle and can change MAST batching and digests. This cost applies when variable-location metadata is emitted; debug-enabled and debug-disabled code are not required to have identical cycle counts or MAST.

The independent verifier derives the group-to-anchor relation from the final annotated body without using the optimizer’s grouping or placement decisions. It rejects missing, misplaced, duplicate, or otherwise unjustified canonical anchors, while checking their zero stack and memory effect without consuming or skipping allocation actions. A debug anchor still names and consumes the exact pending `DebugLocation` action ID, preserves its position, and forms an optimization barrier. Anchors cannot carry arbitrary instructions or effects, publish results, or manufacture or relocate source, unit, control, or edge evidence. Serialization emits only the already verified anchors; it does not insert new ones.

Initially, peepholes simplify adjacent instructions only within the same active lowering unit, including its internal structured bodies. They stop at every proof, control, and debug boundary. Allocation actions and selected synthetic-control instructions retain exact correspondence with the physical plan. No general annotation-remapping mechanism or optimization-driven plan rewriting is needed for this policy.

### Diagnostics

`-Z`/trace output should render one stable physical-plan format: source operation, incoming stack, valid spill slots, lowering unit requirements, chosen actions, outgoing state, and edge contract.

Selection and replay errors attach the source span and inline ancestry available at their boundary, preserving a more precise nested location. A replay failure names its selected step when one exists, and identity mismatches report the expected semantic lane and an actual identity only when replay has established it. Raw stack depth is reported only for a physical stack position: alias publication has semantic result positions but no required stack prefix. Opaque results and edge-only failures may therefore omit identity or step fields; diagnostics must not invent that context.

Distinguish `InvalidSelectedLowering`, `UnavailableValue`, `InvalidEdgeTransfer`, `IllegalStackAccess`, `InvalidSpillSlotRead`, `AbiMismatch`, and `FrameCapacityExceeded`. Solver search exhaustion is trace information, not a user-facing compilation failure. The same compact plan is suitable for minimized regression fixtures; a second allocation-specific debug subsystem is unnecessary.

## Effects on the middle tier and codegen

The final ordinary function pipeline retains semantic optimization and control-flow conversion:

```text
Canonicalizer -> CSE -> SCCP -> SinkOperandDefs -> Local2Reg
-> LiftControlFlowToSCF -> Canonicalizer
-> LegalizeForMasm -> optional SinkOperandDefs
-> unified backend selection, allocation, verification, emission
```

Operand sinking remains a cost optimization; it is no longer a prerequisite for stack safety. Both placements of `TransformSpills` disappear. `NarrowRegionOps` disappears. Physical legality does not require an HIR region's results or forwarded values to fit sixteen lanes. Source locals are still promoted where legal by `Local2Reg`; backend spill slots do not exist until after it has run.

Delete the MASM-specific spill analysis/materialization/reconstruction path and spill/reload pseudo-operations once their callers have moved. Preserve HIR liveness, SCF rebuilding helpers used by legitimate canonicalization, and independent dead-result elimination. Existing tests that pinned spill-created HIR are replaced with physical-plan expectations and execution tests for the same semantics.

Within codegen, move all location decisions out of `BlockEmitter`: block-entry cleanup, last-use copying/dropping, transparent casts, region result placement, branch reconciliation, loop backedge checks, and return cleanup. Keeping any of these as a second allocator would retain conflicting authorities.

Global initializers become one selected generated procedure with a finalized frame. Their existing sequencing and effects remain; spill slots can be reused between initializer fragments whose lifetimes do not overlap. The current default empty frame and zero-local initializer declaration must be replaced. Supporting functions, adapters, and entrypoint wrappers pass through the same physical planning boundary, even when they did not pass through the ordinary HIR rewrite pipeline.

Debug locations query the current allocation map. A live value can be described through accessible stack lanes or valid frame cells; multi-lane values retain lane order. If the existing debug-expression format cannot describe a split location, mark it unavailable rather than report a wrong location. The pinned MASM debug format and debugger encode local displacements as signed sixteen-bit integers. Compute each displacement from the final frame with checked arithmetic; if it cannot be represented, report the location as unavailable. This accepted debugger limitation does not reduce the legal executable frame size or introduce storage solely for debugging. Test the −32,768/−32,769 boundary and the maximum legal frame. Debug references do not force allocation or extend semantic liveness. Source local offsets, backend spill slot offsets, and executable addressing share the same finalized frame. Inline call-chain and source-span metadata follow the selected lowering unit and generated actions.

## Required observable behavior

The [acceptance document](unified-stack-allocator-acceptance.md) specifies every required observable behavior, regression case, and performance-assessment obligation. The [performance report](unified-stack-allocator-performance.md) records historical results, explanations, measurement limits and reproduction commands.

## Limits and alternatives

The design's constructive guarantee is conditional on supported representations, valid lowering/call contracts, and sufficient procedure-frame capacity. VM address-space limits and dynamic resource exhaustion remain real limits. A malformed lowering unit or uncontracted external MASM body is not repaired by moving more values to memory.

Expanding the current HIR narrowing/spill passes would continue distributing physical location decisions across SSA rewrites. Extending only the shuffle solver would leave storage, phased calls, frame allocation, and join agreement outside its authority. A globally optimal stack scheduler would add search complexity without improving the correctness argument. The selected design instead establishes one checked physical plan, with bounded heuristics improving a constructive legal transition.


### Independent graph analysis and debugger acceptance

Generic canonicalization preserves a region's entry interface. Entry arguments receive initial values from the enclosing operation as well as values from any explicit backedges; enumerating CFG predecessors does not describe all activations. Consequently, generic single-predecessor merging and argument substitution cannot erase the entry or replace its arguments based only on explicit edges. Return copying may still simplify a predecessor without erasing the entry. Skipping entry-argument elimination must not skip traversal into nested regions. CFG-to-SCF can then introduce a typed preheader for entry cycles without losing initial argument identities before lifting. Operation-specific interface rewrites remain responsible for updating the enclosing operation when they change region entry arguments.

SCF canonicalization distinguishes yielding values to an enclosing operation from terminating execution. Select conversion and redundant-yield folding require actual `scf.yield` terminators before inspecting yielded operands. Removing unused If or IndexSwitch results prunes only yield operands and preserves return/trap arms unchanged. Constant-switch folding declines when the selected arm does not yield; its ordinary result substitution cannot erase a return or trap. Inlining a terminal arm would additionally require correct continuation removal and is outside this transformation. These rules preserve existing source validity and do not extend backend support for mixed return and normal continuation paths.

Critical-edge splitting also preserves positional entry identity: a split block for an edge targeting the region entry is inserted after that entry, while other split blocks retain their existing placement before the target. It forwards the original edge operands without becoming a new initial activation path. This placement rule does not change which edges are classified as critical.

Conditional passthrough collapse uses exact incoming-edge counts. It bypasses a nonentry forwarding block only when that block has one incoming edge owned by the branch being rewritten and its final target has one incoming edge from the forwarding block. After replacing the branch, the rewrite removes the now-unused nonentry forwarding block through the rewriter before returning success. This removes its obsolete outgoing edge before critical-edge splitting can react to it. Shared or parallel-edge shapes are conservatively retained. Unconditional bypass keeps its existing eligibility and uses the same cleanup only for forwarding blocks that are actually unused and nonentry. Entry activation, ordered argument remapping and listener notifications remain intact; no new reachability analysis or critical-edge classification is introduced.

Within one graph verification, the checker computes its own dominance relation from the immutable checked selected graph once and reuses it when constructing entry hypotheses. This shares neither allocator facts nor dynamic token state. Padding, provenance, memory and token obligations are still replayed and discharged for every relevant transfer and fixed-point iteration.

Physical debug-location acceptance may supply a known source type for an explicitly authored fixture when the frontend does not emit its DWARF type. The test must still obtain the variable identity, location and frame state from actual compiled metadata and execution and validate each expected live value. Such evidence establishes location/frame correctness, not automatic source-type reconstruction. Complete DWARF source-type ingestion remains outside this allocator change.

## Appendix: historical integration states

Production code uses the adoption state below: every supported HIR and generated procedure passes through the unified allocator and mandatory physical verification. The first two states describe the earlier review boundaries; they do not provide alternate production paths. Local verification and CI landing evidence are recorded separately in the implementation plan.

### Physical contracts and independent verifier

The backend has immutable selected/physical contracts, lane layouts, explicit frame references and independent raw verification, exercised through hand-built plans and registered generated fixtures. Production compilation retains its existing path. This state does not claim to correct production ABI or allocation behavior.

### Allocation and integrated lowering infrastructure

The final allocator, frame assignment, expansion, annotated peepholes and verifier operate together on checked selected programs. Representative real lowerings and selected control-flow fixtures exercise this path directly. Production still retains its existing path; no adapter makes the legacy allocator produce temporary physical plans.

### Production adoption and removal of old allocation owners

Every supported HIR and generated procedure uses the unified pipeline. All lowering families have explicit contracts; frontend FPI exposes semantic operands; indirect legality follows actual ABI limits. Final emitted identities are resolved before selection, and packaging is inert. HIR spill insertion/materialization, obsolete pseudo-operations, hidden result publication and legacy stack management are deleted together. Generic semantic transformations and the optional movement solver remain where they have valid callers. Region narrowing is omitted on the implementation base, where it does not exist.

These three review boundaries have one production cutover. The first two states test the final infrastructure without a rollout switch, compatibility adapter or partially converted production backend. Mandatory production verification and ABI corrections arrive with the complete cutover.

