//! Known Boolean case semantics, independent of the accepted pipeline.
use nash_ir::{
    build::Builder,
    core::*,
    hygiene, known_bool,
    pretty::pretty,
    ty::{ConstTy, Ty},
};
use nash_plutus::{arena::Arena, constant::Constant};
const INT: Ty<'static> = Ty::Const(&ConstTy::Int);
fn trace<'a>(b: &Builder<'a>, s: &'a str, x: &'a Core<'a>) -> &'a Core<'a> {
    b.trace(b.lit(Constant::string(b.arena, s)), x)
}
fn check(name: &str, b: &Builder<'_>, before: &Core<'_>, logs: &[&str], fails: bool) {
    let after = known_bool::reduce(b, before);
    assert_eq!(before.ty, after.ty);
    hygiene::validate(after, &[]).unwrap();
    let left = crate::harness::eval_core_raw(b.arena, before);
    let right = crate::harness::eval_core_raw(b.arena, after);
    assert_eq!(left.observable, right.observable);
    assert_eq!(left.logs, right.logs);
    assert_eq!(right.logs, logs);
    assert_eq!(right.result.starts_with("error:"), fails);
    insta::assert_snapshot!(
        name,
        format!(
            "--- core before\n{}\n--- core after\n{}\n--- uplc before\n{}\n--- uplc after\n{}\n--- result\n{}\n--- logs\n{:?}",
            pretty(before),
            pretty(after),
            left.uplc,
            right.uplc,
            right.result,
            right.logs
        )
    );
}
#[test]
fn known_true_false_and_unselected_effects() {
    let a = Arena::new();
    let b = Builder::new(&a);
    for (value, name) in [(true, "true"), (false, "false")] {
        let good = trace(&b, "chosen", b.int(42));
        let bad = trace(&b, "wrong", b.error(INT));
        check(
            name,
            &b,
            b.if_(
                b.lit(Constant::bool(&a, value)),
                if value { good } else { bad },
                if value { bad } else { good },
            ),
            &["chosen"],
            false,
        );
    }
    check(
        "chosen_failure",
        &b,
        b.if_(
            b.lit(Constant::bool(&a, true)),
            trace(&b, "chosen", b.error(INT)),
            b.int(42),
        ),
        &["chosen"],
        true,
    );
}
#[test]
fn defaults_missing_matches_and_reversed_order() {
    let a = Arena::new();
    let b = Builder::new(&a);
    let bs = [Branch {
        test: Test::False,
        binders: &[],
        body: b.int(0),
    }];
    let yes = b.lit(Constant::bool(&a, true));
    check(
        "default",
        &b,
        b.case(CaseKind::Bool, yes, &bs, Some(b.int(42)), INT),
        &[],
        false,
    );
    let missing = b.case(CaseKind::Bool, yes, &bs, None, INT);
    assert!(std::ptr::eq(missing, known_bool::reduce(&b, missing)));
    check("missing", &b, missing, &[], true);
    check(
        "reversed",
        &b,
        b.case(
            CaseKind::Bool,
            yes,
            &[
                bs[0],
                Branch {
                    test: Test::True,
                    binders: &[],
                    body: b.int(42),
                },
            ],
            Some(b.error(INT)),
            INT,
        ),
        &[],
        false,
    );
}
#[test]
fn earlier_strict_work_remains_and_effectful_subject_is_not_folded() {
    let a = Arena::new();
    let b = Builder::new(&a);
    let x = Binder {
        name: b.fresh("ignored"),
        ty: INT,
    };
    let c = b.if_(b.lit(Constant::bool(&a, true)), b.int(42), b.error(INT));
    check(
        "strict_trace",
        &b,
        b.let_(x, trace(&b, "before", b.int(0)), c),
        &["before"],
        false,
    );
    check("strict_failure", &b, b.let_(x, b.error(INT), c), &[], true);
    let c = b.if_(
        trace(&b, "subject", b.lit(Constant::bool(&a, true))),
        b.int(42),
        b.error(INT),
    );
    assert!(std::ptr::eq(c, known_bool::reduce(&b, c)));
    check("effectful_subject", &b, c, &["subject"], false);
}
#[test]
fn returned_functions_delays_and_nested_cases() {
    let a = Arena::new();
    let b = Builder::new(&a);
    let yes = b.lit(Constant::bool(&a, true));
    let p = Binder {
        name: b.fresh("p"),
        ty: INT,
    };
    let f = b.lam(&[p], trace(&b, "called", b.var(p.name, p.ty)));
    check(
        "returned_function",
        &b,
        b.app(b.if_(yes, f, b.error(f.ty)), &[b.int(42)], INT),
        &["called"],
        false,
    );
    let d = b.delay(trace(&b, "forced", b.int(42)));
    check(
        "returned_delay",
        &b,
        b.force(b.if_(yes, d, b.error(d.ty)), INT),
        &["forced"],
        false,
    );
    check(
        "nested",
        &b,
        b.if_(
            b.if_(yes, yes, b.lit(Constant::bool(&a, false))),
            b.int(42),
            b.error(INT),
        ),
        &[],
        false,
    );
}
#[test]
fn malformed_boolean_tables_are_not_hidden() {
    let a = Arena::new();
    let b = Builder::new(&a);
    let p = Binder {
        name: b.fresh("field"),
        ty: INT,
    };
    let yes = b.lit(Constant::bool(&a, true));
    for (name, bs) in [
        (
            "duplicate",
            vec![
                Branch {
                    test: Test::True,
                    binders: &[],
                    body: b.int(42),
                },
                Branch {
                    test: Test::True,
                    binders: &[],
                    body: b.int(0),
                },
            ],
        ),
        (
            "wrong_test",
            vec![Branch {
                test: Test::Nil,
                binders: &[],
                body: b.int(42),
            }],
        ),
        (
            "fields",
            vec![Branch {
                test: Test::True,
                binders: b.arena.alloc_slice_copy(&[p]),
                body: b.int(42),
            }],
        ),
    ] {
        let root = b.case(CaseKind::Bool, yes, &bs, None, INT);
        let after = known_bool::reduce(&b, root);
        assert!(std::ptr::eq(root, after));
        let left = crate::lower::lower(&a, root).unwrap_err();
        let right = crate::lower::lower(&a, after).unwrap_err();
        insta::assert_snapshot!(
            name,
            format!(
                "--- before\n{}\n--- after\n{}\n--- errors\n{left:?}\n{right:?}",
                pretty(root),
                pretty(after)
            )
        );
    }
}
