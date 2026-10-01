---
cargo/nash-source: minor
cargo/nash-ast: minor
cargo/nash-parse: minor
cargo/nash-can: minor
cargo/nash-constrain: minor
cargo/nash-solve: minor
cargo/nash-nitpick: patch
cargo/nash-report: minor
cargo/nash-fmt: minor
cargo/nash-codegen: minor
cargo/nash-driver: minor
cargo/nash-cli: minor
cargo/nash-proof: minor
---

Add module-local proof blocks and a Lean-blaster proof command over compiled UPLC,
with symbolic primitive and Cardano ledger domains, portable pinned Lean projects,
structured results, explicit SMT trust, and CEK execution limits with exhaustion treated as rejection.

Support partial correctness with `Proof.returns`, separate postcondition evaluation
limits, and explicit inconclusive checker exhaustion. Add universal properties
to existing conversion, ordering and arithmetic fixtures and authorization
properties to the vesting examples. Extend deadline, signer and malformed-context
regressions while preserving runtime and oracle coverage.
