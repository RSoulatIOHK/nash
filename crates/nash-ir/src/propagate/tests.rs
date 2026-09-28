use super::*;
use crate::{
    anf,
    core::Binder,
    hygiene,
    pretty::pretty,
    ty::{ConstTy, Ty},
};
use nash_plutus::{arena::Arena, builtin::DefaultFunction as F, constant::Constant};
const INT: Ty<'static> = Ty::Const(&ConstTy::Int);
fn binder<'a>(b: &Builder<'a>, text: &'a str, ty: Ty<'a>) -> Binder<'a> {
    Binder {
        name: b.fresh(text),
        ty,
    }
}
macro_rules! assert_optimization_snapshot {
    ($builder:expr, $input:expr $(, $name:expr)?) => {{
        let b = $builder;
        let before = $input;
        let after = propagate(b, before);
        insta::assert_snapshot!($($name,)? format!(
            "--- core before\n{}\n--- core after\n{}",
            pretty(before),
            pretty(after)
        ));
        anf::validate(after).unwrap();
        hygiene::validate(after, &[]).unwrap();
        assert_eq!(before.ty, after.ty);
        assert_eq!(pretty(after), pretty(propagate(b, after)));
    }};
}
#[test]
fn alias_chain_exposes_single_use_literal() {
    let arena = Arena::new();
    let b = Builder::new(&arena);
    let x = binder(&b, "x", INT);
    let y = binder(&b, "y", INT);
    let z = binder(&b, "z", INT);
    let core = b.let_(
        x,
        b.int(42),
        b.let_(
            y,
            b.var(x.name, INT),
            b.let_(z, b.var(y.name, INT), b.var(z.name, INT)),
        ),
    );
    assert_optimization_snapshot!(&b, core);
}
#[test]
fn aliases_do_not_duplicate_shared_literal_payloads() {
    let arena = Arena::new();
    let b = Builder::new(&arena);
    let string = Ty::Const(&ConstTy::String);
    let x = binder(&b, "text", string);
    let y = binder(&b, "alias", string);
    let value = b.lit(Constant::string(
        &arena,
        "a deliberately shared string payload",
    ));
    let core = b.let_(
        x,
        value,
        b.let_(
            y,
            b.var(x.name, string),
            b.builtin(
                F::AppendString,
                &[b.var(y.name, string), b.var(y.name, string)],
                string,
            ),
        ),
    );
    assert_optimization_snapshot!(&b, core);
}
#[test]
fn computed_values_and_delays_keep_their_bindings() {
    let arena = Arena::new();
    let b = Builder::new(&arena);
    let value = b.builtin(F::AddInteger, &[b.int(20), b.int(22)], INT);
    let x = binder(&b, "computed", INT);
    let y = binder(&b, "alias", INT);
    let delayed = b.delay(b.var(y.name, INT));
    let d = binder(&b, "delayed", delayed.ty);
    let core = b.let_(
        x,
        value,
        b.let_(
            y,
            b.var(x.name, INT),
            b.let_(d, delayed, b.force(b.var(d.name, d.ty), INT)),
        ),
    );
    assert_optimization_snapshot!(&b, core);
}
#[test]
fn same_spelling_does_not_capture_outer_alias() {
    let arena = Arena::new();
    let b = Builder::new(&arena);
    let outer = binder(&b, "x", INT);
    let alias = binder(&b, "alias", INT);
    let inner = binder(&b, "x", INT);
    let core = b.lam(
        &[outer],
        b.let_(
            alias,
            b.var(outer.name, INT),
            b.lam(&[inner], b.var(alias.name, INT)),
        ),
    );
    assert_optimization_snapshot!(&b, core);
}
#[test]
fn occurrence_and_root_coercion_views_survive() {
    let arena = Arena::new();
    let b = Builder::new(&arena);
    let bytes = Ty::Const(&ConstTy::Bytes);
    let x = binder(&b, "x", INT);
    let y = binder(&b, "y", bytes);
    let core = b.lam(&[x], b.let_(y, b.var(x.name, bytes), b.var(y.name, bytes)));
    let after = propagate(&b, core);
    let CoreKind::Lam { body, .. } = after.kind else {
        panic!("lambda")
    };
    assert_eq!(body.ty, bytes);
    assert_eq!(after.ty, core.ty);
    assert!(matches!(body.kind, CoreKind::Var(n) if n == x.name));
    let literal = b.with_type(
        b.let_(y, b.with_type(b.int(42), bytes), b.var(y.name, bytes)),
        INT,
    );
    assert_eq!(propagate(&b, literal).ty, INT);
}
#[test]
fn unused_literal_is_removed_but_unused_failure_remains() {
    let arena = Arena::new();
    let b = Builder::new(&arena);
    let x = binder(&b, "literal", INT);
    let y = binder(&b, "failure", INT);
    assert_optimization_snapshot!(&b, b.let_(x, b.int(1), b.let_(y, b.error(INT), b.int(42))));
}

fn repeated_literal<'a>(b: &Builder<'a>, value: &'a Core<'a>, func: F) -> &'a Core<'a> {
    let x = binder(b, "literal", value.ty);
    let alias = binder(b, "alias", value.ty);
    b.let_(
        x,
        value,
        b.let_(
            alias,
            b.var(x.name, x.ty),
            b.builtin(
                func,
                &[b.var(alias.name, alias.ty), b.var(alias.name, alias.ty)],
                value.ty,
            ),
        ),
    )
}
#[test]
fn repeated_integer_inlines_without_a_size_cap() {
    let arena = Arena::new();
    let b = Builder::new(&arena);
    let huge=arena.alloc_integer("123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890".parse().unwrap());
    let core = repeated_literal(&b, b.lit(Constant::integer(&arena, huge)), F::AddInteger);
    assert_optimization_snapshot!(&b, core);
}
#[test]
fn repeated_bytes_at_64_byte_limit_inline() {
    let arena = Arena::new();
    let b = Builder::new(&arena);
    let core = repeated_literal(
        &b,
        b.lit(Constant::byte_string(&arena, &[42; 64])),
        F::AppendByteString,
    );
    assert_optimization_snapshot!(&b, core);
}
#[test]
fn repeated_bytes_above_limit_stay_shared() {
    let arena = Arena::new();
    let b = Builder::new(&arena);
    let core = repeated_literal(
        &b,
        b.lit(Constant::byte_string(&arena, &[42; 65])),
        F::AppendByteString,
    );
    assert_optimization_snapshot!(&b, core);
}
#[test]
fn repeated_bls_g1_constant_inlines() {
    let arena = Arena::new();
    let b = Builder::new(&arena);
    let core = repeated_literal(
        &b,
        b.lit(Constant::g1(&arena, arena.alloc(Default::default()))),
        F::Bls12_381_G1_Add,
    );
    assert_optimization_snapshot!(&b, core);
}
#[test]
fn repeated_bls_g2_constant_inlines() {
    let arena = Arena::new();
    let b = Builder::new(&arena);
    let core = repeated_literal(
        &b,
        b.lit(Constant::g2(&arena, arena.alloc(Default::default()))),
        F::Bls12_381_G2_Add,
    );
    assert_optimization_snapshot!(&b, core);
}
#[test]
fn repeated_bls_ml_constant_inlines() {
    let arena = Arena::new();
    let b = Builder::new(&arena);
    let core = repeated_literal(
        &b,
        b.lit(Constant::ml_result(&arena, arena.alloc(Default::default()))),
        F::Bls12_381_MulMlResult,
    );
    assert_optimization_snapshot!(&b, core);
}
