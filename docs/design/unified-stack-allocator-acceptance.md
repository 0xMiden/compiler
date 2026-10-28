# Unified stack allocator acceptance

These are the required observable behaviors for the [unified stack allocator design](unified-stack-allocator.md). The architecture, physical invariants, and verification trust boundary remain specified there.

Acceptance is defined by these behaviors rather than a particular spill count:

- Existing supported programs retain their values, side effects, trap behavior, and ABI behavior under VM/differential execution.
- Wide `if`, switch, and while interfaces compile without removing their HIR columns. Both live iteration/result groups may be independently wide, with distinct per-column values.
- A sixteen-value condition handoff can temporarily contain its predicate plus sixteen forwarded lanes when the actual access sequence is legal.
- A full sixteen-input FPI call constructs its twenty-two-lane executor layout and executes correctly.
- Repeated operands, mixed-width values, buried live operands, and dead buried occurrences produce valid schedules even when the bounded shuffle optimizer declines a request.
- Loop parameter permutations, duplicate bindings, zero-trip loops, nested captures, unreachable arms, and residual terminal branches preserve their edge semantics.
- Call-context tests include live caller values beneath a complete sixteen-lane argument window, clobbered nonresult lanes, same-context overflow preservation, and exact-depth context returns.
- Minimum-depth tests enter at physical depth sixteen, force drain/rebuild on only one branch, repeat the transition in a loop, and verify meaningful stack values, bounded bottom padding, and return cleanup.
- Raw spill-slot tests preserve exact felt bit patterns and limb order independently of typed HIR memory operations.
- Fixed-stack intrinsic tests check every result lane and the complete suffix, including overflow. Caller produces ZERO outside a syscall and the actual caller hash inside one without overwriting that suffix; deleting its owned padding fails physical verification. Crypto, advice order/exhaustion, exact UnpackWord aliases and actual clock values are checked through VM execution.
- AdviceLoadWord accepts four mixed nonempty one-lane operands, including pointers and packed values, and rejects wide/ZST/unrepresented operands before helper mutation. Tests cover every operand position, exact Felt results, one-word advice consumption/order, exhaustion with zero through three advice elements, physical floor and a protected suffix deeper than sixteen lanes.
- HornerBase/HornerExt tests compare both accumulator lanes against independent extension-field folds, preserve all fourteen retained lanes and the full suffix, and cover source-local reads, spill preparation and invalid evaluation words/addresses. LogDeferred tests compare every output lane and actual deferred nodes/root over two invocations in the same execution, and check unknown-statement traps.
- MemStream tests check both loaded words, all retained lanes and the full suffix; repeated pointer advancement, unwritten cells, minimum depth, spill preparation and terminal native addresses. Compiler-storage provenance and full eight-cell ownership remain physical checks; permitted source alignment failures and physically valid semantic mutations are checked through VM execution.
- FRI tests use an independent extension-field polynomial oracle for all sixteen results and every coset. Default allocation and forced normalization preserve an explicit seventeenth input distinct from the first protected suffix lane; tests also keep original retained operands live after the unit, check the single physical pop and complete overflow suffix, and exercise invalid-input traps.
- AdvicePipe/CryptoStream tests compare all eight returned values with all eight destination cells, retain capacity lanes and the complete suffix, and execute minimum-depth and spill/normalization cases. Advice tests check exact double-word consumption, exhaustion without consumption, and consumption/partial writes on later memory failure. Crypto tests check field addition, unchanged source cells, both pointer updates, adjacent ranges, overlap rejection, alignment, terminal addresses and partial write failure.
- Primitive memory-contract tests reject invalid Input/Output references, widths and read dependencies. Output-backed word stores preserve exact scratch identities and untouched cells; later loads recover those tokens. Read-before-write snapshots, read-only ranges, both source permissions and Input/Output compiler-address escapes remain checked.
- EvalCircuit tests preserve all three retained operands, actual minimum depth and a complete overflow suffix through the selected pipeline. Independent zero/nonzero and invalid/dependent-wire circuit cases distinguish semantic assertion behavior, including native omission. Count guards precede address failures; tests cover the 32768-wire boundary, large canonical counts, footprints above 255 cells, alignment and first/later terminal accesses, and the traced 32-witness limit versus untraced execution.
- Counted-read tests accept runtime source/source-local counts under the ABI while retaining provenance, reject known source/compiler crossings and unresolved or mixed compiler ranges, and check full declared compiler initialization including the final cell. Metadata rejects memory-valued Circuit outputs. Fixed read-token identity, pre-write snapshots and typed-write regressions continue to pass; footprint-only source reads do not materialize unused cell tokens.
- All 22 fixed-window system events select from exact real-HIR contracts and retain every payload/suffix lane. VM tests observe actual advice-map, Merkle-store and deferred-state behavior, including padding/count variants, domain/rate order, TRUE deferred payloads, numeric edge cases and concrete errors. Hqword tests observe the seventeenth physical lane beneath its transient ID, with actual floor depth, default/normalized scheduling, spills and live-through values.
- Observation metadata tests reject out-of-range references and check complete registered windows. Physical tests inject compiler provenance into every observed position, then clean it up before return, requiring rejection at observation. Exact/mixed origins, branch abstraction, late graph/internal-loop origins, source-local acceptance and unobserved compiler suffix preservation are checked. Omitted events and physically valid wrong-ID/payload mutations are detected through VM behavior, without exact-body checking.
- MemToMap tests execute the real source-local address/cast/Add boundary path, including nonempty and empty endpoint ranges, and check exact advice-map values, untouched source cells, full suffix and endpoint cleanup before return. Same-address strict-key/return/store/call negatives, zero source extent, padding, exact mixed evidence, unresolved/equal-token rejection, late graph/internal-loop origins and metadata role mutants preserve the proof boundary. Identify isolated token fixtures honestly when a source-reachable exact-mixed case is unavailable.
- Memory-event tests check READ permissions and whole possible spans above 255 and 65532 cells without materializing unused tokens. No-read shortcuts never waive observations. VM cases cover MemToMap new/idempotent/empty insertion, collisions, zero defaults, budget paths and U32 terminal bounds; DeferredRegisterData covers chunk order, node/memo/root/budget state, every closed guard predicate, terminal addresses and actual first errors, including AND's post-read InvalidTag. Default/forced normalization, spills/live-through, the 24-variant inventory and existing 22-event/fixed/Circuit/typed-write regressions remain required. Semantic event omissions and wrong ID/payload mutations are tested through execution.
- PrintLn tests execute the pinned debugger handler with isolated output/warning capture, including all byte residues, crossing UTF-8, low32 cell decoding, zero length, cap/max-residue footprint, missing first/later words versus zero siblings, high-end byte-boundary crossing and actual first-error order. Decode warnings continue through operand cleanup and following instructions; absent handlers can trap. Check exact source types, no-read permissions, strict ptr/len origins, opaque source-coordinate handling, full suffix/liveness/scheduling and semantic mutations. The child harness must prove nonzero normal/release execution and no record leakage; no evaluator equivalence is claimed.
- Generated initializer/adaptor procedures can spill and have correct executable and debug frame offsets.
- Mutated-plan tests deliberately introduce illegal depths, incorrect copies, swapped lanes, uninitialized spill slots, physical slot collisions, dead stores clobbering live colored spill slots, stale loop spill slots, missing edge repairs, phase-order/result-publication errors, and ABI clobber mistakes; verification rejects each before assembly.

Scheduling acceptance also requires these focused cases, using distinct lane values and checking both plan actions and VM results:

| Case | Required behavior |
| --- | --- |
| One spill-slot operand beside a resident operand at depth fifteen | Gather the resident source before the reload can bury it; prepare the two operands without eviction or full normalization, allowing total depth above sixteen |
| The same resident operand is live afterward | Preserve it through a valid spill copy or an additional stack occurrence; its last-use status must change the preservation obligation |
| Spill-slot operand already loaded earlier in the segment | Reuse the resident occurrence when legal; retaining or consuming it does not by itself invalidate the existing spill copy |
| Repeated operands and mixed-width layouts | Count every requested raw occurrence, duplicate or reload as needed, and preserve limb order, including when a value straddles the active-prefix boundary |
| Bounded preparation requiring one eviction | Remove only the capacity deficit: choose a dead/redundant victim before a backed live victim, and a backed victim before a new spill store; stop once the target fits |
| Sixteen input lanes also needed afterward | Establish spill copies where survivor copies cannot fit, then consume the input occurrences; do not reject the preparation or drain the deeper suffix |
| Duplicate retention candidates | Recompute availability after each eviction and after input consumption; deleting two copies must not erase the last surviving representation. In particular, evicting a retained `a` beside a pinned input `a` requires a spill store if `a` is live afterward and has no other surviving representation |
| Owned stack deeper than sixteen | Preserve the deeper suffix's exact lane identities and order through bounded preparation; an input available there and in a valid slot is loaded from the slot |
| Required input available only below the active prefix | Decline bounded preparation and use full normalization without losing the value or emitting an illegal indexed read |
| Shuffle solver deliberately declines a feasible bounded request | Complete the preparation with the deterministic permutation, without escalating to full normalization |
| Temporary address-generation depth exceeds sixteen | Accept legal raw accesses and restore the expected active state after the load/store; reject a mutated sequence that accesses a buried lane |
| A speculative load-and-shuffle candidate fails | Leave committed actions, slot requests, and location facts unchanged before trying the next candidate or tier |

Lowering interface acceptance requires:

| Case | Required behavior |
| --- | --- |
| Stored input already has a valid spill copy | Reuse its binding without an unnecessary store; a stack-only source is stored before payload preparation |
| Establishing stored inputs requires full normalization, including a stored literal | Preserve all pending stack and stored inputs regardless of live-after status; explicitly push and store the literal into its bound slot |
| A stored input is also a stack input, or repeats in several stored-input positions | Preserve both physical requirements and required multiplicity; equal stored atoms may share a valid slot |
| Stack preparation would overwrite a required stored input | Reject or change that proposal; keep the contents live through preparation, expansion, and physical slot coloring |
| Byte `memmove` aligned and unaligned paths, both overlap directions, and zero count | Verify all branch/loop stack contracts and suffix preservation, and validate copied bytes and trap behavior through VM/differential execution |
| Mutated `memmove` cleanup or internal stack access | Reject unmatched state lanes, suffix corruption, and inaccessible reads during physical verification; algorithm-only mutations such as an incorrect direction predicate are covered by semantic execution tests |
| Typed `MemSet` in byte/native address spaces | Match evaluator contiguous-byte results for supported scalar widths, retain existing small one-lane value support, preserve unaffected bits (of the u32 view of a touched cell) and the complete suffix, and support native addresses above the byte-pointer domain |
| `MemSet` at address-space limits, including zero count | Validate the complete range before writing; accept valid exclusive endpoints, reject invalid zero-count byte bases, and prove range failures leave memory unchanged through execution |
| U16 load/store beside a full-width untouched cell or at the final native cell | Offsets `0..=2` access only the containing cell; offset `3` uses both cells, retaining offset validation; loads keep the touched-cell u32 view requirement, stores keep the low 32 bits of a touched cell that holds a felt at or above 2^32 |
| Small packed-value fills and ordinary dynamic/immediate stores, including 24- and 9/17/25-bit widths | Preserve existing accepted source types and byte strides; use one or two cells according to the actual bit footprint, preserve outside bits (of the u32 view of a touched cell) and untouched neighbors, and reject true terminal-cell crossings |
| Source-local opaque arithmetic versus compiler or mixed origins | Preserve source-local may-origin without inventing equality; retain exact-offset checks where known and reject unresolved compiler-derived addresses, including after joins and loop backedges |
| FPI payload widths zero through sixteen | Emit correct trailing zero padding, stored prefix order, and the registered executor call; the full-width case constructs twenty-two lanes without padding or intervening allocation |
| FPI with live-after inputs and a preserved deeper suffix | Retain valid representations through the call and publish all sixteen result lanes only after normal return |
| Mutated lowering unit bindings and publication | Reject stale or wrong stored atoms, swapped prefix positions, undeclared storage reads, illegal buried-payload accesses, and premature result identities |
| Frontend FPI wrapper conversion | Execute a case with distinct account prefix/suffix and root/payload lanes so an upstream ABI permutation error cannot pass unnoticed |

Call ABI and generated-procedure acceptance requires:

| Case | Required behavior |
| --- | --- |
| `call`/`dyncall`/`syscall` with zero through sixteen argument lanes and unrelated caller values initially addressable | Expose only the declared arguments and explicit zeros in the sixteen-lane window; preserve retained caller values below it or in spill slots |
| Context callee overwrites every nonresult exposed lane, including input padding | Preserve caller live values solely through valid storage or VM-restored overflow; never rely on returned padding or argument copies |
| Context call returns zero through sixteen result lanes | Publish only signature results, legally discard all other returned window lanes, and preserve the restored suffix |
| Same-context callee has fewer than sixteen arguments and an unknown nonzero suffix | Preserve every suffix lane, including those initially addressable; draining owned values must stop at the suffix |
| Direct or indirect invocation uses the wrong family for the target convention | Reject known mismatches and enforce convention in indirect type checks; matching arity or an empty suffix is not an exception |
| Public kernel entry invokes a private helper | Use `syscall` for the entry and its declared ordinary convention for the helper; reject a syscall targeting the private helper. Currently `Internal` follows emitted `Public` visibility; package visibility must remove that kernel eligibility when adopted |
| Authored direct or indirect call resolves to `truncate_stack`, including through an alias | Reject with `AbiMismatch`; only compiler-registered context/kernel returns and generated root cleanup establish the required overflow ownership |
| Generated main selects a program entrypoint | Require an `exec`-compatible convention and invoke it with `exec`; reject a context-call target instead of adapting the invocation |
| Root initialization and optional harness precede the user entrypoint | Preserve the VM-provided sixteen-lane input window exactly, including zero padding and order; keep initialization, harness writes, and entry execution in the same root memory context |
| Program entrypoint returns with floor-induced overflow | Preserve its top sixteen lanes with root `truncate_stack` and return from `begin` at exact depth sixteen; do not require same-context callees to remove caller suffix or padding |
| Generated main, test harness, initializer, or adapter omits or swaps a required stage | Reject against the registered generated definition and parameters; provenance annotations alone are insufficient |
| Artifact packaging attempts to append executable code after verification | Require that code to enter a selected generated body and be verified before assembly |

Generic-event acceptance requires:

| Case | Required behavior |
| --- | --- |
| Dynamic and immediate generic events with default/forced allocation, overflow and spill storage | Actual hosts observe the generated state for that execution; preserve operand stack/memory and continuation values |
| Dynamic event ID or returned alias is replaced | Reject the wrong physical token identity against source-owned evidence |
| Candidate forges generic authority, changes source ownership, or inserts an uncontracted event in a typed unit | Reject through source-correspondence checking and strict ordinary target rules |
| Handler reads absent/stale cells or exports a compiler address through advice | Permit observation without inventing initialization or erasing known provenance; retain the source-memory ABI for subsequent accesses |
| Known compiler-origin write or return follows a generic observation | Keep existing rejection; generic observation grants no later storage authority |
| Generic event omitted, duplicated, or given a wrong immediate while otherwise physically valid | Detect semantic differences through actual event logs/provider/error oracles, without claiming a physical exact-body guarantee |
| Events surround branches, loops, advice and memory operations | Preserve source effect order and actual VM/host error behavior |
| Generic and typed events use the same numerical ID | Keep their source contracts distinct; let the VM dispatch without a second handler implementation in generic lowering |

After preparation commits and before each `RunLowering`, allocation freezes sorted unique `semantic_storage_reads` from complete semantic input demand, including whole-value alias recipe dependencies. Scoped checked reconstructions and established equalities resolve each lane. Literals and equal resident occurrences need no backing cell; otherwise the least existing equal symbolic slot is recorded. Missing representations remain allocation errors. Live-through survivors alone do not reserve cached cells.

Frame assignment treats these boundary dependencies as pseudo-reads alongside opaque stored inputs. Stores kill old contents and existing simultaneous-use interference prevents destructive reuse. The metadata emits no instruction, adds no stored-input contract, and supplies no verification authority: independent raw replay still proves source recoverability from actual representations. Traces display the recorded cells at each unit.

Storage lifetime and frame-layout acceptance requires:

| Case | Required behavior |
| --- | --- |
| A spill-only scalar or wide alias source has no later physical reload | Keep every source lane until complete alias publication, including unused output lanes |
| A value remains live on the stack after its final required spill-slot read | Permit another noninterfering slot to reuse the cell; do not extend storage lifetime solely because the SSA value remains live |
| A join retains a cached spill copy of a resident value | Treat the retained fact as a pseudo-read and preserve it through every later use; a redundant store may be omitted only while the copy is actually valid |
| Slots are stored and read exclusively in different branch arms | Permit one physical cell when no other requirement makes them coexist; retain distinct cells when both contents must coexist before the branch |
| A spill copy is invariant across a loop and ABI-conforming calls | Keep it live around the backedge and through calls without repeated stores or conflicting reuse |
| A changing header parameter reuses its stable abstract slot | Require the next iteration's token after explicit edge repair; parameter renaming alone must not initialize or update memory |
| A dead store is colored onto another slot whose contents remain live | Reject the collision during physical replay; interference construction must prevent it despite the store having no later reader |
| Canonical slot for `a` holds the only copy of needed `b` while `a` is on the stack | Preserve `b` or choose another slot before storing `a`; apply the same rule to normalization, backing stores, input preparation, and edge repair |
| A cyclic edge assignment needs snapshots and stack outputs from old slots | Keep each snapshot until its final required read, including stack construction, and prevent destructive physical overlap |
| A stored input is loaded early inside a lowering unit, including an FPI prefix | Reserve its slot for the whole unit and through its internal calls; do not reuse it based on internal last-load timing |
| Consecutive units each need a four-cell aligned local scratch object | Use one aligned workspace rather than separate per-unit ranges; simultaneous objects within one unit still receive disjoint ranges |
| A later unit or loop iteration reads local scratch without initializing it | Reject reliance on the previous workspace owner's contents; logical ownership changes do not require runtime clearing |
| Scratch contents or an address survive a phase boundary, or initialization occurs outside the consuming expansion | Use an explicitly declared spanning object with a dedicated range; preserve its facts across workspace reuse by other units |
| Staged indirect-call root crosses a selected phase boundary | Carry exact root-cell identities and address provenance in the contract; reject a different initialized digest or stale previous-iteration contents |
| Spanning scratch is required across a join or loop backedge | Establish required facts on every incoming edge with simultaneous content bindings; no annotation or object ID alone establishes initialization or identity |
| A callee may write borrowed scratch without retaining the pointer | Invalidate affected old identities; preserve definite initialization only when justified, and require explicit output relations for stronger facts |
| A synchronous callee borrows a unit-local object | Keep the object's range reserved through the call, require the registered nonretention contract, and include nested emission scratch in the enclosing unit's layout |
| Source locals, scalar colors, spanning scratch, and workspace have different sizes and alignments | Preserve source-local offsets and valid contiguous object ranges; use one final aligned frame for executable addresses, debug information, and the procedure declaration |
| Final frame approaches the target limit or alignment arithmetic overflows | Check before narrowing offsets; accept a legal boundary layout and diagnose an oversized frame with its storage contributions |

Verifier identity and induction acceptance requires:

| Case | Required behavior |
| --- | --- |
| Identical incoming-token vectors at several stack positions and an initialized scratch cell | Produce the same join token for all those locations; retain only equalities justified on every incoming normal path |
| A branch-dependent token is offered for a required preserved SSA value | Reject unless exact identity is established on every incoming path; matching types or widths are insufficient |
| Scratch is initialized on only one normal arm | Reject a subsequent read that requires definite initialization; a trapping arm imposes no normal continuation obligation |
| A carried stack value advances but a scratch cell required to equal it remains stale | Reject the backedge using simultaneous parameter substitution |
| Two carried values have equal initial tokens and different updates | Do not infer an invariant alias; reject a declared equality that any backedge fails to establish |
| A loop has a zero-iteration path or nests another loop | Preserve exact invariant tokens and scoped carried parameters; do not expose body-only results or scratch initialization as unconditional exit facts |
| Publication is used to rename a changed suffix lane or bless overwritten stored input | Reject the physical preservation failure before binding results |
| An identity/alias lowering changes its output token | Reject even when its output shape is correct; the registered equality is an explicit obligation |
| An arithmetic-only mutation retains valid physical effects but changes the result or its limb order | Exercise semantic VM/differential tests; do not claim generic token checking proves the lowering algorithm |
| Allocator liveness, slot-validity, or equality metadata is corrupted while emitted code is invalid | Reject using independently derived obligations and actual instruction/frame replay |
| Target instruction-effect rules are exercised in isolation | Compare modeled permutations, word accesses, frame addresses, and call effects with VM behavior to validate the shared trusted definitions |

Entry-contract and transfer acceptance requires:

| Case | Required behavior |
| --- | --- |
| A single-predecessor segment boundary, including one introduced for a call phase | Inherit the actual owned stack and valid live spill facts; emit no normalization merely for crossing the boundary, including when the stack exceeds sixteen lanes |
| Incoming join states already agree | Adopt their stack and common spill facts with no repair actions only when that common contract represents every live atom; union permissible bottom-padding states separately |
| Incoming layouts disagree | Evaluate the fixed candidates using all actual incoming states and immediate successor preparation cost; reversing predecessor traversal order produces the same chosen contract and edge plans |
| Both arms reload a resident value from the same spill slot | Preserve the common valid slot fact at the join; a later eviction can drop that occurrence without another store |
| A slot is valid on only one arm, or different slots are used on each arm | Do not assume a common spill fact; establish required contents explicitly on the edges that need repair |
| One source binds two live destination parameters | Permit a checked alias to share a representation, but materialize distinct occurrences whenever the chosen stack layout or next operation requires both positions |
| A nonresident loop-invariant capture | Establish its slot on forward entry, preserve it through nested control flow and ABI-conforming calls, and emit no repeated backedge store when its contents remain valid |
| A resident invariant with an existing entry spill copy | Keep the proved slot fact at the header; do not create speculative entry stores for resident invariants without such copies |
| Changing and identity loop-carried values | Update slots with new backedge source values where required; omit identity stores only when current token equality proves them redundant |
| Two loop parameters have equal initial values but different backedge sources | Do not inherit their initial equality into the header contract; preserve both current values on subsequent iterations |
| Nested loops, zero-trip paths, and multiple exits | Fix each header before its body, plan each body once, and establish every exit and backedge contract without waiting for an unplanned backedge to choose the header |
| An exact join target excludes an extra owned occurrence | Remove it legally even if ordinary operand preparation could have left it buried; preserve the caller tail and every retained target occurrence |
| Simultaneous slot permutation with stack outputs reading old slots | Protect all old sources before overwrites, including stack-output sources; incremental repair and full normalization produce the same semantic result |

The existing differential corpus and PR 1461's semantic reproducers remain valuable. Compare guest debug-info configurations because optimization changes the HIR shape. Test-only controls can force bounded preparation or full normalization and exhaust the shuffle budget, so these constructions are exercised independently of earlier-tier success; these are policies within the one allocator, not alternate backends. The full-width FPI acceptance case also ensures that tier three's capacity bound never becomes a global height or operand-count restriction.

Performance assessment reports cycles, code size, frame cells, and allocation/verification compile time on the existing benchmark corpus. The [performance report](unified-stack-allocator-performance.md) records historical measurements and their limits. Plan diagnostics identify the selected scheduling tier, victim choices, entry-contract origin and candidate costs, preserved spill facts, and loads/stores attributable to operand preparation versus edge repair. A correct fallback using spill slots for all live values is an escape path, not permission for the production policy to regress all functions to memory traffic. There is no promised numerical performance improvement before measurement.


## Graph analysis and diagnostics

- Shared terminal successors are selected once, while conditional edges bind their own incoming tuples. Exercise distinct arguments, captures and parameterless bodies, both VM branch outcomes, and direct production CLI paths for componentless worlds and supporting component-sibling modules.
- Compare dominance against an independent path-removal oracle on reachable small graphs, including cycles and nonordered IDs. Exercise longer chains and sequential structured branches through actual selection, allocation and verification. Record preparation and verification timings without machine-dependent pass thresholds, and execute the verified bodies with independent result/suffix oracles.
- Ordinary data creates no empty origin entries; hypotheses omit unrelated segment values while retaining alias dependencies and late-backedge frame provenance. Existing branch/loop origin mutation tests remain required.
- Diagnostic tests retain exact source spans, inline ancestry and selected steps at selection/replay boundaries, report known raw positions and identities, and preserve missing context for opaque values or edge-only failures. Alias-publication failures use resolved semantic tokens, including outputs beyond the physical stack, without inventing raw depths or indexing absent lanes.
