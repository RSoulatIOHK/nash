//! Shared ledger input for the vesting snapshots, artifact tests and explicit perf runner.
use nash_plutus::{arena::Arena, data::PlutusData};

pub fn context<'a>(
    arena: &'a Arena,
    datum: &'a PlutusData<'a>,
    redeemer: &'a PlutusData<'a>,
    lower_time: i128,
    signer: &'a [u8],
) -> &'a PlutusData<'a> {
    let constr = |tag, fields: &[&'a PlutusData<'a>]| {
        PlutusData::constr(arena, tag, arena.alloc_slice_copy(fields))
    };
    let empty_list = PlutusData::list(arena, &[]);
    let empty_map = PlutusData::map(arena, &[]);
    let zero = PlutusData::integer_from(arena, 0);
    let tx_id = PlutusData::byte_string(arena, &[0; 32]);
    let closed = constr(1, &[]);
    let lower = constr(
        0,
        &[
            constr(1, &[PlutusData::integer_from(arena, lower_time)]),
            closed,
        ],
    );
    let upper = constr(0, &[constr(2, &[]), closed]);
    let signers = if signer.is_empty() {
        empty_list
    } else {
        // The owner is deliberately second: signedBy must search the whole list.
        PlutusData::list(
            arena,
            arena.alloc_slice_copy(&[
                PlutusData::byte_string(arena, &[0xbb]),
                PlutusData::byte_string(arena, signer),
            ]),
        )
    };
    let tx = constr(
        0,
        &[
            empty_list,                 // inputs
            empty_list,                 // reference inputs
            empty_list,                 // outputs
            zero,                       // fee
            empty_map,                  // mint
            empty_list,                 // certificates
            empty_map,                  // withdrawals
            constr(0, &[lower, upper]), // validity range
            signers,
            empty_map, // redeemers
            empty_map, // datums
            tx_id,
            empty_map,      // votes
            empty_list,     // proposals
            constr(1, &[]), // current treasury
            constr(1, &[]), // treasury donation
        ],
    );
    let reference = constr(0, &[tx_id, zero]);
    let spending = constr(1, &[reference, constr(0, &[datum])]);
    constr(0, &[tx, redeemer, spending])
}
