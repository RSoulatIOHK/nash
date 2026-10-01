# Semantic builtin proofs

Each builtin has its own Nash file, its compiled source binding, and readable
`proof` blocks describing its behavior. Start with [AddInteger.nash](builtins/AddInteger.nash):
commutativity, associativity, identities, inverses, an unbounded successor law,
Data-represented integer handling, and explicit 64/128-bit boundary witnesses.

There are **324 obligations in 101 files**. The active project compiles and exports
**318 obligations in 99 files**. The two multi-scalar files have six further
obligations in [pending/builtins](pending/builtins); Nash currently cannot encode
the BLS element type in their native list constants. They have a separate project
so this compiler limitation does not prevent exporting the active suite.

The checked-in [verification snapshot](verification.json) records every obligation
as verified, unchecked, timed out, blocked, or refuted in an opaque model.
**64 obligations are SMT-verified** at 300 CEK steps with the pinned Lean 4.24
dependencies. [verified.tsv](verified.tsv) lists that reproducible subset.
Successful SMT verification has the backend's existing `smt_verified` trust level;
it is not a reconstructed Lean kernel proof.

`Proof.int` and `Proof.integer` quantify over arbitrary mathematical integers,
without a bit-width bound. The successor law would fail for wrapping or saturating
addition. Large closed witnesses are constructed from bytes when they exceed the
parser's integer-literal range. Value quantities have a distinct signed 128-bit
contract and must reject overflow rather than inherit unbounded arithmetic laws.

Passing execution obligations require success within the execution limit.
`fail` includes script error and exhaustion. `Proof.returns` checks successful
results only; native regressions also require a successful sample for every such
obligation, including valid compressed BLS points. Native examples supplement the
symbolic checks and do not turn unchecked obligations into universal proofs.

## Run

```sh
# Compile/export every active obligation without Lean.
nash proof proofs --emit-only --fuel 300 --postcondition-fuel 300

# Run all addition laws, including the currently unverified associativity law.
nash proof proofs --lean-project /path/to/built/CardanoLedgerApiBlaster \
  --fuel 300 --postcondition-fuel 300 --match AddInteger --exact

# Execute the actual source obligations and deliberate compiler mutations.
cargo test -p nash-driver --test builtin_proofs

# Recheck the recorded verified subset with Lean and Z3.
NASH_PROOF_LEAN_PROJECT=/path/to/built/CardanoLedgerApiBlaster \
  cargo test -p nash-driver --test builtin_proofs live_builtin_semantics -- --ignored --nocapture

# Attempt any pending active obligation; unsuccessful results fail the test.
NASH_PROOF_MATCH='AddInteger.{associative}' \
NASH_PROOF_LEAN_PROJECT=/path/to/built/CardanoLedgerApiBlaster \
  cargo test -p nash-driver --test builtin_proofs live_builtin_semantics -- --ignored --nocapture
```

The standard regression checks the one-file-per-builtin inventory, compiles and
exports the source proof blocks, executes representative inputs, checks successful
partial-correctness samples, and detects seven deliberate mutations: subtraction
or multiplication substituted for addition, a dropped argument, wrapping,
saturation, reversed byte append, and the wrong pair projection. A live Lean
check also rejected the closed addition contract compiled with subtraction.

## Failure modes and obligations

The contracts follow the [Plutus Core specification](https://plutus.cardano.intersectmbo.org/resources/plutus-core-spec.pdf)
and the pinned model. Each linked file contains the actual Nash statements.

| Possible failure | Corresponding obligations |
|---|---|
| Wrong integer primitive, lost argument, wrap, saturation, signedness | [addition](builtins/AddInteger.nash), [subtraction](builtins/SubtractInteger.nash), [multiplication](builtins/MultiplyInteger.nash), comparison laws and distinguishing results |
| Floor division confused with truncation; zero divisor accepted | [divide](builtins/DivideInteger.nash), [quotient](builtins/QuotientInteger.nash), [mod](builtins/ModInteger.nash), [remainder](builtins/RemainderInteger.nash): reconstruction, signs, rounding, zero-divisor rejection |
| Equality always True, wrong Data variant or comparison ordering | [integer equality](builtins/EqualsInteger.nash), [byte equality](builtins/EqualsByteString.nash), [Data equality](builtins/EqualsData.nash), ordered comparisons |
| Appended inputs reversed; bytes counted as characters | [append](builtins/AppendByteString.nash), [length](builtins/LengthOfByteString.nash): order witnesses and additive lengths |
| Slice offset/count interchanged; negative values mishandled | [slice](builtins/SliceByteString.nash): clamping, empty slices, start/end boundaries |
| Index truncates to a host word or panics above 128 bits | [byte index](builtins/IndexByteString.nash), [array index](builtins/IndexArray.nash), [read bit](builtins/ReadBit.nash), [write bits](builtins/WriteBits.nash): negative, one-past-end, 64-bit and 129-bit indices |
| Byte value out of range; output size exceeds 8192; integer does not fit | [cons](builtins/ConsByteString.nash), [replicate](builtins/ReplicateByte.nash), [integer encoding](builtins/IntegerToByteString.nash): rejection and accepted boundary cases |
| Endianness or padding incorrect; big integers truncated | [integer encoding](builtins/IntegerToByteString.nash), [integer decoding](builtins/ByteStringToInteger.nash): known encodings, roundtrips, arbitrary widths |
| Invalid UTF8 accepted or Unicode/embedded zero corrupted | [encode](builtins/EncodeUtf8.nash), [decode](builtins/DecodeUtf8.nash): roundtrips, non-ASCII witnesses, overlong/truncated/surrogate/out-of-range rejection |
| Pair projection swapped; empty-list access succeeds; branch selection wrong | pair/list/choice files: projections, tails, empty access rejection, all five Data branches, strict conditional arguments |
| Wrong Data decoder accepts another variant or loses payload | Data constructor/decoder files: roundtrips, integer/byte/constructor shape mismatches |
| Whole-byte shifts copy wrong lengths; enormous shifts/rotations panic | [shift](builtins/ShiftByteString.nash), [rotate](builtins/RotateByteString.nash): direction, zero fill, width/inverse laws, 129-bit counts |
| Wrong hash primitive or digest width | six hash files: digest width and empty-message known answers where supplied; cryptographic model contracts remain pending |
| Malformed signature confused with well-formed verification False | signature files: invalid lengths, [RFC8032](https://www.rfc-editor.org/rfc/rfc8032) valid Ed25519 vector and changed-message False |
| Invalid compressed point/DST accepted; group operation or MSM incorrect | BLS files: valid-point algebra, compressed roundtrips, wrong widths, oversized DST, mismatched MSM lengths and scalar range |
| Wrong value update, zero removal, quantity overflow or negative containment | [insert](builtins/InsertCoin.nash), [union](builtins/UnionValue.nash), [scale](builtins/ScaleValue.nash), [containment](builtins/ValueContains.nash) |
| Malformed asset map accepted | [unValueData](builtins/UnValueData.nash): wrong Data variant, empty inner map, zero quantities, duplicate and unsorted names |

## Findings and open model work

The new obligations reproduced and now guard these local evaluator fixes:
`indexByteString` and `indexArray` reject unrepresentable indices instead of
truncating or panicking; shift/rotate counts no longer panic above u64; whole-byte
left shifts copy the correct number of bytes. The array rejection examples use
a monomorphic `array int` binding: an unused generalized literal template has no
runtime specialization in Nash, so it cannot exercise a rejection obligation.

Associativity and several symbolic arithmetic/ordering obligations hit the
preparation wall limit. The ten array/value builtins lack a pinned Lean model.
BLS validation reaches unsupported `Fin`, and some symbolic bytestring operations
reach unsupported `BitVec` in the SMT translator. These outcomes remain pending.

Cryptographic primitives and modular exponentiation are opaque in the pinned
model and lack supporting semantic lemmas. SMT can assign them behavior that
violates the intended rejection contract; the 12 such model counterexamples in
the snapshot are **not concrete Nash counterexamples**. Upstream work should add
contracts for shape validation, digest lengths, group laws and modular-exponentiation
error conditions, alongside the missing array/value model and translator support.
Native vector checks establish the listed examples independently of those SMT
abstractions; they do not verify cryptographic algorithms.

The Rust Data representation stores constructor tags as u64 while Lean permits
arbitrary integer tags. Native smoke inputs for constructor roundtrips use the
common representable range; the Nash obligations retain their full symbolic
domains. The native `dropList` cost calculation also still converts the absolute
count through u64; counts above that range need an overflow-safe costing fix and
a corresponding large-count regression. Fixing these host gaps and proving all
compiler/type/optimizer specializations remain separate work. The existing independent
[calling-convention certificates](../docs/builtin-compilation.md) check opcodes,
forces and application prefixes across all 101 builtins.

## Index

| Builtin source file | Obligations | SMT-verified |
|---|---:|---:|
| [AddInteger](builtins/AddInteger.nash) | 11 | 10 |
| [AndByteString](builtins/AndByteString.nash) | 3 | 0 |
| [AppendByteString](builtins/AppendByteString.nash) | 4 | 0 |
| [AppendString](builtins/AppendString.nash) | 3 | 0 |
| [BData](builtins/BData.nash) | 1 | 0 |
| [Blake2b224](builtins/Blake2b224.nash) | 2 | 0 |
| [Blake2b256](builtins/Blake2b256.nash) | 2 | 0 |
| [Bls12381FinalVerify](builtins/Bls12381FinalVerify.nash) | 1 | 0 |
| [Bls12381G1Add](builtins/Bls12381G1Add.nash) | 1 | 0 |
| [Bls12381G1Compress](builtins/Bls12381G1Compress.nash) | 1 | 0 |
| [Bls12381G1Equal](builtins/Bls12381G1Equal.nash) | 1 | 0 |
| [Bls12381G1HashToGroup](builtins/Bls12381G1HashToGroup.nash) | 2 | 0 |
| [Bls12381G1Neg](builtins/Bls12381G1Neg.nash) | 1 | 0 |
| [Bls12381G1ScalarMul](builtins/Bls12381G1ScalarMul.nash) | 1 | 0 |
| [Bls12381G1Uncompress](builtins/Bls12381G1Uncompress.nash) | 4 | 0 |
| [Bls12381G2Add](builtins/Bls12381G2Add.nash) | 1 | 0 |
| [Bls12381G2Compress](builtins/Bls12381G2Compress.nash) | 1 | 0 |
| [Bls12381G2Equal](builtins/Bls12381G2Equal.nash) | 1 | 0 |
| [Bls12381G2HashToGroup](builtins/Bls12381G2HashToGroup.nash) | 2 | 0 |
| [Bls12381G2Neg](builtins/Bls12381G2Neg.nash) | 1 | 0 |
| [Bls12381G2ScalarMul](builtins/Bls12381G2ScalarMul.nash) | 1 | 0 |
| [Bls12381G2Uncompress](builtins/Bls12381G2Uncompress.nash) | 4 | 0 |
| [Bls12381MillerLoop](builtins/Bls12381MillerLoop.nash) | 1 | 0 |
| [Bls12381MulMlResult](builtins/Bls12381MulMlResult.nash) | 1 | 0 |
| [ByteStringToInteger](builtins/ByteStringToInteger.nash) | 4 | 0 |
| [ChooseData](builtins/ChooseData.nash) | 5 | 0 |
| [ChooseList](builtins/ChooseList.nash) | 3 | 0 |
| [ChooseUnit](builtins/ChooseUnit.nash) | 2 | 0 |
| [ComplementByteString](builtins/ComplementByteString.nash) | 3 | 0 |
| [ConsByteString](builtins/ConsByteString.nash) | 6 | 2 |
| [ConstrData](builtins/ConstrData.nash) | 1 | 0 |
| [CountSetBits](builtins/CountSetBits.nash) | 2 | 0 |
| [DecodeUtf8](builtins/DecodeUtf8.nash) | 7 | 1 |
| [DivideInteger](builtins/DivideInteger.nash) | 5 | 4 |
| [DropList](builtins/DropList.nash) | 5 | 0 |
| [EncodeUtf8](builtins/EncodeUtf8.nash) | 4 | 0 |
| [EqualsByteString](builtins/EqualsByteString.nash) | 4 | 0 |
| [EqualsData](builtins/EqualsData.nash) | 4 | 0 |
| [EqualsInteger](builtins/EqualsInteger.nash) | 5 | 2 |
| [EqualsString](builtins/EqualsString.nash) | 4 | 0 |
| [ExpModInteger](builtins/ExpModInteger.nash) | 6 | 0 |
| [FindFirstSetBit](builtins/FindFirstSetBit.nash) | 2 | 0 |
| [FstPair](builtins/FstPair.nash) | 1 | 0 |
| [HeadList](builtins/HeadList.nash) | 2 | 1 |
| [IData](builtins/IData.nash) | 1 | 0 |
| [IfThenElse](builtins/IfThenElse.nash) | 4 | 0 |
| [IndexArray](builtins/IndexArray.nash) | 5 | 0 |
| [IndexByteString](builtins/IndexByteString.nash) | 6 | 2 |
| [InsertCoin](builtins/InsertCoin.nash) | 7 | 0 |
| [IntegerToByteString](builtins/IntegerToByteString.nash) | 8 | 4 |
| [Keccak256](builtins/Keccak256.nash) | 1 | 0 |
| [LengthOfArray](builtins/LengthOfArray.nash) | 2 | 0 |
| [LengthOfByteString](builtins/LengthOfByteString.nash) | 4 | 0 |
| [LessThanByteString](builtins/LessThanByteString.nash) | 3 | 0 |
| [LessThanEqualsByteString](builtins/LessThanEqualsByteString.nash) | 2 | 0 |
| [LessThanEqualsInteger](builtins/LessThanEqualsInteger.nash) | 4 | 2 |
| [LessThanInteger](builtins/LessThanInteger.nash) | 5 | 3 |
| [ListData](builtins/ListData.nash) | 1 | 0 |
| [ListToArray](builtins/ListToArray.nash) | 1 | 0 |
| [LookupCoin](builtins/LookupCoin.nash) | 3 | 0 |
| [MapData](builtins/MapData.nash) | 1 | 0 |
| [MkCons](builtins/MkCons.nash) | 3 | 0 |
| [MkNilData](builtins/MkNilData.nash) | 2 | 0 |
| [MkNilPairData](builtins/MkNilPairData.nash) | 2 | 0 |
| [MkPairData](builtins/MkPairData.nash) | 1 | 0 |
| [ModInteger](builtins/ModInteger.nash) | 6 | 4 |
| [MultiplyInteger](builtins/MultiplyInteger.nash) | 8 | 5 |
| [NullList](builtins/NullList.nash) | 2 | 0 |
| [OrByteString](builtins/OrByteString.nash) | 3 | 0 |
| [QuotientInteger](builtins/QuotientInteger.nash) | 5 | 4 |
| [ReadBit](builtins/ReadBit.nash) | 4 | 3 |
| [RemainderInteger](builtins/RemainderInteger.nash) | 6 | 4 |
| [ReplicateByte](builtins/ReplicateByte.nash) | 7 | 4 |
| [Ripemd160](builtins/Ripemd160.nash) | 2 | 0 |
| [RotateByteString](builtins/RotateByteString.nash) | 4 | 0 |
| [ScaleValue](builtins/ScaleValue.nash) | 5 | 0 |
| [SerialiseData](builtins/SerialiseData.nash) | 1 | 0 |
| [Sha2256](builtins/Sha2256.nash) | 2 | 0 |
| [Sha3256](builtins/Sha3256.nash) | 2 | 0 |
| [ShiftByteString](builtins/ShiftByteString.nash) | 5 | 0 |
| [SliceByteString](builtins/SliceByteString.nash) | 6 | 0 |
| [SndPair](builtins/SndPair.nash) | 1 | 0 |
| [SubtractInteger](builtins/SubtractInteger.nash) | 6 | 5 |
| [TailList](builtins/TailList.nash) | 3 | 1 |
| [Trace](builtins/Trace.nash) | 2 | 0 |
| [UnBData](builtins/UnBData.nash) | 3 | 0 |
| [UnConstrData](builtins/UnConstrData.nash) | 3 | 0 |
| [UnIData](builtins/UnIData.nash) | 3 | 0 |
| [UnListData](builtins/UnListData.nash) | 3 | 0 |
| [UnMapData](builtins/UnMapData.nash) | 3 | 0 |
| [UnValueData](builtins/UnValueData.nash) | 6 | 0 |
| [UnionValue](builtins/UnionValue.nash) | 3 | 0 |
| [ValueContains](builtins/ValueContains.nash) | 4 | 0 |
| [ValueData](builtins/ValueData.nash) | 1 | 0 |
| [VerifyEcdsaSecp256k1Signature](builtins/VerifyEcdsaSecp256k1Signature.nash) | 2 | 0 |
| [VerifyEd25519Signature](builtins/VerifyEd25519Signature.nash) | 4 | 0 |
| [VerifySchnorrSecp256k1Signature](builtins/VerifySchnorrSecp256k1Signature.nash) | 2 | 0 |
| [WriteBits](builtins/WriteBits.nash) | 6 | 3 |
| [XorByteString](builtins/XorByteString.nash) | 3 | 0 |
| [Bls12381G1MultiScalarMul](pending/builtins/Bls12381G1MultiScalarMul.nash) | 3 | 0 |
| [Bls12381G2MultiScalarMul](pending/builtins/Bls12381G2MultiScalarMul.nash) | 3 | 0 |
