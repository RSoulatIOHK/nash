# Plan 08 — Core -> Core optimizer

## Status and accepted scope

Chunks 1 and 2 are accepted and complete, including mandatory Core typing,
pre-ANF static-parameter lifting and ANF. Chunk 3 is complete with an isolated
performance runner. The user
accepted Chunk 4 rules 1 and 2, including repeated-constant propagation and
their fixed-point loop. Rule 3 was accepted on 27 September 2026. The measured Rule 4 identity and
builtin-wrapper cases are accepted, including their measured size tradeoffs;
conditional bodies are excluded for now, including recursive-only inlining.
Broader body-size heuristics, partial/indirect-call expansion and further Rule 4
experiments are deferred. The retained cases still need implementation as a pass;
the measured selected-binding experiment is not an installed optimizer.
Current assembly in
`nash-codegen/src/program.rs`
rewrites recursion and lowers directly; `nash-ir` has no installed optimizer.
Reuse its existing Core, Builder, traversal and free-variable facilities.

Accepted decisions (26 September 2026):

- Start with binder hygiene, static-parameter lifting and A-normal form (ANF),
  retaining explicit recursive workers in Core. Lift before ANF splits calls;
  do not reconstruct application chains to recover this information.
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
2. Add ordinary semantic tests and paired before/after snapshots first. A Core
   optimization shows `--- core before` and `--- core after` in the same snapshot;
   a UPLC optimization shows `--- uplc before` and `--- uplc after`. Include
   downstream UPLC and evaluation sections as needed to verify semantics.
   Integration fixtures use the same sectioned format as unit fixtures; do not
   split one fixture's Core, UPLC and outcomes into separate snapshot files.
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
Core with LetRec -> unique names -> static-parameter lifting -> ANF
  -> accepted main passes -> refresh recursive groups
  -> recursion rewrite once -> unique names -> UPLC lowering
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
- Each main optimization pass preserves ANF. Inline atoms or splice binding
  sequences at call sites; do not substitute a compound expression into an atomic operand. Avoid cycles
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
- Lift static parameters before ANF while complete source calls remain visible.
  Retain explicit recursive workers, so main optimization still precedes knot
  rewriting. Recompute recursive groups before final rewriting. Discovering
  additional static parameters after optimization is a separate future candidate;
  do not add ANF application-chain recovery as a prerequisite.
- Freshen binder occurrences again after rewriting: generated lambda subtrees can
  be shared at multiple use sites. Do not normalize again: generated wrappers,
  self-applications, packets and cases may contain non-atomic operands. Lowering
  accepts this nested Core. Do not run ANF-dependent passes across this boundary.
  Any future post-rewrite cleanup requires a separate review and must accept
  nested Core or operate on UPLC; it must not reintroduce a second ANF pass.

## Chunk 1 — Shared analysis and hygiene

**Complete — kept by user review (26 September 2026).**
`nash-ir::analysis` provides lexical occurrence reports, free variables,
conservative discard safety and structural size. `nash-ir::hygiene` provides
scope/uniqueness checks, alpha-renaming and capture-free substitution. Existing
Builder/name supply and traversal are reused; codegen re-exports the existing
free-variable algorithm from IR. Assembly does not call any new transformation.

Occurrence reports distinguish branch, lambda, delay and recursive-body scopes;
empty-parameter lambdas add no execution boundary. Substitution freshens each
inserted copy and preserves free names, but does not by itself justify inlining
an effectful or strict expression. Discard safety assumes variables already
denote values and conservatively rejects calls, saturated builtins, forces,
cases, projections and recursive groups. Size counts nodes and binders, including
repeated occurrences; it excludes literal payload and is not a Flat-size estimate.
Semantic tests belong to normal Cargo discovery; this chunk adds no
performance experiments or performance baselines.

Validation: 17 new semantic tests cover the analyses and transformations,
including evaluated capture avoidance and strict-effect fixtures. Core/UPLC and
analysis snapshots were reviewed. `cargo fmt --all`, Clippy with all targets and
features and warnings denied, and `cargo test` pass. Existing O0 snapshots are
unchanged. No performance claim is made for this infrastructure-only chunk.

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

## Chunk 2 — Static-parameter lifting and ANF normalization

**Typing prerequisite accepted (26 September 2026):**
Core is now `Core { ty, kind }`, with mandatory result metadata supplied or derived
at construction. Source translation retains solved runtime-specialized types;
delays, knot workers and heterogeneous dispatch use explicit internal runtime
descriptors. Existing deliberate parametric erasure remains supported. There is
no optional metadata lookup or missing-type presence check. Tests cover source
nominal identity, partial functions, erased generics, fields and recursion-created
values. Core annotation snapshots change; emitted UPLC and evaluation remain the
baseline. The user accepted this prerequisite and the paired snapshot contract;
The accepted static-lifting and ANF implementation below builds on this prerequisite.

Validation: 11 new tests cover builder typing, source metadata, recursion-created
types and substitution preserving explicit coercion views. Formatting and strict
workspace Clippy pass; the full workspace test run passes (3,577 passed, 3 ignored).
All 154 updated existing codegen snapshots containing UPLC retain byte-identical
UPLC, evaluation results, traces and budgets. The vesting Core annotations change
only delayed binder types; their UPLC and ledger checks pass unchanged. Vesting
fixtures now keep Core, UPLC and ledger outcomes together in one sectioned snapshot.

**27 September pipeline revision:** normalize once, before the main optimization
passes. Rewrite recursion after those passes, freshen generated binder occurrences,
and lower directly. The previous second ANF pass added a binding around `self self`
on each recursive step; it is removed along with its ANF-dependent cleanup loop.
Semantic phase snapshots retain the pre-ANF, post-ANF and rewritten Core, plus
baseline/optimized UPLC. Earlier two-normalization measurements below are history;
Chunk 3's explicit baseline is updated for this revised pipeline.

Validation: formatting, strict Clippy for both workspaces, all 20 explicit
performance cases, and the full workspace suite pass (3664 passed, 3 ignored).
The new regression test fails under the old pipeline and proves nested recursive
self-application lowers correctly with preserved results and trace order. Reviewed
19 updated phase snapshots and one new snapshot; removed the obsolete fixture
that normalized already-rewritten recursion. O0 snapshot sections are unchanged.

**Static-parameter lifting and ANF accepted (26 September 2026):**
`nash-ir::anf` provides `normalize`, the shared atom predicate and an ANF shape
checker. Atoms are variables, literals, nonempty lambdas, delays and bare builtin
references. Normalize value bodies locally; empty lambdas/applications unwrap
with their original type view. Reassociate let prefixes under globally unique
names, retaining exact result types. Preserve atomic application runs and execute
earlier stages before a later non-atomic argument. Builtins retain their n-ary
form because valid builtin nodes only execute on saturation.

Test-only codegen wiring lifts static parameters before the sole ANF pass,
then rewrites recursion and freshens binder occurrences, without normalizing again.
Production assembly and public flags are unchanged. Ten IR tests cover lifting, shape, types, hygiene and idempotence;
Twenty semantic tests produce twenty paired phase snapshots. Existing source fixtures
also compare original versus optimized execution across the recursion boundary,
including Big/little wildcard, captured/function fallback, Logic and Lift cases.
Ground values, error categories/messages and trace order are compared; returned
opaque functions are exercised through dedicated applications rather than checked
for identical lambda syntax. New optimizer snapshots do not pin performance budgets.

26 September validation: formatting and strict all-target/all-feature Clippy pass.
The full workspace test run passes (3,606 passed, 3 ignored), including doctests. The
combined implementation added 29 tests and 28 paired snapshots relative to the accepted
typing prerequisite. No snapshots are pending. The interleaved-parameter fixture
proves both captured values, retained dynamic-parameter order and initial argument
trace order; ordinary O0 source snapshots remain unchanged.

Initial ANF-only measurements (superseded by the pre-ANF lifting revision below)
ran outside Cargo test discovery using
`/tmp/nash-anf-measure.rs` (temporary experiment, not a permanent runner). Baseline
is `9f454f1c`; candidate code was working-copy revision `07d012061767ba5f875121f3218f019190724b4b`.
Target: Plutus V3 / UPLC 1.1.0, repository `nash-plutus` default V3 cost model,
default evaluation budget; size is raw Flat bytes. Results and logs agree in all
five cases. The nested arithmetic returns 43; the other cases return 42. Countdown
uses `equalsInteger n 0` and `subtractInteger n 1`. Values below are baseline →
candidate; none are acceptance thresholds.

| Input | CPU | Memory | Flat bytes |
| --- | ---: | ---: | ---: |
| Atomic `addInteger 20 22` | 181308 → 181308 | 602 → 602 | 10 → 10 |
| Nested `addInteger (multiplyInteger 6 7) 1` | 336261 → 384261 | 1004 → 1304 | 15 → 18 |
| Curried addition: `trace "first" 20`, `trace "second" 22`; body traces `"stage"` | 791802 → 935802 | 3398 → 4298 | 55 → 62 |
| Countdown 3 to 0, returning static second argument 42 | 1681056 → 2209056 | 7410 → 10710 | 39 → 49 |
| Same countdown, static argument first and computed argument second | 1681056 → 2401056 | 7410 → 11910 | 39 → 48 |

The final row exposed a loss of static lifting when ANF split a complete call.
The user rejected recovering ANF call chains and chose early static lifting.
`static_lift::lift` now captures unchanged parameters before normalization and
retains `LetRec` workers for later optimization. An all-static worker is explicitly
delayed and forced per call, without adding a dummy parameter or memoization.
The original wrapper retains its arity and argument evaluation behavior.
Genuine partial/escaping self uses and mutual groups retain the existing policy.


Revised measurements used `/tmp/nash-static-lift-measure.rs`, outside Cargo test
discovery, on candidate code revision `1a516a43fbef5d42dbffc9ce02f1843e7f0d82a9`.
With the same inputs, baseline and cost model, both countdown parameter orders
now cost 2,209,056 CPU, 10,710 memory and 49 Flat bytes. The static-first case no
longer loses capture during ANF. The other three measurement rows are unchanged.
Results and logs agree with the baseline in all five cases. ANF still adds binding
overhead; this fixes the lifting loss without claiming a speedup over O0. The user
accepted this implementation. Temporary measurement scripts and binaries were
removed after recording the results; functional tests and paired snapshots remain.

Implement `anf::normalize` and an invariant checker using the contract above.
Normalize once at main-phase entry, before recursion rewriting. Add temporary harness
access to inspect phases without choosing public optimizer flags.

Tests cover nested applications/builtins, lets, fields, constructors, case subjects,
force/delay, explicit recursion and rewritten recursion. Differential evaluation
covers trace-before-body, subject once, ignored strict fields, partial and
oversaturated calls, intermediate failure, and unselected/unforced/uncalled bodies.
Reuse Big/little wildcard fixtures, captured/function-valued fallback results,
shared helpers and `Logic` short-circuit/selected-Lift fixtures. Test normalization
idempotence and valid types/names, not just pretty output.

**Done when:** normalization preserves semantics and its invariant, with paired
before/after Core sections and unchanged baseline output; review binding overhead
in UPLC.

## Chunk 3 — Explicit performance-only test path

Implemented in `tools/optimizer-perf`, an unpublished package with its own
workspace and lockfile, explicitly excluded from the root workspace. Commands:
`measure`, `check [baseline]`, `record NEW.json`, and `experiment MODULE.nash`.
See its README for exact invocations and fixture scope. There are no performance
test targets or ordinary CI changes.

The initial 20 rows compare O0 against accepted static lifting, ANF and rules
1–3 before recursion rewriting. Inputs cover list traversal, static recursion,
Data hits/misses, field decoding, validation pass/fail, real base Logic helpers,
and six ledger scenarios for each of Vesting and VestingParam. Reports embed
source inputs, record runtime/cost-model settings and provenance, and measure
CPU, memory and raw Flat bytes. Validator size excludes applied ledger arguments;
their evaluation budgets include those arguments. Known results are checked
before recording; both pipelines must agree on results and traces.

`check` requires exact metrics, inputs and outcomes, including improvements;
`record` refuses to overwrite existing files. Initial baselines document current
accepted-pass behavior, including ANF overhead; they are not universal performance
targets. Temporary source experiments remain outside permanent fixtures and
baselines. A 120-second watchdog bounds workload compilation and execution;
each CEK run has explicit CPU/memory caps and budget exhaustion fails the command.

Representative current O0 → optimized figures (full rows in `baseline.json`):

| Input | CPU | Memory | Flat bytes |
| --- | ---: | ---: | ---: |
| List sum of 1–8 | 5788660 → 5692660 | 27872 → 27272 | 102 → 134 |
| Static countdown 8, returning 42 | 4736761 → 6320761 | 21725 → 31625 | 52 → 60 |
| Data integer match | 978518 → 786518 | 5496 → 4296 | 58 → 48 |
| Vesting claim after deadline | 2621392 → 2285392 | 14525 → 12425 | 271 → 246 |
| VestingParam claim after deadline | 2834600 → 2546600 | 15227 → 13427 | 275 → 253 |

These compare the full accepted pipeline with O0, not rule 3 in isolation.
The normalize-once revision saves 384000 CPU and 2400 memory on each recursive
fixture versus the previous pipeline; Flat size falls 137 → 134 for list traversal
and 62 → 60 for countdown. The other 18 rows, all O0 results, and all optimized
results/traces are unchanged. This baseline update was explicit and reviewed.
Countdown's remaining first-ANF overhead is a separate investigation.

Validation (27 September 2026): all 20 explicit baselines match. Deliberately
lowering a CPU baseline by one unit makes `check` fail; changed fixture sources
also fail even with identical metrics. `record` rejects overwrites. A temporary
source experiment returns 42, and a diverging experiment fails at the explicit
budget limit. Both workspace formatting checks and strict Clippy checks pass.
Root all-feature executable tests pass (3662); the separate doctest rerun passes
(1 passed, 3 ignored). The first doctest run encountered a crate-ID collision
while concurrent builds were active; the rerun completed after those builds.
Root nextest passes all 3662 tests. Root metadata, Cargo test listing and nextest
discovery exclude the performance package; its metadata confirms a separate
workspace. Normal CI and ordinary semantic snapshots remain unchanged.

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

### Rule 1 — atom/alias propagation (accepted)

`nash-ir::propagate::propagate` removes variable aliases transitively, then
propagates literals with zero or one remaining syntactic uses. On 27 September,
the user also approved duplication of integer constants (no magnitude cap), byte
strings up to 64 bytes inclusive, and all three BLS constant variants (G1, G2,
Miller-loop result), regardless of use count. Other repeated literals stay shared.
Count uses after alias removal: `let x = largeString; let y = x; use y y` must retain the
shared string binding. The byte-string limit is explicitly user-selected. Lambda, delay, bare
builtin and computed bindings remain for later rules. Alias substitution never
moves the target computation. Globally unique binder IDs prevent capture;
substitutions preserve occurrence and root type views and preserve ANF.

This accepted rule is exercised before recursion rewriting in the test-only pipeline. Production assembly is unchanged. Paired Core/UPLC
semantic snapshots cover scope, sharing, strict failures and delayed captures;
performance measurements run separately from normal test discovery. The user
kept this rule on 26 September 2026. Validation: formatting, strict all-target /
all-feature Clippy, and the full workspace run (3618 passed, 3 ignored). Added
12 tests and 11 paired snapshots; existing snapshots remain unchanged.

Temporary isolated measurements (Plutus V3 default cost model, raw Flat bytes;
accepted ANF baseline versus this rule alone):

| Fixture | CPU before → after | Memory before → after | Bytes before → after |
| --- | ---: | ---: | ---: |
| Two-binding literal chain | 112100 → 16100 | 800 → 200 | 11 → 6 |
| Shared string through alias | 536842 → 488842 | 1210 → 910 | 31 → 28 |
| Alias of computed integer | 277308 → 229308 | 1202 → 902 | 15 → 13 |
| Already minimal integer | 16100 → 16100 | 200 → 200 | 6 → 6 |

These are small rule-isolation examples, not whole-program performance claims.
The temporary runner was explicitly compiled and run outside Cargo test
discovery, then removed after the keep decision. Chunk 3's permanent runner
remains pending.

### Rule 2 — direct lambda application and the rules 1 + 2 loop (accepted)

`nash-ir::beta::reduce` rewrites direct `App(Lam(...), args)` into strict
parameter bindings. Partial application binds supplied arguments outside the
remaining lambda. Exact saturation exposes the body. Oversaturation evaluates
the saturated body before applying its result to extra arguments; a non-atomic
result receives a fresh ANF binding. It does not inline named function bindings.

Leading `Let`/`LetRec` sequences are spliced into the surrounding strict context,
without crossing lambda, delay, branch, trace or recursive-function scopes.
Each pass preserves typed ANF and unique binders; no whole-tree re-normalization
is needed between iterations. Fresh names avoid all input binder and use IDs.

`beta::simplify` runs rule 1 followed by rule 2 until neither changes Core.
Unchanged-pointer preservation supplies the change flag; this is not a node-count
comparison or a fixed iteration limit. Each beta rewrite consumes at least one
lambda parameter, and rule 1 introduces none. Partial applications therefore
also make progress. The loop runs before recursion rewriting and after its final
ANF, only in the test pipeline. No UPLC pass is added.

Snapshots cover partial capture, multiple iterations, nested and recursive RHS
prefixes, explicit type views, fresh-builder name collisions, strict unused
arguments, argument order, saturated-body failure, oversaturation success and
suspended branch/delay bodies. Performance experiments remain outside ordinary
Cargo test discovery.

Temporary isolated experiments compare accepted ANF + rule 1 against rules 1 + 2
(Plutus V3 default cost model, raw Flat bytes):

| Fixture | CPU before → after | Memory before → after | Bytes before → after |
| --- | ---: | ---: | ---: |
| Identity applied to literal | 64100 → 16100 | 500 → 200 | 8 → 6 |
| Identity applied to computation | 277308 → 229308 | 1202 → 902 | 15 → 13 |
| Three nested direct applications | 160100 → 16100 | 1100 → 200 | 15 → 6 |
| Partial application then named call | 325308 → 277308 | 1502 → 1202 | 18 → 15 |
| Oversaturation with shared parameter and computed result | 448806 → 496806 | 1934 → 2234 | 35 → 37 |
| Already minimal integer | 16100 → 16100 | 200 → 200 | 6 → 6 |

The original oversaturation regression came from an added ANF binding for the
computed function result, while the multiply-used integer parameter retained its
binding under the original rule 1 policy. The table above records that initial
experiment; the approved repeated-constant policy is measured below. This
initial experiment is retained as history; the repeated-constant refinement
below removes that particular regression. No UPLC cleanup or cost heuristic
has been introduced.
The temporary runner was explicitly compiled and run outside Cargo test
discovery, then removed after the keep decision.

Repeated-constant refinement (27 September): compare ANF before these rewrites
against rules 1 + 2 with repeated integers/BLS and byte strings up to 64 bytes.

| Fixture | CPU before → after | Memory before → after | Bytes before → after |
| --- | ---: | ---: | ---: |
| Original oversaturated integer example | 448806 → 448806 | 1934 → 1934 | 35 → 35 |
| Twice-used 64-byte string | 131868 → 83868 | 916 → 616 | 78 → 142 |
| Twice-used 65-byte string | 132214 → 132214 | 918 → 918 | 79 → 79 |

The integer example loses its `x` binding, offsetting the new function binding.
The byte-string policy intentionally permits serialized-size growth to remove
binding evaluation costs. This is the user-selected 64-byte limit, not a claim
that all metrics improve. Snapshot coverage includes 64/65-byte boundaries,
large integers, all three BLS constant variants and the original oversaturation
example. These tests concern literal constants; computed BLS operations remain
bound. Native BLS Miller-loop constants have no UPLC text literal; their Core
snapshot uses the existing unsupported-constant display.

Accepted on 27 September 2026, including the repeated-constant refinement.
Validation: formatting and strict all-target/all-feature Clippy pass;
full workspace tests pass (3643 passed, 3 ignored). Added 25 tests and 23 paired
snapshots. Existing snapshots remain unchanged.

### Rule 3 — single-use values and immediate computed returns (accepted)

`nash-ir::single_use::inline` tries two ANF-preserving rewrites:

- Substitute an ANF atom at its sole syntactic use. This includes single-use
  lambda, delay and unforced bare builtin bindings. Forced builtin references
  are excluded even when returned directly: their bindings must remain shared
  for top-level hoisting. Moving a value into a branch or
  delayed scope does not execute its body. No binder-bearing body is duplicated.
- Replace `let x = computation in x` with the computation. This preserves the
  exact evaluation point, including failure, divergence and trace effects.

Other computed bindings stay bound. A sole use inside a branch, lambda or delay,
or after a trace, does not permit moving the original computation. Computed
function operands also stay bound: inlining them would violate ANF. This trial
therefore does not resolve the separate post-ANF/UPLC cleanup decision.

Count uses by globally unique ID, then rewrite bottom-up using mapped children.
Substituting original RHS pointers could resurrect bindings removed inside a
lambda or delay; tests cover that trap. Preserve occurrence and enclosing result
type views. No freshening or function-size threshold is needed. Local substitution
walks can be quadratic for long chains; retain the simple implementation absent
measured need for more machinery.

`single_use::simplify` alternates the accepted rules 1 + 2 fixed point with rule 3
until unchanged. Rule 3 removes bindings without duplicating lambda parameters,
so it cannot undo the progress argument for beta reduction. Pointer identity
remains the change flag. The test pipeline runs the candidate before recursion
rewriting only; production assembly remains unchanged.

Tests cover immediate computed returns, newly exposed beta reduction, nested
function bindings, delayed values, bare builtins, argument failures, trace order,
computed captures, computed function operands, shared functions, type views and
fixed-point idempotence. Paired snapshots show both Core and downstream UPLC.
Temporary measurements run separately through `/tmp/nash-single-measure.rs`.
Temporary experiments compare accepted rules 1 + 2 with the candidate loop
(Plutus V3 default cost model, raw Flat bytes):

| Fixture | CPU before → after | Memory before → after | Bytes before → after |
| --- | ---: | ---: | ---: |
| Immediate computed return | 229308 → 181308 | 902 → 602 | 13 → 10 |
| Single-use bound function | 277308 → 181308 | 1202 → 602 | 15 → 10 |
| Single-use builtin | 229308 → 181308 | 902 → 602 | 13 → 10 |
| Single-use delay | 96100 → 48100 | 700 → 400 | 9 → 7 |
| Computed function operand | 283598 → 283598 | 1532 → 1532 | 26 → 26 |
| Shared function | 208100 → 208100 | 1400 → 1400 | 16 → 16 |
| Forced builtin used once inside a function called eight times | 1836084 → 1836084 | 8856 → 8856 | 61 → 61 |

The initial candidate inlined the last fixture's forced builtin, adding 64000
CPU and 400 memory while saving 3 bytes. One syntactic use is not one runtime
evaluation: moving the forced builtin into the repeated function loses shared
forcing work. On 27 September the user decided
that forced builtin references must always be top-level hoisted. Rule 3 therefore
preserves every bare builtin binding whose `force_count()` is nonzero, regardless
of use count or execution scope, including direct returns. With that exception,
the eight-call fixture is unchanged: CPU 1836084, memory 8856, Flat size 61 bytes
before and after. All other measurements in the table remain as shown. Chunk 5
owns the actual top-level hoisting pass. This is a fixed policy, not a
cost/frequency heuristic.

Validation: formatting and strict all-target/all-feature Clippy pass. The full
workspace run passes 3663 tests with 3 ignored. This candidate adds 20 semantic
tests and 19 snapshots; no pending snapshots remain. Performance measurements
stay in the explicit temporary experiment, outside normal test runs.

The user accepted rule 3 on 27 September 2026. Temporary measurement source and
binary were removed after recording the results; semantic tests and snapshots remain.

### Rule 4 trial 1 — multiple-use identity helper

The first of three concrete experiments is `\x -> x`. It is measured before
choosing any size threshold; builtin wrappers and conditional helpers remain
separate, pending experiments. The temporary explicit command is:

```sh
cargo run --locked --manifest-path tools/optimizer-perf/Cargo.toml --example identity_trial
```

The trial selects a fixture binding by unique ID, freshens its lambda at each
fully applied direct call, and reuses the accepted rules 1–3 loop. It removes the
shared pure lambda binding only if all uses were replaced. The trial does not
recognize identity bodies as a compiler special case and installs no production
pass or general selection policy. It normalizes once, before these rewrites;
recursion rewriting and lowering follow without another ANF pass.

Baseline is the accepted normalize-once/rules 1–3 pipeline, not O0. Measurements
use Plutus V3, UPLC 1.1.0, the bundled default V3 model and raw Flat bytes. The
experiment asserts equal ground results and trace logs, expected results/logs,
ANF before recursion rewriting, binder hygiene and root types. It has explicit
CPU/memory caps and a 120-second watchdog. Paired Core and UPLC are printed by
the command; the permanent performance baseline is not changed.

| Identity fixture | CPU before → after | Memory before → after | Flat bytes before → after |
| --- | ---: | ---: | ---: |
| Two calls, summing two ones | 634516 → 394516 | 2804 → 1304 | 30 → 18 |
| Eight calls, summing eight ones | 2489764 → 1673764 | 10616 → 5516 | 99 → 60 |
| Recursive caller: setup call plus eight iterations | 7930425 → 7018425 | 36641 → 30941 | 67 → 55 |
| Two calls with traced arguments | 1073512 → 833512 | 4868 → 3368 | 60 → 48 |
| First argument fails; later trace must not execute | 59598 → 59598 | 132 → 132 | 59 → 47 |
| One selected branch; other argument traces and fails | 363598 → 219598 | 2032 → 1132 | 52 → 39 |

The recursive fixture has two syntactic uses of the identity helper and nine
runtime calls. Its recursion is retained; only the nonrecursive helper is inlined.
The simple identity body disappears after beta reduction and alias propagation,
so these cases incur no duplicated body cost in final UPLC. This does not establish
an acceptable threshold for larger helpers. No general partial/escaping-call or
polymorphic-inlining claim is made by this first trial.

The review has moved on to the builtin-wrapper experiment below. No general
inlining rule or threshold is accepted yet.
Delete the temporary example after recording the decision or replace it with
appropriate semantic coverage when implementing a retained general rule.

### Rule 4 trial 2 — multiple-use builtin wrapper

The same temporary harness also measures `\x -> addInteger x 1`:

```sh
cargo run --locked --manifest-path tools/optimizer-perf/Cargo.toml --example identity_trial -- builtin-wrapper
```

It uses the same selected-binding rewrite, accepted baseline, semantic checks,
measurement settings and six call patterns as trial 1. Only the helper body and
expected results change. No builtin-wrapper exception is installed in the compiler.

| Wrapper fixture | CPU before → after | Memory before → after | Flat bytes before → after |
| --- | ---: | ---: | ---: |
| Two calls | 964932 → 820932 | 3608 → 2708 | 34 → 32 |
| Eight calls | 3811428 → 3379428 | 13832 → 11132 | 104 → 117 |
| Recursive caller: setup call plus eight iterations | 9417297 → 8937297 | 40259 → 37259 | 72 → 70 |
| Two calls with traced arguments | 1403928 → 1259928 | 5672 → 4772 | 65 → 62 |
| First argument fails; later trace must not execute | 59598 → 59598 | 132 → 132 | 63 → 61 |
| One selected branch; other argument traces and fails | 528806 → 432806 | 2434 → 1834 | 57 → 53 |

All six preserve results and logs. Unlike identity, the builtin body remains at
each call site: eight static uses save CPU and memory but add 13 Flat bytes. The
recursive caller has only two static uses and nine runtime calls, so it saves
runtime costs while reducing size by two bytes. Review this tradeoff before the
third (conditional-helper) experiment; no size threshold follows from this alone.

### Rule 4 — applying the accepted identity case to source workloads

The user kept all measured identity and builtin-wrapper cases, including the
small size growth. The explicit experiment now also supports:

```sh
cargo run --locked --manifest-path tools/optimizer-perf/Cargo.toml --example identity_trial -- source-cases
```

This compiles the existing `booleanHelpers` and `staticRecursion` workloads,
selects each fixture's outer literal-conversion helper binding, and applies the
same selected-helper inlining plus rules 1–3 cleanup. No new case folding,
second ANF normalization, or general function-selection policy is added.

| Workload | CPU O0 / rules 1–3 / plus Rule 4 trial | Memory O0 / rules 1–3 / plus Rule 4 trial | Flat bytes O0 / rules 1–3 / plus Rule 4 trial |
| --- | ---: | ---: | ---: |
| Boolean helpers | 400100 / 496100 / 160100 | 2600 / 3200 / 1100 | 29 / 34 / 18 |
| Static countdown | 4736761 / 6320761 / 4448761 | 21725 / 31625 / 19925 | 52 / 60 / 39 |

Both source-workload regressions disappear in this trial. Results and empty trace
logs agree across all three variants; the trial asserts expected results, ANF,
binder hygiene, and unchanged root types. The boolean expression retains its
case-result binding and known cases; countdown retains bindings for its comparison
and decrement. The permanent baseline and production pipeline remain unchanged.

### Rule 4 trial 3 — smallest comparison-based conditional helper

Measured `nonNegative x = if lessThanInteger x 0 then 0 else x` through the same
explicit selected-helper experiment:

```sh
cargo run --locked --manifest-path tools/optimizer-perf/Cargo.toml --example identity_trial -- conditional
```

This is one comparison and one conditional, with only a literal and a variable
in its branches. Body size is fixed. No constant evaluation or case folding is
added. Baseline is accepted rules 1–3; candidate adds selected-helper inlining
and their existing cleanup loop, with ANF only once. Same V3 model, raw Flat
sizes, budgets, watchdog, result/log comparison and IR checks as earlier trials.

The 25 samples cover 2/4/8/16 fully applied call sites with all positive, all
negative, and alternating inputs; 2/4/8/16 mutually exclusive sites with only the
first or last selected and every other argument tracing then failing; two static
sites making nine runtime calls via recursion with positive or negative inputs;
traced computed arguments; first-argument failure; and the zero boundary.

| All sites execute | CPU before → after | Memory before → after | Flat bytes before → after |
| --- | ---: | ---: | ---: |
| 2 sites | 1013096 → 869096 | 4606 → 3706 | 41 → 48 |
| 4 sites | 2010092 → 1770092 | 9012 → 7512 | 65 → 92 |
| 8 sites | 4004084 → 3572084 | 17824 → 15124 | 111 → 180 |
| 16 sites | 7992068 → 7176068 | 35448 → 30348 | 204 → 357 |

Positive, negative, and alternating inputs have the same costs in this matrix.

| Other usage | CPU before → after | Memory before → after | Flat bytes before → after |
| --- | ---: | ---: | ---: |
| 2 sites, first selected | 333390 → 237390 | 1901 → 1301 | 42 → 47 |
| 4 sites, first selected | 333390 → 237390 | 1901 → 1301 | 80 → 105 |
| 8 sites, first selected | 333390 → 237390 | 1901 → 1301 | 156 → 221 |
| 16 sites, first selected | 333390 → 237390 | 1901 → 1301 | 308 → 453 |
| 16 sites, last selected | 781390 → 685390 | 4701 → 4101 | 307 → 453 |
| 2 static sites, 9 recursive runtime calls (either input sign) | 9634035 → 9154035 | 44750 → 41750 | 79 → 85 |
| Traced computed arguments, both signs | 1569300 → 1425300 | 6772 → 5872 | 80 → 85 |
| First argument fails | 59598 → 59598 | 132 → 132 | 70 → 75 |
| Zero boundary | 799888 → 655888 | 3904 → 3004 | 34 → 40 |

All 25 semantic checks pass; 24 reduce CPU and memory, and the early failure is
unchanged. All grow in size. This supports runtime call-overhead savings for
this particular body, but not unlimited duplication: cold sites add bytes without
adding runtime savings. No threshold or production rule is chosen from these
synthetic measurements. The user subsequently deferred conditional inlining; retain these measurements
as evidence, not as an accepted transformation.

### Rule 4 trial 4 — conditional inlining only inside recursive bodies

At the user's request, the same conditional is inlined only at direct call sites
inside `LetRec` function bodies. The `LetRec` continuation is not selected. This
is a syntactic trial restriction, not a loop-frequency estimate or a production
policy. Identity and builtin-wrapper acceptance is unchanged.

```sh
cargo run --locked --manifest-path tools/optimizer-perf/Cargo.toml --example identity_trial -- conditional --recursive-matrix --recursive-only
```

The matrix uses 1/2/4/8/16 sites and 0/1/4/8 iterations, with two outside calls
so the shared helper remains after normal rules 1–3 cleanup. It also covers
mutually exclusive sites (first/last selected; unselected arguments trace and
fail), and two cases with no outside uses. All 30 pass expected outcomes,
trace equality, ANF, type and binder checks under the existing 100M CPU cap.
An initial 32-iteration sweep exceeded that cap for larger site counts; it was
replaced by the bounded 0/1/4/8 sweep, not treated as a semantic mismatch.

| Recursive-only case | CPU saved | Memory saved | Flat bytes before → after |
| --- | ---: | ---: | ---: |
| 1 site, zero iterations, two outside calls | 0 | 0 | 98 → 108 |
| 1 site, eight iterations, two outside calls | 384000 | 2400 | 98 → 108 |
| 2 sites, eight iterations, two outside calls | 768000 | 4800 | 109 → 130 |
| 8 sites, eight iterations, two outside calls | 3072000 | 19200 | 179 → 263 |
| 16 sites, eight iterations, two outside calls | 6144000 | 38400 | 272 → 440 |
| 16 sites, eight iterations, only first executes | 384000 | 2400 | 375 → 536 |
| 2 sites, eight iterations, no outside calls | 816000 | 5100 | 91 → 97 |

Compared with unrestricted inlining on the identical matrix, retaining the two
outside calls saves roughly 6–7 bytes but gives up 144000 CPU and 900 memory.
If no outside calls exist, the two strategies are identical. Restricting by
recursive scope does not prevent duplication of cold branches within that scope.

The previous 25 conditional samples also pass with `--recursive-only`: all 23
nonrecursive controls have unchanged metrics. The two recursive samples each
leave just one outside helper use, which accepted Rule 3 then inlines; their final
output is identical to unrestricted inlining. This cleanup interaction is expected
and is why the new matrix retains two outside uses.

Decision (27 September 2026): leave conditional bodies out for now, including
this recursive-only variant. Defer further Rule 4 experiments and broad heuristic
design; retain the already accepted identity and builtin-wrapper cases. No
production code or permanent performance baseline changes. Strict experiment
Clippy and formatting pass.

### Rule 4 retained scope and deferred work

The user accepted the established identity and small builtin-wrapper cases,
including their measured size growth. Conditional bodies are excluded for now;
recursive placement does not override that exclusion. This decision does not
restrict the accepted identity/builtin-wrapper cases to recursive bodies.

Defer broader body-size/growth heuristics, larger bodies, partial-application
expansion, indirect/escaping-call expansion, and further exploratory experiments.
Do not pick an arbitrary size threshold or add per-program search/tuning machinery.

Implementation remains outstanding for the retained cases: replace fixture-selected
binding IDs with a pass limited to the accepted shapes, preserve strict arguments
and application staging, freshen copied binders, retain shared definitions when
other uses remain, and integrate with rules 1–3 cleanup. Add paired semantic
snapshots and review the resulting full performance baseline. The decision to
stop exploring is not a claim that this pass has already landed.

**Done when:** retained cases are implemented with semantic snapshots, verified
measurements and the keep decision recorded. Deferred cases do not block completion.

## Chunk 5 — Builtin sharing

Treat force caching and constant currying as separate review units.

- Always hoist forced builtin references to the validator's outermost binding
  prefix, outside its argument lambdas, and reuse them throughout its body,
  including one-use references. For non-validator entry points, use the same
  outermost program scope. Bind each distinct forced builtin once per program.
  This is the user's
  27 September decision; no use-count, size or budget threshold gates it.
  Hoist only the forced builtin value, never its applied arguments or a
  saturated computation. Rule 3 must preserve these bindings. Measure standalone
  calls, loops and branch-local uses to document costs, not to choose placement.
- Share repeated constant partial applications at a safe common scope. Only hoist
  safe partial applications; never pre-evaluate a failing saturated call.
  Move a constant across operands only when the operation and evaluation order
  permit it. Equality/addition examples do not justify reordering subtraction or
  comparisons indiscriminately.

Preserve ANF and correct types. Measure cached forces together with pair projections
later. The old minimum-use constant of two does not apply to forced builtin
references. Ensure cleanup does not inline away intentional sharing and recreate it
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
that parameter removal preserves worker captures and does not leave stale
parameter indices. Newly exposed static parameters may be evaluated as a separate
future candidate; early lifting must not depend on recovering ANF call chains.

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
passes in cleanup only if their input contracts allow nested post-rewrite Core.
Do not repeat ANF-dependent passes or whole-program sharing after recursion rewriting.

Detect actual structural progress or accurate rewrite reports, not equal node
counts. Keep generated names deterministic. Test same-size rewrites, cycles,
optimizer and cleanup idempotence, and hygiene after every pass. Check ANF only
in the main optimization phase, before recursion rewriting.

Retain named phase sections for raw Core, ANF, optimized recursive Core, rewritten
Core and lowered output (plus any separately accepted later cleanup). Keep each optimization's before/after pair in
one snapshot at the representation it transforms. Differentially evaluate baseline
and optimized programs, including selected traits, Logic laziness and Big/little case fixtures.
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
