# Plan 14 — Explicit constructor tags for Big types

Status: pending. This plan records the design decision; source syntax and compiler
support have not landed.

## Goal

Allow a user-defined Big ADT to declare the integers used by its `Data.Constr`
constructors, for compatibility with external on-chain encodings. For example,
three constructors may use tags `59`, `65`, and `122` instead of `0`, `1`, and `2`.
Those integers are preserved in emitted Data and used when matching it.

Little ADTs retain consecutive, zero-based tags in declaration order. Explicit
tags are rejected on little types, including annotations that happen to repeat
the default indices. Types without explicit tags keep their current encoding.

Specification: [representation](../docs/representation.md). This is a language
and representation feature, independent of the deferred Plan 08 optimizer.

## Required invariants

- Keep constructor identity/declaration index separate from its Big Data tag.
  Existing `Ctor.index` uses for coverage, field layout, constructor identity,
  and native little dispatch must not silently become sparse Data tags.
- Reject duplicate Data tags within a type. Report the conflicting declarations.
- Explicit tags apply to nominal Big ADT constructors, both positional and
  labeled. They do not change alias-record encoding or primitive Data shapes.
- Construction, matching, labeled updates, interfaces, and derived codecs must
  agree on the same tag. Imported types retain their declared tags.
- Exhaustiveness and redundancy remain constructor-based, not dependent on
  filling the numeric gaps between tags.
- Never renumber incoming Data tags to make a dispatch table fit. Match the
  declared integers exactly; no table proportional to an arbitrary maximum tag.
- Preserve existing unchecked `fromData`/`coerce` boundaries. This feature does
  not introduce implicit recursive validation or an unknown constructor.
- Keep the consecutive-tag invariant for Core `CaseKind::Tag` and its lowerer.
  Sparse Big tags require a separate comparison path or explicit mapping to a
  compact internal dispatch index before reaching that node.

## Implementation chunks

### 1. Source contract, AST, diagnostics

- [ ] Choose and document constructor-tag syntax in `docs/syntax.md`, including
  its placement on multiline and labeled constructor declarations. Review the
  existing attribute grammar before introducing new syntax.
- [ ] Specify the accepted integer range against the Data representation and
  serialization API; reject unsupported values without truncating to the
  current `u16` declaration index.
- [ ] Decide whether explicit and implicit tags may mix. Prefer all-explicit or
  all-implicit per type to avoid ambiguous assignment and accidental collisions;
  record the final rule before implementation.
- [ ] Write parser/canonicalization snapshots first: sparse and reordered tags,
  duplicate tags, invalid range, little-type rejection, and unchanged defaults.
- [ ] Carry distinct declaration indices and Data tags through source/canonical
  ASTs, type metadata, module interfaces, and cache invalidation/versioning.

### 2. Construction and case lowering

- [ ] Emit declared tags for Big constructors, including nullary constructors
  and labeled updates. Keep little constructor emission unchanged.
- [ ] Extract a Big scrutinee's constructor tag once and match explicit tags
  exactly. Start with equality comparisons for sparse tags; direct native case
  remains available for the existing consecutive-tag layout.
- [ ] Preserve branch order, lazy branch bodies, strict subject evaluation, and
  field extraction only on the matching path.
- [ ] Define and test unknown-tag behavior for explicit constructor branches
  and wildcard-only/constructor-plus-wildcard cases under the existing unchecked
  input contract. Do not accidentally accept an unknown tag as a declared
  constructor or change existing behavior for unannotated types.
- [ ] Test valid sparse tags `59`, `65`, `122`, gaps, tags outside the declared
  set, wrong Data shapes, and missing fields. Cover single-constructor types
  and mixed-arity types so existing shortcuts cannot ignore an explicit tag
  where checking it is required by the matching contract.

### 3. Integration and completion

- [ ] Audit representation-dependent consumers: Lift/lower between Big and
  little twins, Validate/Decode derivation, encoders, interfaces, formatter,
  generated docs, and test/fuzzer constructor generation. Map corresponding
  constructors by identity/order, never by equality of their runtime tags.
- [ ] Ensure pending Plan 11/12 derivation work consumes Data-tag metadata;
  do not claim those deferred features are implemented by this plan.
- [ ] Add source-to-UPLC snapshots and evaluator tests for local/imported types,
  Big/little correspondence, round trips, effects/failures, and unchanged
  encodings for existing declarations. Check supported ledger targets.
- [ ] Update representation/codegen docs and the case explorer with actual
  compiler output. Any later dispatch optimization must preserve these tests.
- [ ] Add changesets for affected crates when implementation lands; run format,
  strict Clippy, and workspace tests. Tick SPEC only when the plan is complete.

## Separate optimizer question

The integer-literal dispatch slot limit discussed alongside this feature remains
a Plan 08 measurement question. It does not restrict the tags users may declare
on Big constructors, and this plan does not choose a cutoff such as 9 or 10.
