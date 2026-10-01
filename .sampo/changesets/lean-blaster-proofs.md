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
cargo/nash-plutus: patch
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

Represent proofs with dedicated source and canonical AST types, symbolic-domain
binders, and distinct execution and successful-return obligations. Reject proof
budgets during parsing and report proof-specific domain, expectation and
postcondition errors before code generation.

Add independent compilation contracts for all 101 builtins and all application
prefixes. Export kernel-checked syntax certificates and universal CEK equality
corollaries for the 91 builtins supported by pinned PlutusCoreBlaster. Document
the ten missing array/value models and the translation-validation scope.

Add one Nash semantic specification per builtin with algebraic laws, representation
roundtrips, boundary cases and invalid-input rejection obligations. Record verified
and pending results explicitly and execute the source obligations in regressions,
including negative controls. Fix host-word index truncation, oversized index panics,
large shift/rotation panics and whole-byte left-shift copying in the local evaluator.
