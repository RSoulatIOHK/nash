# Builtin compilation certificates

The builtin regression validates all **101 entries** in Nash's synthetic
`Builtin` module against a frozen UPLC compilation contract. It generates
**283 source declarations**: a first-class reference and every application
prefix up to full saturation for each builtin. Each declaration passes through
ordinary parsing, canonicalization, inference, specialization, O0 assembly,
and Flat encoding/decoding.

The expected opcode and calling convention are independent of Nash's compiler
inventory and runtime metadata. The contract is recorded in
`crates/nash-driver/tests/fixtures/builtin-compilation/spec.tsv`. Its tags and
signatures come from the [Plutus Core specification](https://plutus.cardano.intersectmbo.org/resources/plutus-core-spec.pdf)
(Tables 4.18, 4.20 and C.8); modeled calling conventions also agree with the pinned
PlutusCoreBlaster `expectedArgs` definition. Inventory changes fail the regression
until the independent contract is updated.

For every declaration, the emitted term must contain the correct builtin,
required forces, supplied arguments in their original order, and the expected
lambda and strict administrative bindings. Reference terms are constructed
directly; the oracle is never compiled through Nash. The regression checks the
opcode, forces and arity separately and rejects swapped opcodes, extra forces
and reversed arguments. The six saturated decoder/projection calls retain a
strict result binding, and all roots retain their O0 declaration binding.
These bindings remain in both terms, so their fuel overhead is included.

## What Lean checks

The opt-in suite preserves a generated `BuiltinCompilation.lean` and diagnostic
log in the system temporary directory. The reusable definitions are in
`crates/nash-driver/tests/fixtures/builtin-compilation/Certificate.lean`.

| Coverage | Certificate |
|---|---|
| All 101 builtins, 283 declarations | Captured syntax equals the independent reference; applying arbitrary argument syntax gives equal results under any observation function |
| 91 builtins supported by pinned PlutusCoreBlaster, 254 declarations | Import the actual Flat bytes with PlutusCoreBlaster's independent decoder and prove the resulting term equals the reference UPLC term |
| Those same 254 declarations | Equal CEK states for every argument term list, semantics variant and fuel limit, preserving unfinished states at exhaustion |
| 91 modeled calling conventions | Force count and value arity agree with PlutusCoreBlaster's `expectedArgs` |

These are Lean kernel-checked equality proofs, using `rfl` and equality rewriting;
no SMT solver, `native_decide`, or admitted Nash theorem is used. The assumption
audit requires all 283 generic observation certificates to be axiom-free.
The CEK corollaries depend only on Lean's standard `propext`, `Classical.choice`
and `Quot.sound` axioms through the imported evaluator; the regression rejects
other dependencies, including `sorryAx` and reduction-oracle axioms.

The statements quantify over arbitrary UPLC argument terms, including errors,
function values and malformed inputs. They do not use a successful-return guard,
so rejection and exhaustion cannot make a wrong compilation pass vacuously.
They compare the complete CEK state at the same fuel, rather than only successful
return values. Crypto builtins are covered as calls to the same primitive;
this does not establish correctness of cryptographic implementations.

## Limits

This is **translation validation of the captured builtin declarations**, not a
formal verification of the Rust compiler for all Nash programs. The source
fixtures instantiate representation-polymorphic parameters as Data. Certificates
do not establish correctness of all type schemes, representation specializations,
compiler intrinsics, or optimizer passes. Those require separate contracts and
simulation theorems. Source `Primitive.coerce` is an intrinsic outside the 101-entry
runtime inventory and is not included.

The pinned Lean model lacks `lengthOfArray`, `listToArray`, `indexArray`,
`insertCoin`, `lookupCoin`, `unionValue`, `valueContains`, `valueData`, `unValueData`
and `scaleValue`. Their compilation syntax is certified, but no PlutusCoreBlaster
CEK theorem is claimed for them. Completing that connection requires array/value
types, Flat decoding, calling conventions, evaluation and corresponding cost
models in PlutusCoreBlaster. CardanoLedgerApiBlaster is not needed for the current
compilation certificates because no ledger-context assumptions are involved.

The link from the actual bytes to Lean terms uses the library's compile-time
Flat decoder. That importer, the Rust syntax capture and the compiler executable
are part of the validation setup; the decoder itself has not been formally
verified. Mathematical fidelity of the pinned builtin models and Cardano's
implementation remains a separate obligation.

## Run

The ordinary compilation regression needs only the Rust toolchain:

```sh
cargo test -p nash-driver --test builtin_compilation
```

With the pinned Lean project already built:

```sh
NASH_PROOF_LEAN_PROJECT=/path/to/built/CardanoLedgerApiBlaster \
  cargo test -p nash-driver --test builtin_compilation -- --ignored --nocapture
```

This follows the same opt-in toolchain arrangement as the live Nash proof
regressions. It does not require Z3 because the certificates use structural
equality rather than SMT verification.
