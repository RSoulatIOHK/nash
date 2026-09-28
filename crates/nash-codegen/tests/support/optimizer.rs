//! Shared accepted optimizer pipeline for unit and integration snapshots.
//! Test-only: production assembly remains O0.
use crate::{lower, recursion};
use nash_ir::{anf, build::Builder, core::Core, hygiene, pretty::pretty};
use nash_plutus::{
    arena::Arena,
    pretty as uplc,
    program::{Program, Version},
};

/// Normalize once before optimization, then rewrite recursion and lower nested
/// Core directly. Used only by tests, never production assembly.
pub fn optimize<'a>(arena: &'a Arena, core: &'a Core<'a>) -> &'a Core<'a> {
    optimize_with(&Builder::new(arena), core)
}

fn optimize_with<'a>(b: &Builder<'a>, core: &'a Core<'a>) -> &'a Core<'a> {
    let fresh = hygiene::freshen(b, core);
    hygiene::validate(fresh, &[]).unwrap();
    let lifted = nash_ir::static_lift::lift(b, fresh);
    hygiene::validate(lifted, &[]).unwrap();
    let shortened = nash_ir::unused_params::reduce(b, lifted);
    hygiene::validate(shortened, &[]).unwrap();
    let normalized = anf::normalize(b, shortened);
    anf::validate(normalized).unwrap();
    hygiene::validate(normalized, &[]).unwrap();
    let propagated = nash_ir::small_inline::simplify(b, normalized);
    anf::validate(propagated).unwrap();
    assert_eq!(core.ty, propagated.ty);
    propagated
}

pub fn candidate<'a>(arena: &'a Arena, core: &'a Core<'a>) -> &'a Core<'a> {
    let b = Builder::new(arena);
    let optimized = optimize_with(&b, core);
    let rewritten = recursion::rewrite(&b, optimized).unwrap();
    // Recursion rewriting reuses self-application lambda subtrees.
    let result = hygiene::freshen(&b, rewritten);
    hygiene::validate(result, &[]).unwrap();
    assert_eq!(core.ty, result.ty);
    result
}

pub fn code_snapshot<'a>(arena: &'a Arena, core: &'a Core<'a>) -> String {
    let b = Builder::new(arena);
    let before = recursion::rewrite(&b, core).expect("O0 recursion encoding");
    let before = lower::lower(arena, before).expect("O0 lowering");
    let b = Builder::new(arena);
    let optimized = optimize(arena, core);
    let rewritten = recursion::rewrite(&b, optimized).expect("optimized recursion encoding");
    let rewritten = hygiene::freshen(&b, rewritten);
    let after = lower::lower_with_constant_sharing(arena, rewritten).expect("optimized lowering");
    for term in [before, after] {
        let closed =
            nash_plutus::debruijn::to_debruijn(arena, term).expect("closed snapshot program");
        nash_plutus::script::validate_program(
            Program::new(arena, Version::plutus_v3(arena), closed),
            nash_plutus::machine::PlutusVersion::V3,
        )
        .expect("valid snapshot target");
    }
    let render = |term| uplc::program(Program::new(arena, Version::plutus_v3(arena), term));
    format!(
        "--- unoptimized Core\n{}\n--- unoptimized UPLC\n{}\n--- optimized Core\n{}\n--- optimized UPLC\n{}",
        pretty(core),
        render(before),
        pretty(optimized),
        render(after)
    )
}
