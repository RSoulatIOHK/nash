# Plan 08 — Core -> Core optimizer

## Status and accepted scope

Pending implementation. Current assembly in `nash-codegen/src/program.rs`
rewrites recursion and lowers directly; `nash-ir` has no installed optimizer.
Reuse its existing Core, Builder, traversal and free-variable facilities.

Accepted decisions (26 September 2026):

- Start with binder hygiene and A-normal form (ANF), using existing Core nodes.
- Optimize while recursive functions remain explicit `LetRec`, then rewrite
  recursion once, normalize the generated code and run cleanup.
- Consider inlining small functions even when used multiple times. This is a
  candidate to measure, not blanket permission to duplicate code.
- Evaluate optimizations one chunk or one individual rewrite at a time. Review
  the result with the user and keep, revise or discard it before proceeding.
- Keep permanent performance regression tests and temporary per-chunk
  experiments, on an explicit performance-only execution path. Ordinary
  `cargo test` and `cargo nextest run` must not run either category.
- Decide CPU, memory and serialized-size tradeoffs case by case. There is no
  universal priority order or requirement that every metric improve.
- Decide the accepted optimizations before deciding flags or optimization levels.
  Previous O1/O2 assignments and numerical thresholds are not accepted policy.
- Preserve O0 as the existing pre-optimizer semantics baseline. Do not replace its
  snapshots with optimized output.

Specification: [docs/codegen.md](../docs/codegen.md), section 6.
Progress is tracked in [SPEC.md](../SPEC.md); listing a candidate here does not
mean it is implemented or accepted for the final optimizer.

## Execution and review contract

For each chunk, or each independently reviewable optimization within a chunk:

1. State the exact rewrite, its preconditions, and the expected benefit.
2. Add ordinary semantic tests and before/after Core and UPLC snapshots first.
3. Implement the smallest candidate; use temporary internal harness wiring to
   exercise it without enabling a public optimization mode.
4. Compare against the unchanged baseline: results, traces, errors, termination,
   CPU, memory, and serialized size. Record inputs, target version, cost model,
   baseline revision and candidate revision. Include favorable and adverse cases.
5. Present the concrete code, snapshots and measurements to the user. A passing
   test suite alone does not approve a candidate. Wait for the keep/revise/discard
   decision before advancing to the next chunk or optimization.
6. If kept, retain functional coverage and meaningful performance regressions,
   record the decision and make a focused local jj commit. If rejected, remove
   candidate code and candidate-only fixtures; retain a concise finding and any
   independently useful semantic coverage. Delete disposable experiment code
   after review unless promoted into the permanent performance suite.

A shared chunk may contain multiple candidates, but do not implement the whole
plan before review. The candidate review is the user-requested stopping point,
not a request to reauthorize routine implementation or testing within that chunk.

## Pipeline and invariants

```text
O0: Core -> recursion rewrite -> UPLC lowering

Candidate optimized pipeline:
Core with LetRec -> unique names -> ANF -> accepted main passes
  -> refresh recursive groups and static-parameter metadata
  -> recursion rewrite once -> ANF -> accepted cleanup passes -> UPLC lowering
```

Assembly coordinates the phases. Passes belong in `nash-ir`; recursion rewriting,
lowering and the evaluation adapter remain in `nash-codegen`. Do not introduce
an IR-to-codegen dependency. Reuse the existing nash-plutus evaluator and encoder.

Every rewrite must preserve values, trace content/order, failures and termination.
An unused computation can still be strict and observable. Conservative safety
analysis must cover logging and divergence as well as throwing. Allocation and
execution budgets can change; report those changes separately from semantics.

### ANF and evaluation order

- Define one atom predicate shared by normalization, invariant checking and
  passes. Variables and literals are atoms; lambda/delay bodies normalize in
  their own scopes. Decide precisely how values and bare builtin references fit.
- Name non-atomic operands of applications, builtins, constructors, projections,
  forces and case subjects. Let RHSs and tail positions may be computations.
  Reassociate administrative lets without capture or pointless literal aliases.
- Fresh binders must carry accurate types. Establish intermediate application
  types through the existing type/representation machinery; do not invent fake
  types or erase representation merely to manufacture a binding.
- Keep work inside its original case branch, lambda or delay unless movement
  has a separate semantic proof. Preserve strict subject evaluation exactly once.
- Preserve staged application: applying an earlier argument can fail before a
  later argument is evaluated. Do not hoist all arguments ahead of those stages.
- Preserve trace order: evaluate the message, emit it, then evaluate the body.
- Preserve strict constructor fields, including ignored fields of a folded case.
  Big field extraction and shared wildcard helpers retain their selected scopes.
- Each pass preserves ANF. Inline atoms or splice binding sequences at call sites;
  do not substitute a compound expression into an atomic operand. Avoid cycles
  between substitution and reintroducing identical administrative bindings.

### Recursion boundary

- Traverse `LetRec` bodies under their parameter scopes, retaining simultaneous
  group scope and the continuation. Creating a group does not execute its bodies.
- Remove dead members by reachability from the continuation, including escaping
  references, rather than counting internal self uses.
- Do not unfold recursive calls in the inliner or repeatedly expand generated
  self-application/dispatchers during cleanup. Nonrecursive helpers may inline
  into recursive bodies.
- Remove recursive parameters only when all affected uses can be safely rewritten,
  including group-internal calls. Preserve strict argument evaluation and staging;
  keep unsafe partial/escaping signatures. Do not create unsupported zero-parameter
  recursive values.
- Recompute groups and static-parameter metadata immediately before rewriting.
  Discover newly static arguments as well as rejecting stale indices. Deleting a
  dead call which changes `config` may make `config` capturable by a worker.
- Restore ANF after rewriting wrappers, self-applications, packets and cases.
  Cleanup reuses accepted passes and semantic checks; it is not another optimizer.

## Chunk 1 — Shared analysis and hygiene

Audit and reuse existing `nash-ir/src/traverse.rs`, Core/Builder and codegen name
supply facilities. Add only missing capture-free substitution, binder hygiene,
occurrence analysis, safe-to-discard analysis and size estimation.

Occurrences must track execution scope and lambda/delay boundaries, not just use
counts. Size analysis must support `LetRec`: recursion has not been rewritten yet.
Estimated Core size guides candidates; actual Flat size judges the emitted result.
Check name uniqueness and binding scope after transformations; renaming alone is
not evidence that a faulty substitution preserved semantics.

Tests: shadowed binders, unbound/duplicate names, recursive groups, branch-local
uses, delayed uses, partial builtins and strict fields. Safe-to-discard must reject
tracing, failing and potentially diverging computations.

**Done when:** shared analyses are tested and reviewed; no optimization is enabled.

## Chunk 2 — ANF normalization

Implement `anf::normalize` and an invariant checker using the contract above.
Normalize at main-phase entry and after recursion rewriting. Add temporary harness
access to inspect phases without choosing public optimizer flags.

Tests cover nested applications/builtins, lets, fields, constructors, case subjects,
force/delay, explicit recursion and rewritten recursion. Differential evaluation
covers trace-before-body, subject once, ignored strict fields, partial and
oversaturated calls, intermediate failure, and unselected/unforced/uncalled bodies.
Reuse Big/little wildcard fixtures, captured/function-valued fallback results,
shared helpers and `Logic` short-circuit/selected-Lift fixtures. Test normalization
idempotence and valid types/names, not just pretty output.

**Done when:** normalization preserves semantics and its invariant, with separate
ANF snapshots alongside unchanged O0 snapshots; review binding overhead in UPLC.

## Chunk 3 — Explicit performance-only test path

Keep two categories:

- **Permanent regression cases:** retained representative inputs and reviewed
  CPU/memory/serialized-size baselines or limits for accepted optimizations.
- **Temporary experiments:** per-chunk exploration, alternative implementations,
  threshold sweeps and adverse examples; delete after recording the decision.

Use an isolated package such as `tools/optimizer-perf/`, with its own workspace
boundary and explicit exclusion from the root workspace where needed. Neither
category belongs in automatically discovered root integration tests. A dedicated
runner can execute regression checks and experiments through explicit commands,
for example `cargo run --manifest-path tools/optimizer-perf/Cargo.toml --release
-- check`. Final runner names/arguments are implementation details, not optimizer
level decisions. Keep the harness small and reuse existing evaluation/encoding.

Do not rely solely on ignored tests or a root `required-features` gate: broader
root test commands can enable those. Verify root `cargo test`, `cargo test
--workspace --all-features`, and `cargo nextest run --workspace --all-features`
do not execute or discover the performance workload. Ordinary semantic tests
still run normally. New optimizer semantic fixtures assert behavior and code
shape, not performance thresholds. Preserve historical O0 snapshots, including
any incidental budget output already present; new dedicated performance workloads
and regression limits belong only to the explicit runner.

Require explicit baseline updates; never accept changed budgets automatically as
part of ordinary snapshot acceptance. A dedicated CI performance job may be added
only by an explicit later decision; normal CI test jobs remain unaffected.

Record versions, cost models, inputs and before/after figures. Evaluate tradeoffs
case by case with the user; no unconditional memory-first or size-first policy.
Include vesting paths, list traversal, static recursion, Data matching, validation,
decoding and boolean helper compositions. Guard experiments against unbounded
compilation/evaluation. Verify that an intentional regression fails the explicit
regression command while ordinary test discovery remains unaffected.

**Done when:** both permanent and temporary workflows work only through the
special path, isolation is verified, and initial measurements are reproducible.

## Chunk 4 — Inlining and binding cleanup

Review these independently: atom/alias propagation; direct lambda application;
safe single-use bindings; small functions with multiple call sites.

Bind strict arguments before substitution and preserve application staging. Single
use is not proof that moving work into a conditional branch is safe. Respect
recursive and escaping/partial-call restrictions. Keep large literals and strings
from being duplicated indiscriminately. Consider builtin wrappers as candidates,
not an unconditional exception to code-size and ANF rules.

Multiple-use small-function inlining is approved for consideration. Measure call
savings against duplicated body/constant size, including recursive callers and
cold branches. Choose a simple deterministic threshold only from reviewed
findings; the old size constant of 12 is not a decision. Do not add per-program
search/tuning machinery.

Tests: builtin wrappers, repeated small helpers, identity/Lift/Logic wrappers,
strict argument failure/trace order, single uses in unselected branches or delays,
strings, escaping functions and nonrecursive helpers within `LetRec`.

**Done when:** each retained rule has semantic coverage, measured tradeoffs and a
keep decision; rejected rules are removed.

## Chunk 5 — Builtin sharing

Treat force caching and constant currying as separate review units.

- Share forced builtin references when repeated use justifies the binding cost.
  Compare standalone calls, loops and branch-local uses; preserve evaluation.
- Share repeated constant partial applications at a safe common scope. Only hoist
  safe partial applications; never pre-evaluate a failing saturated call.
  Move a constant across operands only when the operation and evaluation order
  permit it. Equality/addition examples do not justify reordering subtraction or
  comparisons indiscriminately.

Preserve ANF and correct types. Measure cached forces together with pair projections
later. The old minimum-use constant of two is a candidate to test, not a fixed
policy. Ensure cleanup does not inline away intentional sharing and recreate it
indefinitely.

**Done when:** each accepted sharing rule has measurements and regressions for
profitable and unfavorable cases, including lazy scopes.

## Chunk 6 — Dead bindings, functions and parameters

Remove unused bindings only when their evaluation is safe to discard. Remove
unreachable recursive members by continuation reachability. Remove unused
parameters only for rewritable uses; keep strict evaluation of dropped arguments
at the correct call stage, even if that needs an unused let.

Test tracing/failing/diverging unused RHSs, strict ignored arguments, saturated
and partial/escaping calls, self/mutual recursion and all-static workers. Check
that dead-call elimination exposes newly static parameters and parameter removal
does not leave stale static indices.

**Done when:** each retained rule preserves semantics and demonstrates a reviewed
benefit; refresh recursion metadata before rewriting.

## Chunk 7 — Known-case and field simplification

Fold cases on known native constructors, booleans, integers, bytes, lists and Data
shapes using their actual branch tests, binders and defaults. Keep strict subject
and constructor-field evaluation in the original order. An ignored failing field
must still fail. Preserve out-of-range/malformed-case errors; do not assume every
hand-built Core case has a matching branch.

Simplify fields of known constructors without dropping observable evaluation of
other fields. ANF often makes that evaluation explicit; do not mistake a selected
field alone for the original strict construction.

Tests: selected/default branches, empty/nonempty lists, each Data shape, field
ordering, ignored failing/traced fields, returned functions/delays, and Big/little
wildcard fixtures. Cover known conditions produced by `Logic` helpers.

**Done when:** retained rewrites have before/after Core and UPLC snapshots,
equivalence tests, measurements and a keep decision.

## Chunk 8 — Representation and force/delay cleanup

Cancel `force (delay x)` and valid inverse builtin pairs such as
`unIData (iData x)`. Establish preconditions per direction and representation;
`iData (unIData d)` is not an unconditional replacement for arbitrary Data.
Preserve shape-check failures, traces and strictness. Simplify administrative
applications/aliases only under the ANF and application-staging contract.

List and map Eq selection is already library work, not an optimizer rewrite:
Big-element lists use structural Eq through `listData`; Little-element lists use
selected element Eq. `Primitive.map` is a Storable pair-list alias: Big/Big Eq
uses `mapData`, while mixed/Little maps use selected element Eq. Big `Map` has
structural Eq. Do not recognize Eq impl names to replace arbitrary user behavior,
or introduce an Ord/Show change. Reuse selected-Lift and custom-Eq trace tests.

**Done when:** each accepted cancellation has explicit preconditions, malformed
input tests and measured output; no trait-selection magic is added.

## Chunk 9 — Constant builtin evaluation

Use a callback supplied by codegen around its existing closed-term evaluator;
keep `nash-ir` independent of codegen. Evaluate only supported, saturated,
constant-argument builtin calls under an explicit compile-time budget. Review
per-builtin input-shape and error-safety rules against the current runtime.

On unsupported results, budget exhaustion or runtime failure, leave the original
expression. A runtime failure must not become a compile error. Account for result
literal size: a computation that produces a huge constant is not automatically a
win. This is optimizer folding, separate from explicit user `comptime` semantics.

Tests: arithmetic, byte/string/Data/container operations, nonzero/zero division,
empty/nonempty head/tail, malformed data, oversized results and exhausted budgets.

**Done when:** supported folds are reviewed for semantics and cost/size tradeoffs;
unsupported/failing computations retain runtime behavior.

## Chunk 10 — Single-field native pair projection

Compare `CaseKind::Pair` with `fstPair`/`sndPair` when exactly one branch binder
is used, for a valid one-branch case without a default:

```text
case p of pair a _ -> body a  => let a = fstPair p in body a
case p of pair _ b -> body b  => let b = sndPair p in body b
```

A direct field return can become the projection itself. Keep subject evaluation
exactly once and the projection strict at the original case point, including a
field captured by a returned lambda/delay. Keep accurate field types. Both fields
used retains the case. Neither used is outside this rewrite: preserve necessary
evaluation/shape checks. Do not drop strict fields of known pair constructions.

Applies to native builtin pairs, including the tag/fields pair from `unConstrData`,
not arbitrary two-field ADTs. Test malformed input, traced/failing subjects,
ignored failing fields, direct return and larger bodies. Measure CPU, memory and
Flat size, standalone and with builtin-force caching, then review a deterministic
rule. No assumption that projection wins, and no per-program tuning engine.

**Done when:** keep or discard is decided from measurements; O0 stays unchanged.

## Chunk 11 — Composition, convergence and final configuration

Compose only the accepted passes. Establish their actual order from interactions:
inlining exposes dead code and known cases, sharing can conflict with inlining,
and recursion rewriting creates cleanup opportunities. Reuse the same accepted
passes in cleanup where applicable; do not blindly repeat whole-program sharing.

Detect actual structural progress or accurate rewrite reports, not equal node
counts. Keep generated names deterministic. Test same-size rewrites, cycles,
optimizer and cleanup idempotence, and hygiene/ANF after every pass.

Retain separate snapshots for raw Core, ANF, optimized recursive Core, rewritten
Core and cleaned/lowered output. Differentially evaluate baseline and optimized
programs, including selected traits, Logic laziness and Big/little case fixtures.
Keep O0 snapshots; never mass-replace them with optimized ones. Run ordinary
semantic checks separately from the explicit performance regression command.

Only after reviewing the accepted set, decide whether there is one optimized mode
or several, their flags/configuration, defaults and pass assignments. Do not
implement the old `Level::O1/O2` sketch or expose arbitrary tuning knobs first.
Existing CLI/config optimizer rejection remains until a reviewed mode is wired.

**Done when:** the accepted combination is semantically equivalent, convergent,
measured and reviewed; final configuration is decided and documented. Mark
SPEC.md complete only when the retained scope is implemented and validated.

## Boundaries and deferred work

- Integer dispatch is explicit in [Plan 11](11-macros-comptime.md), via an AST/Core
  operation. Ordinary integer literal case remains equality-based. Do not add
  density/max-index thresholds, automatic dispatch selection, guards or table
  filling. Optimizing a known explicit dispatch must preserve its failure and
  branch behavior.
- Explicit Big constructor tags are a separate representation feature; they are
  not permission to renumber tags or treat wildcards as arbitrary runtime tags.
- Source decoder fusion is outside this plan. It needs its own design/review;
  do not sneak in recognition of stdlib function names during folding.
- Native boolean case lowering already exists. Remove no delays on the assumption
  that it still lowers through eager `ifThenElse`; test the actual current backend.
- Cost-model changes require deliberate review/rebaselining of performance tests,
  not silent acceptance. Record rejected candidates as well as retained ones.

## References

Use these as references for individual reviewed rules, not an implementation to
copy wholesale:

- Aiken `crates/uplc/src/optimize.rs`: pass composition and fixed points.
- Aiken `optimize/shrinker.rs`: occurrence analysis, inlining, force caching,
  currying, safe builtin evaluation, inverse conversions and common scopes.
- Aiken `optimize/interner.rs`: binder hygiene.
- Elm `elm/compiler/src/Optimize/Expression.hs`: traversal organization for a
  different target.
