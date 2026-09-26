---
cargo/nash-ast: minor
cargo/nash-can: minor
cargo/nash-driver: minor
cargo/nash-solve: patch
cargo/nash-report: patch
cargo/nash-docs: patch
---

Add Primitive.map as a native pair-list alias with storable key/value types.
Map builtins and library results preserve the alias. Library Eq implementations
use structural Data equality for Big-element lists and Big/Big maps, and retain
selected element equality for Little elements and mixed maps. No optimizer
special case is needed. Generic callers with unknown element representations
must request container Eq directly. Map equality preserves order and duplicates.

Keep reflexive Lift inference nominal when checking alias identity, so explicit
alias conversions can infer hidden type parameters without a false competitor.
