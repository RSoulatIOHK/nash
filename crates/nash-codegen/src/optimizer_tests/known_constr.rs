//! Direct native-constructor case folding, including isolated-pass evidence.
use nash_ir::{
    build::Builder,
    core::*,
    hygiene, known_case,
    pretty::pretty,
    ty::{ConstTy, RuntimeTy, Ty},
};
use nash_plutus::{arena::Arena, builtin::DefaultFunction as F, constant::Constant};

const INT: Ty<'static> = Ty::Const(&ConstTy::Int);
fn bind<'a>(b: &Builder<'a>, label: &'a str) -> Binder<'a> {
    Binder {
        name: b.fresh(label),
        ty: INT,
    }
}
fn trace<'a>(b: &Builder<'a>, label: &'a str, value: &'a Core<'a>) -> &'a Core<'a> {
    b.trace(b.lit(Constant::string(b.arena, label)), value)
}
fn constr<'a>(b: &Builder<'a>, tag: u16, fields: &[&'a Core<'a>]) -> &'a Core<'a> {
    let types: Vec<_> = fields.iter().map(|f| f.ty).collect();
    let ty = Ty::Runtime(b.arena.alloc(RuntimeTy::Constr {
        tag,
        fields: b.arena.alloc_slice_copy(&types),
    }));
    b.constr(tag, fields, ty)
}
fn arm<'a>(b: &Builder<'a>, tag: u16, binders: &[Binder<'a>], body: &'a Core<'a>) -> Branch<'a> {
    Branch {
        test: Test::Tag(tag),
        binders: b.arena.alloc_slice_copy(binders),
        body,
    }
}
fn check(name: &str, b: &Builder<'_>, before: &Core<'_>, fails: bool) {
    let after = known_case::reduce_constr(b, before);
    let left = crate::harness::eval_core_raw(b.arena, before);
    let right = crate::harness::eval_core_raw(b.arena, after);
    assert_eq!(right.result.starts_with("error:"), fails);
    insta::assert_snapshot!(
        name,
        crate::harness::pass_snapshot(
            b.arena,
            before,
            format!(
                "--- core before\n{}\n--- uplc before\n{}\n--- core after\n{}\n--- uplc after\n{}\n--- result\n{}\n--- logs\n{:?}",
                pretty(before),
                left.uplc,
                pretty(after),
                right.uplc,
                right.result,
                right.logs
            )
        )
    );
    hygiene::validate(before, &[]).unwrap();
    hygiene::validate(after, &[]).unwrap();
    assert_eq!(before.ty, after.ty);
    assert_eq!(left.observable, right.observable);
    assert_eq!(left.logs, right.logs);
    assert!(std::ptr::eq(after, known_case::reduce_constr(b, after)));
}

#[test]
fn selects_tag_with_fields_in_source_order() {
    let a = Arena::new();
    let b = Builder::new(&a);
    let x = bind(&b, "x");
    let y = bind(&b, "y");
    let body = trace(
        &b,
        "body",
        b.builtin(
            F::SubtractInteger,
            &[b.var(x.name, INT), b.var(y.name, INT)],
            INT,
        ),
    );
    let arms = [
        arm(&b, 1, &[x, y], body),
        arm(&b, 0, &[], trace(&b, "cold", b.error(INT))),
    ];
    check(
        "field_order",
        &b,
        b.case(
            CaseKind::Tag,
            constr(
                &b,
                1,
                &[trace(&b, "first", b.int(50)), trace(&b, "second", b.int(8))],
            ),
            &arms,
            None,
            INT,
        ),
        false,
    );
}

#[test]
fn nullary_and_expanded_wildcard_branches() {
    let a = Arena::new();
    let b = Builder::new(&a);
    for (tag, name) in [
        (0, "nullary"),
        (1, "wildcard_one_field"),
        (2, "wildcard_two_fields"),
    ] {
        let x = bind(&b, "ignored");
        let y = bind(&b, "ignored");
        let z = bind(&b, "ignored");
        let arms = [
            arm(&b, 0, &[], b.int(42)),
            arm(&b, 1, &[x], b.int(42)),
            arm(&b, 2, &[y, z], b.int(42)),
        ];
        let fields: Vec<_> = (0..tag).map(|_| trace(&b, "field", b.int(0))).collect();
        check(
            name,
            &b,
            b.case(CaseKind::Tag, constr(&b, tag, &fields), &arms, None, INT),
            false,
        );
    }
}

#[test]
fn ignored_failing_field_still_fails_before_later_fields_and_body() {
    let a = Arena::new();
    let b = Builder::new(&a);
    let x = bind(&b, "ignored");
    let y = bind(&b, "ignored");
    check(
        "ignored_failure",
        &b,
        b.case(
            CaseKind::Tag,
            constr(
                &b,
                0,
                &[
                    trace(&b, "first", b.error(INT)),
                    trace(&b, "second", b.int(0)),
                ],
            ),
            &[arm(&b, 0, &[x, y], trace(&b, "body", b.int(42)))],
            None,
            INT,
        ),
        true,
    );
}

#[test]
fn returned_function_and_delay_capture_evaluated_fields() {
    let a = Arena::new();
    let b = Builder::new(&a);
    for (delayed, name) in [(false, "returned_function"), (true, "returned_delay")] {
        let x = bind(&b, "x");
        let p = bind(&b, "p");
        let body = trace(&b, "body", b.var(x.name, INT));
        let result = if delayed {
            b.delay(body)
        } else {
            b.lam(&[p], body)
        };
        let case = b.case(
            CaseKind::Tag,
            constr(&b, 0, &[trace(&b, "field", b.int(42))]),
            &[arm(&b, 0, &[x], result)],
            None,
            result.ty,
        );
        let invoke = if delayed {
            b.force(case, INT)
        } else {
            b.app(case, &[trace(&b, "argument", b.int(0))], INT)
        };
        check(name, &b, invoke, false);
    }
}

#[test]
fn nested_cases_and_cold_construction() {
    let a = Arena::new();
    let b = Builder::new(&a);
    let inner = b.case(
        CaseKind::Tag,
        constr(&b, 0, &[]),
        &[arm(&b, 0, &[], b.int(42))],
        None,
        INT,
    );
    check(
        "nested",
        &b,
        b.case(
            CaseKind::Tag,
            constr(&b, 0, &[]),
            &[arm(&b, 0, &[], inner)],
            None,
            INT,
        ),
        false,
    );
    let x = bind(&b, "ignored");
    let cold = b.case(
        CaseKind::Tag,
        constr(&b, 0, &[trace(&b, "cold", b.error(INT))]),
        &[arm(&b, 0, &[x], b.int(0))],
        None,
        INT,
    );
    check(
        "cold",
        &b,
        b.if_(b.lit(Constant::bool(&a, true)), b.int(42), cold),
        false,
    );
}

#[test]
fn unknown_tag_and_arity_mismatch_are_unchanged() {
    let a = Arena::new();
    let b = Builder::new(&a);
    let x = bind(&b, "x");
    let cases = [
        (
            "unknown_tag",
            constr(&b, 1, &[trace(&b, "field", b.int(0))]),
            arm(&b, 0, &[], b.int(42)),
        ),
        (
            "extra_field",
            constr(&b, 0, &[b.int(0)]),
            arm(&b, 0, &[], b.int(42)),
        ),
    ];
    for (name, subject, branch) in cases {
        let c = b.case(CaseKind::Tag, subject, &[branch], None, INT);
        check(name, &b, c, true);
        assert!(std::ptr::eq(c, known_case::reduce_constr(&b, c)));
    }
    // Too few fields returns a lambda in UPLC; explicitly apply it for evaluation.
    let c = b.case(
        CaseKind::Tag,
        constr(&b, 0, &[]),
        &[arm(&b, 0, &[x], b.var(x.name, INT))],
        None,
        b.lam(&[x], b.var(x.name, INT)).ty,
    );
    check("missing_field", &b, b.app(c, &[b.int(42)], INT), false);
    assert!(std::ptr::eq(c, known_case::reduce_constr(&b, c)));
}

#[test]
fn invalid_tables_remain_lowering_errors() {
    let a = Arena::new();
    let b = Builder::new(&a);
    let good = arm(&b, 0, &[], b.int(42));
    for (name, branches, default) in [
        ("sparse", vec![arm(&b, 59, &[], b.int(42))], None),
        ("duplicate", vec![good, good], None),
        (
            "wrong_test",
            vec![Branch {
                test: Test::True,
                ..good
            }],
            None,
        ),
        ("unexpanded_default", vec![good], Some(b.int(0))),
    ] {
        let c = b.case(CaseKind::Tag, constr(&b, 0, &[]), &branches, default, INT);
        let after = known_case::reduce_constr(&b, c);
        let left = crate::lower::lower(&a, c).unwrap_err();
        let right = crate::lower::lower(&a, after).unwrap_err();
        insta::assert_snapshot!(
            name,
            format!(
                "--- before\n{}\n--- after\n{}\n--- errors\n{left:?}\n{right:?}",
                pretty(c),
                pretty(after)
            )
        );
        assert!(std::ptr::eq(c, after));
    }
}

#[test]
fn empty_table_and_effectful_subject_are_unchanged() {
    let a = Arena::new();
    let b = Builder::new(&a);
    let empty = b.case(CaseKind::Tag, constr(&b, 0, &[]), &[], None, INT);
    check("empty_table", &b, empty, true);
    assert!(std::ptr::eq(empty, known_case::reduce_constr(&b, empty)));
    let subject = trace(&b, "subject", constr(&b, 0, &[]));
    let c = b.case(
        CaseKind::Tag,
        subject,
        &[arm(&b, 0, &[], b.int(42))],
        None,
        INT,
    );
    check("effectful_subject", &b, c, false);
    assert!(std::ptr::eq(c, known_case::reduce_constr(&b, c)));
}
