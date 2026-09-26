# Plan 08 — Core -> Core optimizer

## Goal

The four optimizations named in [docs/overview.md](../docs/overview.md)
as Core -> Core passes in `crates/nash-ir`, beginning with A-normal form (ANF).
Give binders unique names, normalize evaluation into explicit bindings, then run
ANF-preserving optimizations while `LetRec` is explicit. Then rewrite recursion,
normalize the generated code, and run cleanup passes before lowering. Use temporary
cost measurements to compare candidate implementations.
Remove measurement code and fixtures after each experiment; keep findings in docs
and functional snapshot coverage in the test suite.

Passes (after hygiene and ANF normalization):

1. Inline single-use `Let`s and small lambdas.
2. Builtin force caching (and constant-argument currying).
3. Dead code elimination and unused-parameter removal.
4. Case-of-known-constructor and constant folding (via the CEK machine),
   including inverse builtin simplification and `Force(Delay)` removal.

Specification: [docs/codegen.md](../docs/codegen.md), section
"6. Optimizations".

## Prerequisites

- Plan 07 through chunk 11 (`assemble` calls `nash_ir::optimize::run`,
  which is the identity until this plan lands) and chunk 12 (the
  `Vesting` baseline).
- `Core::walk` / `Core::map` from plan 07 chunk 8.

## Crates touched

`crates/nash-ir` (all passes), `crates/nash-codegen` (harness, `assemble`
wiring, `Case(Bool)` lowering tweak), `crates/nash-plutus` (nothing new;
`flat::encode` and `Program::eval` are used as they are).

## Reference files

Aiken `crates/uplc/src/optimize.rs`: `optimize_repeatedly`,
`aiken_optimize_and_intern` (the pass order and fixed-point loop).

Aiken `crates/uplc/src/optimize/shrinker.rs`:

- `OccurrenceTracker`, `VarLookup`, `var_occurrences` — occurrence
  counting with delay awareness.
- `lambda_reducer`, `inline_reducer`, `identity_reducer`,
  `substitute_var`, `substitute_single_var`, `is_a_builtin_wrapper`.
- `builtin_force_reducer`, `forceable_wrapped_names`,
  `builtin_curry_reducer`, `CurriedBuiltin`, `BuiltinArgs`,
  `try_curry_builtin`, `can_curry_builtin`, `is_order_agnostic_builtin`.
- `builtin_eval_reducer`, `is_error_safe`, `cast_data_reducer`,
  `force_delay_reducer`, `case_constr_apply_reducer`,
  `convert_arithmetic_ops`, `flip_constants`.
- `Scope`, `ScopePath` — the common-ancestor logic reused for hoisting.

Aiken `crates/uplc/src/optimize/interner.rs` — `CodeGenInterner` (the
uniquifier).

Elm `elm/compiler/src/Optimize/Expression.hs` — Elm's optimizer is a
different target but shows the shape of a tree-walking `Optimize` pass over
`Can.Expr`.

## Conventions

- Every pass has the signature `fn(&Builder<'a>, &'a Core<'a>) -> &'a Core<'a>`
  and is pure: input untouched, output freshly allocated where changed
  (`Core::map` rebuilds only the spine above a change).
- Every pass is an `insta` snapshot test on pretty `Core` (before/after)
  and a budget test on the CEK machine.
- Correctness bar: a pass may only change a program to one with the same
  result, logs, and error behaviour. "Cannot throw" is the one analysis
  every pass shares.

---

## Chunk 1 — Traversals, hygiene, occurrence analysis

**Files**

- `crates/nash-ir/src/core.rs` (`walk`, `map`, `free_vars`)
- `crates/nash-ir/src/uniquify.rs` (new)
- `crates/nash-ir/src/occurrences.rs` (new)
- `crates/nash-ir/src/analysis.rs` (new: `cannot_throw`, `size`)
- `crates/nash-ir/src/lib.rs`

**Change**

Add the shared machinery. `uniquify` renumbers every binder so that no two
binders in the program share a `unique` (codegen already tries; this pass
is the guarantee and runs first and last so a bug in a pass shows up as
an assertion, not as capture). `Occurrences` counts uses of each binder
with the delay/lambda context Aiken's `VarLookup` tracks.

**Code**

```rust
// core.rs
impl<'a> Core<'a> {
    pub fn walk(&self, f: &mut impl FnMut(&Core<'a>)) { ... }
}

/// Rebuild-on-change traversal: `f` returns `Some(new)` to replace a node
/// (children of `new` are not revisited), `None` to recurse into it.
pub fn map<'a>(build: &Builder<'a>, core: &'a Core<'a>, f: &mut impl FnMut(&'a Core<'a>) -> Option<&'a Core<'a>>) -> &'a Core<'a>;

/// Capture-free because every binder is unique (uniquify.rs).
pub fn substitute<'a>(build: &Builder<'a>, core: &'a Core<'a>, name: Name<'a>, with: &'a Core<'a>) -> &'a Core<'a>;
```

```rust
// uniquify.rs
//! Assign a fresh `unique` to every binder, in one pass, so substitution
//! never captures. Port of Aiken's CodeGenInterner in spirit; Nash names
//! are already `text + unique`, so this only renumbers.

pub fn uniquify<'a>(build: &Builder<'a>, core: &'a Core<'a>) -> &'a Core<'a>;

/// Panics if two binders share a unique or a variable is unbound.
pub fn check_hygiene(core: &Core<'_>);
```

```rust
// occurrences.rs
#[derive(Clone, Copy, Debug, Default)]
pub struct Occurrence {
    pub count: u32,
    /// Some use sits under a `Lam` or `Delay` relative to the binder, so the
    /// bound value may be evaluated zero or many times there.
    pub under_lambda: bool,
}

pub struct Occurrences<'a> {
    map: HashMap<Name<'a>, Occurrence>,
}

impl<'a> Occurrences<'a> {
    pub fn of(core: &Core<'a>) -> Self;
    pub fn get(&self, name: Name<'a>) -> Occurrence { self.map.get(&name).copied().unwrap_or_default() }
}
```

```rust
// analysis.rs
/// Evaluating this term can neither fail nor loop nor log.
pub fn cannot_throw(core: &Core<'_>) -> bool {
    match core {
        Core::Var(_) | Core::Lit(_) | Core::Lam { .. } | Core::Delay(_) => true,
        Core::Builtin { func, args } => args.len() < func.arity() && args.iter().all(|a| cannot_throw(a)),
        Core::Constr { fields, .. } => fields.iter().all(|f| cannot_throw(f)),
        _ => false,
    }
}

/// Approximate flat-encoded size in bytes; the inliner's currency.
pub fn size(core: &Core<'_>) -> usize {
    let mut n = 0;
    core.walk(&mut |c| n += match c {
        Core::Var(_) => 1,
        Core::Lit(k) => literal_size(k),
        Core::Lam { params, .. } => params.len(),
        Core::App { args, .. } => args.len(),
        Core::Let { .. } => 2,
        Core::Builtin { func, args } => 1 + func.force_count() + args.len(),
        Core::Case { branches, .. } => 1 + branches.len(),
        Core::Constr { fields, .. } => 1 + fields.len(),
        Core::Field { arity, .. } => 1 + *arity as usize,
        Core::Trace { .. } => 3,
        Core::Delay(_) | Core::Force(_) | Core::Error => 1,
        Core::LetRec { .. } => unreachable!("rewritten before optimization"),
    });
    n
}
```

**Aiken reference**: `OccurrenceTracker::new`, `var_occurrences`,
`VarLookup::delay_if_found`, `substitute_var`, `CodeGenInterner`.

**Tests**

- `uniquify_renumbers_shadowing`: `let x = 1 in let x = x in x` gets two
  uniques; `check_hygiene` passes after, panics on a hand-built duplicate.
- `occurrences_under_lambda`: `let x = 1 in \y -> x` reports
  `under_lambda`.
- `cannot_throw_partial_builtin`: `headList` with zero args is safe, with
  one is not.
- `size_monotone`: `size(App(f, [a]))` > `size(f)`.

**Done when**: unit tests pass; `assemble` calls `uniquify` then
`check_hygiene` at the start and end of `optimize::run`.

---

## Chunk 1a — A-normal form before optimization

**Accepted decision: ANF is the first transformation after binder hygiene.**
This is pending implementation, like the rest of Plan 08. Reuse existing Core
Let/LetRec/Case/Lam/Delay nodes rather than adding a parallel optimizer IR.

Pipeline:

```text
Core with LetRec -> unique names -> ANF -> main optimization passes
                 -> recursion rewrite -> ANF for generated code
                 -> cleanup passes -> UPLC lowering
```

Current assembly rewrites recursion immediately before lowering; the optimizer is
not installed yet. For `O1`/`O2`, assembly must run the main optimizer before that
rewrite and cleanup afterward. `O0` keeps the current path: recursion rewrite
then lowering, without optimizer normalization or reduction.

**Recursion boundary**

- Normalize and optimize `LetRec` function bodies inside their parameter scopes;
  retain the group's simultaneous binding scope and its continuation. Do not
  evaluate a recursive body while creating the group.
- Simplify bodies and calls while function identities and recursive groups are
  explicit. Remove unreachable group members by reachability from the continuation
  (including escaping function references), not by counting internal self uses.
- Do not unfold recursive calls in the fixed-point inliner. Ordinary nonrecursive
  helpers may still be inlined into recursive bodies. Post-rewrite cleanup must
  likewise avoid repeatedly expanding generated self-application or dispatchers.
- Remove unused recursive parameters only when all affected uses are known and
  rewritable, including calls within the group. Preserve argument evaluation and
  application staging; keep signatures for escaping/partial uses when unsafe to
  rewrite. Keep at least one parameter where removing all would create an
  unsupported zero-parameter recursive value.
- Recompute recursive groups and static-parameter metadata after changes to calls,
  parameter positions, or reachability, immediately before recursion rewrite.
  Rechecking stale hints alone is insufficient: newly proven static parameters
  must be discovered. For example, deleting a dead call that changes `config`
  can let the worker capture `config` instead of forwarding it on each call.
- Recursion rewrite remains in `nash-codegen`; optimization remains in `nash-ir`.
  Assembly coordinates the two phases without introducing an IR-to-codegen
  dependency. Rewrite recursion once, using a collision-free fresh-name supply.
- Normalize generated wrappers, self-applications, constructor packets and cases
  back into ANF. Then simplify safe applications, aliases, unused bindings and
  force/delay pairs; at O2 also fold known cases and constants. Reuse the same
  pass implementations and semantic checks, rather than adding another optimizer.
  Preserve selected-branch execution, strict arguments and delayed worker bodies.

**Representation invariant**

- Variables and literals are atoms. Lambdas and delays are values whose bodies
  are normalized recursively within their own scopes; a zero-argument builtin
  reference can remain atomic. The exact atom predicate must be shared by the
  normalizer, invariant checker, and optimization passes.
- Name non-atomic intermediate computations when used as operands. Bindings
  make evaluation order explicit; they do not eagerly evaluate lambda/delay bodies.
- Application operands, builtin arguments, constructor fields, projections,
  force operands, and case subjects use atoms. Let right-hand sides and tail
  positions can contain computations; case branches have their own ANF bodies.
- Normalize existing let right-hand sides and reassociate administrative lets
  without capture. Do not bind every literal or introduce pointless alias lets.
- Supply fresh binder identities and accurate operand/result types through the
  existing type/representation machinery. Do not invent a fake type or erase
  representation merely to manufacture a binder; establish how intermediate
  application types are obtained before wiring normalization into assembly.

Example (schematic Core):

```text
addInteger (multiplyInteger a b) (subtractInteger c d)

let product = multiplyInteger a b in
let difference = subtractInteger c d in
addInteger product difference
```

**Strictness and lazy boundaries**

- Preserve the actual Core/UPLC evaluation order, failure, termination, and logs.
  ANF exposes order; it does not grant permission to reorder computations.
- Keep branch-local bindings inside their branches and lambda/delay bindings
  inside their bodies. Never hoist work from an unselected or uncalled body.
- Preserve trace timing: evaluate the message, emit the trace, then evaluate its
  body as current lowering does. Do not pull body computations before the trace.
- Preserve application staging. N-ary App lowers to successive applications;
  applying an earlier argument may fail before a later argument is evaluated.
  Normalize with explicit intermediate applications where necessary, rather than
  hoisting every argument computation ahead of the entire application. Test
  partial/over-application and intermediate failure.
- Preserve strict constructor-field evaluation even when later case folding
  selects a body that ignores fields. Big field extraction and shared wildcard
  helpers stay in their existing selected/delayed scopes.

**Pass contract**

Add `anf::normalize` and `anf::check` in `nash-ir` (or its existing traversal
organization). Normalize at entry and again after recursion rewrite; each
optimizer pass must preserve ANF, using local normalization/reassociation when a
rewrite introduces computations.
Check the invariant after each pass in tests/debug builds.

Inlining must not substitute a non-atomic computation into an atomic operand.
It can propagate atoms, splice an inlined function's binding sequence at its call
site, and simplify/reassociate lets while keeping evaluation order. Single use
alone does not justify moving a strict binding into a conditional branch or past
an effect/failure. Update chunk 3's historical tree-substitution sketch to this
contract; don't repeatedly inline out of ANF and normalize back into identical
bindings. Recompute occurrence/effect information after relevant rewrites.

Case/constant folding, force caching, currying, and dead-binding removal must
also preserve the invariant. Administrative bindings are an optimizer structure,
not evidence of a speedup: measure emitted UPLC as well as Core and ensure binding
introduction does not hide regressions. No new backend or speculative optimizer
framework is required for ANF.

**Acceptance checklist**

- [ ] Implement normalization and structural invariant checking with fresh names
  and valid types; reuse existing Core traversal/substitution facilities.
- [ ] Snapshot nested applications/builtins, existing let nesting, case subjects,
  constructor fields, projections, force/delay, both explicit `LetRec` and
  recursion-rewritten Core.
- [ ] Differentially evaluate before/after ANF: values, traces, failures, subject
  once, left-to-right evaluation, trace-before-body, partial application, and
  unselected branch/unforced delay/uncalled lambda behavior.
- [ ] Reuse the Big/little wildcard regression fixtures, including shared helpers,
  ignored fields, and function-valued results. ANF must not force helpers early.
- [ ] Test idempotence of normalization and hygiene/ANF preservation after every
  optimization pass. Include no-op and same-size rewrites in convergence tests.
- [ ] Test dead recursive members, self/mutual recursion, escaping and partial
  recursive calls, safe unused-parameter removal, and all-static workers.
- [ ] Test that dead-branch removal exposes a newly static argument, and parameter
  removal cannot leave stale static indices. Verify recursion is rewritten once
  and neither phase repeatedly unfolds recursive calls.
- [ ] Preserve raw O0 snapshots; add separate ANF and optimized snapshots. Record
  temporary CPU/memory/serialized-size findings, then remove experiment code.

**Done when:** ANF invariants and semantic equivalence are tested, and every later
pass explicitly consumes/preserves ANF before enabling the optimizer.

---

## Chunk 2 — Temporary performance experiments

Measure CPU, memory and serialized size for representative programs while choosing
optimizations. Use the existing evaluator and codegen helpers in temporary code.
Compare vesting paths, list traversal, static recursion, Data matching, validation
and decoding. Prefer memory when costs are close, then CPU.

Record the inputs, cost model and findings in the implementation notes. Remove
experiment code and fixtures when the comparison is complete. Do not add benchmark
targets, committed budget baselines or CI performance gates. Keep functional
snapshots that show the selected lowering and its results.

**Done when**: findings are recorded and temporary experiment code is removed.

---

## Chunk 3 — Inliner

**Files**

- `crates/nash-ir/src/inline.rs` (new)
- `crates/nash-ir/src/optimize.rs` (new: `run` with the pass list)

**Change**

Adapt the historical sketch below to the ANF contract in chunk 1a. Never insert
a compound expression into an atomic operand; keep strict evaluation at its
original execution point unless a separate safety proof permits movement.

Traverse `LetRec` bodies without unfolding recursive calls, as specified in
chunk 1a. The rules below also apply inside those bodies.

One pass, three rules:

1. **Value bindings.** `Let x = v in b` where `v` is a `Var`, `Lit`
   (except `string` constants, which stay hoisted), zero-argument
   `Builtin`, or a lambda that is a "builtin wrapper" (`\a b -> Builtin(f, [a, b])`)
   is substituted everywhere. (Aiken `lambda_reducer`.)
2. **Single-use bindings.** `Let x = v in b` with `count == 1` is
   simplified only when ANF and evaluation semantics are preserved. A use outside
   a lambda can still be inside an unselected branch or after a failing operation.
   Track execution scope/order rather than using `under_lambda` alone as proof.
   The pseudocode below is historical and must be updated accordingly.
3. **Small lambdas.** `App(Lam(ps, body), args)` and
   `Let f = Lam(ps, body) in b` where `size(body) <= INLINE_LAMBDA_SIZE`
   are beta-reduced at every saturated call site, binding each argument
   with a `Let` (so rules 1–2 decide whether it is substituted). Unused
   bindings are left for chunk 5.

**Code**

```rust
pub const INLINE_LAMBDA_SIZE: usize = 12;

pub fn inline<'a>(build: &Builder<'a>, core: &'a Core<'a>) -> &'a Core<'a> {
    let occ = Occurrences::of(core);
    map(build, core, &mut |node| match node {
        Core::Let { binder, value, body } => {
            let o = occ.get(binder.name);
            let value = inline(build, value);
            if is_value_binding(value) || (o.count == 1 && (!o.under_lambda || cannot_throw(value))) {
                Some(inline(build, substitute(build, body, binder.name, value)))
            } else if let Core::Lam { params, body: lam_body } = value && size(lam_body) <= INLINE_LAMBDA_SIZE && all_uses_saturated(body, binder.name, params.len()) {
                Some(inline(build, beta_at_calls(build, body, binder.name, params, lam_body)))
            } else {
                None
            }
        }
        Core::App { func: Core::Lam { params, body }, args } if args.len() == params.len() => {
            Some(params.iter().zip(args.iter()).rev().fold(*body, |b, (p, a)| build.let_(*p, a, b)))
        }
        _ => None,
    })
}

fn is_value_binding(value: &Core<'_>) -> bool {
    match value {
        Core::Var(_) => true,
        Core::Lit(Constant::String(_)) => false,
        Core::Lit(_) => true,
        Core::Builtin { args, .. } => args.is_empty(),
        Core::Lam { params, body } => is_builtin_wrapper(params, body),
        _ => false,
    }
}

/// `\a b -> Builtin(f, [a, b])` or the same with literal arguments mixed in.
fn is_builtin_wrapper(params: &[Binder<'_>], body: &Core<'_>) -> bool {
    matches!(body, Core::Builtin { args, .. } if args.iter().all(|a| matches!(a, Core::Var(v) if params.iter().any(|p| p.name == *v)) || matches!(a, Core::Lit(_))))
}
```

The size heuristic is the only tunable. `INLINE_LAMBDA_SIZE = 12` is
about one builtin call with three arguments plus a `case`; it is chosen so
that `Field` selectors, Data builtin wrappers and decision-tree leaves
used twice inline, and a recursive decoder does not.

**Aiken reference**: `lambda_reducer` (1753), `inline_reducer` (2316),
`is_a_builtin_wrapper` (2797), `substitute_single_var` (1461),
`identity_reducer` (2252; Nash's rule 1 covers `\x -> x`).

**Tests** (`inline.rs`, `assert_pass_snapshot!(inline, src)` prints
`Core` before and after):

- `inline_single_use_let`: `let x = f 1 in g x` -> `g (f 1)`.
- `keep_single_use_under_lambda`: `let x = f 1 in \y -> x` unchanged.
- `inline_single_use_under_lambda_when_safe`: `let x = 1 in \y -> x` ->
  `\y -> 1`.
- `inline_multi_use_var`: `let x = y in (x, x)` -> `(y, y)`.
- `keep_string_constant`: `let m = "hello" in (trace m 1, trace m 2)`
  unchanged.
- `beta_reduce_small_lambda`: `let sel = \a b c -> b in sel 1 2 3` -> `2`
  (after chunk 5 removes the dead lets).
- `keep_large_lambda`: a `validate#Datum`-sized lambda used twice is
  not inlined.
- budgets: `vesting_*`, `data_match`, `decoder_datum` must improve;
  record the temporary comparison.

**Done when**: snapshots accepted; temporary measurements confirm the intended
improvement and experiment code is removed.

---

## Chunk 4 — Builtin force caching and constant currying

**Files**

- `crates/nash-ir/src/builtins.rs` (new)

**Change**

Two rewrites over the whole program, run once (not in the fixed-point
loop):

1. **Force caching.** Every `Builtin { func, args }` with
   `func.force_count() > 0` becomes `App(Var forced_f, args)` where
   `forced_f` is bound once at the program root to `Builtin { func, args: [] }`
   (which lowers to `force^k (builtin f)`). Saves a `force` per call;
   costs one root `Let` per distinct forced builtin. (Aiken
   `builtin_force_reducer` + `run_once_pass`.)
2. **Constant currying.** For builtins where the first argument may be
   a constant that repeats (`equalsInteger 0 _`, `lessThanInteger _ 10`,
   `appendByteString #"" _`, ...) and which are order-agnostic or take the
   constant first, `Builtin(f, [Lit k, x])` occurring at least twice
   becomes `App(Var f_k, [x])` with `f_k = Builtin(f, [Lit k])` bound at
   the lowest common ancestor scope of the uses. (Aiken
   `builtin_curry_reducer`, `CurriedBuiltin`, `is_order_agnostic_builtin`,
   `Scope::common_ancestor`.)

**Code**

```rust
pub fn cache_forces<'a>(build: &Builder<'a>, core: &'a Core<'a>) -> &'a Core<'a> {
    let mut forced: BTreeMap<DefaultFunction, Binder<'a>> = BTreeMap::new();
    let body = map(build, core, &mut |node| match node {
        Core::Builtin { func, args } if func.force_count() > 0 => {
            let b = *forced.entry(*func).or_insert_with(|| build.fresh_binder(forced_name(*func), builtin_ty(*func)));
            Some(if args.is_empty() { build.var(b.name) } else { build.app(build.var(b.name), args) })
        }
        _ => None,
    });
    forced.into_iter().rev().fold(body, |body, (func, b)| build.let_(b, build.builtin(func, &[]), body))
}

pub const CURRY_MIN_USES: usize = 2;

pub fn curry_constants<'a>(build: &Builder<'a>, core: &'a Core<'a>) -> &'a Core<'a>;

fn can_curry(func: DefaultFunction) -> bool {
    matches!(func,
        AddInteger | SubtractInteger | MultiplyInteger | DivideInteger | ModInteger | QuotientInteger | RemainderInteger
        | EqualsInteger | LessThanInteger | LessThanEqualsInteger
        | AppendByteString | EqualsByteString | ConsByteString | LessThanByteString | LessThanEqualsByteString
        | AppendString | EqualsString | EqualsData | ConstrData | MkCons)
}

fn is_order_agnostic(func: DefaultFunction) -> bool {
    matches!(func, AddInteger | MultiplyInteger | EqualsInteger | EqualsByteString | EqualsString | EqualsData)
}
```

`curry_constants` collects `(func, constant)` pairs with their `Scope`
paths (a `Vec<u32>` of child indices from the root, as Aiken's
`ScopePath`), keeps those with `>= CURRY_MIN_USES`, binds each at the
common ancestor, and rewrites the uses. For order-agnostic builtins a
constant in second position is moved first.

**Aiken reference**: `builtin_force_reducer` (1813), `run_once_pass`
(2907), `builtin_curry_reducer` (3084), `BuiltinArgs::args_from_arg_stack`
(657), `CurriedArgs::merge_node_by_path` (821), `flip_constants` (2753).

**Tests**

- `forces_cached_once`: three `headList` calls -> one root binding, three
  `App`s.
- `partial_builtin_becomes_var`: a bare `Builtin(HeadList, [])` becomes the
  cached var.
- `curry_equals_zero`: two `equalsInteger n 0` (constant second,
  order-agnostic) -> one `equalsInteger 0` binding.
- `no_curry_single_use`.
- `curry_scope_is_common_ancestor`: uses in two `case` branches bind above
  the `case`, uses in one branch bind inside it.
- budgets: `list_length_100`, `data_match`, `validate_datum` must improve
  (fewer `force`s).

**Done when**: snapshots accepted; measurement findings recorded and experiment
code removed.

---

## Chunk 5 — Dead code elimination and unused parameters

**Files**

- `crates/nash-ir/src/dce.rs` (new)

**Change**

1. **Dead lets.** `Let x = v in b` with `count == 0` and `cannot_throw(v)`
   becomes `b`. (A binding that can throw is kept: dropping it would turn
   a failing program into a succeeding one.) Top-level bindings the root
   does not reach are removed the same way, since `assemble` makes them
   `Let`s.
2. **Unused parameters.** For `Let f = Lam(ps, body) in b` where every
   use of `f` in `b` is the head of a saturated `App`, each parameter with
   zero occurrences in `body` is removed from `ps` and from every call
   site, provided every dropped argument `cannot_throw` (otherwise it is
   kept as a `Let _ = arg` at the call site, which rule 1 then keeps or
   drops). A parameter is never removed from a function whose `Lam` is the
   `inner` of a self-application (its first parameter is the function
   itself; it is used), so plan 07's static-param lifting is preserved.

Before recursion rewrite, also apply the `LetRec` reachability and recursive
parameter rules in chunk 1a. The historical sketch below covers only ordinary
`Let`/`Lam`; extend it to recursive groups and refresh affected metadata.

**Code**

```rust
pub fn dce<'a>(build: &Builder<'a>, core: &'a Core<'a>) -> &'a Core<'a> {
    let occ = Occurrences::of(core);
    map(build, core, &mut |node| match node {
        Core::Let { binder, value, body } if occ.get(binder.name).count == 0 && cannot_throw(value) => Some(dce(build, body)),
        Core::Let { binder, value: Core::Lam { params, body: lam }, body } => {
            let unused: Vec<u16> = params.iter().enumerate()
                .filter(|(_, p)| occ.get(p.name).count == 0)
                .map(|(i, _)| i as u16)
                .collect();
            if unused.is_empty() || !all_uses_saturated(body, binder.name, params.len()) { return None; }
            Some(drop_params(build, *binder, params, lam, body, &unused))
        }
        _ => None,
    })
}

fn drop_params<'a>(build: &Builder<'a>, f: Binder<'a>, params: &[Binder<'a>], lam: &'a Core<'a>, body: &'a Core<'a>, unused: &[u16]) -> &'a Core<'a>;
```

**Aiken reference**: `inline_reducer`'s "strip out unused terms that can't
throw" arm (2316), `remove_inlined_ids` (2730). Aiken has no
unused-parameter pass; Nash needs one because decision-tree leaves are
hoisted as lambdas over all their pattern variables.

**Tests**

- `drop_unused_safe_let`: `let x = 1 in 2` -> `2`.
- `keep_unused_throwing_let`: `let x = fail "no" in 2` unchanged.
- `drop_unreachable_top_level`: a module with an unused helper loses it.
- `drop_unused_param`: `let f = \a b -> a in f 1 2` -> `let f = \a -> a in f 1`
  (then chunk 3 inlines).
- `keep_param_when_arg_throws`: `f 1 (fail "x")` keeps a `let _ = fail`.
- `keep_self_param`: the chunk 8 `sumTo` program keeps its self parameter.
- budgets: `decoder_datum`, `vesting_*` must improve.

**Done when**: snapshots accepted; measurement findings recorded and experiment
code removed.

---

## Chunk 6 — Case-of-known-constructor, constant folding, inverse builtin simplification, Big-list fast paths

**Files**

- `crates/nash-ir/src/fold.rs` (new)
- `crates/nash-ir/src/fastpath.rs` (new): rewrites a monomorphized call of
  core's elementwise `Eq (list 'a)` method at a ground Big element type
  into `equalsData (listData a) (listData b)`. Do not apply this rewrite to
  `Ord` or `Show`: Big representation does not fix user ordering or rendering. Keyed on the core impl's
  `ImplRef` plus the ground `MonoKey`; semantics identical because Big
  equality is structural `equalsData` per element. Budget test: `list Int`
  equality of 100 elements must cost one `equalsData` plus two `listData`.
- `crates/nash-codegen/src/comptime.rs` (`eval_closed` reused)
- `crates/nash-codegen/src/lower.rs` (`Case(Bool)` without delay when
  both branches are values)

**Change**

One bottom-up pass with these rules:

| Before | After | Condition |
|---|---|---|
| `Case(Tag, Constr(i, fs), bs)` | `Let b_j = f_j in body_i` | always |
| `Case(Bool, Lit true/false, [t, e])` | `t` / `e` | always |
| `Case(Int, Lit k, bs, d)` | matching branch or `d` | always |
| `Case(Bytes, Lit k, ..)` | same | always |
| `Case(List, Lit [], ..)` / `Lit (x :: xs)` | `nil` / `Let h, t in cons` | always |
| `Case(Data, Lit data, ..)` | the branch for its shape, payload bound to a `Lit` | always |
| `Field(Constr(_, fs), i)` | `f_i` | every other `f_j` `cannot_throw` |
| `Builtin(f, lits)` saturated | `Lit(result)` | `is_error_safe(f, lits)` |
| `Builtin(UnIData, [Builtin(IData, [x])])` and the other three pairs | `x` | always |
| `Force(Delay(x))` | `x` | always |
| `App(App(f, as), bs)` | `App(f, as ++ bs)` | always |
| `Case(Bool, Builtin(IfThenElse, [c, Lit true, Lit false]), ..)` | `Case(Bool, c, ..)` | always |

**Single-field pair projection**

Compare native `CaseKind::Pair` with `FstPair`/`SndPair` when exactly one
branch binder is used. For a valid one-branch pair case with no default:

```text
case p of pair a _ -> body a   => let a = fstPair p in body a
case p of pair _ b -> body b   => let b = sndPair p in body b
```

A body that directly returns the selected field reduces to the projection itself.
Use binder identities/occurrence analysis, retain accurate field types, and
preserve ANF and subject evaluation exactly once. Keep the projection strict at
the original case point, including when its result is only used inside a returned
lambda or delay. Do not discard evaluation of either field when simplifying a
known pair construction. Both fields used keeps the pair case; neither field
used is outside this rewrite and must retain required evaluation/shape checks.
This applies to native builtin pairs, including `unConstrData`'s tag/fields pair,
not arbitrary two-field ADTs.

Measure emitted UPLC CPU, memory and serialized size for both forms, including
builtin forces and branch-lambda applications. Cover direct projection and a
larger branch body, and compare standalone versus cached builtin forces. Record
which form wins and any tradeoff before choosing a deterministic lowering rule;
do not add per-program tuning machinery. Keep O0 pair-case snapshots unchanged.

Constant folding evaluates the saturated builtin on the CEK machine
through plan 07's `eval_closed` (which needs no bindings for a
literal-only term). `is_error_safe` is ported from Aiken and lists, per
builtin, the argument shapes under which evaluation cannot fail
(division by a non-zero literal, `headList` of a non-empty literal list,
integer arithmetic on integer literals, `iData` on an integer, ...).
Everything not listed is not folded: `fail`s must stay `fail`s at
runtime, not become compile errors.

**Code**

```rust
pub struct Folder<'a, F: FnMut(&'a Core<'a>) -> Option<&'a Constant<'a>>> {
    pub build: &'a Builder<'a>,
    /// Evaluates a closed, error-safe builtin application; `None` when the
    /// evaluator declines (budget, unsupported constant).
    pub eval: F,
}

pub fn fold<'a>(build: &Builder<'a>, eval: &mut impl FnMut(&'a Core<'a>) -> Option<&'a Constant<'a>>, core: &'a Core<'a>) -> &'a Core<'a>;

pub fn is_error_safe(func: DefaultFunction, args: &[&Core<'_>]) -> bool {
    let all_ints = || args.iter().all(|a| matches!(a, Core::Lit(Constant::Integer(_))));
    match func {
        AddInteger | SubtractInteger | MultiplyInteger | EqualsInteger | LessThanInteger | LessThanEqualsInteger | IData => all_ints(),
        DivideInteger | ModInteger | QuotientInteger | RemainderInteger =>
            all_ints() && !matches!(args[1], Core::Lit(Constant::Integer(i)) if i.is_zero()),
        AppendByteString | EqualsByteString | LessThanByteString | LessThanEqualsByteString | LengthOfByteString | BData | Sha2_256 | Sha3_256 | Blake2b_256 | Blake2b_224 | Keccak_256 =>
            args.iter().all(|a| matches!(a, Core::Lit(Constant::ByteString(_)))),
        ConsByteString => matches!(args[0], Core::Lit(Constant::Integer(i)) if (0..=255).contains(i)) && matches!(args[1], Core::Lit(Constant::ByteString(_))),
        AppendString | EqualsString | EncodeUtf8 => args.iter().all(|a| matches!(a, Core::Lit(Constant::String(_)))),
        HeadList | TailList => matches!(args[0], Core::Lit(Constant::ProtoList(_, xs)) if !xs.is_empty()),
        NullList => matches!(args[0], Core::Lit(Constant::ProtoList(..))),
        FstPair | SndPair => matches!(args[0], Core::Lit(Constant::ProtoPair(..))),
        UnIData => matches!(args[0], Core::Lit(Constant::Data(PlutusData::Integer(_)))),
        UnBData => matches!(args[0], Core::Lit(Constant::Data(PlutusData::ByteString(_)))),
        UnListData => matches!(args[0], Core::Lit(Constant::Data(PlutusData::List(_)))),
        UnMapData => matches!(args[0], Core::Lit(Constant::Data(PlutusData::Map(_)))),
        UnConstrData => matches!(args[0], Core::Lit(Constant::Data(PlutusData::Constr { .. }))),
        ConstrData | ListData | MapData | MkCons | MkPairData | EqualsData | SerialiseData => args.iter().all(|a| matches!(a, Core::Lit(_))),
        _ => false,
    }
}
```

The `eval` closure in `assemble` is
`|core| nash_codegen::comptime::eval_closed(arena, &[], core).ok()`.

Protocol 11 lowering already uses native `case` for `Case(Bool)`, preserving
lazy branches without `delay`/`force`. The earlier proposed eager
`ifThenElse` lowering is superseded; Plan 08 remains deferred.

**Aiken reference**: `builtin_eval_reducer` (2674), `is_error_safe`
(412), `cast_data_reducer` (2522), `force_delay_reducer` (2448),
`case_constr_apply_reducer` (2058), `convert_arithmetic_ops` (2652),
`inline_constr_ops` (2490).

**Tests**

- `case_known_constr`: `case Some 3 of Some x -> x; None -> 0` -> `3`
  after chunks 3+5.
- `case_known_bool`, `case_known_int_default`, `case_known_data`.
- `field_of_constr`: `Field(Constr 0 [a, fail], 0)` is not simplified;
  `Field(Constr 0 [a, b], 0)` is.
- `pair_first_only`, `pair_second_only`: direct return and larger body; snapshot
  baseline case and candidate projection UPLC and verify equivalent evaluation.
- `pair_both_used`, `pair_neither_used`: do not apply the single-field rewrite.
- `pair_projection_strict`: traced/failing subject, failure in an ignored field
  of a strict pair construction, and field captured in a returned function/delay;
  preserve result, logs, failure and evaluation count.
- `pair_from_unconstr`: tag-only and fields-only access retain malformed-Data
  failures and do not duplicate `unConstrData`.
- `fold_add`: `addInteger 40 2` -> `Lit 42`.
- `no_fold_div_zero`: `divideInteger 1 0` stays.
- `no_fold_head_nil`: `headList []` stays.
- `cancel_un_i_data_i_data`.
- `force_delay`.
- `flatten_apps`.
- `if_of_values_is_strict` (lowering snapshot).
- budgets: `sum_static`, `data_match`, `validate_datum`, `decoder_datum`
  must improve.

**Done when**: snapshots accepted; measurement findings recorded and experiment
code removed.

---

## Chunk 7 — Driver loop and regression gate

**Files**

- `crates/nash-ir/src/optimize.rs`
- `crates/nash-codegen/src/program.rs` (`assemble` wiring)

**Change**

Assembly runs two optimizer phases around the existing recursion rewrite:

```text
O0: recursion::rewrite -> lower
O1/O2: optimize::run -> refresh recursive groups/static metadata
       -> recursion::rewrite -> optimize::cleanup -> lower
```

`run` consumes explicit `LetRec`; `cleanup` consumes the rewritten Core and
normalizes its generated code before reductions. Both use shared pass functions
and fresh names. The sketch below describes the main phase; cleanup reuses its
inlining/DCE loop and, at O2, folding. It need not repeat whole-program force
caching/currying unless generated code exposes new eligible sites. Test the
complete assembly pipeline as well as each phase separately.

Main-phase order and fixed point, following `aiken_optimize_and_intern`:

```rust
/// `--optimize 0|1|2` (docs/cli.md, docs/validators.md); `Options.optimize: u8`
/// in plan 07 maps onto it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    /// No optimization passes; assembly still performs required recursion rewrite.
    O0,
    /// Inlining, DCE, builtin force caching and currying.
    O1,
    /// `O1` plus case-of-known-constructor and CEK constant folding.
    O2,
}

impl Level {
    pub fn from_flag(n: u8) -> Level {
        match n { 0 => Level::O0, 1 => Level::O1, _ => Level::O2 }
    }
}

pub fn run<'a>(build: &Builder<'a>, eval: &mut impl FnMut(&'a Core<'a>) -> Option<&'a Constant<'a>>, core: &'a Core<'a>, level: Level) -> &'a Core<'a> {
    if level == Level::O0 { return core; }
    let core = uniquify(build, core);
    check_hygiene(core);
    let core = anf::normalize(build, core);
    anf::check(core);

    let core = repeat(build, core, |b, c| {
        let c = inline(b, c);
        let c = dce(b, c);
        if level == Level::O2 { fold(b, eval, c) } else { c }
    });
    let core = cache_forces(build, core);
    let core = curry_constants(build, core);
    let core = repeat(build, core, |b, c| dce(b, inline(b, c)));

    check_hygiene(core);
    anf::check(core);
    core
}

// repeat must detect unchanged structure (or use accurate rewrite-progress
// reporting), not merely equal node counts. Each pass preserves/checks ANF.
// Keep generated-name handling deterministic so normalization cannot create
// spurious progress. Detect and resolve rewrite cycles; do not oscillate between
// substitution and reintroducing the same administrative bindings.
```

Convergence tests must include same-size rewrites and a second optimizer run.
A size metric is useful for inlining decisions, not proof of a fixed point.


`assemble` passes `Level::from_flag(build.options.optimize)` to both phases;
recursion rewriting remains required at every level. The plan 07 test macros
gain a variant `assert_eval_snapshot_unoptimized!` so front-end tests keep
readable output, and every existing `Core` snapshot in plan 07 is
re-accepted once with the optimizer on (their evaluation results must not
change; the test asserts that separately by running both).

**Aiken reference**: `optimize.rs` `aiken_optimize_and_intern` (25),
`optimize_repeatedly` (9), `multi_pass` (2965), `afterwards` (3049).

**Tests**

- `run_is_idempotent`: `run(run(x)) == run(x)` on every fixture (pretty
  `Core` equality).
- `cleanup_is_idempotent`: cleanup of recursion-rewritten fixtures reaches a
  fixed point without expanding recursion indefinitely.
- `phase_snapshots`: retain explicit recursive Core, optimized recursive Core,
  rewritten Core, and cleaned Core snapshots for self and mutual recursion.
- `results_unchanged`: every plan 07 evaluation snapshot has the same
  `result` and `logs` with and without the optimizer (a loop over the
  fixtures).
- Temporary measurements compare representative programs before and after the
  optimizer; retain a summary of the findings, then delete experiment code.

**Done when**: idempotence and result snapshots pass in CI; temporary measurement
code is removed.

---

## Open questions

1. **Strictness of dropped arguments.** Chunk 5 keeps arguments that may
   throw. A future strictness analysis could drop more; the conservative
   rule is chosen because a validator that fails must keep failing.
2. **`INLINE_LAMBDA_SIZE` and `CURRY_MIN_USES`** are constants. Making
   them `Options` fields is trivial if a project needs a size/cost
   trade-off knob.
3. **Fusion of source decoding functions** (docs/data.md) is not in this
   plan. It would be a fifth pass after chunk 6, recognizing the
   monomorphized stdlib names.
4. **Cost-model changes.** Historical measurements apply to their recorded model.
   Run a temporary comparison when a new performance decision needs current data.
