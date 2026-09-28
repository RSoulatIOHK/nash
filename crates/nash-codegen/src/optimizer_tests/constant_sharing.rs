//! Chunk 5 step 2 candidate, compared with forced-reference sharing alone.
use nash_ir::{
    build::Builder,
    core::*,
    ty::{ConstTy, TermTy, Ty},
};
use nash_plutus::{arena::Arena, builtin::DefaultFunction as F, constant::Constant};
const INT: Ty<'static> = Ty::Const(&ConstTy::Int);
fn trace<'a>(b: &Builder<'a>, text: &'a str, value: &'a Core<'a>) -> &'a Core<'a> {
    b.trace(b.lit(Constant::string(b.arena, text)), value)
}
fn check<'a>(name: &str, b: &Builder<'a>, core: &'a Core<'a>, fails: bool) {
    check_args(name, b, core, fails, &[])
}
fn check_args<'a>(name: &str, b: &Builder<'a>, core: &'a Core<'a>, fails: bool, args: &[i128]) {
    let original = core;
    let core = crate::recursion::rewrite(b, core).unwrap();
    let before = crate::lower::lower_with_builtin_sharing(b.arena, core).unwrap();
    let after = crate::lower::lower_with_constant_sharing(b.arena, core).unwrap();
    let apply = |term| {
        args.iter()
            .fold(term, |f: &'a nash_plutus::term::Term<'a, _>, arg| {
                f.apply(
                    b.arena,
                    nash_plutus::term::Term::integer_from(b.arena, *arg),
                )
            })
    };
    let baseline = crate::harness::eval_named(b.arena, apply(before));
    let candidate = crate::harness::eval_named(b.arena, apply(after));

    assert_eq!(candidate.result.starts_with("error:"), fails);

    insta::assert_snapshot!(
        name,
        crate::harness::pass_snapshot(
            b.arena,
            original,
            format!(
                "--- uplc before\n{}\n--- uplc after\n{}\n--- result\n{}\n--- logs\n{:?}",
                nash_plutus::pretty::term(before),
                nash_plutus::pretty::term(after),
                candidate.result,
                candidate.logs
            )
        )
    );
    // Properties independent of the expected snapshot.
    assert_eq!(baseline.observable, candidate.observable);
    assert_eq!(baseline.logs, candidate.logs);
    assert_eq!(
        nash_plutus::pretty::term(after),
        nash_plutus::pretty::term(
            crate::lower::lower_with_constant_sharing(b.arena, core).unwrap()
        )
    );
}
fn sum<'a>(b: &Builder<'a>, left: &'a Core<'a>, right: &'a Core<'a>) -> &'a Core<'a> {
    b.builtin(F::AddInteger, &[left, right], INT)
}
#[test]
fn repeated_prefix_keeps_later_argument_order() {
    let a = Arena::new();
    let b = Builder::new(&a);
    let left = b.builtin(
        F::SubtractInteger,
        &[b.int(100), trace(&b, "left", b.int(1))],
        INT,
    );
    let right = b.builtin(
        F::SubtractInteger,
        &[b.int(100), trace(&b, "right", b.int(2))],
        INT,
    );
    check("repeated", &b, sum(&b, left, right), false);
}
#[test]
fn one_occurrence_and_different_prefixes_stay_put() {
    let a = Arena::new();
    let b = Builder::new(&a);
    check(
        "different",
        &b,
        sum(
            &b,
            b.builtin(F::SubtractInteger, &[b.int(100), b.int(1)], INT),
            b.builtin(F::SubtractInteger, &[b.int(99), b.int(2)], INT),
        ),
        false,
    );
    check(
        "different_builtin",
        &b,
        sum(
            &b,
            b.builtin(F::SubtractInteger, &[b.int(100), b.int(1)], INT),
            b.builtin(F::MultiplyInteger, &[b.int(100), b.int(2)], INT),
        ),
        false,
    );
}
#[test]
fn trailing_literals_and_computed_first_arguments_are_not_moved() {
    let a = Arena::new();
    let b = Builder::new(&a);
    let left = b.builtin(
        F::SubtractInteger,
        &[trace(&b, "left", b.int(100)), b.int(1)],
        INT,
    );
    let right = b.builtin(
        F::SubtractInteger,
        &[trace(&b, "right", b.int(99)), b.int(1)],
        INT,
    );
    check("trailing", &b, sum(&b, left, right), false);
}
#[test]
fn repeated_trace_prefix_does_not_trace_during_initialization() {
    let a = Arena::new();
    let b = Builder::new(&a);
    check(
        "trace",
        &b,
        trace(
            &b,
            "before",
            sum(
                &b,
                trace(&b, "same", b.int(20)),
                trace(&b, "same", b.int(22)),
            ),
        ),
        false,
    );
}
#[test]
fn unselected_and_selected_division_failures_remain_at_call_sites() {
    let a = Arena::new();
    let b = Builder::new(&a);
    let fail = b.builtin(F::DivideInteger, &[b.int(10), b.int(0)], INT);
    let cold = b.if_(
        b.lit(Constant::bool(&a, true)),
        b.int(42),
        sum(&b, fail, fail),
    );
    check("unselected", &b, cold, false);
    check(
        "failure",
        &b,
        trace(&b, "before", sum(&b, fail, fail)),
        true,
    );
}
#[test]
fn unary_saturated_calls_are_never_partially_shared() {
    let a = Arena::new();
    let b = Builder::new(&a);
    let empty = b.lit(Constant::proto_list(
        &a,
        &nash_plutus::typ::Type::Integer,
        &[],
    ));
    let fail = b.builtin(F::HeadList, &[empty], INT);
    check(
        "unary",
        &b,
        b.if_(
            b.lit(Constant::bool(&a, true)),
            b.int(42),
            sum(&b, fail, fail),
        ),
        false,
    );
}
#[test]
fn returned_partial_values_can_be_applied_independently() {
    let a = Arena::new();
    let b = Builder::new(&a);
    let ty = Ty::Term(a.alloc(TermTy::Fun(&[INT], INT)));
    let partial = b.builtin(F::SubtractInteger, &[b.int(100)], ty);
    let x = Binder {
        name: b.fresh("x"),
        ty,
    };
    let y = Binder {
        name: b.fresh("y"),
        ty,
    };
    check(
        "partial",
        &b,
        b.let_(
            x,
            partial,
            b.let_(
                y,
                partial,
                sum(
                    &b,
                    b.app(b.var(x.name, ty), &[b.int(1)], INT),
                    b.app(b.var(y.name, ty), &[b.int(2)], INT),
                ),
            ),
        ),
        false,
    );
}
#[test]
fn delayed_calls_and_discarded_defaults() {
    let a = Arena::new();
    let b = Builder::new(&a);
    let repeated = sum(
        &b,
        trace(&b, "same", b.int(20)),
        trace(&b, "same", b.int(22)),
    );
    check("delay", &b, b.force(b.delay(repeated), INT), false);
    let core = b.case(
        CaseKind::Bool,
        b.lit(Constant::bool(&a, true)),
        &[
            Branch {
                test: Test::True,
                binders: &[],
                body: b.int(42),
            },
            Branch {
                test: Test::False,
                binders: &[],
                body: b.int(0),
            },
        ],
        Some(repeated),
        INT,
    );
    check("discarded", &b, core, false);
}

#[test]
fn prefix_outside_validator_arguments_and_polymorphic_trace() {
    let a = Arena::new();
    let b = Builder::new(&a);
    let x = Binder {
        name: b.fresh("x"),
        ty: INT,
    };
    let call = b.builtin(F::SubtractInteger, &[b.int(100), b.var(x.name, INT)], INT);
    check_args(
        "validator",
        &b,
        b.lam(&[x], sum(&b, call, call)),
        false,
        &[3],
    );
    let root = b.if_(
        trace(&b, "same", b.lit(Constant::bool(&a, true))),
        trace(&b, "same", b.int(42)),
        b.int(0),
    );
    check("polymorphic", &b, root, false);
}

#[test]
fn ternary_builtin_shares_only_the_first_literal() {
    let a = Arena::new();
    let b = Builder::new(&a);
    let bytes = b.lit(Constant::byte_string(&a, &[1, 2, 3, 4]));
    let first = b.builtin(F::SliceByteString, &[b.int(1), b.int(2), bytes], bytes.ty);
    let second = b.builtin(F::SliceByteString, &[b.int(1), b.int(3), bytes], bytes.ty);
    check(
        "first_literal_only",
        &b,
        b.builtin(F::AppendByteString, &[first, second], bytes.ty),
        false,
    );
}

#[test]
fn separate_lambda_scopes_share_the_closed_prefix() {
    let a = Arena::new();
    let b = Builder::new(&a);
    let x = Binder {
        name: b.fresh("x"),
        ty: INT,
    };
    let y = Binder {
        name: b.fresh("y"),
        ty: INT,
    };
    let left = b.lam(
        &[x],
        b.builtin(F::SubtractInteger, &[b.int(100), b.var(x.name, INT)], INT),
    );
    let right = b.lam(
        &[y],
        b.builtin(F::SubtractInteger, &[b.int(100), b.var(y.name, INT)], INT),
    );
    check(
        "two_scopes",
        &b,
        sum(
            &b,
            b.app(left, &[b.int(1)], INT),
            b.app(right, &[b.int(2)], INT),
        ),
        false,
    );
}
