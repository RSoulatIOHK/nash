# Explicit optimizer performance checks

This unpublished package has its own Cargo workspace and lockfile. Root
`cargo test`, `cargo test --workspace --all-features`, and
`cargo nextest run --workspace --all-features` do not discover it. There are no
benchmark targets, ignored performance tests, feature gates or CI jobs.

Run from the repository root:

```sh
cargo run --locked --manifest-path tools/optimizer-perf/Cargo.toml -- check
cargo run --locked --manifest-path tools/optimizer-perf/Cargo.toml -- measure
```

`check` compares every row, including inputs, results, trace logs, CPU, memory and
raw Flat size, against `baseline.json`. Both improvements and regressions require
review; none of the metrics has priority. It exits nonzero on drift, a semantic
mismatch, a compilation failure or an exhausted budget. `measure` prints JSON
without changing any baseline. Debug/release profiles produce the same ledger
budgets: these are CEK costs, not Rust wall-clock benchmarks.

The 23 rows include the original 20 covering list traversal, static recursion, Data matching and
misses, field decoding, validation success/failure, the real base Logic helpers,
and six ledger scenarios each for the existing Vesting and VestingParam source
fixtures, plus constant-prefix two-use, cold and loop regressions. Validator CPU/memory include applying the documented ledger arguments;
validator bytes exclude those arguments. Ordinary expression fixtures include
their inputs in the measured program.

The `before` pipeline is O0 (recursion rewrite and lowering). The `after` pipeline
is the accepted static lifting, pre-ANF unused-parameter removal, one ANF normalization and rules 1+2+3+4 plus safe dead-binding, recursive-reachability and force/delay cleanup,
then recursion rewrite, binder freshening and lowering with both Chunk 5 sharing steps. No second normalization
or ANF-dependent cleanup runs after recursion rewriting. Rule 3 was accepted on 27 September 2026. These figures record current behavior, including overhead
from ANF; they are not a claim that the incomplete optimizer beats O0 everywhere.

## Explicit baseline updates

```sh
cargo run --locked --manifest-path tools/optimizer-perf/Cargo.toml -- record /tmp/proposed-perf.json
diff -u tools/optimizer-perf/baseline.json /tmp/proposed-perf.json
```

`record` requires a new path and refuses to overwrite any existing file. Review
changed inputs, outcomes and costs before explicitly copying the proposal over
`baseline.json`. Snapshot acceptance never updates this file. `check PATH` can
check a proposed or deliberately corrupted baseline without changing the original.
The initial baseline records measured accepted-pass behavior, not regression limits
selected from a performance policy.

Reports embed all fixture source inputs and record the Plutus/UPLC versions, bundled default V3 cost model, evaluation
limits, repository revision and Rust version. The nested Cargo.lock pins dependency
versions. Keep source and lockfile with a report to reproduce it. Revision and Rust
version are provenance, not equality gates: exact row/settings comparison detects
cost drift without rejecting unrelated commits or toolchain updates.

## Temporary experiments

```sh
cargo run --locked --manifest-path tools/optimizer-perf/Cargo.toml -- experiment /tmp/Experiment.nash
```

Supply a standalone Nash module with a monomorphic, ground-valued `main` (or one
that fails). It can import Primitive, Builtin, the integration fixture Literal and
Lift modules, base Logic, and the integration fixture Cardano.Tx. It is not a full
project loader. Apply any returned functions inside `main`; the runner rejects
opaque function/delay results instead of pretending to compare them semantically.
The JSON embeds the experiment source. Nothing adds it to permanent baselines.

For alternate optimization algorithms, temporarily edit `accepted` in
`src/main.rs`, run an experiment, record the decision and remove the temporary
change/source. Do not update permanent baselines to bless a trial. Use the same
explicit runner for threshold sweeps; keep sweep scripts outside Cargo discovery.

Each invocation has a 120-second process watchdog covering source compilation,
optimization and evaluation. Each evaluation is capped at 100,000,000 CPU and
2,000,000 memory units. Experiment source is limited to 64 KiB. Budget exhaustion
is a harness failure, even when both pipelines exhaust the budget. These limits
do not bound Cargo's build step; that step does not execute experiment source.

## Maintainer checks

The isolated package needs its own formatting and lint commands:

```sh
cargo fmt --manifest-path tools/optimizer-perf/Cargo.toml --check
cargo clippy --locked --manifest-path tools/optimizer-perf/Cargo.toml --all-targets --all-features -- -D warnings
```

Verify isolation after changing workspace manifests using root `cargo metadata
--no-deps --format-version 1`, `cargo test -- --list`,
`cargo test --workspace --all-features -- --list`, and
`cargo nextest list --workspace --all-features --message-format json`. None may
include the `nash-optimizer-perf` package/binary. Run root tests normally as well.

Chunk 5 force-sharing measurements (isolated from Core optimizations):

```sh
cargo run --locked --manifest-path tools/optimizer-perf/Cargo.toml --example builtin_sharing
```

This compares one-force and two-force builtin references across single/repeated
sites, loops with 0/1/8/64 calls, and an unselected branch. It checks results and
traces, and reports CPU, memory and raw Flat bytes. It is an explicit experiment,
not a root Cargo test. The permanent baseline also includes forced-builtin sharing
in optimized lowering; O0 still uses structural lowering without sharing.

Chunk 5 constant-prefix sharing (accepted at two or more occurrences):

```sh
cargo run --locked --manifest-path tools/optimizer-perf/Cargo.toml --example constant_sharing
```

This compares force sharing alone with additional sharing of one repeated leading
literal argument. It covers 1/2/3/8 call sites, cold and exclusive branches,
recursive calls, an unused lambda, small/large byte strings, and nine successful
source workloads after the accepted Core passes. Synthetic cases isolate lowering;
they do not run the Core optimizer. Size is Flat bytes of the supplied root,
including explicit applications where present. Failed outcomes have semantic
snapshot coverage separately. This is an explicit-only experiment, not a root test.

The permanent optimized baseline includes both Chunk 5 steps. The example keeps
Step 1 as its comparison so Step 2's individual costs remain visible.

## Chunk 6 unused-binding trial

```sh
cargo run --locked --manifest-path tools/optimizer-perf/Cargo.toml --example dead_bindings
```

Compares accepted lowering with/without conservative unused-let elimination
before recursion rewriting: six direct Core cases and nine source workloads.
The source workloads run accepted Core cleanup first. Results and traces must
match; CPU, memory and Flat bytes are printed separately. Dead-binding removal is now accepted in the Core cleanup loop; source workloads
already include it, while the direct Core cases isolate its effect. This experiment
does not update the permanent baseline. It is outside root test
discovery, like the other explicit experiments.

## Chunk 6 recursive reachability experiment

```sh
cargo run --locked --manifest-path tools/optimizer-perf/Cargo.toml --example dead_recursive
```

Compares accepted lowering with/without recursive-member pruning before recursion
rewriting. Includes groups of 1/2/4/8 members with none, one or all reachable;
countdown loops in groups of 2/8 with 0/1/8/64 recursive calls; live/dead delayed
workers; and nine source workloads after accepted Core cleanup. Results and
trace logs must match. CPU, memory and Flat bytes are printed separately. This
experiment is outside root test discovery. Recursive reachability is accepted
in the cleanup loop, so source cases already include it; direct Core cases isolate
its effects. The experiment does not update the permanent baseline.

## Chunk 6 nonrecursive unused-parameter trial

```sh
cargo run --locked --manifest-path tools/optimizer-perf/Cargo.toml --example unused_params
```

Compares a standalone exact-call-only pass after ANF. Direct Core fixtures vary
first/middle/last/all unused parameters, 1/2/8 call sites and selected/cold paths.
Nine existing source workloads and four targeted source fixtures run accepted
cleanup first. The pass leaves partial, escaping, staged and oversaturated uses
unchanged. All-unused helpers become delays forced at each call. The explicit
runner verifies results/logs and prints CPU, memory and Flat sizes; it changes
neither the accepted pipeline nor the baseline and stays outside root tests.


```sh
cargo run --locked --manifest-path tools/optimizer-perf/Cargo.toml --example unused_params_pre_anf
```

Compares three full pipelines: accepted cleanup, removal after cleanup, and
removal after static lifting but before ANF and cleanup. Each normalizes once.
Runs 24 synthetic cases, nine existing source workloads and six targeted source
fixtures, including ordinary literals and effectful retained/discarded arguments.
Results/logs must match; CPU, memory and Flat size are reported for all three.
This 39-case trial is explicit-only and does not change the accepted baseline.
The single-call all-unused case exposes missing force/delay cancellation; keep
that regression visible while evaluating placement.

Pre-ANF removal was accepted on 27 September 2026 and is now included in the
main measured pipeline. The two placement examples retain their original
comparison pipelines so their experiments remain reproducible.

## Chunk 8 direct force/delay trial

```sh
cargo run --locked --manifest-path tools/optimizer-perf/Cargo.toml --example force_delay
```

Runs the same 39 placement fixtures with accepted pre-ANF parameter removal,
then compares direct cancellation alone and cancellation plus existing cleanup.
Each pipeline normalizes once. Results and logs must agree. The experiment
reports CPU, memory and Flat size, stays outside root tests, and never updates
the accepted baseline.

Direct force/delay cancellation is accepted in the cleanup loop. Its standalone
experiment keeps a frozen pre-cancellation control for comparison.
