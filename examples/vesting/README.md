# Vesting validator

Build both Plutus V3 scripts from the repository root:

```sh
cargo run -p nash-cli -- build examples/vesting --trace-level verbose --compiler-traces
```

The command writes `Vesting` and `VestingParam` as UPLC text, Flat bytes,
and a single CBOR byte string containing the Flat bytes under `build/`.
`Vesting` takes one Data argument: the Plutus V3 ScriptContext. Its redeemer
is the context's second field, and its datum comes from SpendingScript's
optional datum. Missing spending datums and other script purposes fail.
`VestingParam` adds a native integer minimum-lock parameter, which must be
applied off-chain before deployment. The ledger still supplies one argument.

`VestingTx` reads the V3 TxInfo validity interval and extra-signatory list.
Claims require a finite lower validity bound strictly greater than the datum's
deadline (plus the optional minimum lock). These numbers are POSIX milliseconds,
not slots or the current wall-clock time. Cancellations require the owner among
the extra signatories. The helper uses the ledger's Data layout; the fixture
transactions are synthetic, not submission-ready transactions. Datum/redeemer
casts assume this application's encoding; they do not recursively validate it.

The codegen snapshots and serialized artifact tests apply exactly one context
and exercise successful and failed claims and cancellations. The snapshots
compare unoptimized and optimized Core/UPLC and execution outcomes. Budget
regressions run separately in `tools/optimizer-perf`.

Both validators include a `proof` block for partial correctness: every successful
return within the execution limit must satisfy the deadline or owner-signature
policy. `Proof.spendingV3` quantifies over CardanoLedgerApiBlaster's ledger-valid
spending contexts. These are candidate obligations: export and type checking
pass, but the current symbolic evaluator times out during verification.

```sh
nash proof examples/vesting --emit-only
nash proof examples/vesting --fuel 1000 --postcondition-fuel 1500
```

The runtime suite retains artifact and optimizer checks and adds exact-deadline,
just-after-deadline, negative-deadline, wrong-signer and malformed-context cases.
See [proof semantics and current coverage](../../docs/proofs.md).
